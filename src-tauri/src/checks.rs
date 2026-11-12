//! Runs a project's check command (its test command) for a plan step, with a time limit, and keeps its output.

use serde::Serialize;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Output kept from a check (the tail matters most; failures are picked out by the interface).
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// Standard output and error together, cut to the last part when very long.
    pub output: String,
}

fn shell(command: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/D", "/S", "/C", command]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        // Its own process group, so a timeout can stop everything it started.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        c
    }
}

/// Stops a command and everything it started (a test runner may start other programs that keep running).
fn kill_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    if cfg!(windows) {
        let mut k = Command::new("taskkill");
        k.args(["/T", "/F", "/PID", &pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            k.creation_flags(0x0800_0000);
        }
        let _ = k.status();
    } else {
        #[cfg(unix)]
        kill_group(child.id());
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Sends SIGKILL to the process group led by `pid` (a child started with `process_group(0)`), straight through the
/// system call. Not the `kill` program: some versions read `kill -9 -<pid>` wrongly and signalled every process of
/// the user, which ended the test run and the CI runner with it. Does nothing unless `pid` really leads its group.
#[cfg(unix)]
pub fn kill_group(pid: u32) {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return;
    };
    if pid <= 1 {
        return;
    }
    // SAFETY: plain system calls on a positive pid; a group that is gone gives an error that is ignored.
    unsafe {
        if libc::getpgid(pid) == pid && pid != libc::getpgrp() {
            libc::killpg(pid, libc::SIGKILL);
        }
    }
}

/// Collects a stream on its own thread; the result is waited for only a short time after the command ends, since a
/// program the command left running could keep the stream open.
fn collect(mut stream: impl Read + Send + 'static) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = stream.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    rx
}

fn keep_tail(bytes: &[u8]) -> String {
    let start = bytes.len().saturating_sub(MAX_OUTPUT_BYTES);
    let text = String::from_utf8_lossy(&bytes[start..]).to_string();
    if start > 0 {
        format!("[… earlier output cut …]\n{text}")
    } else {
        text
    }
}

pub fn run(dir: &Path, command: &str, timeout: Duration) -> Result<CheckResult, String> {
    run_with_env(dir, command, timeout, &[])
}

/// Runs the command with extra environment variables (the project's secrets).
pub fn run_with_env(
    dir: &Path,
    command: &str,
    timeout: Duration,
    env: &[(String, String)],
) -> Result<CheckResult, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("This project has no test command. Set one in the project's settings.".into());
    }
    let started = Instant::now();
    let mut child = shell(command)
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CI", "1")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .spawn()
        .map_err(|e| format!("The test command could not start: {e}"))?;
    let out_rx = collect(child.stdout.take().ok_or("no output")?);
    let err_rx = collect(child.stderr.take().ok_or("no error output")?);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            kill_tree(&mut child);
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let grace = Duration::from_secs(2);
    let mut bytes = out_rx.recv_timeout(grace).unwrap_or_default();
    let errors = err_rx.recv_timeout(grace).unwrap_or_default();
    if !errors.is_empty() {
        if !bytes.is_empty() && !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(&errors);
    }
    let exit_code = status.and_then(|s| s.code());
    Ok(CheckResult {
        ok: !timed_out && exit_code == Some(0),
        exit_code,
        timed_out,
        duration_ms: started.elapsed().as_millis() as u64,
        output: keep_tail(&bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_fails_and_keeps_output_of_both_streams() {
        let dir = tempfile::tempdir().unwrap();
        let ok = run(dir.path(), "echo all good", Duration::from_secs(20)).unwrap();
        assert!(ok.ok);
        assert!(ok.output.contains("all good"));
        let fail = run(
            dir.path(),
            "echo broken 1>&2 && exit 3",
            Duration::from_secs(20),
        )
        .unwrap();
        assert!(!fail.ok);
        assert_eq!(fail.exit_code, Some(3));
        assert!(fail.output.contains("broken"));
        assert!(run(dir.path(), "  ", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn stops_a_command_that_runs_too_long() {
        let dir = tempfile::tempdir().unwrap();
        let slow = if cfg!(windows) {
            "ping -n 30 127.0.0.1 >NUL"
        } else {
            "sleep 30"
        };
        let r = run(dir.path(), slow, Duration::from_millis(500)).unwrap();
        assert!(r.timed_out);
        assert!(!r.ok);
        assert!(r.duration_ms < 10_000);
    }

    #[test]
    fn keeps_the_end_of_very_long_output() {
        let long = vec![b'x'; MAX_OUTPUT_BYTES + 10];
        let kept = keep_tail(&long);
        assert!(kept.starts_with("[… earlier output cut …]"));
        assert!(kept.len() <= MAX_OUTPUT_BYTES + 40);
    }
}
