//! Stopping a process the run itself started (found on a real run, 2026-11-28): a step started a demo server, the
//! never-list refused its `kill`, the server kept the port and the next step's check failed. A kill of a process
//! that belongs to the project or the run is the step cleaning up after itself; any other kill stays refused.

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

/// Told to the agent with a refused stop command, so it stops its own process the way the rules let it.
pub const STOP_HINT: &str = "To stop a process this project started (a server or watcher a step opened), give its \
     number itself: `kill 12345` (Linux, macOS) or `taskkill /PID 12345 /F` (Windows), alone in the command, not in \
     a variable; Qanuvi allows it when the process belongs to the project. Find the number with `ps`, `pgrep -f` or \
     `lsof -i :PORT`.";

/// Whether a command part is a stop command at all (kill, taskkill, Stop-Process), whatever it names.
pub fn is_stop(part: &str) -> bool {
    static STOPS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^\s*(kill|taskkill|stop-process)\b").unwrap());
    STOPS.is_match(part)
}

/// `$(cat FILE)` and `` `cat FILE` `` with a process number written in FILE (a pid file, absolute or in the project)
/// put in, so `kill $(cat server.pid)` is judged by the number it names.
fn with_pid_files(part: &str, root: &Path) -> String {
    static CAT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#""?(?:\$\(\s*cat\s+([^\s)`"]+)\s*\)|`\s*cat\s+([^\s)`"]+)\s*`)"?"#).unwrap()
    });
    CAT.replace_all(part, |c: &regex::Captures| {
        let file = c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str());
        let path = if Path::new(file).is_absolute() {
            Path::new(file).to_path_buf()
        } else {
            root.join(file)
        };
        std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.chars().all(|ch| ch.is_ascii_digit()))
            .unwrap_or_else(|| c[0].to_string())
    })
    .into_owned()
}

/// The process ids a stop command names, when the whole command is only that: `kill [-SIG] <pid>...`,
/// `taskkill [/F] [/T] /PID <pid>...`, `Stop-Process [-Id] <pid>[,<pid>] [-Force]`, with an optional
/// `2>/dev/null` or `>/dev/null 2>&1` at the end. None for anything else (a name, a pattern, a variable).
pub fn stop_pids(part: &str) -> Option<Vec<u32>> {
    static TAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\s*(2>\s*/dev/null|>\s*/dev/null\s+2>&1)\s*$").unwrap());
    static KILL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^kill(\s+-(?:[0-9]+|[A-Z]+|s\s+[A-Z0-9]+))*((?:\s+[0-9]+)+)$").unwrap()
    });
    static TASKKILL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^taskkill((?:\s+/(?:f|t))*(?:\s+/pid\s+[0-9]+)+(?:\s+/(?:f|t))*)$")
            .unwrap()
    });
    static STOP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^stop-process(?:\s+-id)?\s+([0-9]+(?:\s*,\s*[0-9]+)*)(?:\s+-force)?$")
            .unwrap()
    });
    static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]+").unwrap());
    let p = TAIL.replace(part.trim(), "");
    let p = p.trim();
    let digits = if let Some(c) = KILL.captures(p) {
        c[2].to_string()
    } else if let Some(c) = TASKKILL.captures(p) {
        static PID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/pid\s+([0-9]+)").unwrap());
        PID.captures_iter(&c[1])
            .map(|m| m[1].to_string())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        STOP.captures(p)?[1].to_string()
    };
    let pids: Vec<u32> = NUM
        .find_iter(&digits)
        .filter_map(|m| m.as_str().parse().ok())
        .collect();
    (!pids.is_empty()).then_some(pids)
}

/// Whether the process belongs to the project or the run: never this process, its parent or pid 0/1. On Linux and
/// macOS its working folder is inside the project; on Windows it was started (through
/// any number of steps) by this Qanuvi.
pub fn belongs(pid: u32, root: &Path) -> bool {
    let me = std::process::id();
    if pid <= 1 || pid == me || Some(pid) == parent_of(me) {
        return false;
    }
    if cfg!(windows) {
        return descends_from_me(pid);
    }
    let Some(cwd) = cwd_of(pid) else {
        return false;
    };
    std::fs::canonicalize(root)
        .map(|b| cwd.starts_with(&b))
        .unwrap_or(false)
}

/// A stop command part whose every process belongs to the project or the run.
pub fn stops_own(part: &str, root: &Path) -> bool {
    stop_pids(&with_pid_files(part, root))
        .is_some_and(|pids| pids.iter().all(|&p| belongs(p, root)))
}

#[cfg(target_os = "linux")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(target_os = "macos")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('n'))
        .map(std::path::PathBuf::from)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn cwd_of(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

#[cfg(unix)]
fn parent_of(pid: u32) -> Option<u32> {
    if pid == std::process::id() {
        // SAFETY: getppid has no preconditions.
        return Some(unsafe { libc::getppid() } as u32);
    }
    None
}

#[cfg(windows)]
fn parent_of(pid: u32) -> Option<u32> {
    parents().get(&pid).copied()
}

#[cfg(not(any(unix, windows)))]
fn parent_of(_pid: u32) -> Option<u32> {
    None
}

#[cfg(windows)]
fn parents() -> std::collections::HashMap<u32, u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    let mut map = std::collections::HashMap::new();
    // SAFETY: the snapshot handle is checked and closed; the entry struct is sized as the API requires.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return map;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                map.insert(e.th32ProcessID, e.th32ParentProcessID);
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    map
}

#[cfg(windows)]
fn descends_from_me(pid: u32) -> bool {
    let me = std::process::id();
    let map = parents();
    let mut at = pid;
    for _ in 0..64 {
        match map.get(&at) {
            Some(&p) if p == me => return true,
            Some(&p) if p != 0 && p != at => at = p,
            _ => return false,
        }
    }
    false
}

#[cfg(not(windows))]
fn descends_from_me(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_plain_stop_commands_by_number() {
        assert_eq!(stop_pids("kill 67381"), Some(vec![67381]));
        assert_eq!(stop_pids("kill -9 12 34"), Some(vec![12, 34]));
        assert_eq!(stop_pids("kill -TERM 12 2>/dev/null"), Some(vec![12]));
        assert_eq!(stop_pids("kill -s KILL 12"), Some(vec![12]));
        assert_eq!(stop_pids("taskkill /F /PID 4412"), Some(vec![4412]));
        assert_eq!(stop_pids("taskkill /PID 1 /PID 2 /T"), Some(vec![1, 2]));
        assert_eq!(stop_pids("Stop-Process -Id 77 -Force"), Some(vec![77]));
        assert_eq!(stop_pids("kill $(cat server.pid)"), None);
        assert_eq!(stop_pids("kill %1"), None);
        assert_eq!(stop_pids("pkill -f node"), None);
        assert_eq!(stop_pids("taskkill /IM node.exe /F"), None);
        assert_eq!(stop_pids("kill 12; rm -rf /"), None);
    }

    #[test]
    fn never_this_process_or_init() {
        let root = std::env::temp_dir();
        assert!(!belongs(std::process::id(), &root));
        assert!(!belongs(1, &root));
        assert!(!belongs(0, &root));
    }

    /// A process started in the project folder (Unix) or by this process (Windows) belongs; it can be stopped.
    #[test]
    fn a_process_started_in_the_project_belongs() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = if cfg!(windows) {
            std::process::Command::new("cmd")
                .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
                .current_dir(dir.path())
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("sleep")
                .arg("30")
                .current_dir(dir.path())
                .spawn()
                .unwrap()
        };
        let ok = belongs(child.id(), dir.path());
        let _ = child.kill();
        let _ = child.wait();
        assert!(ok);
    }
}
