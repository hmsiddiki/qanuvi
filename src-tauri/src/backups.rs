//! Automatic backups and restore (1d). Once a day (and on demand) Qanuvi copies its data into a folder the user
//! chooses (another drive is best): a consistent copy of the database taken while Qanuvi runs (`VACUUM INTO` from a
//! second, read-only connection, so the app never waits), the other files of the data folder (core records, agent
//! histories, MCP configs, the device file) and the window's own settings (WebView2's Local Storage). The last 7
//! daily and manual backups are kept; a copy made before a restore stays until the user removes it.
//!
//! A restore is booked in the pointer file and applied at the next start, before the database and the window open
//! (nothing is in use then). The data in use is backed up first, as a "before restore" copy beside the others.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG: &str = "backups.json";
const MANIFEST: &str = "backup.json";
const PREFIX: &str = "qanuvi-backup-";
pub const KEEP: usize = 7;
const DAY: u64 = 24 * 60 * 60;

/// Files of the data folder that are never copied: the database (copied its own way) and its side files, the pointer,
/// locks, the window's storage (copied apart), folders that are rebuilt or only temporary, and a master's service
/// file, restart mark and lock (its port and token are made new at each start; the lock is held while it runs), and
/// the machine's lasting id (a backup restored on another computer must not make it the same machine), nor a
/// server's kept account sign-in (a secret; a restored copy must not sign another machine in). bk1: nor test builds'
/// output (gigabytes of build files, made again by the next build), downloaded updates, or the window's web caches.
const SKIP: [&str; 18] = [
    "test-builds",
    "updates",
    "WebKitCache",
    "CacheStorage",
    "qanuvi.db",
    "qanuvi.db-wal",
    "qanuvi.db-shm",
    "qanuvi.db-journal",
    "data-location.json",
    CONFIG,
    "webview",
    "tmp",
    "browser-profiles",
    "master.json",
    "master-restarting",
    "master.lock",
    crate::machine::ID_FILE,
    crate::account::KEPT_FILE,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Where backups go; None = the default folder (`default_dir`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PathBuf>,
    /// Daily backups switched off by the user.
    #[serde(default)]
    pub off: bool,
    /// Seconds since 1970 of the last good backup.
    #[serde(default)]
    pub last_at: u64,
    /// Why the last try failed (cleared by the next good one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Daily,
    Manual,
    BeforeRestore,
}

/// What a backup folder holds (`backup.json` inside it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub created: u64,
    pub kind: Kind,
    pub version: String,
    /// The data folder it was taken from.
    pub from: PathBuf,
    pub files: u64,
    pub bytes: u64,
    /// Whether the window's settings are in it.
    pub window: bool,
}

/// A backup as listed in Settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    #[serde(flatten)]
    pub manifest: Manifest,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `2026-11-12-083005` (UTC) for a time in seconds since 1970: names sort in time order.
pub fn stamp(secs: u64) -> String {
    let days = (secs / DAY) as i64;
    let rest = secs % DAY;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The default backups folder: "Qanuvi backups" in the user's Documents (a test build: "Qanuvi backups (test build)"),
/// outside the data folder, so removing the app's data never takes the backups with it.
pub fn default_dir(test_build: bool) -> PathBuf {
    documents(if test_build {
        "Qanuvi backups (test build)"
    } else {
        "Qanuvi backups"
    })
}

/// A master's default backups folder (24z3 D3): on like the desktop app's, in the same Documents folder, under its
/// own name, so a master on the same computer as a desktop app never lists or prunes the desktop's backups.
/// Windows and macOS: Documents/"Qanuvi master backups", where people look there. Linux and other systems, usually a
/// server without a desktop: ~/qanuvi-backups, in sight in the home folder and easy to copy off the machine (rsync,
/// scp), with no Documents folder made on a server. Decided by the system only, so the default never moves.
pub fn master_default_dir(test_build: bool) -> PathBuf {
    master_default_for(cfg!(any(windows, target_os = "macos")), test_build)
}

fn master_default_for(desktop_system: bool, test_build: bool) -> PathBuf {
    match (desktop_system, test_build) {
        (true, false) => documents("Qanuvi master backups"),
        (true, true) => documents("Qanuvi master backups (test build)"),
        (false, false) => home().join("qanuvi-backups"),
        (false, true) => home().join("qanuvi-backups-test-build"),
    }
}

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn documents(name: &str) -> PathBuf {
    home().join("Documents").join(name)
}

pub fn read_config(data: &Path) -> Config {
    std::fs::read_to_string(data.join(CONFIG))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_config(data: &Path, c: &Config) -> Result<(), String> {
    let text = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    std::fs::write(data.join(CONFIG), text).map_err(|e| format!("backups.json: {e}"))
}

/// The folder backups go to for this data folder: the chosen one, else `default` (the desktop app's
/// `default_dir`). A master's is `master_default_dir`
/// (24z3 D3); None (tests only) means no backups until a folder is chosen.
pub fn folder(data: &Path, default: Option<&Path>) -> Option<PathBuf> {
    read_config(data)
        .dir
        .or_else(|| default.map(Path::to_path_buf))
}

/// Sets the backups folder (empty: the default one) and whether daily backups
/// run. A chosen folder must be a full path outside the data folder; it is made when missing.
pub fn set(data: &Path, folder: &str, off: bool) -> Result<(), String> {
    let mut c = read_config(data);
    let folder = folder.trim();
    c.dir = if folder.is_empty() {
        None
    } else {
        let p = PathBuf::from(folder);
        if !p.is_absolute() {
            return Err("Choose a full folder path.".into());
        }
        if p.starts_with(data) {
            return Err("The backups folder cannot be inside Qanuvi's data folder.".into());
        }
        std::fs::create_dir_all(&p)
            .map_err(|e| format!("{} cannot be created: {e}", p.display()))?;
        Some(p)
    };
    c.off = off;
    write_config(data, &c)
}

/// "Back up now". Answers the new backup's folder.
pub fn now_backup(
    data: &Path,
    webview: Option<&Path>,
    default: Option<&Path>,
) -> Result<String, String> {
    run_due(data, webview, default, true)?
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "No backup was made.".to_string())
}

/// Books `backup` (one in this machine's backups folder) to be restored at the next start.
pub fn book_restore(
    data: &Path,
    base: &Path,
    default: Option<&Path>,
    backup: &Path,
) -> Result<(), String> {
    let dest = folder(data, default).ok_or("No backups folder is chosen.")?;
    plan_restore(base, &dest, backup)
}

/// Removes one backup from this machine's backups folder.
pub fn remove_one(data: &Path, default: Option<&Path>, backup: &Path) -> Result<(), String> {
    let dest = folder(data, default).ok_or("No backups folder is chosen.")?;
    remove(&dest, backup)
}

/// What Settings → Backups shows for one machine.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub data: String,
    /// Where backups go (empty only without a default, in tests).
    pub dir: String,
    /// This machine's default folder.
    pub default_dir: String,
    pub custom: bool,
    pub off: bool,
    pub last_at: u64,
    pub last_error: Option<String>,
    pub keep: usize,
    pub restore: Option<String>,
    /// What the last restore did (kept in the pointer file until the next one).
    pub note: Option<String>,
    pub backups: Vec<Entry>,
}

/// Backups facts for the data folder `data` whose pointer file is in `base`.
pub fn info(data: &Path, base: &Path, default: Option<&Path>) -> Info {
    let c = read_config(data);
    let dir = folder(data, default);
    let loc = crate::data_location::read(base);
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let note = loc
        .note
        .filter(|n| n.starts_with("Restored") || n.starts_with("The backup was not restored"));
    Info {
        data: s(data),
        dir: dir.as_deref().map(s).unwrap_or_default(),
        default_dir: default.map(s).unwrap_or_default(),
        custom: c.dir.is_some(),
        off: c.off,
        last_at: c.last_at,
        last_error: c.last_error,
        keep: KEEP,
        restore: loc.restore.as_deref().map(s),
        note,
        backups: dir.as_deref().map(list).unwrap_or_default(),
    }
}

/// Where the window keeps its settings inside its storage folder (WebView2).
fn local_storage(webview: &Path) -> PathBuf {
    webview
        .join("EBWebView")
        .join("Default")
        .join("Local Storage")
}

fn copy_dir(from: &Path, to: &Path, skip: &[&str], tally: &mut (u64, u64)) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) || name.to_string_lossy().ends_with(".lock") {
            continue;
        }
        let (src, dst) = (e.path(), to.join(&name));
        let Ok(t) = e.file_type() else { continue };
        let result = if t.is_dir() {
            copy_dir(&src, &dst, &[], tally)
        } else if t.is_file() {
            std::fs::copy(&src, &dst).map(|n| {
                tally.0 += 1;
                tally.1 += n;
            })
        } else {
            Ok(())
        };
        match result {
            // Gone meanwhile (a helper tidying up), or held open with no sharing (a lock file): left out.
            Err(err)
                if err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(32) =>
            {
                continue;
            }
            other => other?,
        }
    }
    Ok(())
}

/// A consistent copy of the database, taken from a read-only connection (WAL: the app keeps writing meanwhile).
fn copy_database(db: &Path, to: &Path) -> Result<(), String> {
    use rusqlite::{Connection, OpenFlags};
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("the database could not be opened: {e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    conn.execute("VACUUM INTO ?1", [to.to_string_lossy()])
        .map_err(|e| format!("the database could not be copied: {e}"))?;
    // The copy must open and pass SQLite's own check.
    let copy = Connection::open_with_flags(to, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("the copied database does not open: {e}"))?;
    let ok: String = copy
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if ok != "ok" {
        return Err(format!("the copied database failed its check: {ok}"));
    }
    Ok(())
}

/// Makes one backup of `data` (and the window's settings in `webview`) in `dest`. Answers the new backup's folder.
pub fn make(
    data: &Path,
    webview: Option<&Path>,
    dest: &Path,
    kind: Kind,
) -> Result<PathBuf, String> {
    if dest.starts_with(data) {
        return Err("The backups folder cannot be inside Qanuvi's data folder.".into());
    }
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("{} cannot be created: {e}", dest.display()))?;
    let created = now();
    let mut name = format!("{PREFIX}{}", stamp(created));
    if kind == Kind::BeforeRestore {
        name.push_str("-before-restore");
    }
    let mut target = dest.join(&name);
    let mut n = 2;
    while target.exists() {
        target = dest.join(format!("{name}-{n}"));
        n += 1;
    }
    // Built under a hidden name, renamed when whole: a half-made backup is never listed or restored.
    let work = dest.join(format!(
        ".partial-{}",
        target.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&work);
    let result = (|| {
        let mut tally = (0u64, 0u64);
        copy_dir(data, &work.join("data"), &SKIP, &mut tally)
            .map_err(|e| format!("copying {} failed: {e}", data.display()))?;
        let db = data.join("qanuvi.db");
        if db.exists() {
            let to = work.join("data").join("qanuvi.db");
            copy_database(&db, &to)?;
            tally.0 += 1;
            tally.1 += std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
        }
        let mut window = false;
        if let Some(web) = webview.map(local_storage).filter(|p| p.exists()) {
            copy_dir(
                &web,
                &work.join("window").join("Local Storage"),
                &["LOCK"],
                &mut tally,
            )
            .map_err(|e| format!("copying the window's settings failed: {e}"))?;
            window = true;
        }
        let m = Manifest {
            created,
            kind: kind.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            from: data.to_path_buf(),
            files: tally.0,
            bytes: tally.1,
            window,
        };
        let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
        std::fs::write(work.join(MANIFEST), text).map_err(|e| e.to_string())?;
        std::fs::rename(&work, &target).map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&work);
        return Err(e);
    }
    Ok(target)
}

/// The backups in a folder, newest first. Folders without a readable manifest are not Qanuvi backups.
pub fn list(dest: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(dest)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(PREFIX))
        .filter_map(|e| {
            let path = e.path();
            let manifest: Manifest =
                serde_json::from_str(&std::fs::read_to_string(path.join(MANIFEST)).ok()?).ok()?;
            Some(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path,
                manifest,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.manifest
            .created
            .cmp(&a.manifest.created)
            .then(b.name.cmp(&a.name))
    });
    out
}

/// Keeps the newest `keep` daily and manual backups; before-restore copies stay until the user removes them.
pub fn prune(dest: &Path, keep: usize) {
    for old in list(dest)
        .into_iter()
        .filter(|e| e.manifest.kind != Kind::BeforeRestore)
        .skip(keep)
    {
        let _ = std::fs::remove_dir_all(&old.path);
    }
}

/// Removes one backup the user chose (the window asked first). Only a Qanuvi backup in the backups folder.
pub fn remove(dest: &Path, path: &Path) -> Result<(), String> {
    if !list(dest).iter().any(|e| e.path == path) {
        return Err("That is not a backup in the backups folder.".into());
    }
    std::fs::remove_dir_all(path)
        .map_err(|e| format!("{} could not be removed: {e}", path.display()))
}

/// The daily backup: made when backups are on and the last good one is a day old. Records the outcome.
pub fn run_due(
    data: &Path,
    webview: Option<&Path>,
    default: Option<&Path>,
    force: bool,
) -> Result<Option<PathBuf>, String> {
    let mut c = read_config(data);
    if !force && (c.off || now().saturating_sub(c.last_at) < DAY) {
        return Ok(None);
    }
    let Some(dest) = folder(data, default) else {
        // A master with no folder chosen: nothing to do, and "Back up now" says why.
        return if force {
            Err("Choose a backups folder on this machine first.".into())
        } else {
            Ok(None)
        };
    };
    let kind = if force { Kind::Manual } else { Kind::Daily };
    let result = make(data, webview, &dest, kind);
    // Read again: the window may have changed the settings while the copy ran.
    let mut fresh = read_config(data);
    match &result {
        Ok(_) => {
            fresh.last_at = now();
            fresh.last_error = None;
            prune(&dest, KEEP);
        }
        Err(e) => fresh.last_error = Some(e.clone()),
    }
    c = fresh;
    let _ = write_config(data, &c);
    result.map(Some)
}

/// Checks a backup and books it to be restored at the next start.
pub fn plan_restore(base: &Path, dest: &Path, backup: &Path) -> Result<(), String> {
    if !list(dest).iter().any(|e| e.path == backup) {
        return Err("That is not a backup in the backups folder.".into());
    }
    if !backup.join("data").is_dir() {
        return Err("That backup has no data in it.".into());
    }
    let mut loc = crate::data_location::read(base);
    if loc.pending.is_some() {
        return Err("A move of the data folder is booked; cancel it first.".into());
    }
    loc.restore = Some(backup.to_path_buf());
    loc.note = None;
    crate::data_location::write(base, &loc)
}

/// Takes away the note about the last restore (a note about a moved data folder stays: it is shown elsewhere).
pub fn dismiss_note(base: &Path) -> Result<(), String> {
    let mut loc = crate::data_location::read(base);
    if !loc
        .note
        .as_deref()
        .is_some_and(|n| n.starts_with("Restored") || n.starts_with("The backup was not restored"))
    {
        return Ok(());
    }
    loc.note = None;
    crate::data_location::write(base, &loc)
}

pub fn cancel_restore(base: &Path) -> Result<(), String> {
    let mut loc = crate::data_location::read(base);
    loc.restore = None;
    crate::data_location::write(base, &loc)
}

/// Removes everything in the data folder but the pointer, the backups settings, the machine's id and a backups folder
/// inside it.
fn clear_data(data: &Path) {
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        let name = e.file_name();
        if name == "data-location.json" || name == CONFIG || name == crate::machine::ID_FILE {
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

/// At start-up, after the data folder is known and before the database and the window open: applies a booked
/// restore. The data in use is backed up first; if that fails, nothing is changed. Answers a note for the user.
pub fn apply_pending(
    base: &Path,
    data: &Path,
    webview: Option<&Path>,
    default: Option<&Path>,
) -> Option<String> {
    let mut loc = crate::data_location::read(base);
    let backup = loc.restore.take()?;
    let note = (|| {
        let dest = folder(data, default).ok_or_else(|| {
            "no backups folder is chosen, so the data in use could not be backed up first"
                .to_string()
        })?;
        let src = backup.join("data");
        if !src.is_dir() {
            return Err(format!("{} is not there any more", backup.display()));
        }
        let kept = make(data, webview, &dest, Kind::BeforeRestore)
            .map_err(|e| format!("the data in use could not be backed up first ({e})"))?;
        clear_data(data);
        let mut tally = (0, 0);
        copy_dir(&src, data, &[], &mut tally).map_err(|e| {
            format!(
                "copying the backup failed ({e}); the data in use is in {}",
                kept.display()
            )
        })?;
        let win = backup.join("window").join("Local Storage");
        if let Some(web) = webview.map(local_storage)
            && win.is_dir()
        {
            let _ = std::fs::remove_dir_all(&web);
            copy_dir(&win, &web, &[], &mut tally).map_err(|e| {
                format!(
                    "copying the window's settings failed ({e}); the data in use is in {}",
                    kept.display()
                )
            })?;
        }
        Ok(format!(
            "Restored the backup {}. The data from before is kept as {}.",
            backup.file_name().unwrap_or_default().to_string_lossy(),
            kept.file_name().unwrap_or_default().to_string_lossy()
        ))
    })()
    .unwrap_or_else(|e: String| format!("The backup was not restored: {e}."));
    loc.note = Some(note.clone());
    let _ = crate::data_location::write(base, &loc);
    Some(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_master_default_folder_follows_the_system() {
        let h = home();
        // Windows and macOS: Documents; Linux and other systems (servers): ~/qanuvi-backups.
        assert_eq!(
            master_default_for(true, false),
            h.join("Documents").join("Qanuvi master backups")
        );
        assert_eq!(
            master_default_for(true, true),
            h.join("Documents")
                .join("Qanuvi master backups (test build)")
        );
        assert_eq!(master_default_for(false, false), h.join("qanuvi-backups"));
        assert_eq!(
            master_default_for(false, true),
            h.join("qanuvi-backups-test-build")
        );
        // Never the desktop app's folder, on any system.
        for (d, t) in [(true, false), (true, true), (false, false), (false, true)] {
            assert_ne!(master_default_for(d, t), default_dir(false));
            assert_ne!(master_default_for(d, t), default_dir(true));
        }
    }

    /// A data folder with a database in WAL mode; the connection is answered so the caller decides how long it stays
    /// open (while it is, the rows are still in the WAL, as in a running Qanuvi).
    fn data_with_db(dir: &Path, rows: &[&str]) -> rusqlite::Connection {
        std::fs::create_dir_all(dir.join("mcp")).unwrap();
        let conn = rusqlite::Connection::open(dir.join("qanuvi.db")).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS t (v TEXT)")
            .unwrap();
        for r in rows {
            conn.execute("INSERT INTO t VALUES (?1)", [r]).unwrap();
        }
        std::fs::write(dir.join("brain.json"), b"{\"models\":{}}").unwrap();
        std::fs::write(dir.join("mcp/a.json"), b"{}").unwrap();
        std::fs::write(dir.join("brain.lock"), b"").unwrap();
        conn
    }

    fn rows(db: &Path) -> Vec<String> {
        let c = rusqlite::Connection::open(db).unwrap();
        let mut s = c.prepare("SELECT v FROM t").unwrap();
        s.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn stamps_sort_in_time_order() {
        assert_eq!(stamp(0), "1970-01-01-000000");
        assert_eq!(stamp(1_794_472_205), "2026-11-12-083005");
        assert_eq!(stamp(951_782_400), "2000-02-29-000000");
    }

    #[test]
    fn a_backup_copies_the_open_database_and_the_window_settings() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let web = root.path().join("web");
        let _open = data_with_db(&data, &["a", "b"]);
        std::fs::create_dir_all(local_storage(&web).join("leveldb")).unwrap();
        std::fs::write(local_storage(&web).join("leveldb/000003.log"), b"settings").unwrap();
        let dest = root.path().join("backups");
        // bk1: a test build's output is left out (it is made again), a folder of the same name deeper in is not.
        std::fs::create_dir_all(data.join("test-builds/p1/out")).unwrap();
        std::fs::write(data.join("test-builds/p1/out/app"), b"build").unwrap();
        std::fs::create_dir_all(data.join("mcp/test-builds")).unwrap();
        std::fs::write(data.join("mcp/test-builds/x"), b"kept").unwrap();

        let b = make(&data, Some(&web), &dest, Kind::Manual).unwrap();
        assert!(!b.join("data/test-builds").exists());
        assert!(b.join("data/mcp/test-builds/x").exists());
        assert_eq!(
            rows(&b.join("data/qanuvi.db")),
            ["a", "b"],
            "rows still in the WAL are in the copy"
        );
        assert!(!b.join("data/qanuvi.db-wal").exists());
        assert!(b.join("data/brain.json").exists() && b.join("data/mcp/a.json").exists());
        assert!(!b.join("data/brain.lock").exists());
        assert!(b.join("window/Local Storage/leveldb/000003.log").exists());
        let l = list(&dest);
        assert_eq!(l.len(), 1);
        assert!(l[0].manifest.window && l[0].manifest.files >= 4);

        // Never inside the data folder.
        assert!(make(&data, None, &data.join("b"), Kind::Manual).is_err());
    }

    #[test]
    fn daily_backups_run_once_a_day_and_keep_seven() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        drop(data_with_db(&data, &["a"]));
        let dest = root.path().join("backups");
        write_config(
            &data,
            &Config {
                dir: Some(dest.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        assert!(run_due(&data, None, None, false).unwrap().is_some());
        assert!(
            run_due(&data, None, None, false).unwrap().is_none(),
            "not again the same day"
        );
        // Ten more (as if on ten days): only the newest seven stay, a before-restore copy stays too.
        make(&data, None, &dest, Kind::BeforeRestore).unwrap();
        for _ in 0..10 {
            let mut c = read_config(&data);
            c.last_at = 0;
            write_config(&data, &c).unwrap();
            run_due(&data, None, None, false).unwrap().unwrap();
        }
        let l = list(&dest);
        assert_eq!(
            l.iter().filter(|e| e.manifest.kind == Kind::Daily).count(),
            KEEP
        );
        assert_eq!(
            l.iter()
                .filter(|e| e.manifest.kind == Kind::BeforeRestore)
                .count(),
            1
        );

        // Switched off: no backup, but "back up now" still works.
        let mut c = read_config(&data);
        c.off = true;
        c.last_at = 0;
        write_config(&data, &c).unwrap();
        assert!(run_due(&data, None, None, false).unwrap().is_none());
        assert!(run_due(&data, None, None, true).unwrap().is_some());

        // A folder that cannot be made: the error is recorded for the window.
        let file = root.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        write_config(
            &data,
            &Config {
                dir: Some(file.join("sub")),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(run_due(&data, None, None, false).is_err());
        assert!(read_config(&data).last_error.is_some());
    }

    #[test]
    fn a_restore_happens_at_the_next_start_and_keeps_the_data_from_before() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base");
        let data = base.clone();
        let web = root.path().join("web");
        let dest = root.path().join("backups");
        drop(data_with_db(&data, &["old"]));
        std::fs::create_dir_all(local_storage(&web)).unwrap();
        std::fs::write(local_storage(&web).join("s"), b"old settings").unwrap();
        write_config(
            &data,
            &Config {
                dir: Some(dest.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let backup = make(&data, Some(&web), &dest, Kind::Manual).unwrap();

        // Later: more rows, other settings, a new file.
        {
            let c = rusqlite::Connection::open(data.join("qanuvi.db")).unwrap();
            c.execute("INSERT INTO t VALUES ('new')", []).unwrap();
        }
        std::fs::write(local_storage(&web).join("s"), b"new settings").unwrap();
        std::fs::write(data.join("later.json"), b"{}").unwrap();

        // Only a listed backup can be booked.
        assert!(plan_restore(&base, &dest, &root.path().join("nope")).is_err());
        plan_restore(&base, &dest, &backup).unwrap();
        let note = apply_pending(&base, &data, Some(&web), None).unwrap();
        assert!(note.starts_with("Restored"), "{note}");
        assert_eq!(rows(&data.join("qanuvi.db")), ["old"]);
        assert!(!data.join("later.json").exists());
        assert_eq!(
            std::fs::read(local_storage(&web).join("s")).unwrap(),
            b"old settings"
        );
        assert_eq!(
            read_config(&data).dir.as_deref(),
            Some(dest.as_path()),
            "backup settings stay"
        );
        assert!(crate::data_location::read(&base).restore.is_none());
        assert!(
            apply_pending(&base, &data, Some(&web), None).is_none(),
            "once"
        );

        // The data from before is a backup of its own, and can be restored back.
        let before = list(&dest)
            .into_iter()
            .find(|e| e.manifest.kind == Kind::BeforeRestore)
            .unwrap();
        assert_eq!(rows(&before.path.join("data/qanuvi.db")), ["old", "new"]);
        plan_restore(&base, &dest, &before.path).unwrap();
        apply_pending(&base, &data, Some(&web), None).unwrap();
        assert_eq!(rows(&data.join("qanuvi.db")), ["old", "new"]);
        assert!(data.join("later.json").exists());
        assert_eq!(
            std::fs::read(local_storage(&web).join("s")).unwrap(),
            b"new settings"
        );
    }

    #[test]
    fn a_machine_without_a_default_backs_up_only_once_a_folder_is_chosen() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("master");
        drop(data_with_db(&data, &["a"]));
        std::fs::write(data.join("master.json"), b"{\"token\":\"t\"}").unwrap();
        let id = crate::machine::lasting_id(&data);

        // No folder chosen: the daily check does nothing, "Back up now" says why, the page shows no folder.
        assert!(run_due(&data, None, None, false).unwrap().is_none());
        assert!(run_due(&data, None, None, true).is_err());
        let i = info(&data, &data, None);
        assert_eq!((i.dir.as_str(), i.default_dir.as_str()), ("", ""));
        assert!(i.backups.is_empty());

        // A folder that is not a full path, or inside the data, is refused.
        assert!(set(&data, "backups", false).is_err());
        assert!(set(&data, &data.join("b").to_string_lossy(), false).is_err());
        let dest = root.path().join("backups");
        set(&data, &dest.to_string_lossy(), false).unwrap();
        let made = run_due(&data, None, None, false).unwrap().unwrap();
        assert!(made.join("data/qanuvi.db").exists());
        assert!(
            !made.join("data/master.json").exists(),
            "the service file stays out"
        );
        assert!(
            !made.join("data").join(crate::machine::ID_FILE).exists(),
            "the machine's id stays out"
        );
        let i = info(&data, &data, None);
        assert_eq!(i.backups.len(), 1);
        assert!(i.custom);

        // A restore on the machine itself (pointer file in its own data folder).
        plan_restore(&data, &dest, &made).unwrap();
        let note = apply_pending(&data, &data, None, None).unwrap();
        assert!(note.starts_with("Restored"), "{note}");
        assert!(info(&data, &data, None).note.is_some());
        // The restore keeps the machine's id; Dismiss takes the note away.
        assert_eq!(crate::machine::lasting_id(&data), id);
        dismiss_note(&data).unwrap();
        assert!(info(&data, &data, None).note.is_none());
        // A note about a moved data folder is not this page's to dismiss.
        let mut loc = crate::data_location::read(&data);
        loc.note = Some("Qanuvi's data moved to x.".into());
        crate::data_location::write(&data, &loc).unwrap();
        dismiss_note(&data).unwrap();
        assert!(crate::data_location::read(&data).note.is_some());

        // Back to no folder: off again; a booked restore cannot keep the data first, so nothing changes.
        plan_restore(&data, &dest, &made).unwrap();
        set(&data, "", false).unwrap();
        let note = apply_pending(&data, &data, None, None).unwrap();
        assert!(note.starts_with("The backup was not restored"), "{note}");
        assert_eq!(rows(&data.join("qanuvi.db")), ["a"]);
    }
}
