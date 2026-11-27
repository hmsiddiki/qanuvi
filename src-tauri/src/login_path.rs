//! s5 (ST2): a master started from the desktop (menu, Dock, tray, start at login) gets the desktop session's PATH,
//! which on Linux comes from ~/.profile and on macOS from launchd, not from ~/.bashrc or ~/.zshrc. Programs set up
//! there (nvm, fnm, pyenv, Homebrew on some Macs) were not found by agents, terminals and checks. Like VS Code, the
//! master asks the user's login shell for its PATH once at start and adds what is missing.

#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::time::Duration;

/// How long the login shell may take to answer (a slow ~/.bashrc must not hold the master up).
#[cfg(unix)]
const WAIT: Duration = Duration::from_secs(6);

/// The marks around the PATH in the shell's output (a ~/.bashrc may print its own lines).
const MARK: &str = "_QANUVI_PATH_";

/// Adds the login shell's PATH entries that this process lacks (theirs first, then the ones only this process has).
/// Unix only; `QANUVI_NO_SHELL_PATH` turns it off.
pub fn adopt() {
    #[cfg(unix)]
    {
        if std::env::var_os("QANUVI_NO_SHELL_PATH").is_some() {
            return;
        }
        let Some(shell_path) = ask_shell() else {
            return;
        };
        let now = std::env::var("PATH").unwrap_or_default();
        let merged = merge(&shell_path, &now);
        if merged != now {
            // SAFETY: called once at the master's start, before its threads that read the environment.
            unsafe { std::env::set_var("PATH", merged) };
        }
    }
}

#[cfg(unix)]
fn ask_shell() -> Option<String> {
    use std::process::{Command, Stdio};
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
    let mut child = Command::new(&shell)
        .args(["-i", "-l", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < WAIT => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    between_marks(&out)
}

/// The PATH between the marks, when the shell printed one.
fn between_marks(out: &str) -> Option<String> {
    let start = out.find(MARK)? + MARK.len();
    let len = out[start..].find(MARK)?;
    let p = out[start..start + len].trim();
    (!p.is_empty()).then(|| p.to_string())
}

/// The shell's entries first, then this process's own that the shell lacks; no entry twice, no empty ones.
fn merge(shell: &str, now: &str) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for p in shell.split(':').chain(now.split(':')) {
        if !p.is_empty() && !seen.contains(&p) {
            seen.push(p);
        }
    }
    seen.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shells_path_comes_first_and_nothing_is_lost() {
        assert_eq!(
            merge(
                "/home/u/.nvm/versions/node/v24/bin:/usr/bin:/bin",
                "/usr/local/bin:/usr/bin:/bin:/opt/app"
            ),
            "/home/u/.nvm/versions/node/v24/bin:/usr/bin:/bin:/usr/local/bin:/opt/app"
        );
        assert_eq!(merge("", "/usr/bin"), "/usr/bin");
    }

    #[test]
    fn lines_a_bashrc_prints_are_left_out() {
        let out = format!("Welcome!\n{MARK}/a/bin:/usr/bin{MARK}\nbye");
        assert_eq!(between_marks(&out).as_deref(), Some("/a/bin:/usr/bin"));
        assert_eq!(between_marks("no marks"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_real_shell_answers() {
        // Whatever the machine's shell is, the PATH it gives is not empty.
        assert!(ask_shell().is_some_and(|p| !p.is_empty()));
    }
}
