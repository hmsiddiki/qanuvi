//! Keeping Claude Code current: the installed version against the newest published one (the public npm registry), and
//! an update through Claude Code's own `claude update` (its official way). Qanuvi never ships or changes Claude Code
//! itself; the update is the owner's choice (a button, or "keep it up to date by itself").

use serde::Serialize;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Versions {
    pub installed: String,
    pub latest: String,
    pub newer: bool,
}

/// The version number in text like "2.1.302 (Claude Code)".
pub fn version_in(text: &str) -> String {
    text.split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.contains('.'))
        .unwrap_or("")
        .trim_end_matches(',')
        .to_string()
}

/// Whether version `a` is newer than `b` (numbers compared part by part).
pub fn is_newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (p, q) = (
            x.get(i).copied().unwrap_or(0),
            y.get(i).copied().unwrap_or(0),
        );
        if p != q {
            return p > q;
        }
    }
    false
}

fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn installed() -> Result<String, String> {
    let path = crate::agent::find_claude().ok_or("Claude Code is not installed.")?;
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    hidden(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    Ok(version_in(&String::from_utf8_lossy(&out.stdout)))
}

fn latest() -> Result<String, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );
    let url = "https://registry.npmjs.org/@anthropic-ai/claude-code/latest";
    let mut res = agent
        .get(url)
        .call()
        .map_err(|e| format!("{url} could not be reached: {e}"))?;
    if res.status().as_u16() != 200 {
        return Err(format!("{url} answered {}.", res.status().as_u16()));
    }
    let body = res
        .body_mut()
        .with_config()
        .limit(2_000_000)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    v["version"]
        .as_str()
        .map(str::to_string)
        .ok_or("No version in the answer.".into())
}

pub fn versions() -> Result<Versions, String> {
    let installed = installed()?;
    let latest = latest()?;
    Ok(Versions {
        newer: is_newer(&latest, &installed),
        installed,
        latest,
    })
}

/// The proxy address Claude Code's own user settings on this machine send it to (headroom wrap sets one), if any:
/// then every Claude Code there, Qanuvi's too, goes through it already.
pub fn settings_base_url() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let path = std::path::Path::new(&home)
        .join(".claude")
        .join("settings.json");
    base_url_in(&std::fs::read_to_string(path).ok()?)
}

/// Whether Claude Code's user settings turned its own tool search off as part of a Headroom setup (Headroom defers
/// the tools itself, so it asks for `ENABLE_TOOL_SEARCH` "false"). A session sent past Headroom (built-in compression on
/// "All models") then gets Claude Code's own "auto" back; a "false" set without Headroom is the user's and is kept.
pub fn tool_search_off_for_headroom() -> bool {
    let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) else {
        return false;
    };
    let path = std::path::Path::new(&home)
        .join(".claude")
        .join("settings.json");
    std::fs::read_to_string(path)
        .map(|t| tool_search_off_for_headroom_in(&t))
        .unwrap_or(false)
}

fn tool_search_off_for_headroom_in(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let env = &v["env"];
    env["ENABLE_TOOL_SEARCH"].as_str() == Some("false")
        && env["ANTHROPIC_BASE_URL"]
            .as_str()
            .is_some_and(crate::compress::looks_like_headroom)
}

fn base_url_in(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    v["env"]["ANTHROPIC_BASE_URL"].as_str().map(str::to_string)
}

/// Whether the machine's settings ask to keep Claude Code up to date by itself (Settings, Agents).
pub fn auto_wanted(settings: &serde_json::Map<String, serde_json::Value>) -> bool {
    settings
        .get("updates")
        .and_then(|u| u["claudeAuto"].as_bool())
        .unwrap_or(false)
}

/// Keeps this machine's Claude Code current by itself when its settings ask for that and no agent is working
/// (24z3 D4: each machine's core does it, This PC's and every master's, with or without a window).
pub fn auto_update(db: &crate::store::Db) {
    let settings = crate::settings::get_all(db).unwrap_or_default();
    if !auto_wanted(&settings) || crate::agent::any_working(db) {
        return;
    }
    if let Ok(v) = versions()
        && v.newer
    {
        match update() {
            Ok(_) => eprintln!("Claude Code updated from {} to {}.", v.installed, v.latest),
            Err(e) => eprintln!("The Claude Code update did not finish: {e}"),
        }
    }
}

/// Runs Claude Code's own update; answers what it printed.
pub fn update() -> Result<String, String> {
    let path = crate::agent::find_claude().ok_or("Claude Code is not installed.")?;
    let mut cmd = Command::new(path);
    cmd.arg("update");
    hidden(&mut cmd);
    let out = cmd.output().map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let text: String = text.trim().chars().take(2000).collect();
    // Installed with winget: Claude Code leaves the update to winget, so Qanuvi asks winget (the owner pressed Update).
    if text.contains("winget upgrade") {
        return winget_upgrade();
    }
    if out.status.success() {
        Ok(text)
    } else {
        Err(if text.is_empty() {
            "The update did not finish.".into()
        } else {
            text
        })
    }
}

/// Updates a Claude Code that winget installed, the way Claude Code itself asks for. Agreements are not accepted on
/// the owner's behalf: when winget needs that, the answer says to run the command in a terminal.
fn winget_upgrade() -> Result<String, String> {
    let mut cmd = Command::new("winget");
    cmd.args([
        "upgrade",
        "--id",
        "Anthropic.ClaudeCode",
        "--exact",
        "--silent",
    ]);
    hidden(&mut cmd);
    let out = cmd.output().map_err(|e| {
        format!("winget could not start ({e}). Run: winget upgrade Anthropic.ClaudeCode")
    })?;
    let text: String = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.chars().all(|c| r"-\|/█▒ ".contains(c)))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let text: String = text.chars().take(1500).collect();
    if out.status.success() {
        Ok(text)
    } else {
        Err(format!(
            "winget did not finish the update. Run this in a terminal: winget upgrade Anthropic.ClaudeCode
{text}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_compares_versions() {
        assert_eq!(version_in("2.1.302 (Claude Code)"), "2.1.302");
        assert_eq!(version_in("Claude Code 2.1.322"), "2.1.322");
        assert!(is_newer("2.1.322", "2.1.302"));
        assert!(is_newer("2.2.0", "2.1.999"));
        assert!(!is_newer("2.1.302", "2.1.302"));
        assert!(!is_newer("2.1.300", "2.1.302"));
    }

    #[test]
    fn reads_the_proxy_address_and_the_auto_choice() {
        assert_eq!(
            base_url_in(r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:8787"}}"#).as_deref(),
            Some("http://127.0.0.1:8787")
        );
        assert_eq!(base_url_in("{}"), None);
        assert_eq!(base_url_in("not json"), None);
        // Tool search turned off by a Headroom setup is given back past Headroom; the user's own "false" is kept.
        assert!(tool_search_off_for_headroom_in(
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:8787/p/x","ENABLE_TOOL_SEARCH":"false"}}"#
        ));
        assert!(!tool_search_off_for_headroom_in(
            r#"{"env":{"ENABLE_TOOL_SEARCH":"false"}}"#
        ));
        assert!(!tool_search_off_for_headroom_in(
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:8787","ENABLE_TOOL_SEARCH":"true"}}"#
        ));
        assert!(!tool_search_off_for_headroom_in("not json"));
        let on = serde_json::json!({"updates": {"claudeAuto": true}});
        assert!(auto_wanted(on.as_object().unwrap()));
        assert!(!auto_wanted(&serde_json::Map::new()));
    }
}
