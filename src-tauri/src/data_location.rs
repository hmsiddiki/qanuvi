//! Where Qanuvi keeps its data (15e): any folder on any drive, not only the system drive. A small pointer file stays in
//! the default place (`data-location.json`) and names the folder. Moving happens at the next start, before the
//! database and the window open (nothing is in use then): copy everything, check the copy, switch. The old copy is
//! kept until the user removes it.
//!
//! What moves: the database (projects, sessions, memory), the core's records, agent histories, MCP configs, browser
//! state, the device file, and (Windows) the window's own storage (settings, guides, layout) in `webview/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The app's identifier (tauri.conf.json); the default folders are named after it.
pub const IDENTIFIER: &str = "dev.qanuvi.desktop";
const POINTER: &str = "data-location.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// The data folder; None = the default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
    /// Move the data here at the next start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<PathBuf>,
    /// Left behind by the last move, until the user removes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<PathBuf>,
    /// The old copy of the window's storage (Windows), removed with `old`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_webview: Option<PathBuf>,
    /// A backup to restore at the next start (1d, `backups::apply_pending`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restore: Option<PathBuf>,
    /// What happened at the last start (a finished or failed move or restore), shown once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The two default places: the data folder (what Tauri calls the app data dir) and, on Windows, the folder holding
/// the window's own storage. Passed around so tests never touch the real ones.
#[derive(Debug, Clone)]
pub struct Defaults {
    pub base: PathBuf,
    pub webview: Option<PathBuf>,
}

/// Whether this Qanuvi runs from a build in the source tree (`target/debug` or `target/release`): such a test build
/// keeps its own data, so trying a change never touches the installed copy's projects, sessions and settings.
/// QANUVI_SHARED_DATA=1 turns this off.
pub fn from_source() -> bool {
    if std::env::var_os("QANUVI_SHARED_DATA").is_some() {
        return false;
    }
    std::env::current_exe().is_ok_and(|exe| from_source_path(&exe))
}

/// The rule behind `from_source`: the program sits in a `target/debug` or `target/release` folder.
pub fn from_source_path(exe: &Path) -> bool {
    let parts: Vec<String> = exe
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    parts
        .windows(2)
        .any(|w| w[0] == "target" && (w[1] == "debug" || w[1] == "release"))
}

/// The name of the data folders: the identifier, with `.source` for a test build.
pub fn folder_name(source: bool) -> String {
    if source {
        format!("{IDENTIFIER}.source")
    } else {
        IDENTIFIER.to_string()
    }
}

/// A test build's first start: its folders begin as a copy of the installed copy's data (from wherever that data
/// lives), so it has the projects and settings to try things on. Answers a note for the user, or None when there was
/// nothing to do. A copy that fails leaves an empty start, which works too.
pub fn seed_from_installed(test: &Defaults, installed: &Defaults) -> Option<String> {
    if test.base.exists() || !installed.base.exists() {
        return None;
    }
    let from = read(&installed.base)
        .dir
        .unwrap_or_else(|| installed.base.clone());
    let data = copy_checked(&from, &test.base);
    let web = match (installed.webview_for(&from), &test.webview) {
        (Some(a), Some(b)) => copy_checked(&a, b),
        _ => Ok(()),
    };
    // The installed copy's pointer (a custom data folder) is not this build's: this build stays in its own folder.
    let _ = std::fs::remove_file(test.base.join(POINTER));
    // Nor its backups settings: a test build backs up into its own folder, never pruning the installed copy's backups.
    let _ = std::fs::remove_file(test.base.join("backups.json"));
    Some(match data.and(web) {
        Ok(()) => format!(
            "This test build keeps its own data, started as a copy of {}.",
            from.display()
        ),
        Err(e) => format!(
            "This test build keeps its own data; copying the installed copy's data failed ({e})."
        ),
    })
}

impl Defaults {
    pub fn real() -> Self {
        Self::named(&folder_name(from_source()))
    }

    /// The installed copy's folders (also when this is a test build).
    pub fn installed() -> Self {
        Self::named(IDENTIFIER)
    }

    fn named(name: &str) -> Self {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
        let base = if cfg!(windows) {
            env("APPDATA").join(name)
        } else if cfg!(target_os = "macos") {
            env("HOME").join("Library/Application Support").join(name)
        } else {
            let xdg = env("XDG_DATA_HOME");
            let root = if xdg.as_os_str().is_empty() {
                env("HOME").join(".local/share")
            } else {
                xdg
            };
            root.join(name)
        };
        let webview = cfg!(windows).then(|| env("LOCALAPPDATA").join(name));
        Defaults { base, webview }
    }

    /// The window's storage for a data folder: the default place for the default folder, else `webview/` inside it.
    pub fn webview_for(&self, dir: &Path) -> Option<PathBuf> {
        let web = self.webview.as_ref()?;
        Some(if dir == self.base {
            web.clone()
        } else {
            dir.join("webview")
        })
    }
}

pub fn read(base: &Path) -> Location {
    std::fs::read_to_string(base.join(POINTER))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write(base: &Path, loc: &Location) -> Result<(), String> {
    std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(loc).map_err(|e| e.to_string())?;
    std::fs::write(base.join(POINTER), text)
        .map_err(|e| format!("The pointer file could not be written: {e}"))
}

/// Files and bytes under a folder (to check a copy), leaving out the pointer file.
fn tally(dir: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == POINTER) {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    let (f, b) = tally(&p);
                    files += f;
                    bytes += b;
                }
                Ok(t) if t.is_file() => {
                    files += 1;
                    bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    (files, bytes)
}

/// Cache folders of the window's engine: rebuilt by itself, and still changing for a few seconds after a restart.
const SKIP: [&str; 10] = [
    "Cache",
    "Code Cache",
    "GPUCache",
    "DawnCache",
    "DawnGraphiteCache",
    "DawnWebGPUCache",
    "GrShaderCache",
    "ShaderCache",
    "Crashpad",
    "component_crx_cache",
];

fn skipped(name: &std::ffi::OsStr) -> bool {
    name == POINTER || SKIP.iter().any(|s| name == *s)
}

/// Copies a folder. A file that disappears while copying (a helper process tidying up) is left out.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        if skipped(&e.file_name()) {
            continue;
        }
        let src = e.path();
        let dst = to.join(e.file_name());
        let Ok(t) = e.file_type() else { continue };
        let result = if t.is_dir() {
            copy_dir(&src, &dst)
        } else if t.is_file() {
            std::fs::copy(&src, &dst).map(|_| ())
        } else {
            Ok(())
        };
        match result {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            other => other?,
        }
    }
    Ok(())
}

/// Every file still in `from` (caches left out) is in `to` with the same size. Answers the first one that is not.
fn missing(from: &Path, to: &Path) -> Option<PathBuf> {
    for e in std::fs::read_dir(from).ok()?.flatten() {
        if skipped(&e.file_name()) {
            continue;
        }
        let (src, dst) = (e.path(), to.join(e.file_name()));
        match e.file_type() {
            Ok(t) if t.is_dir() => {
                if let Some(m) = missing(&src, &dst) {
                    return Some(m);
                }
            }
            Ok(t) if t.is_file() => {
                // Gone from the source meanwhile: nothing to check.
                let Ok(a) = std::fs::metadata(&src) else {
                    continue;
                };
                if std::fs::metadata(&dst).map(|b| b.len()).ok() != Some(a.len()) {
                    return Some(src);
                }
            }
            _ => {}
        }
    }
    None
}

/// Copies a folder and checks the copy, trying again a few times while helper processes finish.
fn copy_checked(from: &Path, to: &Path) -> Result<(), String> {
    if !from.exists() {
        return Ok(());
    }
    let mut last = String::new();
    for attempt in 0..4 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        match copy_dir(from, to) {
            Err(e) => last = format!("copying {} failed: {e}", from.display()),
            Ok(()) => match missing(from, to) {
                None => return Ok(()),
                Some(p) => last = format!("{} was not copied whole", p.display()),
            },
        }
    }
    Err(last)
}

/// Removes everything in a folder but the pointer file.
fn clear_except_pointer(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name() == POINTER {
                continue;
            }
            let p = e.path();
            let _ = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
        }
    }
}

/// At start-up, before the database and the window: finishes a pending move, and answers the data folder.
pub fn resolve(d: &Defaults) -> PathBuf {
    let base = d.base.as_path();
    let mut loc = read(base);
    if let Some(target) = loc.pending.take() {
        let from = loc.dir.clone().unwrap_or_else(|| base.to_path_buf());
        let from_web = d.webview_for(&from);
        let to_web = d.webview_for(&target);
        // Back into the default folder: it still holds an outdated copy (with SQLite's side files, which must never sit
        // next to a newer database). Empty it first, the pointer excepted; the copy being moved is the current one.
        if target == base && from != base {
            clear_except_pointer(base);
            if let Some(w) = &to_web {
                let _ = std::fs::remove_dir_all(w);
            }
        }
        let result = copy_checked(&from, &target).and_then(|()| match (&from_web, &to_web) {
            (Some(a), Some(b)) => copy_checked(a, b),
            _ => Ok(()),
        });
        match result {
            Ok(()) => {
                // In the default folder the window's storage has its own place: no copy of it inside the data.
                if target == base && from != base {
                    let _ = std::fs::remove_dir_all(base.join("webview"));
                }
                loc.note = Some(format!(
                    "Qanuvi's data moved to {}. The old copy is kept until you remove it.",
                    target.display()
                ));
                loc.old = Some(from);
                loc.old_webview = from_web;
                loc.dir = (target != base).then_some(target);
            }
            Err(e) => {
                // The target was checked empty when the move was booked: only a new folder is cleaned up, never the
                // default one (it holds the pointer and may be the data in use).
                if target != base {
                    let _ = std::fs::remove_dir_all(&target);
                }
                loc.note = Some(format!(
                    "The data was not moved: {e}. Qanuvi kept using {}.",
                    from.display()
                ));
            }
        }
        let _ = write(base, &loc);
    }
    loc.dir.unwrap_or_else(|| base.to_path_buf())
}

/// Waits (at most `limit`) until the process `pid` has ended: a restart must not copy the data while the old
/// Qanuvi still has it open.
#[cfg(windows)]
pub fn wait_for_exit(pid: u32, limit: std::time::Duration) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    // SAFETY: plain calls with a handle that is checked and closed.
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            WaitForSingleObject(h, limit.as_millis().min(u32::MAX as u128) as u32);
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
pub fn wait_for_exit(pid: u32, limit: std::time::Duration) {
    let end = std::time::Instant::now() + limit;
    while std::time::Instant::now() < end {
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success());
        if !alive {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// The process a restart waits for (`--after-pid=1234`; a dash-flag, so never taken for a project folder).
pub fn after_pid(args: impl Iterator<Item = String>) -> Option<u32> {
    args.filter_map(|a| a.strip_prefix("--after-pid=").and_then(|p| p.parse().ok()))
        .next()
}

/// Where a move should go: the chosen folder if it is empty or new, else a "Qanuvi" folder inside it.
pub fn target_for(chosen: &Path) -> PathBuf {
    let empty = std::fs::read_dir(chosen)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true);
    if empty {
        chosen.to_path_buf()
    } else {
        chosen.join("Qanuvi")
    }
}

#[cfg(windows)]
fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut probe = dir.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = probe.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: a null-terminated path and a valid out pointer; the other outputs are optional.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Checks a move to `chosen` and books it for the next start. Answers the folder the data will be in.
pub fn plan_move(d: &Defaults, current: &Path, chosen: &Path) -> Result<PathBuf, String> {
    let base = d.base.as_path();
    if !chosen.is_absolute() {
        return Err("Choose a full folder path.".into());
    }
    let target = if chosen == base {
        base.to_path_buf()
    } else {
        target_for(chosen)
    };
    if target == current {
        return Err("Qanuvi's data is already there.".into());
    }
    if target.starts_with(current) {
        return Err("The new folder cannot be inside the current data folder.".into());
    }
    if target != base && target.exists() && tally(&target).0 > 0 {
        return Err(format!("{} is not empty.", target.display()));
    }
    // Writable?
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("{} cannot be created: {e}", target.display()))?;
    let probe = target.join(".qanuvi-write-test");
    std::fs::write(&probe, b"ok")
        .map_err(|e| format!("{} is not writable: {e}", target.display()))?;
    let _ = std::fs::remove_file(&probe);
    // Room for the copy, with a margin.
    let need = tally(current).1 + d.webview_for(current).map(|w| tally(&w).1).unwrap_or(0);
    if let Some(free) = free_bytes(&target)
        && free < need + need / 5 + 50_000_000
    {
        return Err(format!(
            "That drive has {:.1} GB free; the data needs about {:.1} GB.",
            free as f64 / 1e9,
            need as f64 / 1e9
        ));
    }
    let mut loc = read(base);
    if loc.restore.is_some() {
        return Err("A backup is booked to be restored; cancel that first.".into());
    }
    loc.pending = Some(target.clone());
    loc.note = None;
    write(base, &loc)?;
    Ok(target)
}

/// Cancels a move booked for the next start.
pub fn cancel_move(base: &Path) -> Result<(), String> {
    let mut loc = read(base);
    loc.pending = None;
    write(base, &loc)
}

/// Removes the copy left by the last move (the user confirmed). Never the folder in use.
pub fn remove_old(base: &Path, current: &Path) -> Result<(), String> {
    let mut loc = read(base);
    let old = loc.old.clone().ok_or("There is no old copy.")?;
    if old == current || current.starts_with(&old) || !current.join("qanuvi.db").exists() {
        return Err("The old copy is still needed.".into());
    }
    if old == base {
        // The default folder also holds the pointer: remove everything else in it.
        for e in std::fs::read_dir(&old)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            if e.file_name() != POINTER {
                let p = e.path();
                let _ = if p.is_dir() {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                };
            }
        }
    } else {
        std::fs::remove_dir_all(&old)
            .map_err(|e| format!("{} could not be removed: {e}", old.display()))?;
    }
    if let Some(w) = loc.old_webview.take() {
        let _ = std::fs::remove_dir_all(&w);
    }
    loc.old = None;
    write(base, &loc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_names_the_process_to_wait_for() {
        let a = |v: &[&str]| {
            v.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        };
        assert_eq!(
            after_pid(a(&["qanuvi", "--after-pid=4321", "E:/site"])),
            Some(4321)
        );
        assert_eq!(after_pid(a(&["qanuvi", "E:/site"])), None);
        // A process that already ended is not waited for.
        let t = std::time::Instant::now();
        wait_for_exit(u32::MAX - 7, std::time::Duration::from_secs(5));
        assert!(t.elapsed() < std::time::Duration::from_secs(4));
    }

    #[test]
    fn identifier_matches_the_app_config() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
    }

    #[test]
    fn moves_at_the_next_start_and_keeps_the_old_copy_until_removed() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("default");
        let local = root.path().join("local");
        std::fs::create_dir_all(local.join("EBWebView")).unwrap();
        std::fs::write(local.join("EBWebView/settings"), b"window storage").unwrap();
        let d = Defaults {
            base: base.clone(),
            webview: Some(local.clone()),
        };
        std::fs::create_dir_all(base.join("agent-history")).unwrap();
        std::fs::write(base.join("qanuvi.db"), b"database").unwrap();
        std::fs::write(base.join("agent-history/a.json"), b"[]").unwrap();
        assert_eq!(resolve(&d), base);

        // Refused: inside the current folder, or a folder with other things in it.
        assert!(plan_move(&d, &base, &base.join("sub")).is_err());
        let busy = root.path().join("busy");
        std::fs::create_dir_all(busy.join("Qanuvi")).unwrap();
        std::fs::write(busy.join("Qanuvi/x"), b"x").unwrap();
        assert!(
            plan_move(&d, &base, &busy)
                .unwrap_err()
                .contains("not empty")
        );

        // A drive folder with other things: the data goes into a Qanuvi folder inside it.
        let drive = root.path().join("E");
        std::fs::create_dir_all(&drive).unwrap();
        std::fs::write(drive.join("photos.txt"), b"mine").unwrap();
        let target = plan_move(&d, &base, &drive).unwrap();
        assert_eq!(target, drive.join("Qanuvi"));
        assert_eq!(read(&base).pending.as_deref(), Some(target.as_path()));

        // Next start: copied (window storage too), checked, switched; the old copy kept.
        assert_eq!(resolve(&d), target);
        assert_eq!(
            std::fs::read(target.join("qanuvi.db")).unwrap(),
            b"database"
        );
        assert!(target.join("agent-history/a.json").exists());
        assert_eq!(
            std::fs::read(target.join("webview/EBWebView/settings")).unwrap(),
            b"window storage"
        );
        let loc = read(&base);
        assert!(loc.note.unwrap().contains("moved"));
        assert_eq!(loc.old.as_deref(), Some(base.as_path()));
        assert!(base.join("qanuvi.db").exists());
        assert_eq!(resolve(&d), target, "stays there");

        // Removing the old copy keeps the pointer; the user's other files are never touched.
        remove_old(&base, &target).unwrap();
        assert!(!base.join("qanuvi.db").exists());
        assert!(!local.exists(), "the old window storage went with it");
        assert!(base.join(POINTER).exists());
        assert!(
            !base.join("webview").exists(),
            "the window's storage is not copied into the default data folder"
        );
        assert!(drive.join("photos.txt").exists());
        assert!(remove_old(&base, &target).is_err());

        // Back to the default place, window storage included. An outdated side file left there (as an old copy would
        // have) is cleared, not mixed with the newer database.
        std::fs::write(base.join("qanuvi.db-wal"), b"stale").unwrap();
        std::fs::write(base.join("qanuvi.db"), b"old database").unwrap();
        assert_eq!(plan_move(&d, &target, &base).unwrap(), base);
        assert_eq!(resolve(&d), base);
        assert_eq!(std::fs::read(base.join("qanuvi.db")).unwrap(), b"database");
        assert!(
            !base.join("qanuvi.db-wal").exists(),
            "no stale side file next to the moved database"
        );
        assert!(base.join(POINTER).exists());
        assert_eq!(
            std::fs::read(local.join("EBWebView/settings")).unwrap(),
            b"window storage"
        );
    }
}

#[cfg(test)]
mod test_build_tests {
    use super::*;

    #[test]
    fn a_build_in_the_source_tree_keeps_its_own_data() {
        // Each system's own paths (a backslash separates folders on Windows only).
        #[cfg(windows)]
        {
            assert!(from_source_path(Path::new(
                r"E:\Projects\qanuvi\src-tauri\target\release\qanuvi.exe"
            )));
            assert!(!from_source_path(Path::new(r"D:\qanuvi\qanuvi.exe")));
            assert!(!from_source_path(Path::new(r"C:\target\qanuvi.exe")));
        }
        assert!(from_source_path(Path::new(
            "/home/u/qanuvi/src-tauri/target/debug/qanuvi"
        )));
        assert!(!from_source_path(Path::new("/opt/qanuvi/qanuvi")));
        assert!(!from_source_path(Path::new("/target/qanuvi")));
        assert_eq!(folder_name(true), format!("{IDENTIFIER}.source"));
        assert_eq!(folder_name(false), IDENTIFIER);
    }

    #[test]
    fn the_first_start_copies_the_installed_data_once() {
        let dir = tempfile::tempdir().unwrap();
        let installed = Defaults {
            base: dir.path().join("inst"),
            webview: Some(dir.path().join("inst-web")),
        };
        let test = Defaults {
            base: dir.path().join("test"),
            webview: Some(dir.path().join("test-web")),
        };
        std::fs::create_dir_all(&installed.base).unwrap();
        std::fs::write(installed.base.join("qanuvi.db"), b"db").unwrap();
        std::fs::create_dir_all(installed.webview.as_ref().unwrap()).unwrap();
        std::fs::write(
            installed.webview.as_ref().unwrap().join("Local State"),
            b"x",
        )
        .unwrap();
        let note = seed_from_installed(&test, &installed).unwrap();
        assert!(note.contains("own data"));
        assert_eq!(std::fs::read(test.base.join("qanuvi.db")).unwrap(), b"db");
        assert!(test.webview.as_ref().unwrap().join("Local State").exists());
        // Never again: the test build's data is its own from now on.
        std::fs::write(installed.base.join("qanuvi.db"), b"newer").unwrap();
        assert!(seed_from_installed(&test, &installed).is_none());
        assert_eq!(std::fs::read(test.base.join("qanuvi.db")).unwrap(), b"db");
    }
}
