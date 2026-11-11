//! Running the OmniRoute on this computer for the user (2026-11-11): its connections and their health, adding a free
//! provider with a key (also the OpenRouter key Qanuvi already keeps), renewing a sign-in, and a fallback combo for
//! Qanuvi. Everything goes through OmniRoute's own command line, which signs in to the local server with this
//! machine's token. Keys go to OmniRoute through stdin, never on a command line, and never back to the window.

use crate::llm::Endpoint;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// One provider connection inside OmniRoute, as the window shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub name: String,
    /// "apikey" or "oauth".
    pub auth: String,
    /// "working", "needs sign-in", "resting" or "error".
    pub state: String,
    pub problem: String,
}

/// A provider OmniRoute knows (its catalog), for expert users who choose their own.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub has_free: bool,
    pub website: String,
}

/// Runs `omniroute <args>` hidden, with `input` on stdin, for at most `timeout`. Answers stdout, or why it failed.
fn run(
    ep: &Endpoint,
    args: &[&str],
    input: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    if crate::local_servers::program_for(ep) != Some("omniroute") {
        return Err("This is not an OmniRoute on this computer.".into());
    }
    let path = crate::local_servers::find("omniroute")
        .ok_or("OmniRoute is not installed on this computer.")?;
    let mut cmd = Command::new(&path);
    cmd.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(port) = crate::local_servers::port_of(&ep.url) {
        cmd.env("OMNIROUTE_BASE_URL", format!("http://localhost:{port}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("OmniRoute could not be asked: {e}"))?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(text.as_bytes());
        let _ = stdin.write_all(b"\n");
    }
    // Wait in a thread so a stuck command can be given up on.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx
        .recv_timeout(timeout)
        .map_err(|_| "OmniRoute did not finish in time.".to_string())?
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    Err(reason(&String::from_utf8_lossy(&out.stderr), &stdout))
}

/// The useful line of a failed command (OmniRoute prints notes about its own setup first). Never echoes keys: the
/// commands Qanuvi runs get keys only on stdin, and lines that look like keys are left out.
fn reason(stderr: &str, stdout: &str) -> String {
    let noise = [
        "Loaded env",
        "STORAGE_ENCRYPTION",
        "database already",
        "storage.sqlite",
        "Not auto-generating",
        "previous key",
        "DeprecationWarning",
        "trace-deprecation",
    ];
    stderr
        .lines()
        .chain(stdout.lines())
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !noise.iter().any(|n| l.contains(n)))
        .find(|l| !l.contains("sk-"))
        .map(|l| l.chars().take(200).collect())
        .unwrap_or_else(|| "OmniRoute gave no reason.".into())
}

/// The JSON in a command's output (OmniRoute prints a line about its settings first).
fn json_in(text: &str) -> Option<Value> {
    let start = text
        .lines()
        .position(|l| l.starts_with('{') || l.starts_with('['))?;
    serde_json::from_str(&text.lines().skip(start).collect::<Vec<_>>().join("\n")).ok()
}

/// What a connection's status record means for the user.
pub fn connection_of(c: &Value, now: &str) -> Connection {
    let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
    let error = s("lastError");
    let resting = c["rateLimitedUntil"].as_str().is_some_and(|t| t > now);
    let state = if !c["isActive"].as_bool().unwrap_or(true) {
        if s("authType") == "oauth" {
            "needs sign-in"
        } else {
            "error"
        }
    } else if resting {
        "resting"
    } else if !error.is_empty() && c["testStatus"] != "active" {
        "error"
    } else {
        "working"
    };
    Connection {
        id: s("id"),
        provider: s("provider"),
        name: s("name"),
        auth: s("authType"),
        state: state.into(),
        problem: if state == "working" {
            String::new()
        } else {
            error.chars().take(200).collect()
        },
    }
}

pub fn connections(ep: &Endpoint) -> Result<Vec<Connection>, String> {
    let out = run(
        ep,
        &["providers", "status", "--json"],
        None,
        Duration::from_secs(60),
    )?;
    let v = json_in(&out).ok_or("OmniRoute's answer could not be read.")?;
    let now = now_iso();
    Ok(v["connections"]
        .as_array()
        .map(|a| a.iter().map(|c| connection_of(c, &now)).collect())
        .unwrap_or_default())
}

fn now_iso() -> String {
    // RFC 3339 in UTC, the form OmniRoute uses, so times compare as text.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// OmniRoute's catalog of providers (for expert users).
pub fn catalog(ep: &Endpoint) -> Result<Vec<CatalogEntry>, String> {
    let out = run(
        ep,
        &["providers", "available", "--json"],
        None,
        Duration::from_secs(60),
    )?;
    let v = json_in(&out).ok_or("OmniRoute's catalog could not be read.")?;
    let s = |p: &Value, k: &str| p[k].as_str().unwrap_or("").to_string();
    Ok(v["providers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|p| !p["deprecated"].as_bool().unwrap_or(false))
                .map(|p| CatalogEntry {
                    id: s(p, "id"),
                    name: s(p, "name"),
                    category: s(p, "category"),
                    has_free: p["hasFree"].as_bool().unwrap_or(false),
                    website: s(p, "website"),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// A provider id as OmniRoute takes it (letters, digits, dashes).
fn valid_id(id: &str) -> Result<(), String> {
    // Starts with a letter or digit, so it can never be read as an option.
    if id.len() <= 64
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err("That is not a provider id.".into())
    }
}

/// Connects a provider that works with an API key. The key goes to OmniRoute on stdin.
pub fn add_key(ep: &Endpoint, provider: &str, key: &str) -> Result<(), String> {
    valid_id(provider)?;
    let key = key.trim();
    if key.len() < 8 || key.chars().any(char::is_whitespace) {
        return Err("That does not look like an API key.".into());
    }
    let name = format!("Qanuvi {provider}");
    run(
        ep,
        &[
            "providers",
            "add",
            provider,
            "--name",
            &name,
            "--credential-stdin",
            "--yes",
            "--json",
        ],
        Some(key),
        Duration::from_secs(90),
    )
    .map(|_| ())
}

/// Tests one connection (a real call through OmniRoute).
pub fn test(ep: &Endpoint, id: &str) -> Result<(), String> {
    valid_id(id)?;
    let out = run(
        ep,
        &["providers", "test", id],
        None,
        Duration::from_secs(90),
    )?;
    // A skipped test is not a pass (OmniRoute cannot probe sign-in connections).
    if out.contains("SKIP") || out.contains("FAIL") {
        return Err(reason("", &out));
    }
    Ok(())
}

/// Removes one connection from OmniRoute (the user asked, and confirmed it in the window).
pub fn remove(ep: &Endpoint, id: &str) -> Result<(), String> {
    valid_id(id)?;
    run(
        ep,
        &["providers", "remove", id, "--yes", "--json"],
        None,
        Duration::from_secs(60),
    )
    .map(|_| ())
}

/// Renews a sign-in. Antigravity: OmniRoute opens Google's sign-in page and catches the answer itself; the user only
/// signs in. Other sign-ins go through OmniRoute's own page (the caller opens it).
pub fn sign_in(ep: &Endpoint, provider: &str) -> Result<(), String> {
    valid_id(provider)?;
    match provider {
        "antigravity" => run(
            ep,
            &["login", "antigravity", "--push", "--timeout", "300000"],
            None,
            Duration::from_secs(320),
        )
        .map(|_| ()),
        _ => Err("panel".into()),
    }
}

/// Makes (or remakes) a combo: OmniRoute tries the models in this order, each a fallback for the one before.
pub fn make_combo(ep: &Endpoint, name: &str, models: &[String]) -> Result<(), String> {
    valid_id(name)?;
    if models.is_empty()
        || models
            .iter()
            .any(|m| m.contains(',') || m.chars().any(char::is_whitespace))
    {
        return Err("No usable models for the combo.".into());
    }
    let _ = run(
        ep,
        &["combo", "delete", name, "--yes"],
        None,
        Duration::from_secs(60),
    );
    run(
        ep,
        &[
            "combo",
            "create",
            name,
            "--strategy",
            "priority",
            "--models",
            &models.join(","),
        ],
        None,
        Duration::from_secs(60),
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_connection_health() {
        let now = "2026-11-11T03:00:00Z";
        let c = |v: Value| connection_of(&v, now);
        assert_eq!(c(json!({"id": "a", "provider": "openrouter", "authType": "apikey", "isActive": true, "testStatus": "active"})).state, "working");
        let kiro = c(
            json!({"id": "k", "provider": "kiro", "authType": "oauth", "isActive": false,
            "lastError": "[400]: Missing bearer"}),
        );
        assert_eq!(kiro.state, "needs sign-in");
        assert!(kiro.problem.contains("Missing bearer"));
        assert_eq!(
            c(json!({"isActive": true, "rateLimitedUntil": "2026-11-11T04:00:00Z"})).state,
            "resting"
        );
        assert_eq!(
            c(json!({"isActive": true, "rateLimitedUntil": "2026-11-11T02:00:00Z"})).state,
            "working"
        );
        assert_eq!(
            c(json!({"isActive": true, "lastError": "boom", "testStatus": "failed"})).state,
            "error"
        );
    }

    #[test]
    fn keeps_keys_and_odd_ids_out_of_commands() {
        assert!(valid_id("openrouter").is_ok());
        assert!(valid_id("a b").is_err());
        assert!(valid_id("--name").is_err());
        assert!(valid_id("kimi-coding").is_ok());
        assert!(valid_id("x;rm").is_err());
        assert_eq!(
            reason("Loaded env from x\nError: invalid credential\n", ""),
            "Error: invalid credential"
        );
        assert_eq!(reason("sk-or-v1-secret\nbad", ""), "bad");
        assert!(now_iso().starts_with("20") && now_iso().ends_with('Z') && now_iso().len() == 20);
        assert_eq!(json_in("note\n{\"a\": 1}").unwrap()["a"], 1);
    }
}
