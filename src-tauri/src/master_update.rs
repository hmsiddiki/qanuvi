//! Masters update themselves, part one (migration M8a): every master looks for a newer version on the same signed
//! update channel the app reads (`latest.json`), on its own, window or not. It says so in `qanuvi-master status`, at
//! the start of every terminal command, and to a window through `update_status`.
//!
//! The channel lists one version and a build per kind of machine: the app installer under Tauri's own key
//! (`windows-x86_64`), which This PC's master installs (both the master and the screen, M8d), and a master build under
//! `master-<os>-<arch>` for every other master. Every build is signed with the update key; [`verify`] refuses a file
//! whose signature does not match the public key in tauri.conf.json (used by the download, M8b).
//!
//! Part two (M8b): `qanuvi-master update` asks the running master to put the update in ([`Install`]): it downloads the
//! build beside the program, checks its signature, starts it once to ask its version, waits while work runs (and
//! says so), swaps it in with the old one kept as `.prev`, and restarts the service from the new one.

use crate::claude_update::is_newer;
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// The key a downloaded build is checked against: the app's own, or for a test channel on this machine only
/// (`QANUVI_UPDATES_URL` at localhost or 127.0.0.1) a throwaway test key in `QANUVI_UPDATE_TEST_KEY`. A channel
/// anywhere else is always checked against the app's key.
pub fn trusted_key() -> String {
    trusted_key_for(
        &channel_url(),
        std::env::var("QANUVI_UPDATE_TEST_KEY").ok().as_deref(),
    )
}

fn trusted_key_for(channel: &str, test_key: Option<&str>) -> String {
    let local = [
        "http://127.0.0.1:",
        "http://localhost:",
        "http://127.0.0.1/",
        "http://localhost/",
    ]
    .iter()
    .any(|start| channel.starts_with(start));
    match test_key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(k) if local => k.to_string(),
        _ => public_key(),
    }
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
    /// M8b: putting an update in, while it goes on (and how the last one ended when it failed).
    pub install: Install,
}

/// Where putting an update in is. `stage`: "" (nothing), "downloading", "checking", "waiting" (for running work,
/// `waiting_for` says which), "restarting", or "failed" (`error` says why; nothing was changed).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Install {
    pub stage: String,
    pub version: String,
    pub done: u64,
    pub total: u64,
    pub waiting_for: String,
    pub error: String,
    /// The program being updated (its `.prev` is kept beside it).
    pub program: String,
}

impl Install {
    pub fn under_way(&self) -> bool {
        matches!(
            self.stage.as_str(),
            "downloading" | "checking" | "waiting" | "restarting"
        )
    }
}

type Busy = Box<dyn Fn() -> Option<String> + Send>;
type Waits = Box<dyn Fn(&str, &str) + Send>;
type Restart = Box<dyn FnOnce(&Path) -> Result<(), String> + Send>;

/// What putting an update in needs from the master it runs in.
pub struct InstallHooks {
    /// The running work, in words ("1 plan run"), or None when nothing works.
    pub busy: Busy,
    /// The first time it has to wait (for an Inbox note): the version and what it waits for.
    pub waits: Waits,
    /// Waiting is over (the note goes).
    pub waited: Box<dyn Fn() + Send>,
    /// After the swap: restart the service from this program (the new one).
    pub restart: Restart,
}

/// How often running work is looked at while an update waits for it.
pub const BUSY_EVERY: Duration = Duration::from_secs(5);

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

    fn set_install(&self, change: impl FnOnce(&mut Install)) {
        change(&mut self.0.lock().unwrap().install);
    }

    /// M8b: puts the update the last look found in, on a thread of its own (the answer goes out at once; `update_status`
    /// follows it). Refused when there is nothing to put in; asked again while one is under way, it changes nothing.
    pub fn install(
        self: &std::sync::Arc<Self>,
        exe: PathBuf,
        hooks: InstallHooks,
    ) -> Result<UpdateState, String> {
        let found = {
            let mut s = self.0.lock().unwrap();
            if s.install.under_way() {
                return Ok(s.clone());
            }
            let found = s.available.clone().ok_or_else(|| {
                format!("{} is the newest; there is nothing to update.", s.current)
            })?;
            if found.url.is_empty() {
                return Err(format!(
                    "{} is out, but has no build for this machine yet.",
                    found.version
                ));
            }
            s.install = Install {
                stage: "downloading".into(),
                version: found.version.clone(),
                program: exe.display().to_string(),
                ..Install::default()
            };
            found
        };
        let me = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = me.put_in(&found, &exe, hooks) {
                me.set_install(|i| {
                    i.stage = "failed".into();
                    i.waiting_for.clear();
                    i.error = e;
                });
            }
        });
        Ok(self.state())
    }

    fn put_in(&self, found: &Found, exe: &Path, hooks: InstallHooks) -> Result<(), String> {
        let new = prepare(
            found,
            exe,
            &trusted_key(),
            |done, total| {
                self.set_install(|i| {
                    i.done = done;
                    i.total = total;
                })
            },
            || self.set_install(|i| i.stage = "checking".into()),
        )?;
        // Running work is never cut off: the swap waits until nothing works, and says so once.
        let mut told = false;
        while let Some(what) = (hooks.busy)() {
            self.set_install(|i| {
                i.stage = "waiting".into();
                i.waiting_for = what.clone();
            });
            if !told {
                (hooks.waits)(&found.version, &what);
                told = true;
            }
            std::thread::sleep(BUSY_EVERY);
        }
        if told {
            (hooks.waited)();
        }
        self.set_install(|i| {
            i.stage = "restarting".into();
            i.waiting_for.clear();
        });
        swap_in(exe, &new)?;
        (hooks.restart)(exe).inspect_err(|_| {
            // The service could not start again from the new one: the old one goes back, so a later start finds it.
            let _ = swap_back(exe);
        })
    }
}

/// The new build's place while it is checked (beside the program, so the swap is one rename on the same disk).
pub fn new_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    exe.with_file_name(name)
}

/// The program before the last update, kept to go back to.
pub fn prev_path(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".prev");
    exe.with_file_name(name)
}

/// Downloads `found` beside `exe`, checks its signature against `key` and starts it once to ask its version. Answers
/// the checked file; on any failure it is removed and nothing else has changed.
pub fn prepare(
    found: &Found,
    exe: &Path,
    key: &str,
    progress: impl FnMut(u64, u64),
    checking: impl FnOnce(),
) -> Result<PathBuf, String> {
    let new = new_path(exe);
    let result = (|| {
        let bytes = download(&found.url, progress)?;
        checking();
        verify(&bytes, &found.signature, key)?;
        std::fs::write(&new, &bytes)
            .map_err(|e| format!("The update could not be saved beside the program: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        let said = version_of(&new)?;
        if said != found.version {
            return Err(format!(
                "The downloaded build says it is {said}, not {}, so it was refused.",
                found.version
            ));
        }
        Ok(new.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&new);
    }
    result
}

/// The largest build a download takes.
const MAX_BUILD: u64 = 300 << 20;

/// Downloads a build, telling (bytes so far, bytes in all; 0 when the server does not say).
fn download(url: &str, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, String> {
    let mut res = agent(Duration::from_secs(30 * 60))
        .get(url)
        .call()
        .map_err(|e| format!("The update could not be downloaded: {e}"))?;
    let status = res.status().as_u16();
    if status != 200 {
        return Err(format!("The update's download answered {status}."));
    }
    let total = res
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    if total > MAX_BUILD {
        return Err(
            "The update's download is far larger than a Qanuvi build, so it was refused.".into(),
        );
    }
    let mut reader = res.body_mut().with_config().limit(MAX_BUILD).reader();
    let mut bytes = Vec::with_capacity(total as usize);
    let mut chunk = vec![0u8; 64 * 1024];
    let mut told = Instant::now();
    progress(0, total);
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("The update's download broke off: {e}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        if told.elapsed() >= Duration::from_millis(200) {
            progress(bytes.len() as u64, total);
            told = Instant::now();
        }
    }
    if total > 0 && bytes.len() as u64 != total {
        return Err("The update's download broke off before its end.".into());
    }
    progress(bytes.len() as u64, total.max(bytes.len() as u64));
    Ok(bytes)
}

/// Starts a build once with `version` and answers what it prints (a build that does not start on this machine, for
/// example one made for another system library, is caught here, before it replaces anything).
pub fn version_of(program: &Path) -> Result<String, String> {
    let mut cmd = std::process::Command::new(program);
    cmd.arg("version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("The downloaded build does not start on this machine: {e}"))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let mut out = String::new();
            if let Some(mut o) = child.stdout.take() {
                let _ = o.read_to_string(&mut out);
            }
            if !status.success() {
                return Err(
                    "The downloaded build does not start on this machine, so it was refused."
                        .into(),
                );
            }
            return Ok(out.trim().to_string());
        }
        if started.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "The downloaded build did not answer when it was started, so it was refused."
                    .into(),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Swaps the checked build in: the program becomes `.prev` (the one before goes), the new one takes its name. A
/// running program can be renamed on Windows and Linux alike; if the second step fails, the first is undone.
pub fn swap_in(exe: &Path, new: &Path) -> Result<(), String> {
    let prev = prev_path(exe);
    if prev.exists() {
        std::fs::remove_file(&prev).map_err(|e| {
            format!(
                "The kept program before ({}) could not be removed: {e}",
                prev.display()
            )
        })?;
    }
    std::fs::rename(exe, &prev)
        .map_err(|e| format!("The program could not be moved aside for the update: {e}"))?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&prev, exe);
        let _ = std::fs::remove_file(new);
        return Err(format!(
            "The update could not take the program's place: {e}"
        ));
    }
    Ok(())
}

/// Puts the kept `.prev` back in the program's place (the update's own undo).
fn swap_back(exe: &Path) -> Result<(), String> {
    let prev = prev_path(exe);
    let failed = new_path(exe);
    let _ = std::fs::remove_file(&failed);
    std::fs::rename(exe, &failed).map_err(|e| e.to_string())?;
    std::fs::rename(&prev, exe).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&failed);
    Ok(())
}

/// Whether this master looks by itself (`QANUVI_NO_UPDATE_CHECK=1` turns it off, for tests and throwaway masters).
pub fn checks_by_itself() -> bool {
    std::env::var("QANUVI_NO_UPDATE_CHECK").map_or(true, |v| v.trim().is_empty() || v == "0")
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build(),
    )
}

fn fetch(url: &str) -> Result<String, String> {
    if url.is_empty() {
        return Err("This build has no update address.".into());
    }
    let mut res = agent(Duration::from_secs(30))
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

/// M8b: what a terminal following an update prints when its stage changes (download progress aside).
pub fn install_line(install: &Value) -> Option<String> {
    let version = install["version"].as_str().unwrap_or("");
    match install["stage"].as_str().unwrap_or("") {
        "downloading" => Some(format!("Downloading {version}.")),
        "checking" => {
            Some("Checking its signature, and that it starts on this machine.".to_string())
        }
        "waiting" => Some(format!(
            "Waiting: {} working. The update goes in as soon as it ends; nothing is cut off. (Ctrl+C leaves; the master still puts it in then, and its Inbox has a note.)",
            is_or_are(install["waitingFor"].as_str().unwrap_or("work"))
        )),
        "restarting" => Some("Swapping it in and restarting the service.".to_string()),
        _ => None,
    }
}

/// "1 plan run is" / "2 agent sessions and 1 plan run are", for a sentence.
pub fn is_or_are(what: &str) -> String {
    let one = (what.starts_with("1 ") && !what.contains(" and ")) || what == "work";
    format!("{what} {}", if one { "is" } else { "are" })
}

/// Download progress for a terminal: "45% of 10.2 MB" (or the megabytes so far when the size is not known).
pub fn progress_text(done: u64, total: u64) -> String {
    let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
    match (done * 100).checked_div(total) {
        Some(pct) => format!("{}% of {}", pct.min(100), mb(total)),
        None => mb(done),
    }
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

    /// A one-shot local web server answering every request with `body` (the update's download in these tests).
    fn serve(body: &'static [u8]) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let mut s = stream;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            }
        });
        format!("http://127.0.0.1:{port}/build")
    }

    #[test]
    fn a_test_key_counts_only_for_a_channel_on_this_machine() {
        let app = public_key();
        assert_eq!(
            trusted_key_for("http://127.0.0.1:8123/latest.json", Some(TEST_KEY)),
            TEST_KEY
        );
        assert_eq!(
            trusted_key_for("http://localhost/u/latest.json", Some(TEST_KEY)),
            TEST_KEY
        );
        assert_eq!(
            trusted_key_for("https://qanuvi.com/latest.json", Some(TEST_KEY)),
            app
        );
        assert_eq!(
            trusted_key_for("http://127.0.0.1.example.com/latest.json", Some(TEST_KEY)),
            app
        );
        assert_eq!(
            trusted_key_for("http://127.0.0.1:8123/latest.json", None),
            app
        );
        assert_eq!(
            trusted_key_for("http://127.0.0.1:8123/latest.json", Some("  ")),
            app
        );
    }

    #[test]
    fn the_swap_keeps_the_program_before_and_can_go_back() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("qanuvi-master");
        std::fs::write(&exe, "0.2.0").unwrap();
        std::fs::write(prev_path(&exe), "0.1.0").unwrap();
        std::fs::write(new_path(&exe), "0.3.0").unwrap();
        swap_in(&exe, &new_path(&exe)).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "0.3.0");
        assert_eq!(std::fs::read_to_string(prev_path(&exe)).unwrap(), "0.2.0");
        assert!(!new_path(&exe).exists());
        swap_back(&exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "0.2.0");
        assert!(!prev_path(&exe).exists() && !new_path(&exe).exists());
        // No new file: nothing moves.
        assert!(swap_in(&exe, &new_path(&exe)).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "0.2.0");
    }

    #[test]
    fn a_download_is_refused_on_a_wrong_signature_or_when_it_does_not_start_and_leaves_nothing() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("qanuvi-master");
        std::fs::write(&exe, "running").unwrap();
        let found = Found {
            version: "0.9.0".into(),
            notes: String::new(),
            url: serve(TEST_FILE),
            signature: TEST_SIG.into(),
        };
        let mut seen = (0, 0);
        let mut checked = false;
        // Signed by the test key, but not by the app's key.
        let e = prepare(
            &found,
            &exe,
            &public_key(),
            |a, b| seen = (a, b),
            || checked = true,
        )
        .unwrap_err();
        assert!(e.contains("signature is wrong"), "{e}");
        assert!(checked);
        assert_eq!(seen, (TEST_FILE.len() as u64, TEST_FILE.len() as u64));
        assert!(!new_path(&exe).exists());
        // The right key, but the file is no program: refused at its start, before anything is swapped.
        let e = prepare(&found, &exe, TEST_KEY, |_, _| {}, || {}).unwrap_err();
        assert!(e.contains("does not start"), "{e}");
        assert!(!new_path(&exe).exists());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "running");
    }

    #[test]
    fn install_needs_an_update_with_a_build_for_this_machine() {
        let hooks = || InstallHooks {
            busy: Box::new(|| None),
            waits: Box::new(|_, _| {}),
            waited: Box::new(|| {}),
            restart: Box::new(|_| Ok(())),
        };
        let u = std::sync::Arc::new(Updates::default());
        let e = u.install(PathBuf::from("x"), hooks()).unwrap_err();
        assert!(e.contains("nothing to update"), "{e}");
        u.0.lock().unwrap().available = Some(Found {
            version: "0.9.0".into(),
            ..Found::default()
        });
        let e = u.install(PathBuf::from("x"), hooks()).unwrap_err();
        assert!(e.contains("no build for this machine"), "{e}");
    }

    #[test]
    fn an_update_asked_twice_runs_once_and_a_refused_one_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("qanuvi-master");
        std::fs::write(&exe, "running").unwrap();
        let u = std::sync::Arc::new(Updates::default());
        u.0.lock().unwrap().available = Some(Found {
            version: "0.9.0".into(),
            notes: String::new(),
            url: serve(TEST_FILE),
            signature: TEST_SIG.into(),
        });
        let restarted = std::sync::Arc::new(Mutex::new(false));
        let r2 = restarted.clone();
        let hooks = InstallHooks {
            busy: Box::new(|| None),
            waits: Box::new(|_, _| {}),
            waited: Box::new(|| {}),
            restart: Box::new(move |_| {
                *r2.lock().unwrap() = true;
                Ok(())
            }),
        };
        let first = u.install(exe.clone(), hooks).unwrap();
        assert_eq!(first.install.stage, "downloading");
        // Asked again while under way: nothing new starts.
        let again = InstallHooks {
            busy: Box::new(|| None),
            waits: Box::new(|_, _| {}),
            waited: Box::new(|| {}),
            restart: Box::new(|_| Ok(())),
        };
        assert!(u.install(exe.clone(), again).unwrap().install.under_way());
        // Signed by the test key only, so (the app's key in a test) it fails before any waiting or swap.
        let started = Instant::now();
        while u.state().install.under_way() && started.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let end = u.state().install;
        assert_eq!(end.stage, "failed");
        assert!(end.error.contains("signature is wrong"), "{}", end.error);
        assert!(!*restarted.lock().unwrap());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "running");
    }

    #[test]
    fn update_lines_for_a_terminal() {
        let i = |stage: &str, waiting: &str| json!({"stage": stage, "version": "0.9.0", "waitingFor": waiting});
        assert_eq!(
            install_line(&i("downloading", "")).unwrap(),
            "Downloading 0.9.0."
        );
        assert!(
            install_line(&i("waiting", "1 plan run"))
                .unwrap()
                .starts_with("Waiting: 1 plan run is working.")
        );
        assert!(
            install_line(&i("waiting", "2 agent sessions and 1 plan run"))
                .unwrap()
                .starts_with("Waiting: 2 agent sessions and 1 plan run are working.")
        );
        assert_eq!(install_line(&i("failed", "")), None);
        assert_eq!(progress_text(5 << 20, 10 << 20), "50% of 10.0 MB");
        assert_eq!(progress_text(3 << 19, 0), "1.5 MB");
    }
}
