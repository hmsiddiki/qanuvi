//! Real terminals (a pseudo-terminal per tab) running the user's shell in a project folder.

use crate::sink::Sink;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type SharedChild = Arc<Mutex<Box<dyn Child + Send + Sync>>>;

/// A program's exit code, waiting up to `within` for it to end.
fn exit_code(child: &SharedChild, within: Duration) -> Option<u32> {
    let until = Instant::now() + within;
    loop {
        if let Ok(Some(status)) = child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
            return Some(status.exit_code());
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shell {
    pub id: String,
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TermEvent {
    Data { data: String },
    Exit { code: Option<u32> },
}

struct Term {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: SharedChild,
    project_id: String,
    /// The window that opened it (desktop), so closing a second window ends only its own terminals.
    owner: Option<String>,
}

#[derive(Default)]
pub struct Terminals {
    map: Mutex<HashMap<String, Term>>,
}

/// Looks a program up on PATH (with Windows' executable extensions).
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if cfg!(windows) {
            let plain = dir.join(program);
            if plain.is_file() && plain.extension().is_some() {
                return Some(plain);
            }
        }
    }
    None
}

/// The shells available on this machine, the default first.
pub fn shells() -> Vec<Shell> {
    let mut list = Vec::new();
    if cfg!(windows) {
        if let Some(p) = find_on_path("pwsh") {
            list.push(Shell {
                id: "pwsh".into(),
                label: "PowerShell 7".into(),
                program: p.to_string_lossy().into(),
                args: vec!["-NoLogo".into()],
            });
        }
        list.push(Shell {
            id: "powershell".into(),
            label: "Windows PowerShell".into(),
            program: "powershell.exe".into(),
            args: vec!["-NoLogo".into()],
        });
        list.push(Shell {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            program: "cmd.exe".into(),
            args: vec![],
        });
        for candidate in [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
        ] {
            if Path::new(candidate).is_file() {
                list.push(Shell {
                    id: "gitbash".into(),
                    label: "Git Bash".into(),
                    program: candidate.into(),
                    args: vec!["--login".into(), "-i".into()],
                });
                break;
            }
        }
    } else {
        let login = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let name = Path::new(&login)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "shell".into());
        list.push(Shell {
            id: name.clone(),
            label: name,
            program: login.clone(),
            args: vec!["-l".into()],
        });
        for (id, program) in [
            ("bash", "/bin/bash"),
            ("zsh", "/bin/zsh"),
            ("fish", "/usr/bin/fish"),
        ] {
            if program != login && Path::new(program).is_file() {
                list.push(Shell {
                    id: id.into(),
                    label: id.into(),
                    program: program.into(),
                    args: vec!["-l".into()],
                });
            }
        }
    }
    list
}

/// Keeps the bytes of a character that was split between two reads for the next read.
pub fn take_utf8(pending: &mut Vec<u8>) -> String {
    match std::str::from_utf8(pending) {
        Ok(s) => {
            let out = s.to_string();
            pending.clear();
            out
        }
        Err(e) => {
            let valid = e.valid_up_to();
            if e.error_len().is_none() {
                // Incomplete character at the end: keep it for later.
                let out = String::from_utf8_lossy(&pending[..valid]).to_string();
                pending.drain(..valid);
                out
            } else {
                let out = String::from_utf8_lossy(pending).to_string();
                pending.clear();
                out
            }
        }
    }
}

impl Terminals {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &self,
        project_id: &str,
        cwd: &Path,
        shell_id: &str,
        cols: u16,
        rows: u16,
        channel: Sink<TermEvent>,
    ) -> Result<String, String> {
        let all = shells();
        let shell = all
            .iter()
            .find(|s| s.id == shell_id)
            .or_else(|| all.first())
            .ok_or("No shell was found on this machine.")?;
        let mut cmd = CommandBuilder::new(&shell.program);
        cmd.args(&shell.args);
        self.start(project_id, cwd, cmd, cols, rows, channel, false)
    }

    /// Runs one program in a terminal (24z3 D2: a setup install the user can watch and answer, a password prompt
    /// included). When the program ends, its exit code is sent (`Exit { code }`), also where the terminal itself
    /// stays open (Windows keeps the pseudo-console until it is closed).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_program(
        &self,
        tag: &str,
        cwd: &Path,
        program: &str,
        args: &[String],
        cols: u16,
        rows: u16,
        channel: Sink<TermEvent>,
    ) -> Result<String, String> {
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        self.start(tag, cwd, cmd, cols, rows, channel, true)
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        project_id: &str,
        cwd: &Path,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        channel: Sink<TermEvent>,
        watch: bool,
    ) -> Result<String, String> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: rows.max(2),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Qanuvi");
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let child: SharedChild = Arc::new(Mutex::new(child));
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        // The end is told once: by the reader when the output closes, or (a watched program) when it exits.
        let told = Arc::new(AtomicBool::new(false));

        let id = uuid::Uuid::new_v4().to_string();
        let (out, told_r, child_r) = (channel.clone(), told.clone(), child.clone());
        std::thread::Builder::new()
            .name(format!("term-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                let mut pending = Vec::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            pending.extend_from_slice(&buf[..n]);
                            let data = take_utf8(&mut pending);
                            if !data.is_empty() && out.send(TermEvent::Data { data }).is_err() {
                                break;
                            }
                        }
                    }
                }
                let code = if watch {
                    exit_code(&child_r, Duration::from_secs(2))
                } else {
                    None
                };
                if !told_r.swap(true, Ordering::SeqCst) {
                    let _ = out.send(TermEvent::Exit { code });
                }
            })
            .map_err(|e| e.to_string())?;
        if watch {
            let child_w = child.clone();
            std::thread::Builder::new()
                .name(format!("term-wait-{id}"))
                .spawn(move || {
                    let code = loop {
                        match child_w.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
                            Ok(Some(status)) => break Some(status.exit_code()),
                            Ok(None) => {}
                            Err(_) => break None,
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    };
                    // The last output first: the reader may still have some.
                    std::thread::sleep(Duration::from_millis(400));
                    if !told.swap(true, Ordering::SeqCst) {
                        let _ = channel.send(TermEvent::Exit { code });
                    }
                })
                .map_err(|e| e.to_string())?;
        }

        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.clone(),
            Term {
                writer,
                master: pair.master,
                child,
                project_id: project_id.to_string(),
                owner: None,
            },
        );
        Ok(id)
    }

    pub fn write(&self, id: &str, data: &str) -> Result<(), String> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let term = map.get_mut(id).ok_or("That terminal has closed.")?;
        term.writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        term.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let term = map.get(id).ok_or("That terminal has closed.")?;
        term.master
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }

    pub fn kill(&self, id: &str) {
        let term = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(term) = term {
            let mut child = term.child.lock().unwrap_or_else(|e| e.into_inner());
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Closes every terminal of a project (when the project is removed from the list).
    pub fn kill_project(&self, project_id: &str) {
        let ids: Vec<String> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, t)| t.project_id == project_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.kill(&id);
        }
    }

    /// Notes the window a terminal belongs to.
    pub fn set_owner(&self, id: &str, owner: &str) {
        if let Some(t) = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            t.owner = Some(owner.to_string());
        }
    }

    /// Closes the terminals a window opened (when that window closes).
    pub fn kill_owner(&self, owner: &str) {
        let ids: Vec<String> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, t)| t.owner.as_deref() == Some(owner))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.kill(&id);
        }
    }

    pub fn kill_all(&self) {
        let ids: Vec<String> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.kill(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_split_character_for_the_next_read() {
        let bytes = "héllo".as_bytes();
        let mut pending = bytes[..2].to_vec(); // "h" and the first byte of "é"
        assert_eq!(take_utf8(&mut pending), "h");
        assert_eq!(pending.len(), 1);
        pending.extend_from_slice(&bytes[2..]);
        assert_eq!(take_utf8(&mut pending), "éllo");
        assert!(pending.is_empty());
    }

    #[test]
    fn replaces_invalid_bytes_instead_of_waiting_forever() {
        let mut pending = vec![b'a', 0xff, b'b'];
        assert_eq!(take_utf8(&mut pending), "a\u{fffd}b");
        assert!(pending.is_empty());
    }

    #[test]
    fn a_program_run_in_a_terminal_tells_its_exit_code() {
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "cmd.exe",
                vec!["/d".into(), "/c".into(), "echo hi & exit 3".into()],
            )
        } else {
            ("/bin/sh", vec!["-c".into(), "echo hi; exit 3".into()])
        };
        let (tx, rx) = std::sync::mpsc::channel::<TermEvent>();
        let tx = Mutex::new(tx);
        let sink = Sink::new(move |e| tx.lock().unwrap().send(e).is_ok());
        let terms = Terminals::default();
        let dir = std::env::temp_dir();
        let id = terms
            .spawn_program("setup", &dir, program, &args, 80, 24, sink)
            .unwrap();
        let mut out = String::new();
        let code = loop {
            match rx
                .recv_timeout(Duration::from_secs(8))
                .unwrap_or_else(|_| panic!("no end told; output {out:?}"))
            {
                TermEvent::Data { data } => {
                    // Windows' pseudo-console asks where the cursor is before it starts the program; the window's
                    // terminal (xterm.js) answers that itself.
                    if data.contains("\x1b[6n") {
                        terms.write(&id, "\x1b[1;1R").unwrap();
                    }
                    out.push_str(&data)
                }
                TermEvent::Exit { code } => break code,
            }
        };
        assert_eq!(code, Some(3));
        assert!(out.contains("hi"), "{out:?}");
        // Told once only.
        assert!(!matches!(
            rx.recv_timeout(Duration::from_millis(800)),
            Ok(TermEvent::Exit { .. })
        ));
        terms.kill(&id);
    }

    #[test]
    fn offers_at_least_one_shell() {
        assert!(!shells().is_empty());
    }
}
