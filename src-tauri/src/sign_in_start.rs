//! "Start Qanuvi when Windows starts" (migration M4c; renamed in M5b from "...when I sign in"), off by default.
//! When on, signing in to this computer runs `qanuvi --background`: the app starts This PC's master on its own data folder and ends without a
//! window, so the master's background jobs run and its tray icon shows. The entry itself is the setting (nothing
//! else is kept): Windows has it under the user's Run key, Linux in ~/.config/autostart, macOS in
//! ~/Library/LaunchAgents. A test build has its own entry beside the installed copy's.

use std::path::{Path, PathBuf};

/// The flag the entry starts the app with.
pub const FLAG: &str = "--background";

fn test_build() -> bool {
    crate::data_location::from_source()
}

/// The entry's name: the installed copy's, or the test build's own.
fn entry_name() -> &'static str {
    if test_build() {
        "Qanuvi (test build)"
    } else {
        "Qanuvi"
    }
}

/// The desktop app's program: this one, or the app beside the master that asks. f12: the live file, also when this
/// program was replaced while it runs (the tray's "Open Qanuvi" tried the deleted path and nothing opened).
pub fn app_exe() -> Result<PathBuf, String> {
    let me = crate::data_location::current_exe().map_err(|e| e.to_string())?;
    let stem = me
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if stem != "qanuvi-master" {
        return Ok(me);
    }
    // m1 (Mac): a master in its helper app inside Qanuvi.app.
    if cfg!(target_os = "macos")
        && let Some(app) = crate::master::mac_app_of_helper(&me).filter(|a| a.is_file())
    {
        return Ok(app);
    }
    let app = me.with_file_name(if cfg!(windows) {
        "qanuvi.exe"
    } else {
        "qanuvi"
    });
    if app.is_file() {
        Ok(app)
    } else {
        Err(format!(
            "The Qanuvi app was not found next to its master ({}).",
            app.display()
        ))
    }
}

/// Whether signing in starts Qanuvi in the background.
pub fn is_on() -> Result<bool, String> {
    Ok(read_entry()?.is_some())
}

/// Turns it on (or writes the entry again with the app's current place) or off.
pub fn set(on: bool) -> Result<bool, String> {
    if on {
        write_entry(&app_exe()?)?;
    } else {
        remove_entry()?;
    }
    is_on()
}

/// At the app's start: an entry that is on is written again, so it follows the app when it moved.
pub fn refresh() {
    if matches!(is_on(), Ok(true))
        && let Ok(exe) = app_exe()
        && read_entry().ok().flatten().as_deref() != Some(command_line(&exe).as_str())
    {
        let _ = write_entry(&exe);
    }
}

/// The command the entry runs.
fn command_line(exe: &Path) -> String {
    format!("\"{}\" {FLAG}", exe.display())
}

#[cfg(windows)]
mod place {
    use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read(name: &str) -> Result<Option<String>, String> {
        let (key, name) = (wide(RUN), wide(name));
        let mut size: u32 = 0;
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if r == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if r != 0 {
            return Err(format!("The sign-in entry could not be read (error {r})."));
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if r != 0 {
            return Err(format!("The sign-in entry could not be read (error {r})."));
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(Some(String::from_utf16_lossy(&buf[..end])))
    }

    pub fn write(name: &str, command: &str) -> Result<(), String> {
        let (key, name, data) = (wide(RUN), wide(name), wide(command));
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if r != 0 {
            return Err(format!(
                "The sign-in entry could not be written (error {r})."
            ));
        }
        Ok(())
    }

    pub fn remove(name: &str) -> Result<(), String> {
        let (key, name) = (wide(RUN), wide(name));
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        if r != 0 && r != ERROR_FILE_NOT_FOUND {
            return Err(format!(
                "The sign-in entry could not be removed (error {r})."
            ));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn read_entry() -> Result<Option<String>, String> {
    place::read(entry_name())
}

#[cfg(windows)]
fn write_entry(exe: &Path) -> Result<(), String> {
    place::write(entry_name(), &command_line(exe))
}

#[cfg(windows)]
fn remove_entry() -> Result<(), String> {
    place::remove(entry_name())
}

/// The entry's file: a .desktop file (Linux) or a launch agent (macOS) in the user's own folder.
#[cfg(not(windows))]
fn entry_file() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("The home folder is not known.")?;
    let slug = if test_build() {
        "qanuvi-test-build"
    } else {
        "qanuvi"
    };
    Ok(if cfg!(target_os = "macos") {
        home.join("Library/LaunchAgents")
            .join(format!("dev.{slug}.background.plist"))
    } else {
        home.join(".config/autostart")
            .join(format!("{slug}.desktop"))
    })
}

/// The command kept in the entry's file, as `command_line` writes it.
#[cfg(not(windows))]
fn read_entry() -> Result<Option<String>, String> {
    let file = entry_file()?;
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Ok(None);
    };
    let found = if cfg!(target_os = "macos") {
        // <string>exe</string><string>--background</string>
        let strings: Vec<&str> = text
            .split("<string>")
            .skip(1)
            .filter_map(|s| s.split("</string>").next())
            .collect();
        let at = strings.iter().position(|s| *s == FLAG);
        at.and_then(|i| i.checked_sub(1))
            .map(|i| format!("\"{}\" {FLAG}", unxml(strings[i])))
    } else {
        text.lines()
            .find_map(|l| l.strip_prefix("Exec="))
            .map(str::to_string)
    };
    Ok(Some(found.unwrap_or_default()))
}

#[cfg(not(windows))]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(not(windows))]
fn unxml(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(not(windows))]
fn write_entry(exe: &Path) -> Result<(), String> {
    let file = entry_file()?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = if cfg!(target_os = "macos") {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  \
             <key>Label</key><string>{}</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string>\
             <string>{FLAG}</string></array>\n  <key>RunAtLoad</key><true/>\n</dict>\n</plist>\n",
            file.file_stem().unwrap_or_default().to_string_lossy(),
            xml(&exe.display().to_string()),
        )
    } else {
        format!(
            "[Desktop Entry]\nType=Application\nName={}\nComment=Starts Qanuvi in the background\nExec={}\n\
             Terminal=false\nX-GNOME-Autostart-enabled=true\n",
            entry_name(),
            command_line(exe),
        )
    };
    std::fs::write(&file, text).map_err(|e| format!("The sign-in entry could not be written: {e}"))
}

#[cfg(not(windows))]
fn remove_entry() -> Result<(), String> {
    let file = entry_file()?;
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("The sign-in entry could not be removed: {e}")),
    }
}
