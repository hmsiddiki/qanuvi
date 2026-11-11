//! Qanuvi's data folder (database, MCP configs, browser state), set once at start-up.

use std::path::PathBuf;
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

/// The data folder; before it is set (in tests), a folder in the temp directory.
pub fn data_dir() -> PathBuf {
    DATA_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| std::env::temp_dir().join("qanuvi-data"))
}

/// The default data folder of a master without a window: `$XDG_DATA_HOME/qanuvi` or `~/.local/share/qanuvi` (on
/// Windows, `%APPDATA%\\qanuvi-master`).
pub fn default_master_dir() -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
            .join("qanuvi-master");
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("qanuvi");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/qanuvi")
}
