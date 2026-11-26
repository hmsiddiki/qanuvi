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

/// The most recent output each terminal keeps for a window that comes back to it (about the last screens of text).
const RECENT_MAX: usize = 256 * 1024;

/// The kept output also fits in the window's terminal, which holds 5000 rows (TerminalView scrollback), so the
/// trimmed note before it is never pushed out of view (v2). A line is counted as the rows it takes in a terminal 80
/// columns wide or wider (1 + its length / 80), which is never fewer than it really takes; 500 rows are left for the
/// note and the screen itself.
const RECENT_ROWS: usize = 4500;

/// Where a terminal's output goes (24z3 Part B2). While the window shows the terminal's machine, to that window; while
/// the switcher hides the machine, nowhere, but the most recent output is always kept, so the window that shows the
/// machine again gets it back (the shell itself goes on all the while).
#[derive(Default)]
struct Outlet {
    sink: Option<Sink<TermEvent>>,
    recent: String,
    /// Line ends in `recent`.
    recent_lines: usize,
    /// The shell ended (with its exit code when known).
    ended: Option<Option<u32>>,
    /// The oldest output was dropped (more than RECENT_MAX came): a window given it back is told (M5c).
    trimmed: bool,
    /// ST1 flow 25 c2: the connection that opened it ended and nobody has taken it over yet.
    orphaned: bool,
    /// Counts orphanings and takeovers, so a grace that ran out ends only the orphaning it was for.
    turn: u64,
    /// The connection that has it now (0: none noted). Only that one's end orphans it: an old connection that is
    /// noticed gone late (its pings ran out after the window had come back on a new one) leaves it alone.
    holder: usize,
}

/// Said before kept output that lost its beginning (grey, as the window's own notes in a terminal).
pub const TRIMMED_NOTE: &str =
    "\x1b[90m[Earlier output was trimmed; the most recent part follows.]\x1b[0m\r\n";

impl Outlet {
    fn data(&mut self, data: String) {
        self.trimmed |= keep_recent(
            &mut self.recent,
            &mut self.recent_lines,
            &data,
            RECENT_MAX,
            RECENT_ROWS,
        );
        if let Some(sink) = &self.sink
            && sink.send(TermEvent::Data { data }).is_err()
        {
            // Nobody listens any more (the window went): the output is kept for the next one.
            self.sink = None;
        }
    }

    fn exit(&mut self, code: Option<u32>) {
        self.ended = Some(code);
        if let Some(sink) = &self.sink {
            let _ = sink.send(TermEvent::Exit { code });
        }
    }
}

/// Rows kept output is counted as (see RECENT_ROWS).
fn rows(lines: usize, bytes: usize) -> usize {
    lines + bytes / 80
}

/// Adds output to what a terminal keeps, dropping the oldest beyond `max` bytes or `max_rows` rows from the
/// start of a line, so the text given back does not begin halfway through one. `lines` counts the line ends kept.
/// Answers whether anything was dropped.
pub fn keep_recent(
    recent: &mut String,
    lines: &mut usize,
    data: &str,
    max: usize,
    max_rows: usize,
) -> bool {
    recent.push_str(data);
    *lines += data.matches('\n').count();
    let mut dropped = false;
    if recent.len() > max {
        let mut cut = recent.len() - max;
        while !recent.is_char_boundary(cut) {
            cut += 1;
        }
        let line = recent[cut..]
            .char_indices()
            .take_while(|(i, _)| *i < 4096)
            .find(|(_, c)| *c == '\n')
            .map(|(i, _)| cut + i + 1);
        let end = line.unwrap_or(cut);
        *lines -= recent[..end].matches('\n').count();
        recent.drain(..end);
        dropped = true;
    }
    if rows(*lines, recent.len()) > max_rows {
        // Whole lines from the front until the rest fits.
        let (mut end, mut gone) = (0, 0);
        for (i, _) in recent.match_indices('\n') {
            end = i + 1;
            gone += 1;
            if rows(*lines - gone, recent.len() - end) <= max_rows {
                break;
            }
        }
        *lines -= gone;
        recent.drain(..end);
        dropped = true;
    }
    dropped
}

struct Term {
    outlet: Arc<Mutex<Outlet>>,
    /// Shared with the output reader, which answers the pseudo-console's opening cursor question (v3).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: SharedChild,
    project_id: String,
    /// The window that opened it (desktop), so closing a second window ends only its own terminals.
    owner: Option<String>,
}

#[derive(Default)]
pub struct Terminals {
    map: Mutex<HashMap<String, Term>>,
    /// How long a terminal outlives the connection that opened it, in ms (0: TERM_GRACE; tests set less).
    grace_ms: std::sync::atomic::AtomicU64,
}

/// ST1 flow 25 c2: how long a shell goes on after the connection that opened it ended (a Wi-Fi drop, a move to a
/// hotspot), so the window that comes back takes it over with what it wrote meanwhile. Nobody back by then: it ends.
pub const TERM_GRACE: Duration = Duration::from_secs(5 * 60);

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

/// Windows' pseudo-console asks where the cursor is (ESC[6n) before it starts the program, and the program says
/// nothing until it is answered. The master answers it itself (v3): a screen that only got the question before it
/// knew the terminal's id, a slow link, or no screen at all (a terminal nobody shows yet) left the shell silent for
/// ever. A new terminal's cursor is at its top left.
const CURSOR_QUESTION: &str = "\x1b[6n";
const CURSOR_ANSWER: &str = "\x1b[1;1R";

#[derive(Debug, PartialEq, Eq)]
enum Opening {
    /// The output starts with the question: answered here, the rest goes on to the screen.
    Asked(String),
    /// Only the question's first bytes so far.
    Partial,
    /// No question: the output goes on as it is.
    None,
}

/// ST1 Windows w1: the master service starts in a new process group (CREATE_NEW_PROCESS_GROUP), which turns Ctrl+C
/// off for it, and every program started from it inherits that: Ctrl+C in a terminal stopped nothing. Turned on again
/// once, before the first shell starts, so shells and what runs in them get Ctrl+C; this process itself ignores it
/// through its own handler (handlers are not inherited, the off switch is).
fn ctrl_c_for_children() {
    #[cfg(windows)]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
            unsafe extern "system" fn ignore(_kind: u32) -> windows_sys::core::BOOL {
                1
            }
            unsafe {
                SetConsoleCtrlHandler(Some(ignore), 1);
                SetConsoleCtrlHandler(None, 0);
            }
        });
    }
}

/// Looks at a terminal's very first output for the pseudo-console's cursor question.
fn opening_question(data: &str) -> Opening {
    if let Some(rest) = data.strip_prefix(CURSOR_QUESTION) {
        Opening::Asked(rest.to_string())
    } else if CURSOR_QUESTION.starts_with(data) {
        Opening::Partial
    } else {
        Opening::None
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
        ctrl_c_for_children();
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
        let writer = Arc::new(Mutex::new(
            pair.master.take_writer().map_err(|e| e.to_string())?,
        ));
        // The end is told once: by the reader when the output closes, or (a watched program) when it exits.
        let told = Arc::new(AtomicBool::new(false));

        let id = uuid::Uuid::new_v4().to_string();
        let outlet = Arc::new(Mutex::new(Outlet {
            sink: Some(channel),
            ..Outlet::default()
        }));
        let (out, told_r, child_r, answer_to) =
            (outlet.clone(), told.clone(), child.clone(), writer.clone());
        std::thread::Builder::new()
            .name(format!("term-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                let mut pending = Vec::new();
                // Only the very first output can be the opening question, and only Windows' pseudo-console asks it
                // (elsewhere a program's own question is the screen's to answer).
                let mut opening = cfg!(windows);
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            pending.extend_from_slice(&buf[..n]);
                            let mut data = take_utf8(&mut pending);
                            if opening && !data.is_empty() {
                                match opening_question(&data) {
                                    Opening::Partial => {
                                        // Its first bytes only: wait for the rest.
                                        pending.splice(0..0, data.into_bytes());
                                        continue;
                                    }
                                    Opening::Asked(rest) => {
                                        let mut w =
                                            answer_to.lock().unwrap_or_else(|e| e.into_inner());
                                        let _ = w
                                            .write_all(CURSOR_ANSWER.as_bytes())
                                            .and_then(|_| w.flush());
                                        data = rest;
                                    }
                                    Opening::None => {}
                                }
                                opening = false;
                            }
                            if !data.is_empty() {
                                out.lock().unwrap_or_else(|e| e.into_inner()).data(data);
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
                    out.lock().unwrap_or_else(|e| e.into_inner()).exit(code);
                }
            })
            .map_err(|e| e.to_string())?;
        if watch {
            let (child_w, outlet_w) = (child.clone(), outlet.clone());
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
                        outlet_w
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .exit(code);
                    }
                })
                .map_err(|e| e.to_string())?;
        }

        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.clone(),
            Term {
                outlet,
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
        let mut writer = term.writer.lock().unwrap_or_else(|e| e.into_inner());
        writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
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

    /// Stops sending a terminal's output to its window (24z3 Part B2: the switcher hid its machine); the shell goes
    /// on and its recent output is kept.
    pub fn detach(&self, id: &str) {
        if let Some(t) = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            t.outlet.lock().unwrap_or_else(|e| e.into_inner()).sink = None;
        }
    }

    /// Sends a terminal's output to a window again: first what it kept (and its end, if it ended), then what comes.
    pub fn attach(&self, id: &str, sink: Sink<TermEvent>) -> Result<(), String> {
        let outlet = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|t| t.outlet.clone())
            .ok_or("That terminal has closed.")?;
        let mut o = outlet.lock().unwrap_or_else(|e| e.into_inner());
        if !o.recent.is_empty() {
            // Plain output, so a window of any version shows it.
            let note = if o.trimmed { TRIMMED_NOTE } else { "" };
            let _ = sink.send(TermEvent::Data {
                data: format!("{note}{}", o.recent),
            });
        }
        if let Some(code) = o.ended {
            let _ = sink.send(TermEvent::Exit { code });
        }
        o.sink = Some(sink);
        // Taken over (c2): a grace running for an earlier orphaning no longer ends it.
        o.orphaned = false;
        o.turn += 1;
        Ok(())
    }

    /// How long an orphaned terminal is kept (c2).
    pub fn grace(&self) -> Duration {
        match self.grace_ms.load(Ordering::SeqCst) {
            0 => TERM_GRACE,
            ms => Duration::from_millis(ms),
        }
    }

    #[cfg(test)]
    pub fn set_grace(&self, grace: Duration) {
        self.grace_ms
            .store(grace.as_millis().max(1) as u64, Ordering::SeqCst);
    }

    /// The connection that opened (or last took over) a terminal ended (c2): its output is kept, nobody gets it, and
    /// the shell goes on. Answers the turn to hand `end_orphan` once the grace is over; none when there is nothing to
    /// keep (gone, or its shell already ended: then it is closed now).
    pub fn orphan(&self, id: &str, holder: usize) -> Option<u64> {
        let outlet = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|t| t.outlet.clone())?;
        let mut o = outlet.lock().unwrap_or_else(|e| e.into_inner());
        if o.holder != 0 && o.holder != holder {
            // Taken over by another connection meanwhile: that one has it.
            return None;
        }
        if o.ended.is_some() {
            drop(o);
            self.kill(id);
            return None;
        }
        o.sink = None;
        o.orphaned = true;
        o.turn += 1;
        Some(o.turn)
    }

    /// Notes the connection that has a terminal now (c2: spawned or taken over on it).
    pub fn hold(&self, id: &str, holder: usize) {
        if let Some(t) = self.map.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            t.outlet.lock().unwrap_or_else(|e| e.into_inner()).holder = holder;
        }
    }

    /// The grace of one orphaning is over (c2): still nobody's, so it ends.
    pub fn end_orphan(&self, id: &str, turn: u64) {
        let outlet = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|t| t.outlet.clone());
        let Some(outlet) = outlet else { return };
        let still = {
            let o = outlet.lock().unwrap_or_else(|e| e.into_inner());
            o.orphaned && o.turn == turn
        };
        if still {
            self.kill(id);
        }
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

    /// ST1 Windows w1, end to end: Ctrl+C typed into a terminal stops a program that runs until interrupted.
    #[cfg(windows)]
    #[test]
    fn ctrl_c_stops_a_running_program_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<TermEvent>();
        let terms = Terminals::default();
        let shell = shells()[0].id.clone();
        let id = terms
            .spawn(
                "p",
                dir.path(),
                &shell,
                100,
                30,
                Sink::new(move |e| tx.send(e).is_ok()),
            )
            .unwrap();
        let mut seen = String::new();
        let mut read_until = |text: &str, secs: u64| {
            let deadline = Instant::now() + Duration::from_secs(secs);
            while !seen.contains(text) && Instant::now() < deadline {
                if let Ok(TermEvent::Data { data }) = rx.recv_timeout(Duration::from_millis(200)) {
                    seen.push_str(&data);
                }
            }
            seen.contains(text)
        };
        std::thread::sleep(Duration::from_secs(2));
        terms.write(&id, "ping -t 127.0.0.1\r").unwrap();
        assert!(read_until("Reply from", 15), "ping did not start");
        terms.write(&id, "\x03").unwrap();
        // ping says "Control-C" when it is interrupted.
        assert!(read_until("Control-C", 10), "Ctrl+C did not stop ping");
        terms.kill(&id);
    }

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
                // Nobody answers the pseudo-console's cursor question here: the master does (v3).
                TermEvent::Data { data } => out.push_str(&data),
                TermEvent::Exit { code } => break code,
            }
        };
        assert_eq!(code, Some(3));
        assert!(out.contains("hi"), "{out:?}");
        assert!(
            !out.contains(CURSOR_QUESTION),
            "the question reached the screen: {out:?}"
        );
        // Told once only.
        assert!(!matches!(
            rx.recv_timeout(Duration::from_millis(800)),
            Ok(TermEvent::Exit { .. })
        ));
        terms.kill(&id);
    }

    #[test]
    fn keeps_the_most_recent_output_from_a_line_start() {
        let (mut recent, mut lines) = (String::new(), 0);
        let big = usize::MAX;
        assert!(!keep_recent(
            &mut recent,
            &mut lines,
            "first line\nsecond line\n",
            1024,
            big
        ));
        assert_eq!((recent.as_str(), lines), ("first line\nsecond line\n", 2));
        // Over the limit: the oldest goes, and what is kept starts after a line end.
        assert!(keep_recent(&mut recent, &mut lines, "third\n", 16, big));
        assert_eq!((recent.as_str(), lines), ("third\n", 1));
        // A long run without line ends is cut at a character boundary.
        let (mut r, mut n) = (String::new(), 0);
        assert!(keep_recent(&mut r, &mut n, &"é".repeat(20), 9, big));
        assert!(r.len() <= 9 && r.chars().all(|c| c == 'é'), "{r:?}");
    }

    #[test]
    fn v2_kept_output_fits_the_windows_terminal_rows() {
        // 12000 short lines (about 560 KB, as in the drive): fewer bytes than the limit would keep, but no more rows
        // than the window's terminal holds, so the trimmed note before them stays in view.
        let mut o = Outlet::default();
        for i in 1..=12000 {
            o.data(format!("line {i} {}\r\n", "x".repeat(32)));
        }
        assert!(o.trimmed);
        let kept = o.recent.matches('\n').count();
        assert_eq!(kept, o.recent_lines);
        assert!(rows(kept, o.recent.len()) <= RECENT_ROWS, "{kept} lines");
        assert!(
            o.recent.starts_with("line ")
                && o.recent
                    .ends_with("line 12000 xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\n")
        );
        // Long lines: the byte limit is the one that cuts.
        let mut o = Outlet::default();
        for i in 1..=2000 {
            o.data(format!("long {i} {}\r\n", "y".repeat(200)));
        }
        assert!(o.trimmed && o.recent.len() <= RECENT_MAX);
        assert!(rows(o.recent_lines, o.recent.len()) <= RECENT_ROWS);
    }

    #[test]
    fn an_outlet_that_dropped_output_says_so_when_given_back() {
        let mut o = Outlet::default();
        o.data("short\n".into());
        assert!(!o.trimmed);
        o.data("x\n".repeat(RECENT_MAX));
        assert!(o.trimmed && o.recent.len() <= RECENT_MAX);
        assert!(TRIMMED_NOTE.contains("Earlier output was trimmed"));
    }

    #[test]
    fn a_detached_terminal_keeps_its_output_and_end_for_the_next_window() {
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            (
                "cmd.exe",
                vec![
                    "/d".into(),
                    "/c".into(),
                    "ping -n 2 127.0.0.1 >nul & echo later & exit 4".into(),
                ],
            )
        } else {
            (
                "/bin/sh",
                vec!["-c".into(), "sleep 1; echo later; exit 4".into()],
            )
        };
        let (tx, rx) = std::sync::mpsc::channel::<TermEvent>();
        let tx = Mutex::new(tx);
        let first = Sink::new(move |e| tx.lock().unwrap().send(e).is_ok());
        let terms = Terminals::default();
        let id = terms
            .spawn_program(
                "setup",
                &std::env::temp_dir(),
                program,
                &args,
                80,
                24,
                first,
            )
            .unwrap();
        // Gone at once: the pseudo-console's cursor question (Windows) is the master's to answer (v3), so the program
        // runs on with no window at all.
        terms.detach(&id);
        // The window is away: the program ends meanwhile, nothing reaches the first window.
        std::thread::sleep(Duration::from_secs(4));
        assert!(
            rx.try_iter().all(|e| !matches!(e, TermEvent::Exit { .. })),
            "the end reached a detached window"
        );

        let (tx2, rx2) = std::sync::mpsc::channel::<TermEvent>();
        let tx2 = Mutex::new(tx2);
        terms
            .attach(&id, Sink::new(move |e| tx2.lock().unwrap().send(e).is_ok()))
            .unwrap();
        let back: Vec<TermEvent> = rx2.try_iter().collect();
        let text: String = back
            .iter()
            .filter_map(|e| match e {
                TermEvent::Data { data } => Some(data.as_str()),
                _ => None,
            })
            .collect();
        assert!(text.contains("later"), "{text:?}");
        assert!(
            matches!(back.last(), Some(TermEvent::Exit { code: Some(4) })),
            "{back:?}"
        );
        terms.kill(&id);
        assert!(terms.attach(&id, Sink::new(|_| true)).is_err());
    }

    #[test]
    fn v3_the_opening_cursor_question_is_answered_here_and_not_passed_on() {
        assert_eq!(
            opening_question("\x1b[6n\x1b[?9001h"),
            Opening::Asked("\x1b[?9001h".into())
        );
        assert_eq!(opening_question("\x1b[6n"), Opening::Asked(String::new()));
        assert_eq!(opening_question("\x1b["), Opening::Partial);
        assert_eq!(opening_question("\x1b"), Opening::Partial);
        // A shell elsewhere starts with its own output; a question later on is the program's and the screen's.
        assert_eq!(opening_question("$ "), Opening::None);
        assert_eq!(opening_question("hi\x1b[6n"), Opening::None);
    }

    #[test]
    fn offers_at_least_one_shell() {
        assert!(!shells().is_empty());
    }
}
