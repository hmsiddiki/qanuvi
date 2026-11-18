// Migration M5b: `qanuvi-master serve` is a small watcher plus the core that does the work. The watcher holds the
// tray icon (This PC), starts the core (`qanuvi-master core`), keeps the core's log, and starts it again when it
// crashes, with a short wait. On the 3rd crash in a row it stops restarting and looks into why: a local model reads
// the crash notes (when one is set up) and the known causes are checked. A safe fix (one start without the start-up
// list) is made on its own and told; anything else goes to the owner with the report, and nothing changes until the
// owner says to start again. The count starts again after a clean Quit or an hour's clean run.
//
// The core's exit code says what happened: 0 a clean end (the watcher ends too), RESTART_CODE a restart asked for
// (started again at once), UPDATED_CODE an update swapped the program (M8b: a new service is started from the
// program now in place and this watcher ends, as it runs the old one), anything else a crash.

use super::{connect, lock_file, on_app_data, restart_mark, service_file, stop_note_file};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The core's exit code that asks its watcher to start it again at once (Settings' restart, a booked restore).
pub const RESTART_CODE: i32 = 75;
/// The core's exit code after an update put a new program in place (M8b).
pub const UPDATED_CODE: i32 = 76;
/// On this crash in a row the watcher stops restarting and looks into why.
pub const CRASH_LIMIT: u32 = 3;
/// A core that ran this long before it crashed starts the count again.
const CLEAN_RUN: Duration = Duration::from_secs(3600);
/// Set for a core its watcher started.
const WATCHED: &str = "QANUVI_WATCHED";
/// Set for a core started once without its start-up list (the safe fix after crashes at start).
const SKIP_STARTUP: &str = "QANUVI_SKIP_STARTUP";
/// Lines of the core's log kept with a crash.
const KEEP_LINES: usize = 40;
/// The log starts again past this size (the one before is kept as master.log.old).
const LOG_MAX: u64 = 1 << 20;
/// Crash records kept.
const KEEP_CRASHES: usize = 5;

/// This core was started by its watcher (a restart then goes through the watcher).
pub fn watched() -> bool {
    std::env::var_os(WATCHED).is_some()
}

/// This core starts without its start-up list (the safe fix after crashes at start).
pub fn skip_startup() -> bool {
    std::env::var_os(SKIP_STARTUP).is_some()
}

pub(super) fn watcher_lock(dir: &Path) -> PathBuf {
    dir.join("watcher.lock")
}

fn crashes_file(dir: &Path) -> PathBuf {
    dir.join("master-crashes.json")
}

/// A crash report for the next core to put in the Inbox.
pub(super) fn report_file(dir: &Path) -> PathBuf {
    dir.join("master-crash-report.json")
}

/// The whole report as text (a click on the notification opens it).
pub fn report_text_file(dir: &Path) -> PathBuf {
    dir.join("master-crash-report.txt")
}

/// What the running core does, noted every 20 seconds (read after a crash; its age tells a shutdown from a crash).
pub(super) fn state_file(dir: &Path) -> PathBuf {
    dir.join("master-state.json")
}

/// The core's own log (its standard error), kept by the watcher.
pub fn log_file(dir: &Path) -> PathBuf {
    dir.join("master.log")
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// When this computer last started (seconds since 1970), where the system says.
pub fn boot_time() -> Option<u64> {
    #[cfg(windows)]
    {
        // SAFETY: a plain call with no arguments.
        let up_ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        Some(now_secs().saturating_sub(up_ms / 1000))
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        stat.lines()
            .find_map(|l| l.strip_prefix("btime "))
            .and_then(|v| v.trim().parse().ok())
    }
    #[cfg(target_os = "macos")]
    {
        // "{ sec = 1763400000, usec = 0 } ..."
        let out = Command::new("sysctl")
            .args(["-n", "kern.boottime"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let rest = text.split("sec =").nth(1)?;
        rest.split(',').next()?.trim().parse().ok()
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Why a core that left no stop note ended, from its last state note: the computer started again after it
/// ("shutdown"), or it ended some other way ("ended": killed, or its watcher was). None when it left no note.
pub(super) fn unclean_why(state_at: Option<u64>, boot: Option<u64>) -> Option<&'static str> {
    let at = state_at?;
    Some(match boot {
        Some(b) if b > at => "shutdown",
        _ => "ended",
    })
}

/// One crash of the core, as its watcher saw it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Crash {
    pub at: u64,
    /// The exit code; none when the system ended it (a signal).
    pub code: Option<i32>,
    /// How long it ran, in seconds.
    pub up_secs: u64,
    /// The last lines it wrote.
    pub lines: Vec<String>,
    /// What it was doing (its last state note: agent sessions, plan runs, start-up commands).
    pub doing: Value,
}

/// The crashes in a row, kept in the data folder.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Crashes {
    /// Crashes in a row (a clean Quit or an hour's clean run starts it again).
    pub count: u32,
    /// The last few, newest last.
    pub list: Vec<Crash>,
    /// The safe fix "start once without the start-up list" was made for this count.
    pub tried_without_startup: bool,
    /// Restarting stopped at the limit; the owner was told and has not said to start again.
    pub stopped: bool,
}

impl Crashes {
    pub fn load(dir: &Path) -> Self {
        std::fs::read_to_string(crashes_file(dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save(&self, dir: &Path) {
        if let Ok(t) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(crashes_file(dir), t);
        }
    }

    fn clear(dir: &Path) {
        let _ = std::fs::remove_file(crashes_file(dir));
    }

    /// Counts a crash; one after a long clean run starts the count again.
    pub fn add(&mut self, c: Crash) {
        if c.up_secs >= CLEAN_RUN.as_secs() {
            self.count = 0;
            self.tried_without_startup = false;
        }
        self.count += 1;
        self.list.push(c);
        let n = self.list.len();
        if n > KEEP_CRASHES {
            self.list.drain(..n - KEEP_CRASHES);
        }
    }

    /// The crashes of this count (the records may hold older ones).
    fn in_a_row(&self) -> &[Crash] {
        let n = (self.count as usize).min(self.list.len());
        &self.list[self.list.len() - n..]
    }
}

/// What the watcher found after the crash limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// It crashed each time soon after it started, while its start-up list started: safe to start once without it.
    StartupItem,
    OutOfMemory,
    /// Its database: anything touching data waits for the owner.
    Database,
    /// A port it needed was taken by another program (ending that program is the owner's choice).
    PortTaken,
    Unknown,
}

impl Cause {
    /// A fix Qanuvi makes on its own.
    pub fn safe(self) -> bool {
        self == Cause::StartupItem
    }
}

/// The known causes, checked against the crashes' logs and records, with what was seen.
pub fn known_cause(list: &[Crash]) -> (Cause, String) {
    let lines: Vec<&str> = list
        .iter()
        .flat_map(|c| c.lines.iter().map(String::as_str))
        .collect();
    let seen = |needles: &[&str]| {
        lines
            .iter()
            .find(|l| {
                let l = l.to_lowercase();
                needles.iter().any(|n| l.contains(n))
            })
            .map(|l| l.trim().to_string())
    };
    // STATUS_NO_MEMORY on Windows.
    let no_memory = list.iter().any(|c| c.code == Some(0xC000_0017_u32 as i32));
    if let Some(l) = seen(&["memory allocation of", "out of memory"]) {
        return (
            Cause::OutOfMemory,
            format!("It ran out of memory: \"{l}\"."),
        );
    }
    if no_memory {
        return (
            Cause::OutOfMemory,
            "It ran out of memory (Windows ended it so).".into(),
        );
    }
    if let Some(l) = seen(&[
        "database is locked",
        "database disk image is malformed",
        "unable to open database",
        "file is not a database",
    ]) {
        return (
            Cause::Database,
            format!("Its database could not be used: \"{l}\"."),
        );
    }
    if let Some(l) = seen(&["address already in use", "os error 10048", "(os error 98)"]) {
        return (
            Cause::PortTaken,
            format!("A port it needs is taken by another program: \"{l}\"."),
        );
    }
    let early = !list.is_empty() && list.iter().all(|c| c.up_secs < 90);
    let startup = list
        .last()
        .and_then(|c| c.doing["startup"].as_u64())
        .unwrap_or(0);
    if early && startup > 0 {
        let longest = list.iter().map(|c| c.up_secs).max().unwrap_or(0);
        let s = if startup == 1 { "" } else { "s" };
        return (
            Cause::StartupItem,
            format!(
                "Each time it crashed within {longest} seconds of starting, while it started its start-up list ({startup} command{s})."
            ),
        );
    }
    (
        Cause::Unknown,
        "None of the known causes (a start-up command, memory, the database, a taken port) matches its log.".into(),
    )
}

/// An exit code as the system writes it: a Windows status in hex (0xC0000409, not -1073740791).
fn exit_code_text(c: i32) -> String {
    if c < 0 {
        format!("exit code 0x{:08X}", c as u32)
    } else {
        format!("exit code {c}")
    }
}

/// The whole report, as text for the owner.
pub fn report_text(list: &[Crash], why: &str, model: Option<&str>, done: &str) -> String {
    let mut out = format!(
        "Qanuvi's background part crashed {} times in a row.\n\nWhat Qanuvi found: {why}\n",
        list.len()
    );
    if let Some(m) = model {
        out.push_str(&format!(
            "\nA local model read the crash notes: {}\n",
            m.trim()
        ));
    }
    out.push_str(&format!("\n{done}\n"));
    for (i, c) in list.iter().enumerate() {
        let code = c
            .code
            .map_or("ended by the system".to_string(), exit_code_text);
        out.push_str(&format!(
            "\nCrash {} ({code}, after {} s){}:\n",
            i + 1,
            c.up_secs,
            if c.doing.is_null() {
                String::new()
            } else {
                format!(", doing {}", c.doing)
            }
        ));
        for l in &c.lines {
            out.push_str("  ");
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

type CoreCommand = Box<dyn Fn(&Path, bool) -> Command + Send + Sync>;
type Tell = Box<dyn Fn(&Path, &str, &str) + Send + Sync>;
type AskModel = Box<dyn Fn(&Path, &[Crash]) -> Option<String> + Send + Sync>;
/// Starts the service again from the updated program (M8b), told the data folder.
type Updated = Box<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

#[derive(Default)]
struct Flags {
    stopped: bool,
    again: bool,
    quit: bool,
}

/// The watcher of one data folder's core.
pub struct Watch {
    dir: PathBuf,
    core: CoreCommand,
    flags: Mutex<Flags>,
    cv: Condvar,
    /// The waits before the 1st and 2nd restart.
    waits: [Duration; 2],
    tell: Tell,
    ask_model: AskModel,
    updated: Updated,
}

impl Watch {
    pub fn new(dir: &Path) -> Self {
        Watch {
            dir: dir.to_path_buf(),
            core: Box::new(core_command),
            flags: Mutex::default(),
            cv: Condvar::new(),
            waits: [Duration::from_secs(2), Duration::from_secs(5)],
            tell: Box::new(tell_owner),
            ask_model: Box::new(ask_local_model),
            updated: Box::new(start_updated(std::env::current_exe().ok())),
        }
    }

    /// This PC's watcher shows the tray icon.
    pub fn app_data(&self) -> bool {
        on_app_data(&self.dir)
    }

    /// Restarting stopped at the crash limit, waiting for the owner.
    pub fn stopped(&self) -> bool {
        self.flags.lock().unwrap_or_else(|e| e.into_inner()).stopped
    }

    /// The owner's yes after the crash limit: the core starts again, the count from zero.
    pub fn start_again(&self) {
        let mut f = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        f.again = true;
        self.cv.notify_all();
    }

    /// The tray's "Open Qanuvi": the core shows a running app's window or starts the app; with no core, the app
    /// starts here (it then says why the background part does not run).
    pub fn open_app(&self) {
        if !self.tell_core("open") {
            super::start_app(None);
        }
    }

    /// The tray's "Quit Qanuvi": the core asks first when work would stop, then ends (and this watcher with it).
    /// With no core running, the watcher ends.
    pub fn quit(&self) {
        if self.tell_core("quit") {
            return;
        }
        let mut f = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        f.quit = true;
        self.cv.notify_all();
        drop(f);
        if !self.stopped() {
            std::process::exit(0);
        }
    }

    fn tell_core(&self, what: &str) -> bool {
        let Some((mut s, token)) = connect(&self.dir) else {
            return false;
        };
        writeln!(s, "{}", json!({"token": token, "tray": what})).is_ok()
    }

    /// Starts the core and watches it until it ends cleanly (or Quit while stopped at the limit).
    pub fn run(&self) -> Result<(), String> {
        let dir = self.dir.clone();
        let mut without_startup = false;
        // A new watcher after the crash limit is the owner starting Qanuvi again (Quit, then open): a new count.
        if Crashes::load(&dir).stopped {
            Crashes::clear(&dir);
        }
        loop {
            // Nothing stale for a new core to trip on (a gone core's address, an old restart mark).
            let _ = std::fs::remove_file(service_file(&dir));
            if !super::restarting(&dir) {
                let _ = std::fs::remove_file(restart_mark(&dir));
            }
            rotate_log(&dir);
            let started = Instant::now();
            let mut child = (self.core)(&dir, without_startup)
                .spawn()
                .map_err(|e| format!("Qanuvi's background part could not start: {e}"))?;
            // The core and everything it started end with this watcher (killed or not).
            let job = crate::browser::Job::for_child(&child);
            let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
            if let Some(err) = child.stderr.take() {
                let (tail, dir) = (tail.clone(), dir.clone());
                std::thread::spawn(move || keep_log(err, &dir, &tail));
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            let up = started.elapsed();
            // The last lines still on their way.
            std::thread::sleep(Duration::from_millis(200));
            // A crashed core's children (agent sessions, start-up commands) end with it; the next core starts its own.
            drop(job);
            match status.code() {
                Some(0) => {
                    Crashes::clear(&dir);
                    return Ok(());
                }
                Some(RESTART_CODE) => {
                    without_startup = false;
                    continue;
                }
                // The new service waits for this watcher to end before it opens the data.
                Some(UPDATED_CODE) => match (self.updated)(&dir) {
                    Ok(()) => return Ok(()),
                    Err(e) => {
                        eprintln!("The updated program could not start: {e}");
                        without_startup = false;
                        continue;
                    }
                },
                _ => {}
            }
            let lines: Vec<String> = tail
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .cloned()
                .collect();
            let doing = std::fs::read_to_string(state_file(&dir))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let mut crashes = Crashes::load(&dir);
            crashes.add(Crash {
                at: now_secs(),
                code: status.code(),
                up_secs: up.as_secs(),
                lines,
                doing,
            });
            if up >= CLEAN_RUN {
                without_startup = false;
            }
            // Its runs say why after the next start; its state note is read, a new core writes its own.
            let _ = std::fs::write(stop_note_file(&dir), "crash");
            let _ = std::fs::remove_file(state_file(&dir));
            eprintln!(
                "Qanuvi's background part crashed ({status}, crash {} in a row).",
                crashes.count
            );
            if crashes.count < CRASH_LIMIT {
                crashes.save(&dir);
                std::thread::sleep(self.waits[(crashes.count as usize - 1).min(1)]);
                continue;
            }
            let list = crashes.in_a_row().to_vec();
            let (cause, why) = known_cause(&list);
            let model = (self.ask_model)(&dir, &list);
            let title = format!("Qanuvi's background part crashed {} times", list.len());
            if cause.safe() && !crashes.tried_without_startup {
                crashes.tried_without_startup = true;
                crashes.save(&dir);
                without_startup = true;
                let done = "Qanuvi started it once without its start-up list. Turn the commands on again one at a time \
                            in Settings, Start-up, to find the one that crashes it.";
                self.report(&title, &list, &why, model.as_deref(), done);
                continue;
            }
            crashes.stopped = true;
            crashes.save(&dir);
            let done = "Qanuvi changed nothing and does not start it again by itself. Choose \"Start Qanuvi's \
                        background part again\" on the tray icon once you have looked (or quit and open Qanuvi).";
            self.report(&title, &list, &why, model.as_deref(), done);
            let mut f = self.flags.lock().unwrap_or_else(|e| e.into_inner());
            f.stopped = true;
            while !f.again && !f.quit {
                f = self.cv.wait(f).unwrap_or_else(|e| e.into_inner());
            }
            f.stopped = false;
            f.again = false;
            if f.quit {
                return Ok(());
            }
            drop(f);
            Crashes::clear(&dir);
            without_startup = false;
        }
    }

    /// The report: kept as text, put in the Inbox by the next core, and told now.
    fn report(&self, title: &str, list: &[Crash], why: &str, model: Option<&str>, done: &str) {
        let text = report_text(list, why, model, done);
        let _ = std::fs::write(report_text_file(&self.dir), &text);
        let short = format!("{why} {done}");
        let _ = std::fs::write(
            report_file(&self.dir),
            json!({"title": title, "text": text}).to_string(),
        );
        (self.tell)(&self.dir, title, &short);
    }
}

/// The watcher's own start after an update: `exe` is the program's path as it was when this watcher started (the
/// running one is `.prev` by now, under the same name as before the swap the new one lives).
fn start_updated(exe: Option<PathBuf>) -> impl Fn(&Path) -> Result<(), String> + Send + Sync {
    move |dir| {
        let exe = exe.as_ref().ok_or("The program's own path is not known.")?;
        super::spawn_service(exe, dir, Some(std::process::id()))
    }
}

/// `qanuvi-master core` beside this program, for the watcher to start.
fn core_command(dir: &Path, without_startup: bool) -> Command {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("qanuvi-master"));
    let mut c = Command::new(exe);
    c.arg("core").arg("--data").arg(dir).env(WATCHED, "1");
    if without_startup {
        c.env(SKIP_STARTUP, "1");
    } else {
        c.env_remove(SKIP_STARTUP);
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

/// The log grows to a megabyte; then the one before is kept as master.log.old.
fn rotate_log(dir: &Path) {
    let log = log_file(dir);
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > LOG_MAX) {
        let _ = std::fs::rename(&log, log.with_extension("log.old"));
    }
}

/// Copies the core's standard error into its log, keeping the last lines for a crash record.
fn keep_log(err: impl std::io::Read, dir: &Path, tail: &Mutex<VecDeque<String>>) {
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file(dir))
        .ok();
    for line in BufReader::new(err).lines() {
        let Ok(line) = line else { break };
        let line = line.trim_end().to_string();
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{line}");
        }
        let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
        t.push_back(line);
        if t.len() > KEEP_LINES {
            t.pop_front();
        }
    }
}

/// Tells the owner: a desktop notification (a click opens the report), and the watcher's own log on a server.
fn tell_owner(dir: &Path, title: &str, text: &str) {
    eprintln!("{title}. {text}");
    #[cfg(feature = "desktop")]
    {
        let one = crate::notify::Shown {
            title: title.to_string(),
            text: text.to_string(),
            id: None,
        };
        let file = report_text_file(dir);
        let shown =
            crate::master_notify::show(&one, !crate::data_location::from_source(), move |_| {
                open_file(&file)
            });
        if let Err(e) = shown {
            eprintln!("Desktop notification failed: {e}");
        }
    }
    #[cfg(not(feature = "desktop"))]
    let _ = dir;
}

#[cfg(feature = "desktop")]
fn open_file(file: &Path) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(opener).arg(file).spawn();
}

/// A local model of the brain (Settings, Agents, Qanuvi agent; only when local models are on) reads the crash
/// notes. None when there is none, or it did not answer.
fn ask_local_model(dir: &Path, list: &[Crash]) -> Option<String> {
    let db = crate::store::Db::open(&dir.join("qanuvi.db")).ok()?;
    let all = crate::settings::get_all(&db).ok()?;
    let brain = all.get("brain")?;
    if brain["local"] != true {
        return None;
    }
    let models: Vec<Value> = brain["models"]
        .as_array()?
        .iter()
        .filter(|m| {
            m["local"] == true || m["id"].as_str().is_some_and(|i| i.starts_with("ollama/"))
        })
        .map(|m| json!({"id": m["id"], "local": true, "free": true, "context": m["context"]}))
        .collect();
    if models.is_empty() {
        return None;
    }
    let used = |p: &Value| {
        let id = p["id"].as_str().unwrap_or("");
        models.iter().any(|m| {
            m["id"]
                .as_str()
                .is_some_and(|m| m.starts_with(&format!("{id}/")))
        })
    };
    let endpoints: Vec<Value> = brain["providers"]
        .as_array()?
        .iter()
        .filter(|p| used(p))
        .map(|p| json!({"id": p["id"], "kind": p["kind"], "url": p["url"]}))
        .collect();
    let setup: crate::brain::Setup =
        serde_json::from_value(json!({"endpoints": endpoints, "models": models})).ok()?;
    let notes = report_text(list, "(to be found)", None, "");
    let (_, text) = crate::brain::decide(
        &setup,
        "You read crash notes of a background program and say in at most three short sentences what most likely \
         made it crash. Say only what the notes show; say so when they do not show a cause.",
        &notes,
    )
    .ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(600).collect())
}

/// Takes a lock file; `None` when another process holds it.
pub(super) fn take_lock_at(file: &Path) -> Result<Option<std::fs::File>, String> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(file)
        .map_err(|e| e.to_string())?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e.to_string()),
    }
}

/// A watcher or a core holds this folder (it may still be starting and not yet have written its address).
pub(super) fn held(dir: &Path) -> bool {
    matches!(take_lock_at(&watcher_lock(dir)), Ok(None))
        || matches!(take_lock_at(&lock_file(dir)), Ok(None))
}

/// What a connection is told while the watcher waits for the owner after the crash limit.
pub fn stopped_text(dir: &Path) -> Option<String> {
    Crashes::load(dir).stopped.then(|| {
        format!(
            "Qanuvi's background part crashed {CRASH_LIMIT} times and was not started again. The report is in {}. \
             Choose \"Start Qanuvi's background part again\" on the tray icon to start it.",
            report_text_file(dir).display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A core that writes a line and ends with this code.
    fn fake_core(code: i32) -> CoreCommand {
        Box::new(move |_dir: &Path, without: bool| {
            let text = if without { "without start-up" } else { "boom" };
            let mut c = if cfg!(windows) {
                let mut c = Command::new("cmd");
                c.args(["/C", &format!("echo {text} 1>&2 & exit {code}")]);
                c
            } else {
                let mut c = Command::new("sh");
                c.args(["-c", &format!("echo {text} 1>&2; exit {code}")]);
                c
            };
            c.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            c
        })
    }

    fn watch(dir: &Path, core: CoreCommand, told: Arc<Mutex<Vec<String>>>) -> Watch {
        Watch {
            dir: dir.to_path_buf(),
            core,
            flags: Mutex::default(),
            cv: Condvar::new(),
            waits: [Duration::from_millis(10), Duration::from_millis(10)],
            tell: Box::new(move |_, title, text| {
                told.lock().unwrap().push(format!("{title}: {text}"))
            }),
            ask_model: Box::new(|_, _| None),
            updated: Box::new(|_| Err("no updated program in this test".into())),
        }
    }

    #[test]
    fn a_clean_end_ends_the_watcher_and_clears_the_count() {
        let d = tempfile::tempdir().unwrap();
        Crashes {
            count: 2,
            ..Default::default()
        }
        .save(d.path());
        let told = Arc::default();
        watch(d.path(), fake_core(0), told).run().unwrap();
        assert!(!crashes_file(d.path()).exists());
    }

    #[test]
    fn crashes_restart_until_the_limit_then_the_owner_is_told_and_nothing_starts() {
        let d = tempfile::tempdir().unwrap();
        let told: Arc<Mutex<Vec<String>>> = Arc::default();
        let w = Arc::new(watch(d.path(), fake_core(3), told.clone()));
        let w2 = w.clone();
        let t = std::thread::spawn(move || w2.run());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !w.stopped() {
            assert!(
                Instant::now() < deadline,
                "the watcher did not stop at the limit"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let c = Crashes::load(d.path());
        assert_eq!(c.count, CRASH_LIMIT);
        assert!(c.stopped);
        assert_eq!(c.list[0].code, Some(3));
        assert_eq!(c.list[0].lines, vec!["boom".to_string()]);
        assert_eq!(told.lock().unwrap().len(), 1);
        assert!(told.lock().unwrap()[0].contains("crashed 3 times"));
        assert!(report_text_file(d.path()).exists());
        assert!(report_file(d.path()).exists());
        // Its runs say it crashed.
        assert_eq!(
            std::fs::read_to_string(stop_note_file(d.path())).unwrap(),
            "crash"
        );
        assert!(
            stopped_text(d.path())
                .unwrap()
                .contains("Start Qanuvi's background part again")
        );
        // Quit while stopped ends the watcher.
        {
            let mut f = w.flags.lock().unwrap();
            f.quit = true;
            w.cv.notify_all();
        }
        t.join().unwrap().unwrap();
    }

    #[test]
    fn crashes_soon_after_start_with_a_start_up_list_try_once_without_it() {
        let d = tempfile::tempdir().unwrap();
        // The core's state note says it was starting 2 start-up commands.
        let state = d.path().join("state-seed");
        std::fs::write(&state, r#"{"startup": 2}"#).unwrap();
        let seed = state.clone();
        let core: CoreCommand = Box::new(move |dir: &Path, without: bool| {
            let _ = std::fs::copy(&seed, state_file(dir));
            // Without its start-up list it runs and ends cleanly.
            fake_core(if without { 0 } else { 3 })(dir, without)
        });
        let told: Arc<Mutex<Vec<String>>> = Arc::default();
        watch(d.path(), core, told.clone()).run().unwrap();
        let told = told.lock().unwrap();
        assert_eq!(told.len(), 1);
        assert!(
            told[0].contains("once without its start-up list"),
            "{}",
            told[0]
        );
        // The clean end after it cleared the count.
        assert!(!crashes_file(d.path()).exists());
    }

    #[test]
    fn a_restart_asked_for_is_no_crash() {
        let d = tempfile::tempdir().unwrap();
        let n = Arc::new(Mutex::new(0));
        let n2 = n.clone();
        let core: CoreCommand = Box::new(move |dir: &Path, without: bool| {
            let mut k = n2.lock().unwrap();
            *k += 1;
            fake_core(if *k == 1 { RESTART_CODE } else { 0 })(dir, without)
        });
        watch(d.path(), core, Arc::default()).run().unwrap();
        assert_eq!(*n.lock().unwrap(), 2);
        assert!(!crashes_file(d.path()).exists());
        assert!(!stop_note_file(d.path()).exists());
    }

    #[test]
    fn after_an_update_a_new_service_takes_over_and_this_watcher_ends() {
        let d = tempfile::tempdir().unwrap();
        let n = Arc::new(Mutex::new(0));
        let n2 = n.clone();
        let core: CoreCommand = Box::new(move |dir: &Path, without: bool| {
            *n2.lock().unwrap() += 1;
            fake_core(UPDATED_CODE)(dir, without)
        });
        let started = Arc::new(Mutex::new(Vec::new()));
        let mut w = watch(d.path(), core, Arc::default());
        let s2 = started.clone();
        w.updated = Box::new(move |dir| {
            s2.lock().unwrap().push(dir.to_path_buf());
            Ok(())
        });
        w.run().unwrap();
        assert_eq!(*n.lock().unwrap(), 1);
        assert_eq!(*started.lock().unwrap(), vec![d.path().to_path_buf()]);
        assert!(!crashes_file(d.path()).exists());

        // The new program does not start: the core starts again here (from what this watcher runs), no crash.
        let n = Arc::new(Mutex::new(0));
        let n2 = n.clone();
        let core: CoreCommand = Box::new(move |dir: &Path, without: bool| {
            let mut k = n2.lock().unwrap();
            *k += 1;
            fake_core(if *k == 1 { UPDATED_CODE } else { 0 })(dir, without)
        });
        watch(d.path(), core, Arc::default()).run().unwrap();
        assert_eq!(*n.lock().unwrap(), 2);
        assert!(!crashes_file(d.path()).exists());
    }

    #[test]
    fn known_causes() {
        let crash = |lines: &[&str], up: u64, startup: u64| Crash {
            lines: lines.iter().map(|s| s.to_string()).collect(),
            up_secs: up,
            doing: json!({"startup": startup}),
            code: Some(1),
            ..Default::default()
        };
        let oom = [crash(&["memory allocation of 1024 bytes failed"], 400, 0)];
        assert_eq!(known_cause(&oom).0, Cause::OutOfMemory);
        let db = [crash(&["Error: database is locked"], 5, 1)];
        assert_eq!(known_cause(&db).0, Cause::Database);
        assert!(!Cause::Database.safe());
        let early = [crash(&["x"], 3, 1), crash(&["y"], 4, 1)];
        assert_eq!(known_cause(&early).0, Cause::StartupItem);
        assert!(Cause::StartupItem.safe());
        let late = [crash(&["x"], 300, 1)];
        assert_eq!(known_cause(&late).0, Cause::Unknown);
        assert_eq!(exit_code_text(-1073740791), "exit code 0xC0000409");
        assert_eq!(exit_code_text(3), "exit code 3");
    }

    #[test]
    fn a_long_clean_run_starts_the_count_again() {
        let mut c = Crashes {
            count: 2,
            tried_without_startup: true,
            ..Default::default()
        };
        c.add(Crash {
            up_secs: CLEAN_RUN.as_secs() + 1,
            ..Default::default()
        });
        assert_eq!(c.count, 1);
        assert!(!c.tried_without_startup);
    }

    #[test]
    fn a_shutdown_is_told_from_a_crash_by_the_boot_time() {
        assert_eq!(unclean_why(None, Some(100)), None);
        assert_eq!(unclean_why(Some(50), Some(100)), Some("shutdown"));
        assert_eq!(unclean_why(Some(150), Some(100)), Some("ended"));
        assert_eq!(unclean_why(Some(150), None), Some("ended"));
        assert!(boot_time().is_some_and(|b| b <= now_secs()));
    }
}
