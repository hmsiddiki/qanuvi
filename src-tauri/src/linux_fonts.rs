#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
//! L1: a blank window on Linux. WebKitGTK 2.52 (Ubuntu 24.04, Zorin 18) spins at full CPU before its first frame when
//! fontconfig offers it a WOFF font: Zorin installs fonts-opendyslexic as /usr/share/fonts/woff/..., and every page
//! of every WebKit app there stays empty. Qanuvi's own fonts are inside the app, so the window hides the system's
//! WOFF and WOFF2 fonts from WebKit: a fontconfig file of its own that includes the system's and rejects them. A
//! FONTCONFIG_FILE the user set is kept as it is. What the window starts (This PC's master and through it the user's
//! terminals) gets the user's fonts back (`give_back`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The window's fontconfig file: the system's settings, without web-format fonts. A glob is matched against the
/// font's whole path, so it starts with "/".
pub const CONF: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<!-- Qanuvi's window only: the system's font settings without WOFF/WOFF2 fonts (WebKitGTK hangs on them). -->
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <selectfont>
    <rejectfont>
      <glob>/*.woff</glob>
      <glob>/*.woff2</glob>
    </rejectfont>
  </selectfont>
</fontconfig>
"#;

/// Where the window's fontconfig file is kept (in Qanuvi's data folder).
pub fn conf_path(data: &Path) -> PathBuf {
    data.join("window-fonts.conf")
}

/// Points this process (and so WebKit's own processes) at the window's fontconfig file, written first when it is
/// missing or different. Nothing changes when the user set FONTCONFIG_FILE themselves or the file cannot be written.
///
/// # Safety
/// Sets an environment variable: call at start-up, before any other thread exists.
pub unsafe fn hide_web_fonts(data: &Path) {
    if std::env::var_os("FONTCONFIG_FILE").is_some() {
        return;
    }
    let file = conf_path(data);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(CONF)
        && std::fs::create_dir_all(data)
            .and_then(|_| std::fs::write(&file, CONF))
            .is_err()
    {
        return;
    }
    // SAFETY: the caller's promise (start-up, one thread).
    unsafe { std::env::set_var("FONTCONFIG_FILE", &file) };
}

/// A program the window starts gets the user's own font settings back (only when the setting is the window's).
pub fn give_back(cmd: &mut Command) {
    if let Some(v) = std::env::var_os("FONTCONFIG_FILE")
        && Path::new(&v)
            .file_name()
            .is_some_and(|n| n == "window-fonts.conf")
    {
        cmd.env_remove("FONTCONFIG_FILE");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_conf_keeps_the_system_settings_and_rejects_web_fonts() {
        assert!(CONF.contains(r#"<include ignore_missing="yes">/etc/fonts/fonts.conf</include>"#));
        assert!(CONF.contains("<glob>/*.woff</glob>"));
        assert!(CONF.contains("<glob>/*.woff2</glob>"));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            conf_path(dir.path()).file_name().unwrap(),
            "window-fonts.conf"
        );
    }

    #[test]
    fn programs_started_by_the_window_get_the_users_fonts_back() {
        // Only the window's own file is taken away; checked on a command, not on this process's environment.
        let mut cmd = Command::new("true");
        give_back(&mut cmd);
        let removed = cmd
            .get_envs()
            .any(|(k, v)| k == "FONTCONFIG_FILE" && v.is_none());
        let ours = std::env::var_os("FONTCONFIG_FILE")
            .is_some_and(|v| v.to_string_lossy().ends_with("window-fonts.conf"));
        assert_eq!(removed, ours);
    }
}
