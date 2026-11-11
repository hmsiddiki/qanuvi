//! The memory disk (Milestone 18): the user's memory copied, encrypted, to a place of their choice (a folder, which can
//! be a Google Drive, Dropbox or OneDrive folder, a USB disk or a network share; or a folder on their own server over
//! SSH), so their machines share it.
//!
//! Each machine writes only its own file there (`<device>.qxm`, its whole memory with deleted marks), so two machines
//! never write the same file. Syncing reads every machine's file and keeps the newest version of each entry, deletions
//! included. The last versions of a machine's own file are kept (`<device>-<time>.qxv`) and can be restored.
//!
//! Files are encrypted with a key made from the user's passphrase (Argon2id) using XChaCha20-Poly1305; without the
//! passphrase they cannot be read, and any change to them is detected.

use crate::memory::Entry;
use crate::store::{Db, now};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAGIC: &[u8; 4] = b"QXM1";
const SALT: usize = 16;
const NONCE: usize = 24;
/// Versions of a machine's own file kept on the disk.
pub const KEEP_VERSIONS: usize = 10;

// Encryption ----------------------------------------------------------------------------------------------------------

fn key_from(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Could not make a key from the passphrase: {e}"))?;
    Ok(key)
}

/// Encrypts with a new random salt and nonce: MAGIC, salt, nonce, then the sealed text.
pub fn seal(passphrase: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    if passphrase.chars().count() < 8 {
        return Err("The passphrase needs at least 8 characters.".into());
    }
    let mut salt = [0u8; SALT];
    let mut nonce = [0u8; NONCE];
    getrandom::fill(&mut salt).map_err(|e| e.to_string())?;
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key_from(passphrase, &salt)?)
        .map_err(|e| e.to_string())?;
    let sealed = cipher
        .encrypt(&XNonce::from(nonce), plain)
        .map_err(|_| "Encryption failed.".to_string())?;
    let mut out = Vec::with_capacity(4 + SALT + NONCE + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Decrypts; a wrong passphrase or a changed file both fail.
pub fn open(passphrase: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 4 + SALT + NONCE + 16 || &data[..4] != MAGIC {
        return Err("Not a Qanuvi memory file.".into());
    }
    let salt = &data[4..4 + SALT];
    let mut nonce = [0u8; NONCE];
    nonce.copy_from_slice(&data[4 + SALT..4 + SALT + NONCE]);
    let cipher = XChaCha20Poly1305::new_from_slice(&key_from(passphrase, salt)?)
        .map_err(|e| e.to_string())?;
    cipher
        .decrypt(&XNonce::from(nonce), &data[4 + SALT + NONCE..])
        .map_err(|_| "Wrong passphrase, or the file was changed.".into())
}

// What a machine's file holds -----------------------------------------------------------------------------------------

/// An entry as stored on the disk: everything, including the deleted mark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    #[serde(flatten)]
    pub entry: Entry,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub device_id: String,
    pub device_name: String,
    pub written_at: i64,
    pub entries: Vec<Stored>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
}

/// This machine's id on memory disks (made once, kept in the data folder).
pub fn device(data_dir: &Path) -> Device {
    let file = data_dir.join("device.json");
    if let Ok(text) = std::fs::read_to_string(&file)
        && let Ok(d) = serde_json::from_str::<Device>(&text)
        && valid_id(&d.id)
    {
        return d;
    }
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "This computer".into());
    let d = Device {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name,
    };
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(&file, serde_json::to_string(&d).unwrap_or_default());
    d
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Every entry of the memory, deleted ones included (they tell other machines about deletions).
pub fn all(db: &Db) -> Result<Vec<Stored>, String> {
    let conn = db.conn();
    let mut st = conn
        .prepare(
            "SELECT id, kind, scope, title, body, tags, status, source, created_at, updated_at, uses, deleted
             FROM memory ORDER BY created_at",
        )
        .map_err(|e| e.to_string())?;
    st.query_map([], |r| {
        Ok(Stored {
            entry: Entry {
                id: r.get(0)?,
                kind: r.get(1)?,
                scope: r.get(2)?,
                title: r.get(3)?,
                body: r.get(4)?,
                tags: r.get(5)?,
                status: r.get(6)?,
                source: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
                uses: r.get(10)?,
            },
            deleted: r.get::<_, i64>(11)? != 0,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Merged {
    pub added: usize,
    pub changed: usize,
    pub deleted: usize,
}

/// Takes in entries from another copy: an entry that is new here, or newer there, wins (deletions too). Use counts
/// keep the larger number.
pub fn merge(db: &Db, entries: &[Stored]) -> Result<Merged, String> {
    let conn = db.conn();
    let mut m = Merged::default();
    for s in entries {
        let e = &s.entry;
        if !valid_entry(e) {
            continue;
        }
        let local: Option<(i64, i64, i64)> = conn
            .query_row(
                "SELECT updated_at, deleted, uses FROM memory WHERE id = ?1",
                [&e.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match local {
            None => {
                conn.execute(
                    "INSERT INTO memory (id, kind, scope, title, body, tags, status, source, created_at, updated_at,
                     deleted, uses) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        e.id,
                        e.kind,
                        e.scope,
                        e.title,
                        e.body,
                        e.tags,
                        e.status,
                        e.source,
                        e.created_at,
                        e.updated_at,
                        s.deleted as i64,
                        e.uses
                    ],
                )
                .map_err(|e| e.to_string())?;
                if !s.deleted {
                    m.added += 1;
                }
            }
            Some((updated, was_deleted, uses)) if e.updated_at > updated => {
                conn.execute(
                    "UPDATE memory SET kind = ?2, scope = ?3, title = ?4, body = ?5, tags = ?6, status = ?7,
                     source = ?8, updated_at = ?9, deleted = ?10, uses = ?11 WHERE id = ?1",
                    params![
                        e.id,
                        e.kind,
                        e.scope,
                        e.title,
                        e.body,
                        e.tags,
                        e.status,
                        e.source,
                        e.updated_at,
                        s.deleted as i64,
                        uses.max(e.uses)
                    ],
                )
                .map_err(|e| e.to_string())?;
                if s.deleted && was_deleted == 0 {
                    m.deleted += 1;
                } else if !s.deleted {
                    m.changed += 1;
                }
            }
            Some((_, _, uses)) if e.uses > uses => {
                let _ = conn.execute(
                    "UPDATE memory SET uses = ?2 WHERE id = ?1",
                    params![e.id, e.uses],
                );
            }
            Some(_) => {}
        }
    }
    Ok(m)
}

/// Entries from a file are checked like the user's own (a file could have been made by anything).
fn valid_entry(e: &Entry) -> bool {
    e.id.len() <= 64
        && !e.id.is_empty()
        && crate::memory::KINDS.contains(&e.kind.as_str())
        && !e.title.trim().is_empty()
        && e.title.chars().count() <= 200
        && e.body.chars().count() <= 4000
        && e.scope.len() <= 80
        && matches!(e.status.as_str(), "active" | "suggested")
        && matches!(e.source.as_str(), "user" | "agent")
}

// Where the disk is ---------------------------------------------------------------------------------------------------

/// A memory disk: a folder here, or a folder on a server over SSH.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Disk {
    Folder {
        path: String,
    },
    Server {
        /// SSH address, as for Servers.
        target: String,
        #[serde(default)]
        identity: Option<String>,
        /// Folder on the server.
        dir: String,
    },
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !name.starts_with('.')
}

fn valid_remote_dir(dir: &str) -> bool {
    !dir.is_empty()
        && dir.len() <= 300
        && !dir.starts_with('-')
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c))
}

impl Disk {
    fn check(&self) -> Result<(), String> {
        match self {
            Disk::Folder { path } => {
                if path.trim().is_empty() {
                    return Err("Choose a folder for the memory disk.".into());
                }
                Ok(())
            }
            Disk::Server { target, dir, .. } => {
                crate::memory_disk::check_ssh_target(target)?;
                if !valid_remote_dir(dir) {
                    return Err(format!(
                        "\"{dir}\" is not a folder path on the server (letters, digits and / . _ - ~ only)."
                    ));
                }
                Ok(())
            }
        }
    }

    fn ssh(&self, remote: &str, input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let Disk::Server {
            target, identity, ..
        } = self
        else {
            return Err("Not a server disk.".into());
        };
        let mut c = Command::new("ssh");
        if let Some(k) = identity.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            c.arg("-i")
                .arg(expand_home(k))
                .args(["-o", "IdentitiesOnly=yes"]);
        }
        c.args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            target.as_str(),
            remote,
        ])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        let mut child = c.spawn().map_err(|e| format!("SSH could not start: {e}"))?;
        if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
            stdin.write_all(bytes).map_err(|e| e.to_string())?;
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(if err.is_empty() {
                "The server refused the request.".into()
            } else {
                err
            });
        }
        Ok(out.stdout)
    }

    fn remote_dir(&self) -> &str {
        match self {
            Disk::Server { dir, .. } => dir.trim_end_matches('/'),
            Disk::Folder { .. } => "",
        }
    }

    pub fn list(&self) -> Result<Vec<String>, String> {
        self.check()?;
        let names: Vec<String> = match self {
            Disk::Folder { path } => match std::fs::read_dir(expand_home(path)) {
                Ok(rd) => rd
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect(),
                Err(_) => Vec::new(),
            },
            Disk::Server { .. } => {
                let out = self.ssh(
                    &format!("ls -1 {} 2>/dev/null; true", self.remote_dir()),
                    None,
                )?;
                String::from_utf8_lossy(&out)
                    .lines()
                    .map(|l| l.trim().to_string())
                    .collect()
            }
        };
        let mut names: Vec<String> = names.into_iter().filter(|n| valid_name(n)).collect();
        names.sort();
        Ok(names)
    }

    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        if !valid_name(name) {
            return Err("Invalid file name.".into());
        }
        match self {
            Disk::Folder { path } => {
                std::fs::read(expand_home(path).join(name)).map_err(|e| e.to_string())
            }
            Disk::Server { .. } => self.ssh(&format!("cat {}/{name}", self.remote_dir()), None),
        }
    }

    /// Writes a file whole: to a temporary name first, then renamed, so a reader never sees half a file.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !valid_name(name) {
            return Err("Invalid file name.".into());
        }
        match self {
            Disk::Folder { path } => {
                let dir = expand_home(path);
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("The folder {} could not be made: {e}", dir.display()))?;
                let tmp = dir.join(format!("{name}.tmp"));
                std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())
            }
            Disk::Server { .. } => {
                let d = self.remote_dir();
                self.ssh(
                    &format!(
                        "mkdir -p {d} && cat > {d}/{name}.tmp && mv {d}/{name}.tmp {d}/{name}"
                    ),
                    Some(bytes),
                )
                .map(|_| ())
            }
        }
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        if !valid_name(name) {
            return Err("Invalid file name.".into());
        }
        match self {
            Disk::Folder { path } => {
                std::fs::remove_file(expand_home(path).join(name)).map_err(|e| e.to_string())
            }
            Disk::Server { .. } => self
                .ssh(&format!("rm -f {}/{name}", self.remote_dir()), None)
                .map(|_| ()),
        }
    }
}

pub fn check_ssh_target(target: &str) -> Result<(), String> {
    let ok = !target.is_empty()
        && target.len() <= 200
        && !target.starts_with('-')
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@.-_".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!("\"{target}\" is not an SSH address."))
    }
}

fn expand_home(p: &str) -> PathBuf {
    let p = p.trim();
    match p.strip_prefix('~') {
        Some(rest) => {
            let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .unwrap_or_default();
            PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        None => PathBuf::from(p),
    }
}

// Syncing -------------------------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct Report {
    /// Other machines whose memory was read.
    pub devices: Vec<String>,
    pub merged: Merged,
    /// Entries in this machine's file now (without deleted ones).
    pub written: usize,
    pub at: i64,
}

fn own_name(d: &Device) -> String {
    format!("{}.qxm", d.id)
}

/// Reads every machine's file, takes in what is newer, then writes this machine's file (keeping the previous one as
/// a version). Nothing is written when a file cannot be opened with the passphrase.
pub fn sync(db: &Db, disk: &Disk, passphrase: &str, me: &Device) -> Result<Report, String> {
    let names = disk.list()?;
    let mut report = Report {
        at: now(),
        ..Report::default()
    };
    let mut unreadable = Vec::new();
    for name in names.iter().filter(|n| n.ends_with(".qxm")) {
        let snapshot = disk
            .read(name)
            .and_then(|bytes| open(passphrase, &bytes))
            .and_then(|plain| {
                serde_json::from_slice::<Snapshot>(&plain).map_err(|e| e.to_string())
            });
        match snapshot {
            Ok(s) => {
                let m = merge(db, &s.entries)?;
                report.merged.added += m.added;
                report.merged.changed += m.changed;
                report.merged.deleted += m.deleted;
                if s.device_id != me.id {
                    report.devices.push(s.device_name);
                }
            }
            Err(_) => unreadable.push(name.clone()),
        }
    }
    if !unreadable.is_empty() {
        return Err(format!(
            "{} file(s) on the memory disk could not be opened with this passphrase ({}). Nothing was written; use the same passphrase on every machine.",
            unreadable.len(),
            unreadable.join(", ")
        ));
    }
    let entries = all(db)?;
    report.written = entries.iter().filter(|e| !e.deleted).count();
    let snapshot = Snapshot {
        format: 1,
        device_id: me.id.clone(),
        device_name: me.name.clone(),
        written_at: report.at,
        entries,
    };
    let plain = serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?;
    let sealed = seal(passphrase, &plain)?;
    let own = own_name(me);
    // The previous file becomes a version.
    if names.contains(&own)
        && let Ok(previous) = disk.read(&own)
    {
        disk.write(&format!("{}-{}.qxv", me.id, report.at), &previous)?;
    }
    disk.write(&own, &sealed)?;
    prune_versions(disk, me)?;
    Ok(report)
}

fn prune_versions(disk: &Disk, me: &Device) -> Result<(), String> {
    let mut mine: Vec<String> = disk
        .list()?
        .into_iter()
        .filter(|n| n.starts_with(&format!("{}-", me.id)) && n.ends_with(".qxv"))
        .collect();
    mine.sort();
    while mine.len() > KEEP_VERSIONS {
        disk.remove(&mine.remove(0))?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Version {
    pub name: String,
    pub at: i64,
}

/// This machine's kept versions, newest first.
pub fn versions(disk: &Disk, me: &Device) -> Result<Vec<Version>, String> {
    let prefix = format!("{}-", me.id);
    let mut out: Vec<Version> = disk
        .list()?
        .into_iter()
        .filter_map(|n| {
            let at = n
                .strip_prefix(&prefix)?
                .strip_suffix(".qxv")?
                .parse::<i64>()
                .ok()?;
            Some(Version { name: n, at })
        })
        .collect();
    out.sort_by_key(|v| std::cmp::Reverse(v.at));
    Ok(out)
}

/// Makes the memory what it was in a version: its entries come back as they were, entries made since are deleted
/// (with deleted marks, so the other machines follow at the next sync).
pub fn restore(db: &Db, disk: &Disk, passphrase: &str, name: &str) -> Result<usize, String> {
    if !name.ends_with(".qxv") {
        return Err("Choose a version.".into());
    }
    let plain = open(passphrase, &disk.read(name)?)?;
    let snapshot: Snapshot = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
    let t = now();
    let kept: std::collections::HashSet<&str> = snapshot
        .entries
        .iter()
        .filter(|s| !s.deleted)
        .map(|s| s.entry.id.as_str())
        .collect();
    let mut restored: Vec<Stored> = snapshot.entries.clone();
    for s in &mut restored {
        s.entry.updated_at = t;
    }
    for current in all(db)? {
        if !current.deleted && !kept.contains(current.entry.id.as_str()) {
            let mut gone = current.clone();
            gone.deleted = true;
            gone.entry.updated_at = t;
            restored.push(gone);
        }
    }
    merge(db, &restored)?;
    Ok(kept.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory;

    fn entry(title: &str) -> Entry {
        Entry {
            id: String::new(),
            kind: "fact".into(),
            scope: String::new(),
            title: title.into(),
            body: String::new(),
            tags: String::new(),
            status: "active".into(),
            source: "user".into(),
            created_at: 0,
            updated_at: 0,
            uses: 0,
        }
    }

    fn titles(db: &Db) -> Vec<String> {
        let mut t: Vec<String> = memory::list(db, None, true)
            .unwrap()
            .into_iter()
            .map(|e| e.title)
            .collect();
        t.sort();
        t
    }

    #[test]
    fn sealed_files_open_only_with_the_passphrase_and_unchanged() {
        let sealed = seal("correct horse", b"secret memory").unwrap();
        assert_ne!(&sealed[4..], b"secret memory".as_slice());
        assert_eq!(open("correct horse", &sealed).unwrap(), b"secret memory");
        assert!(open("wrong passphrase", &sealed).is_err());
        let mut changed = sealed.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(open("correct horse", &changed).is_err());
        assert!(seal("short", b"x").is_err());
        // Each sealing uses a new salt and nonce.
        assert_ne!(
            seal("correct horse", b"x").unwrap(),
            seal("correct horse", b"x").unwrap()
        );
    }

    #[test]
    fn two_machines_merge_through_a_folder_with_deletions_and_newest_wins() {
        let disk_dir = tempfile::tempdir().unwrap();
        let disk = Disk::Folder {
            path: disk_dir.path().to_string_lossy().to_string(),
        };
        let pc = Db::open_in_memory().unwrap();
        let laptop = Db::open_in_memory().unwrap();
        let pc_dev = Device {
            id: "pc1".into(),
            name: "PC".into(),
        };
        let laptop_dev = Device {
            id: "lap1".into(),
            name: "Laptop".into(),
        };
        let pass = "a long passphrase";

        let shared = memory::save(&pc, &entry("Shared rule")).unwrap();
        let doomed = memory::save(&pc, &entry("To delete")).unwrap();
        sync(&pc, &disk, pass, &pc_dev).unwrap();
        let r = sync(&laptop, &disk, pass, &laptop_dev).unwrap();
        assert_eq!(r.devices, vec!["PC"]);
        assert_eq!(r.merged.added, 2);
        assert_eq!(titles(&laptop), vec!["Shared rule", "To delete"]);

        // The laptop changes one entry and adds one; the PC deletes another, later.
        let mut changed = memory::get(&laptop, &shared.id).unwrap();
        changed.body = "Changed on the laptop".into();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        memory::save(&laptop, &changed).unwrap();
        memory::save(&laptop, &entry("From the laptop")).unwrap();
        sync(&laptop, &disk, pass, &laptop_dev).unwrap();
        memory::delete(&pc, &doomed.id).unwrap();
        let r = sync(&pc, &disk, pass, &pc_dev).unwrap();
        assert_eq!((r.merged.added, r.merged.changed), (1, 1));
        assert_eq!(titles(&pc), vec!["From the laptop", "Shared rule"]);
        assert_eq!(
            memory::get(&pc, &shared.id).unwrap().body,
            "Changed on the laptop"
        );
        let r = sync(&laptop, &disk, pass, &laptop_dev).unwrap();
        assert_eq!(r.merged.deleted, 1, "the deletion reaches the laptop");
        assert_eq!(titles(&laptop), vec!["From the laptop", "Shared rule"]);

        // Only encrypted files on the disk; versions kept.
        let names = disk.list().unwrap();
        assert!(names.contains(&"pc1.qxm".to_string()) && names.contains(&"lap1.qxm".to_string()));
        let raw = std::fs::read(disk_dir.path().join("pc1.qxm")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("Shared rule"));
        assert!(!versions(&disk, &pc_dev).unwrap().is_empty());

        // A wrong passphrase writes nothing.
        let before = std::fs::read(disk_dir.path().join("lap1.qxm")).unwrap();
        assert!(sync(&laptop, &disk, "another passphrase", &laptop_dev).is_err());
        assert_eq!(
            std::fs::read(disk_dir.path().join("lap1.qxm")).unwrap(),
            before
        );
    }

    #[test]
    fn restoring_a_version_brings_back_its_entries_and_removes_newer_ones() {
        let disk_dir = tempfile::tempdir().unwrap();
        let disk = Disk::Folder {
            path: disk_dir.path().to_string_lossy().to_string(),
        };
        let db = Db::open_in_memory().unwrap();
        let me = Device {
            id: "pc1".into(),
            name: "PC".into(),
        };
        let pass = "a long passphrase";
        let a = memory::save(&db, &entry("Kept")).unwrap();
        sync(&db, &disk, pass, &me).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        memory::delete(&db, &a.id).unwrap();
        memory::save(&db, &entry("Added later")).unwrap();
        sync(&db, &disk, pass, &me).unwrap();
        let v = versions(&disk, &me).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(restore(&db, &disk, pass, &v[0].name).unwrap(), 1);
        assert_eq!(titles(&db), vec!["Kept"]);
    }

    #[test]
    fn keeps_only_the_last_versions_and_refuses_odd_names() {
        let disk_dir = tempfile::tempdir().unwrap();
        let disk = Disk::Folder {
            path: disk_dir.path().to_string_lossy().to_string(),
        };
        let me = Device {
            id: "pc1".into(),
            name: "PC".into(),
        };
        for i in 0..(KEEP_VERSIONS + 3) {
            disk.write(&format!("pc1-{}.qxv", 1000 + i), b"x").unwrap();
        }
        prune_versions(&disk, &me).unwrap();
        let v = versions(&disk, &me).unwrap();
        assert_eq!(v.len(), KEEP_VERSIONS);
        assert_eq!(v[0].at, 1000 + KEEP_VERSIONS as i64 + 2);
        assert!(disk.write("../escape", b"x").is_err());
        assert!(disk.read(".hidden").is_err());
        let bad = Disk::Server {
            target: "host".into(),
            identity: None,
            dir: "dir; rm -rf ~".into(),
        };
        assert!(bad.list().is_err());
    }

    /// Syncs two machines through a folder on a real server over SSH. Runs only when QANUVI_TEST_SSH (user@host) is
    /// set, with QANUVI_TEST_SSH_KEY for a key file and QANUVI_TEST_SSH_DIR for the folder.
    #[test]
    fn two_machines_merge_through_a_server_over_ssh() {
        let Ok(target) = std::env::var("QANUVI_TEST_SSH") else {
            return;
        };
        let dir = std::env::var("QANUVI_TEST_SSH_DIR")
            .unwrap_or_else(|_| "qanuvi-test/memory-disk".into());
        let disk = Disk::Server {
            target,
            identity: std::env::var("QANUVI_TEST_SSH_KEY").ok(),
            dir: dir.clone(),
        };
        // A clean folder for the test.
        for n in disk.list().unwrap() {
            disk.remove(&n).unwrap();
        }
        let pass = "a long test passphrase";
        let a = Db::open_in_memory().unwrap();
        let b = Db::open_in_memory().unwrap();
        let da = Device {
            id: "testa".into(),
            name: "Machine A".into(),
        };
        let db_ = Device {
            id: "testb".into(),
            name: "Machine B".into(),
        };
        let one = memory::save(&a, &entry("Written on A")).unwrap();
        let started = std::time::Instant::now();
        sync(&a, &disk, pass, &da).unwrap();
        let r = sync(&b, &disk, pass, &db_).unwrap();
        assert_eq!(
            (r.devices.clone(), r.merged.added),
            (vec!["Machine A".to_string()], 1)
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        memory::delete(&b, &one.id).unwrap();
        memory::save(&b, &entry("Written on B")).unwrap();
        sync(&b, &disk, pass, &db_).unwrap();
        let r = sync(&a, &disk, pass, &da).unwrap();
        assert_eq!((r.merged.added, r.merged.deleted), (1, 1));
        assert_eq!(titles(&a), vec!["Written on B"]);
        eprintln!(
            "server sync: 4 syncs in {:.1} s; files on the server: {:?}",
            started.elapsed().as_secs_f64(),
            disk.list().unwrap()
        );
    }

    /// Acts as a second computer on a real folder: writes its own file with one entry. Runs only when
    /// QANUVI_TEST_FOLDER and QANUVI_TEST_PASS are set (for trying the app's sync by hand).
    #[test]
    fn a_second_computer_writes_to_a_folder() {
        let (Ok(path), Ok(pass)) = (
            std::env::var("QANUVI_TEST_FOLDER"),
            std::env::var("QANUVI_TEST_PASS"),
        ) else {
            return;
        };
        let disk = Disk::Folder { path };
        let db = Db::open_in_memory().unwrap();
        let laptop = Device {
            id: "testlaptop".into(),
            name: "Test laptop".into(),
        };
        let r = sync(&db, &disk, &pass, &laptop).unwrap();
        eprintln!(
            "second computer read {:?}, took in {} entries",
            r.devices, r.merged.added
        );
        memory::save(&db, &entry("Written on the test laptop")).unwrap();
        sync(&db, &disk, &pass, &laptop).unwrap();
    }
}
