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
    let path = crate::terminal::find_on_path("claude").ok_or("Claude Code is not installed.")?;
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

/// Runs Claude Code's own update; answers what it printed.
pub fn update() -> Result<String, String> {
    let path = crate::terminal::find_on_path("claude").ok_or("Claude Code is not installed.")?;
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
}
