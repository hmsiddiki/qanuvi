//! The keys of a machine's relay pairings (migration M3b): the secret each paired computer shares with this machine.
//! A desktop build keeps them in the system's credential store (`relay:<pairing>`, as This PC always did). A server
//! build has no credential store, so it keeps them in `pairing-keys.json` in its data folder, readable by its user
//! only (written private first, then filled, then moved into place). Keys never go back to a window.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn name(pairing: &str) -> String {
    format!("relay:{pairing}")
}

/// A pairing id as the relay gives it (letters, digits, - and _), so it is a safe key name.
fn check(pairing: &str) -> Result<(), String> {
    if pairing.is_empty()
        || pairing.len() > 100
        || !pairing
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("That is not a pairing id.".into());
    }
    Ok(())
}

/// The key file of a server build.
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join("pairing-keys.json")
}

fn read_file(file: &Path) -> BTreeMap<String, String> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

#[cfg(unix)]
fn private(file: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn private(_file: &Path) -> Result<(), String> {
    Ok(())
}

fn write_file(file: &Path, all: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, "").map_err(|e| e.to_string())?;
    private(&tmp)?;
    let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}

/// Reads a pairing's key from a key file.
pub fn get_in(file: &Path, pairing: &str) -> Option<String> {
    read_file(file).remove(&name(pairing))
}

/// Keeps a pairing's key in a key file.
pub fn set_in(file: &Path, pairing: &str, key: &str) -> Result<(), String> {
    check(pairing)?;
    let mut all = read_file(file);
    all.insert(name(pairing), key.to_string());
    write_file(file, &all)
}

/// Forgets a pairing's key in a key file (nothing to forget is fine).
pub fn delete_in(file: &Path, pairing: &str) -> Result<(), String> {
    let mut all = read_file(file);
    if all.remove(&name(pairing)).is_some() {
        write_file(file, &all)?;
    }
    Ok(())
}

fn file() -> PathBuf {
    file_in(&crate::paths::data_dir())
}

/// This machine's key for a pairing.
pub fn get(pairing: &str) -> Result<String, String> {
    check(pairing)?;
    let key = if cfg!(feature = "desktop") {
        crate::secrets::store::read(&name(pairing))
    } else {
        get_in(&file(), pairing)
    };
    key.ok_or_else(|| "This machine's pairing is missing; pair again.".to_string())
}

/// Keeps this machine's key for a pairing.
pub fn set(pairing: &str, key: &str) -> Result<(), String> {
    check(pairing)?;
    if cfg!(feature = "desktop") {
        crate::secrets::store::write(&name(pairing), key)
    } else {
        set_in(&file(), pairing, key)
    }
    .map_err(|e| format!("The pairing could not be kept: {e}"))
}

/// Forgets this machine's key for a pairing.
pub fn delete(pairing: &str) -> Result<(), String> {
    check(pairing)?;
    if cfg!(feature = "desktop") {
        crate::secrets::store::delete(&name(pairing))
    } else {
        delete_in(&file(), pairing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_keeps_and_forgets_pairings() {
        let dir = tempfile::tempdir().unwrap();
        let f = file_in(dir.path());
        assert_eq!(get_in(&f, "p1"), None);
        set_in(&f, "p1", "k1").unwrap();
        set_in(&f, "p-2", "k2").unwrap();
        assert_eq!(get_in(&f, "p1").as_deref(), Some("k1"));
        delete_in(&f, "p1").unwrap();
        delete_in(&f, "never").unwrap();
        assert_eq!(get_in(&f, "p1"), None);
        assert_eq!(get_in(&f, "p-2").as_deref(), Some("k2"));
        assert!(!f.with_extension("tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&f).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn refuses_odd_pairing_ids() {
        let dir = tempfile::tempdir().unwrap();
        let f = file_in(dir.path());
        for bad in ["", "a b", "../x", "a:b"] {
            assert!(set_in(&f, bad, "k").is_err(), "{bad}");
            assert!(get(bad).is_err(), "{bad}");
        }
    }
}
