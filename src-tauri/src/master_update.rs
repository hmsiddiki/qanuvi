//! Masters update themselves, part one (migration M8a): every master looks for a newer version on the same signed
//! update channel the app reads (`latest.json`), on its own, window or not. It says so in `qanuvi-master status`, at
//! the start of every terminal command, and to a window through `update_status`.
//!
//! The channel lists one version and a build per kind of machine: the app installer under Tauri's own key
//! (`windows-x86_64`), which This PC's master installs (both the master and the screen, M8d), and a master build under
//! `master-<os>-<arch>` for every other master. Every build is signed with the update key; [`verify`] refuses a file
//! whose signature does not match the public key in tauri.conf.json (used by the download, M8b).

use crate::claude_update::is_newer;
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

/// The first look a while after the master starts, then one an hour (as the app does).
pub const FIRST_CHECK: Duration = Duration::from_secs(120);
pub const CHECK_EVERY: Duration = Duration::from_secs(3600);

fn conf() -> Value {
    serde_json::from_str(include_str!("../tauri.conf.json")).unwrap_or(Value::Null)
}

/// Where `latest.json` is: the app's own update address, or `QANUVI_UPDATES_URL` (the channel folder's address, as
/// publish-update.mjs takes it) for a test channel.
pub fn channel_url() -> String {
    match std::env::var("QANUVI_UPDATES_URL") {
        Ok(base) if !base.trim().is_empty() => {
            format!("{}/latest.json", base.trim().trim_end_matches('/'))
        }
        _ => conf()["plugins"]["updater"]["endpoints"][0]
            .as_str()
            .unwrap_or("")
            .to_string(),
    }
}

/// The update key's public half (the same one the app checks its updates with).
pub fn public_key() -> String {
    conf()["plugins"]["updater"]["pubkey"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// The channel's name for this machine's build. `this_pc`: This PC's master, which installs the whole app.
pub fn build_key(this_pc: bool) -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    if this_pc {
        format!("{os}-{arch}")
    } else {
        format!("master-{os}-{arch}")
    }
}

/// A newer version on the channel. `url` and `signature` are empty when it has no build for this machine (yet).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub signature: String,
}

/// Reads `latest.json`: the newer version for build `key`, or None when `current` is up to date.
pub fn read_latest(text: &str, current: &str, key: &str) -> Result<Option<Found>, String> {
    let v: Value = serde_json::from_str(text)
        .map_err(|_| "The update channel's latest.json does not read.".to_string())?;
    let version = v["version"]
        .as_str()
        .map(|s| s.trim().trim_start_matches('v'))
        .filter(|s| !s.is_empty())
        .ok_or("The update channel's latest.json names no version.")?;
    if !is_newer(version, current) {
        return Ok(None);
    }
    let mut found = Found {
        version: version.to_string(),
        notes: v["notes"].as_str().unwrap_or("").to_string(),
        ..Found::default()
    };
    let build = &v["platforms"][key];
    if !build.is_null() {
        let url = build["url"].as_str().unwrap_or("").trim();
        let signature = build["signature"].as_str().unwrap_or("").trim();
        if url.is_empty() || signature.is_empty() {
            return Err(format!(
                "The update channel lists a {key} build of {version} without its address or signature."
            ));
        }
        found.url = url.to_string();
        found.signature = signature.to_string();
    }
    Ok(Some(found))
}

/// Checks a downloaded build against its signature (both as the channel keeps them: base64 of minisign's text). A
/// wrong or unreadable signature is refused.
pub fn verify(bytes: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    const WRONG: &str = "The update's signature is wrong, so it was refused.";
    let text = |b64: &str| -> Option<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()?;
        String::from_utf8(raw).ok()
    };
    let key = text(public_key)
        .and_then(|t| minisign_verify::PublicKey::decode(&t).ok())
        .ok_or("The update key in this build does not read.")?;
    let sig = text(signature)
        .and_then(|t| minisign_verify::Signature::decode(&t).ok())
        .ok_or(WRONG)?;
    key.verify(bytes, &sig, true).map_err(|_| WRONG.to_string())
}

/// What this master knows about updates, as `update_status` answers it.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub current: String,
    pub checking: bool,
    pub checked_at: Option<i64>,
    pub available: Option<Found>,
    pub error: String,
}

pub struct Updates(Mutex<UpdateState>);

impl Default for Updates {
    fn default() -> Self {
        Updates(Mutex::new(UpdateState {
            current: env!("CARGO_PKG_VERSION").to_string(),
            ..UpdateState::default()
        }))
    }
}

impl Updates {
    pub fn state(&self) -> UpdateState {
        self.0.lock().unwrap().clone()
    }

    /// Looks on the channel now (the hourly look, or one asked for) and keeps what it found.
    pub fn check(&self, this_pc: bool) -> UpdateState {
        let current = {
            let mut s = self.0.lock().unwrap();
            s.checking = true;
            s.current.clone()
        };
        let result = fetch(&channel_url())
            .and_then(|text| read_latest(&text, &current, &build_key(this_pc)));
        let mut s = self.0.lock().unwrap();
        s.checking = false;
        s.checked_at = Some(crate::inbox::now_ms());
        match result {
            Ok(found) => {
                s.available = found;
                s.error.clear();
            }
            // A failed look keeps what the last one found; it says why beside it.
            Err(e) => s.error = e,
        }
        s.clone()
    }
}

/// Whether this master looks by itself (`QANUVI_NO_UPDATE_CHECK=1` turns it off, for tests and throwaway masters).
pub fn checks_by_itself() -> bool {
    std::env::var("QANUVI_NO_UPDATE_CHECK").map_or(true, |v| v.trim().is_empty() || v == "0")
}

fn fetch(url: &str) -> Result<String, String> {
    if url.is_empty() {
        return Err("This build has no update address.".into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    );
    let mut res = agent
        .get(url)
        .call()
        .map_err(|e| format!("The update channel could not be reached: {e}"))?;
    let status = res.status().as_u16();
    if status != 200 {
        return Err(format!("The update channel answered {status}."));
    }
    res.body_mut()
        .read_to_string()
        .map_err(|e| format!("The update channel's answer did not read: {e}"))
}

/// The line a terminal command starts with when a newer version is out for this machine.
pub fn notice(state: &Value) -> Option<String> {
    let found = &state["available"];
    let version = found["version"].as_str().filter(|v| !v.is_empty())?;
    (!found["url"].as_str().unwrap_or("").is_empty())
        .then(|| format!("Update available: {version}."))
}

/// `qanuvi-master status`'s update line.
pub fn status_line(state: &Value) -> String {
    let current = state["current"].as_str().unwrap_or("?");
    // Inside brackets: its own first letter small and no full stop of its own.
    let raw = state["error"]
        .as_str()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.');
    let mut chars = raw.chars();
    let error = match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    let found = &state["available"];
    let line = match found["version"].as_str().filter(|v| !v.is_empty()) {
        Some(v) if !found["url"].as_str().unwrap_or("").is_empty() => {
            format!("Updates: {v} is available (this is {current}).")
        }
        Some(v) => format!(
            "Updates: {v} is out, but has no build for this machine yet (this is {current})."
        ),
        None if state["checkedAt"].is_null() && error.is_empty() => {
            "Updates: not looked yet (the first look is two minutes after the start).".to_string()
        }
        None if error.is_empty() => format!("Updates: {current} is the newest."),
        None => "Updates:".to_string(),
    };
    if error.is_empty() {
        line
    } else if line == "Updates:" {
        format!("Updates: the last look failed ({error}).")
    } else {
        format!("{line} The last look failed ({error}).")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // A throwaway key pair made for these tests only (tauri signer generate), and one file signed with it.
    const TEST_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIzNjE4RDg3ODhDNjNDQzYKUldUR1BNYUloNDFoc3pQVys4ZjAyOEc5bENEenpxR3V3SGVKay9mb2craGdSczFaWHZyTUdZZjkK";
    const TEST_FILE: &[u8] = b"Qanuvi master test build 0.9.0\n";
    const TEST_SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUR1BNYUloNDFoczZ6L1lYMUJuMnNReS91NDZTUHVDb1E3RHltb1pDUkZ1V0VNM1ZjYUlvT3d3VE9rT0h4NHBFWlFhRmFJbjQzMXVQMGxQT0Q3UXdOaG1EZDVTajhFb0E4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0OTg4NTcwCWZpbGU6YnVpbGQuYmluCjE3bk1JeEZaQWZ4OFlqMXJYMEJwOVVPTUhCZHNtZWFweEp3Z3poZTQrUndWT1FBUEt6WVVyT08xZ1EySHFkOFNpTWFCTTduWCtYRmVUZzFOSDNLNERBPT0K";

    #[test]
    fn a_signed_build_passes_and_a_changed_one_or_another_key_is_refused() {
        assert_eq!(verify(TEST_FILE, TEST_SIG, TEST_KEY), Ok(()));
        let changed = b"Qanuvi master test build 0.9.1\n";
        assert!(
            verify(changed, TEST_SIG, TEST_KEY)
                .unwrap_err()
                .contains("signature is wrong")
        );
        // The real update key did not sign it.
        assert!(
            verify(TEST_FILE, TEST_SIG, &public_key())
                .unwrap_err()
                .contains("signature is wrong")
        );
        assert!(verify(TEST_FILE, "not base64 !", TEST_KEY).is_err());
    }

    #[test]
    fn the_channel_and_key_are_the_apps() {
        assert!(public_key().len() > 40);
        if std::env::var_os("QANUVI_UPDATES_URL").is_none() {
            assert!(channel_url().ends_with("/latest.json"));
        }
        let key = build_key(false);
        assert!(key.starts_with("master-") && !key.contains("macos"));
        assert_eq!(build_key(true), key.trim_start_matches("master-"));
    }

    #[test]
    fn reads_a_newer_version_and_this_machines_build() {
        let latest = json!({
            "version": "0.2.0",
            "notes": "Masters update themselves",
            "platforms": {
                "windows-x86_64": {"url": "https://x/Qanuvi_0.2.0_x64-setup.exe", "signature": "c2lnMQ=="},
                "master-linux-x86_64": {"url": "https://x/qanuvi-master_0.2.0_linux-x86_64", "signature": "c2lnMg=="},
            }
        })
        .to_string();
        let f = read_latest(&latest, "0.1.23", "master-linux-x86_64")
            .unwrap()
            .unwrap();
        assert_eq!(f.version, "0.2.0");
        assert_eq!(f.signature, "c2lnMg==");
        assert!(f.url.ends_with("linux-x86_64"));
        // Up to date, or newer here than on the channel: nothing to do.
        assert_eq!(read_latest(&latest, "0.2.0", "windows-x86_64"), Ok(None));
        assert_eq!(read_latest(&latest, "0.3.1", "windows-x86_64"), Ok(None));
        // Out, but not for this kind of machine yet.
        let mac = read_latest(&latest, "0.1.23", "master-darwin-aarch64")
            .unwrap()
            .unwrap();
        assert_eq!((mac.version.as_str(), mac.url.as_str()), ("0.2.0", ""));
        // A build without its signature, or a file that does not read, is an error, not an update.
        let unsigned = json!({"version": "0.2.0", "platforms": {"windows-x86_64": {"url": "https://x/a.exe"}}});
        assert!(
            read_latest(&unsigned.to_string(), "0.1.0", "windows-x86_64")
                .unwrap_err()
                .contains("without its address or signature")
        );
        assert!(read_latest("<html>", "0.1.0", "windows-x86_64").is_err());
        assert!(read_latest("{}", "0.1.0", "windows-x86_64").is_err());
    }

    #[test]
    fn terminal_lines_say_what_the_last_look_found() {
        let state = |v: Value| serde_json::to_value(v).unwrap();
        let available = state(json!({"current": "0.1.23", "checkedAt": 1,
            "available": {"version": "0.2.0", "url": "https://x/m", "signature": "s"}, "error": ""}));
        assert_eq!(
            notice(&available).as_deref(),
            Some("Update available: 0.2.0.")
        );
        assert_eq!(
            status_line(&available),
            "Updates: 0.2.0 is available (this is 0.1.23)."
        );
        let no_build = state(json!({"current": "0.1.23", "checkedAt": 1,
            "available": {"version": "0.2.0", "url": "", "signature": ""}, "error": ""}));
        assert_eq!(notice(&no_build), None);
        assert!(status_line(&no_build).contains("no build for this machine yet"));
        let newest =
            state(json!({"current": "0.2.0", "checkedAt": 1, "available": null, "error": ""}));
        assert_eq!(notice(&newest), None);
        assert_eq!(status_line(&newest), "Updates: 0.2.0 is the newest.");
        let never =
            state(json!({"current": "0.2.0", "checkedAt": null, "available": null, "error": ""}));
        assert!(status_line(&never).contains("not looked yet"));
        let failed = state(
            json!({"current": "0.2.0", "checkedAt": 1, "available": null,
            "error": "The update channel answered 404."}),
        );
        assert_eq!(
            status_line(&failed),
            "Updates: the last look failed (the update channel answered 404)."
        );
        let kept = state(json!({"current": "0.1.23", "checkedAt": 1,
            "available": {"version": "0.2.0", "url": "https://x/m"}, "error": "timed out"}));
        assert_eq!(
            status_line(&kept),
            "Updates: 0.2.0 is available (this is 0.1.23). The last look failed (timed out)."
        );
    }
}
