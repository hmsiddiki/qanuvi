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
/// locks, the window's storage (copied apart), and folders that are rebuilt or only temporary.
const SKIP: [&str; 9] = [
    "qanuvi.db",
    "qanuvi.db-wal",
    "qanuvi.db-shm",
    "qanuvi.db-journal",
    "data-location.json",
    CONFIG,
    "webview",
    "tmp",
    "browser-profiles",
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
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let name = if test_build {
        "Qanuvi backups (test build)"
    } else {
        "Qanuvi backups"
    };
    home.join("Documents").join(name)
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

/// The folder backups go to for this data folder.
pub fn folder(data: &Path, test_build: bool) -> PathBuf {
    read_config(data)
        .dir
        .unwrap_or_else(|| default_dir(test_build))
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
    test_build: bool,
    force: bool,
) -> Result<Option<PathBuf>, String> {
    let mut c = read_config(data);
    if !force && (c.off || now().saturating_sub(c.last_at) < DAY) {
        return Ok(None);
    }
    let dest = c.dir.clone().unwrap_or_else(|| default_dir(test_build));
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

pub fn cancel_restore(base: &Path) -> Result<(), String> {
    let mut loc = crate::data_location::read(base);
    loc.restore = None;
    crate::data_location::write(base, &loc)
}

/// Removes everything in the data folder but the pointer, the backups settings and a backups folder inside it.
fn clear_data(data: &Path) {
    for e in std::fs::read_dir(data).into_iter().flatten().flatten() {
        let name = e.file_name();
        if name == "data-location.json" || name == CONFIG {
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
    test_build: bool,
) -> Option<String> {
    let mut loc = crate::data_location::read(base);
    let backup = loc.restore.take()?;
    let dest = folder(data, test_build);
    let note = (|| {
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

        let b = make(&data, Some(&web), &dest, Kind::Manual).unwrap();
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

        assert!(run_due(&data, None, false, false).unwrap().is_some());
        assert!(
            run_due(&data, None, false, false).unwrap().is_none(),
            "not again the same day"
        );
        // Ten more (as if on ten days): only the newest seven stay, a before-restore copy stays too.
        make(&data, None, &dest, Kind::BeforeRestore).unwrap();
        for _ in 0..10 {
            let mut c = read_config(&data);
            c.last_at = 0;
            write_config(&data, &c).unwrap();
            run_due(&data, None, false, false).unwrap().unwrap();
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
        assert!(run_due(&data, None, false, false).unwrap().is_none());
        assert!(run_due(&data, None, false, true).unwrap().is_some());

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
        assert!(run_due(&data, None, false, false).is_err());
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
        let note = apply_pending(&base, &data, Some(&web), false).unwrap();
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
            apply_pending(&base, &data, Some(&web), false).is_none(),
            "once"
        );

        // The data from before is a backup of its own, and can be restored back.
        let before = list(&dest)
            .into_iter()
            .find(|e| e.manifest.kind == Kind::BeforeRestore)
            .unwrap();
        assert_eq!(rows(&before.path.join("data/qanuvi.db")), ["old", "new"]);
        plan_restore(&base, &dest, &before.path).unwrap();
        apply_pending(&base, &data, Some(&web), false).unwrap();
        assert_eq!(rows(&data.join("qanuvi.db")), ["old", "new"]);
        assert!(data.join("later.json").exists());
        assert_eq!(
            std::fs::read(local_storage(&web).join("s")).unwrap(),
            b"new settings"
        );
    }
}
