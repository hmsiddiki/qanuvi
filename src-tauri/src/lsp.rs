//! Language servers (15i): the same servers VS Code uses, spoken to over the Language Server Protocol. Nothing is
//! bundled. A server is installed on demand, after the user's yes, into Qanuvi's data folder (npm servers) or through
//! the tool the language already has (rust-analyzer through rustup). One is found on PATH when the user installed it
//! already. A server starts on the first file of its language, runs at below-normal priority, ends with Qanuvi (a job
//! object on Windows), and stops when its project closes or after a while unused (the window decides that).
//!
//! The window speaks the protocol itself (src/lib/lsp). This side only frames the messages: `Content-Length` headers
//! out, whole JSON messages back.

use crate::pipes::Link;
use crate::sink::Sink;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

/// A language server Qanuvi knows how to install and start.
pub struct Spec {
    pub id: &'static str,
    pub label: &'static str,
    /// The languages it serves, for the Settings list.
    pub serves: &'static str,
    /// npm packages installed together into the server's own folder (empty: not an npm server).
    pub npm: &'static [&'static str],
    /// The script node runs, below the folder's node_modules (npm servers).
    pub entry: &'static str,
    /// The program's name on PATH (a server the user installed, or a non-npm one).
    pub program: &'static str,
    pub args: &'static [&'static str],
    /// About how much memory it takes on a mid-size project, checked before it starts.
    pub need_gb: f64,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "php",
        label: "Intelephense",
        serves: "PHP",
        npm: &["intelephense"],
        entry: "intelephense/lib/intelephense.js",
        program: "intelephense",
        args: &["--stdio"],
        need_gb: 0.4,
    },
    Spec {
        id: "typescript",
        label: "TypeScript language server",
        serves: "TypeScript, JavaScript",
        npm: &["typescript-language-server", "typescript"],
        entry: "typescript-language-server/lib/cli.mjs",
        program: "typescript-language-server",
        args: &["--stdio"],
        need_gb: 0.5,
    },
    Spec {
        id: "svelte",
        label: "Svelte language server",
        serves: "Svelte",
        npm: &["svelte-language-server", "typescript"],
        entry: "svelte-language-server/bin/server.js",
        program: "svelteserver",
        args: &["--stdio"],
        need_gb: 0.5,
    },
    Spec {
        id: "rust",
        label: "rust-analyzer",
        serves: "Rust",
        npm: &[],
        entry: "",
        program: "rust-analyzer",
        args: &[],
        need_gb: 1.0,
    },
    Spec {
        id: "python",
        label: "Pyright",
        serves: "Python",
        npm: &["pyright"],
        entry: "pyright/langserver.index.js",
        program: "pyright-langserver",
        args: &["--stdio"],
        need_gb: 0.4,
    },
];

pub fn spec(id: &str) -> Result<&'static Spec, String> {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Qanuvi has no language server called {id}."))
}

/// The machine a language server is found, installed and run on: This PC, or a shown master (migration M1), which
/// only answers generic questions (where a program or file is, free memory, run a command); what a language server
/// is stays here, on the interface's side.
pub trait Host {
    /// Where things go on that machine, for sentences.
    fn place(&self) -> &'static str;
    fn data_dir(&self) -> String;
    /// A path below `base` on that machine (its own separator).
    fn join(&self, base: &str, parts: &[&str]) -> String;
    fn find(&self, program: &str) -> Option<String>;
    fn exists(&self, path: &str) -> bool;
    /// Runs a program to its end: whether it succeeded, and the end of its error output. `live` gets its output
    /// lines while it runs.
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        timeout_secs: u64,
        live: Option<Sink<String>>,
    ) -> Result<(bool, String), String>;
    /// Writes a new file below the data folder (a file already there is kept).
    fn write_new(&self, relative: &str, text: &str) -> Result<(), String>;
    fn room(&self) -> Option<crate::machine::Room>;
}

/// This PC.
pub struct Here;

fn ran(v: &Value) -> (bool, String) {
    (
        v["code"].as_i64() == Some(0),
        v["err"].as_str().unwrap_or("").to_string(),
    )
}

/// `{"line": ...}` events (as `host_run` sends them) to a sink of lines.
fn lines_to(live: Option<Sink<String>>) -> Option<Sink<Value>> {
    live.map(|s| {
        Sink::new(move |e: Value| match e["line"].as_str() {
            Some(l) => s.send(l.to_string()).is_ok(),
            None => true,
        })
    })
}

impl Host for Here {
    fn place(&self) -> &'static str {
        "this computer"
    }
    fn data_dir(&self) -> String {
        crate::paths::data_dir().to_string_lossy().into_owned()
    }
    fn join(&self, base: &str, parts: &[&str]) -> String {
        let mut p = PathBuf::from(base);
        p.extend(parts);
        p.to_string_lossy().into_owned()
    }
    fn find(&self, program: &str) -> Option<String> {
        crate::pipes::host::find(program)
    }
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        timeout_secs: u64,
        live: Option<Sink<String>>,
    ) -> Result<(bool, String), String> {
        let v = crate::pipes::host::run(
            program,
            args,
            cwd.map(Path::new),
            std::time::Duration::from_secs(timeout_secs),
            lines_to(live),
        )?;
        Ok(ran(&v))
    }
    fn write_new(&self, relative: &str, text: &str) -> Result<(), String> {
        crate::pipes::host::write_new(relative, text)
    }
    fn room(&self) -> Option<crate::machine::Room> {
        crate::machine::room()
    }
}

/// A shown master, through its connection.
pub struct On {
    link: Arc<dyn Link>,
    data_dir: String,
    sep: String,
}

impl On {
    pub fn new(link: Arc<dyn Link>) -> Result<Self, String> {
        let info = link.call("host_info", json!({}), None).map_err(|e| {
            if e.contains("does not know") {
                "That machine runs an older Qanuvi; update it to use language servers there."
                    .to_string()
            } else {
                e
            }
        })?;
        Ok(Self {
            data_dir: info["dataDir"].as_str().unwrap_or("").to_string(),
            sep: info["sep"].as_str().unwrap_or("/").to_string(),
            link,
        })
    }
}

impl Host for On {
    fn place(&self) -> &'static str {
        "that machine"
    }
    fn data_dir(&self) -> String {
        self.data_dir.clone()
    }
    fn join(&self, base: &str, parts: &[&str]) -> String {
        let mut p = base.trim_end_matches(['/', '\\']).to_string();
        for part in parts {
            p.push_str(&self.sep);
            p.push_str(part);
        }
        p
    }
    fn find(&self, program: &str) -> Option<String> {
        let v = self
            .link
            .call("host_find", json!({"program": program}), None)
            .ok()?;
        v.as_str().map(String::from)
    }
    fn exists(&self, path: &str) -> bool {
        self.link
            .call("host_exists", json!({"path": path}), None)
            .is_ok_and(|v| v == json!(true))
    }
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
        timeout_secs: u64,
        live: Option<Sink<String>>,
    ) -> Result<(bool, String), String> {
        let v = self.link.call(
            "host_run",
            json!({"program": program, "args": args, "cwd": cwd, "timeoutSecs": timeout_secs}),
            lines_to(live),
        )?;
        Ok(ran(&v))
    }
    fn write_new(&self, relative: &str, text: &str) -> Result<(), String> {
        self.link
            .call(
                "host_write_new",
                json!({"path": relative, "text": text}),
                None,
            )
            .map(|_| ())
    }
    fn room(&self) -> Option<crate::machine::Room> {
        let v = self.link.call("machine_room", json!({}), None).ok()?;
        Some(crate::machine::Room {
            free_ram_gb: v[0].as_f64()?,
            free_vram_gb: v[1].as_f64().unwrap_or(0.0),
        })
    }
}

/// Where Qanuvi's own copy of an npm server lives on that machine.
fn own_dir(host: &dyn Host, spec: &Spec) -> String {
    host.join(&host.data_dir(), &["language-servers", spec.id])
}

fn own_entry(host: &dyn Host, spec: &Spec) -> Option<String> {
    if spec.npm.is_empty() {
        return None;
    }
    let mut parts = vec!["node_modules"];
    parts.extend(spec.entry.split('/'));
    let p = host.join(&own_dir(host, spec), &parts);
    host.exists(&p).then_some(p)
}

/// rustup puts a rust-analyzer proxy on PATH even when the component is missing; asking it for its version tells.
fn rust_analyzer_works(host: &dyn Host, program: &str) -> bool {
    host.run(program, &["--version".to_string()], None, 30, None)
        .is_ok_and(|(ok, _)| ok)
}

/// How a server starts on that machine, if it can: the program and its arguments.
pub fn command(host: &dyn Host, spec: &Spec) -> Option<(String, Vec<String>)> {
    let args: Vec<String> = spec.args.iter().map(|a| a.to_string()).collect();
    if let Some(entry) = own_entry(host, spec)
        && let Some(node) = host.find("node")
    {
        let mut all = vec![entry];
        all.extend(args);
        return Some((node, all));
    }
    let program = host.find(spec.program)?;
    if spec.id == "rust" && !rust_analyzer_works(host, &program) {
        return None;
    }
    Some((program, args))
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub id: &'static str,
    pub label: &'static str,
    pub serves: &'static str,
    /// "qanuvi" (installed by Qanuvi), "path" (found on that machine), "" (not installed).
    pub found: &'static str,
    /// What installing does, in words; empty when Qanuvi cannot install it there.
    pub install: String,
    /// Why it cannot be installed there (no Node.js, no rustup).
    pub missing: String,
}

pub fn status(host: &dyn Host, spec: &'static Spec) -> Status {
    let found = if own_entry(host, spec).is_some() {
        "qanuvi"
    } else if command(host, spec).is_some() {
        "path"
    } else {
        ""
    };
    let place = host.place();
    let (install, missing) = if spec.npm.is_empty() {
        if host.find("rustup").is_some() {
            (
                format!("Runs on {place}: rustup component add rust-analyzer"),
                String::new(),
            )
        } else {
            (
                String::new(),
                format!(
                    "rust-analyzer comes with rustup; install Rust from rustup.rs on {place} first."
                ),
            )
        }
    } else if host.find("npm").is_some() {
        (
            format!(
                "Downloads {} from npm into Qanuvi's data folder on {place} (nothing is installed for the whole machine).",
                spec.npm.join(" and ")
            ),
            String::new(),
        )
    } else {
        (
            String::new(),
            format!(
                "This server runs on Node.js; install Node.js from nodejs.org on {place} first."
            ),
        )
    };
    Status {
        id: spec.id,
        label: spec.label,
        serves: spec.serves,
        found,
        install,
        missing,
    }
}

pub fn statuses(host: &dyn Host) -> Vec<Status> {
    SPECS.iter().map(|s| status(host, s)).collect()
}

/// Installs a server (blocking; npm can take a minute). Only called after the user said yes in the window.
/// `live` gets the install's output lines as they come, for the window to show while it waits.
pub fn install(
    host: &dyn Host,
    spec: &'static Spec,
    live: Option<Sink<String>>,
) -> Result<Status, String> {
    let place = host.place();
    let (ok, err) = if spec.npm.is_empty() {
        let rustup = host.find("rustup").ok_or_else(|| {
            format!(
                "rust-analyzer comes with rustup; install Rust from rustup.rs on {place} first."
            )
        })?;
        let args = ["component", "add", "rust-analyzer"].map(String::from);
        host.run(&rustup, &args, None, 900, live)?
    } else {
        let npm = host.find("npm").ok_or_else(|| {
            format!(
                "This server runs on Node.js; install Node.js from nodejs.org on {place} first."
            )
        })?;
        // A package.json of its own, so npm keeps the install in this folder and never looks above it.
        host.write_new(
            &format!("language-servers/{}/package.json", spec.id),
            "{\"private\":true}\n",
        )?;
        let dir = own_dir(host, spec);
        let mut args: Vec<String> = [
            "install",
            "--no-audit",
            "--no-fund",
            "--omit=dev",
            "--prefix",
        ]
        .map(String::from)
        .to_vec();
        args.push(dir.clone());
        args.extend(spec.npm.iter().map(|p| p.to_string()));
        host.run(&npm, &args, Some(&dir), 900, live)?
    };
    if !ok {
        let tail: Vec<&str> = err.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "Installing {} failed:\n{}",
            spec.label,
            tail.join("\n")
        ));
    }
    let st = status(host, spec);
    if st.found.is_empty() {
        return Err(format!(
            "{} was installed but does not start; see the log above.",
            spec.label
        ));
    }
    Ok(st)
}

/// What a server sends to the window.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspEvent {
    /// One whole JSON-RPC message, as the server sent it.
    Message {
        text: String,
    },
    /// A line the server wrote to its error output (kept for its log).
    Log {
        text: String,
    },
    Exit {
        code: Option<i32>,
    },
}

/// How messages reach a running server.
#[derive(Clone)]
enum Input {
    Local(Arc<Mutex<ChildStdin>>),
    /// A program pipe on a shown master; each write numbered, since the master may get them out of order.
    Remote {
        link: Arc<dyn Link>,
        id: String,
        seq: Arc<AtomicU64>,
    },
}

enum Proc {
    Local {
        child: Child,
        _job: crate::browser::Job,
    },
    Remote,
}

struct Running {
    project: String,
    owner: String,
    input: Input,
    proc: Proc,
}

impl Running {
    fn end(mut self) {
        match &mut self.proc {
            Proc::Local { child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Proc::Remote => {
                if let Input::Remote { link, id, .. } = &self.input {
                    link.tell("pipe_stop", json!({"id": id}));
                }
            }
        }
    }
}

/// The running servers of this Qanuvi, by key.
#[derive(Default, Clone)]
pub struct Servers {
    map: Arc<Mutex<HashMap<String, Running>>>,
}

fn hide(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window; below-normal priority, so a busy server never slows the editor or a game.
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// Reads one framed message (headers, an empty line, then `Content-Length` bytes). None at the end of the stream.
pub fn read_message(r: &mut impl BufRead) -> Option<String> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().ok();
        }
    }
    let mut body = vec![0u8; length?];
    r.read_exact(&mut body).ok()?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

pub fn frame(text: &str) -> Vec<u8> {
    let mut out = format!("Content-Length: {}\r\n\r\n", text.len()).into_bytes();
    out.extend_from_slice(text.as_bytes());
    out
}

/// The bytes of a program pipe's output, read as one stream.
struct Received {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    at: usize,
}

impl Read for Received {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.at >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

impl Servers {
    /// Reads a server's messages until it ends, then forgets it and tells the window.
    fn read(
        &self,
        key: String,
        out: impl Read + Send + 'static,
        sink: Sink<LspEvent>,
        code: Arc<Mutex<Option<i32>>>,
    ) {
        let map = self.map.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(out);
            while let Some(text) = read_message(&mut r) {
                if sink.send(LspEvent::Message { text }).is_err() {
                    break;
                }
            }
            let run = map.lock().unwrap().remove(&key);
            let mut exit = *code.lock().unwrap();
            if let Some(mut run) = run {
                if let Proc::Local { child, .. } = &mut run.proc {
                    let _ = child.kill();
                    exit = child.wait().ok().and_then(|s| s.code());
                }
                run.end();
            }
            let _ = sink.send(LspEvent::Exit { code: exit });
        });
    }

    /// Starts a server for a project folder on This PC. Its messages go to `sink` until it ends.
    pub fn start(
        &self,
        project: &str,
        owner: &str,
        root: &Path,
        id: &str,
        sink: Sink<LspEvent>,
    ) -> Result<String, String> {
        let spec = spec(id)?;
        let (program, args) = command(&Here, spec)
            .ok_or_else(|| format!("{} is not installed on this computer.", spec.label))?;
        crate::machine::check(spec.label, spec.need_gb)?;
        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start {}: {e}", spec.label))?;
        let job = crate::browser::Job::for_child(&child);
        let stdin = child.stdin.take().ok_or("The server has no input.")?;
        let stdout = child.stdout.take().ok_or("The server has no output.")?;
        let stderr = child.stderr.take();
        let key = format!("lsp-{}", uuid::Uuid::new_v4());

        if let Some(err) = stderr {
            let s = sink.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if s.send(LspEvent::Log { text: line }).is_err() {
                        break;
                    }
                }
            });
        }
        self.map.lock().unwrap().insert(
            key.clone(),
            Running {
                project: project.into(),
                owner: owner.into(),
                input: Input::Local(Arc::new(Mutex::new(stdin))),
                proc: Proc::Local { child, _job: job },
            },
        );
        self.read(key.clone(), stdout, sink, Arc::default());
        Ok(key)
    }

    /// Starts a server for a project on a shown master, through a program pipe there (migration M1). `project` is
    /// the master's own id for it.
    pub fn start_on(
        &self,
        link: Arc<dyn Link>,
        project: &str,
        owner: &str,
        id: &str,
        sink: Sink<LspEvent>,
    ) -> Result<String, String> {
        let spec = spec(id)?;
        let host = On::new(link.clone())?;
        let (program, args) = command(&host, spec)
            .ok_or_else(|| format!("{} is not installed on that machine.", spec.label))?;
        if let Some(room) = host.room() {
            crate::machine::fits(spec.label, spec.need_gb, room)
                .map_err(|e| format!("On that machine: {e}"))?;
        }
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let tx = Mutex::new(Some(tx));
        let code: Arc<Mutex<Option<i32>>> = Arc::default();
        let (log, exit) = (sink.clone(), code.clone());
        // The master's stream; when the connection ends it is dropped, so the reader sees the end too.
        let events = Sink::new(move |e: Value| match e["kind"].as_str() {
            Some("out") => {
                let bytes = B64
                    .decode(e["data"].as_str().unwrap_or(""))
                    .unwrap_or_default();
                tx.lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|t| t.send(bytes).is_ok())
            }
            Some("err") => log
                .send(LspEvent::Log {
                    text: e["text"].as_str().unwrap_or("").to_string(),
                })
                .is_ok(),
            Some("exit") => {
                *exit.lock().unwrap() = e["code"].as_i64().map(|c| c as i32);
                tx.lock().unwrap().take();
                false
            }
            _ => true,
        });
        let pipe = link.call(
            "pipe_start",
            json!({"projectId": project, "program": program, "args": args}),
            Some(events),
        )?;
        let pipe = pipe.as_str().unwrap_or("").to_string();
        let key = format!("lsp-{}", uuid::Uuid::new_v4());
        self.map.lock().unwrap().insert(
            key.clone(),
            Running {
                project: project.into(),
                owner: owner.into(),
                input: Input::Remote {
                    link,
                    id: pipe,
                    seq: Arc::default(),
                },
                proc: Proc::Remote,
            },
        );
        let received = Received {
            rx,
            buf: Vec::new(),
            at: 0,
        };
        self.read(key.clone(), received, sink, code);
        Ok(key)
    }

    pub fn send(&self, key: &str, text: &str) -> Result<(), String> {
        // The map is not held while writing: a slow server only holds up its own messages.
        let input = self
            .map
            .lock()
            .unwrap()
            .get(key)
            .map(|r| r.input.clone())
            .ok_or("That language server is not running.")?;
        match input {
            Input::Local(stdin) => {
                let mut w = stdin.lock().unwrap();
                w.write_all(&frame(text))
                    .and_then(|_| w.flush())
                    .map_err(|e| format!("The language server stopped: {e}"))
            }
            Input::Remote { link, id, seq } => {
                let n = seq.fetch_add(1, Ordering::SeqCst);
                link.tell(
                    "pipe_send",
                    json!({"id": id, "seq": n, "data": B64.encode(frame(text))}),
                );
                Ok(())
            }
        }
    }

    pub fn stop(&self, key: &str) {
        let run = self.map.lock().unwrap().remove(key);
        if let Some(run) = run {
            run.end();
        }
    }

    fn stop_where(&self, f: impl Fn(&Running) -> bool) {
        let keys: Vec<String> = {
            let map = self.map.lock().unwrap();
            map.iter()
                .filter(|(_, r)| f(r))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for k in keys {
            self.stop(&k);
        }
    }

    /// Stops a window's servers (before its interface reloads or when it closes).
    pub fn stop_owner(&self, owner: &str) {
        self.stop_where(|r| r.owner == owner);
    }

    pub fn stop_project(&self, project: &str) {
        self.stop_where(|r| r.project == project);
    }

    pub fn stop_all(&self) {
        self.stop_where(|_| true);
    }

    pub fn running(&self) -> usize {
        self.map.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_reads_messages() {
        let a = r#"{"jsonrpc":"2.0","id":1,"result":"é"}"#;
        let b = r#"{"jsonrpc":"2.0","method":"x"}"#;
        let mut bytes = frame(a);
        // A server may send a Content-Type header too, in any case.
        bytes.extend_from_slice(
            b"content-length: 30\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n",
        );
        bytes.extend_from_slice(b.as_bytes());
        let mut r = BufReader::new(&bytes[..]);
        assert_eq!(read_message(&mut r).as_deref(), Some(a));
        assert_eq!(read_message(&mut r).as_deref(), Some(b));
        assert_eq!(read_message(&mut r), None);
        // The length counts bytes, not characters.
        assert_eq!((a.chars().count(), a.len()), (37, 38));
        assert!(
            String::from_utf8(frame(a))
                .unwrap()
                .starts_with("Content-Length: 38\r\n")
        );
    }

    #[test]
    fn stops_at_a_cut_message() {
        let mut r = BufReader::new(&b"Content-Length: 50\r\n\r\n{\"short\":1}"[..]);
        assert_eq!(read_message(&mut r), None);
    }

    #[test]
    fn knows_its_servers() {
        assert!(spec("php").is_ok() && spec("cobol").is_err());
        for s in SPECS {
            assert!(s.npm.is_empty() == s.entry.is_empty(), "{}", s.id);
            let st = status(&Here, s);
            assert!(!st.install.is_empty() || !st.missing.is_empty(), "{}", s.id);
        }
    }

    #[test]
    fn talks_to_a_running_server() {
        // A stand-in server: echo what comes in, through the same framing (no real language server needed).
        let Some(node) = crate::local_servers::find("node") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("echo.js");
        std::fs::write(&script, "process.stdin.pipe(process.stdout);").unwrap();
        let mut cmd = Command::new(node);
        cmd.arg(&script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        hide(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&frame(r#"{"id":7}"#)).unwrap();
        drop(stdin);
        let mut r = BufReader::new(child.stdout.take().unwrap());
        assert_eq!(read_message(&mut r).as_deref(), Some(r#"{"id":7}"#));
        let _ = child.wait();
    }
    /// A shown master as the interface sees it: answers the generic questions from a folder and runs real pipes.
    struct FakeMaster {
        data: PathBuf,
        pipes: crate::pipes::Pipes,
    }

    impl Link for FakeMaster {
        fn call(
            &self,
            cmd: &str,
            args: Value,
            events: Option<Sink<Value>>,
        ) -> Result<Value, String> {
            match cmd {
                "host_info" => Ok(
                    json!({"dataDir": self.data.to_string_lossy(), "sep": std::path::MAIN_SEPARATOR.to_string()}),
                ),
                "host_find" => Ok(json!(crate::pipes::host::find(
                    args["program"].as_str().unwrap()
                ))),
                "host_exists" => Ok(json!(Path::new(args["path"].as_str().unwrap()).exists())),
                "machine_room" => Ok(json!([64.0, 0.0])),
                "pipe_start" => {
                    assert_eq!(args["projectId"], "p-on-master");
                    let a: Vec<String> = serde_json::from_value(args["args"].clone()).unwrap();
                    let id = self.pipes.start(
                        args["program"].as_str().unwrap(),
                        &a,
                        &self.data,
                        events.unwrap(),
                    )?;
                    Ok(json!(id))
                }
                _ => Err(format!("This master does not know the command {cmd}.")),
            }
        }
        fn tell(&self, cmd: &str, args: Value) {
            // Out of order on purpose, as a master's threads may take them.
            let pipes = self.pipes.clone();
            let cmd = cmd.to_string();
            std::thread::spawn(move || match cmd.as_str() {
                "pipe_send" => {
                    let _ = pipes.send(
                        args["id"].as_str().unwrap(),
                        args["seq"].as_u64().unwrap(),
                        args["data"].as_str().unwrap(),
                    );
                }
                "pipe_stop" => pipes.stop(args["id"].as_str().unwrap()),
                _ => {}
            });
        }
    }

    #[test]
    fn runs_a_server_on_a_shown_master_through_its_pipe() {
        if crate::local_servers::find("node").is_none() {
            return;
        }
        let data = tempfile::tempdir().unwrap();
        let link = Arc::new(FakeMaster {
            data: data.path().to_path_buf(),
            pipes: crate::pipes::Pipes::default(),
        });
        let host = On::new(link.clone()).unwrap();
        let php = spec("php").unwrap();
        assert_eq!(status(&host, php).found, "");
        // Qanuvi's own copy on that machine: a stand-in that echoes what comes in.
        let entry = data
            .path()
            .join("language-servers/php/node_modules/intelephense/lib");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(
            entry.join("intelephense.js"),
            "process.stdin.pipe(process.stdout);",
        )
        .unwrap();
        assert_eq!(status(&host, php).found, "qanuvi");
        assert!(status(&host, php).install.contains("that machine"));

        let servers = Servers::default();
        let (tx, rx) = mpsc::channel();
        let sink = Sink::new(move |e: LspEvent| tx.send(e).is_ok());
        let key = servers
            .start_on(link.clone(), "p-on-master", "main", "php", sink)
            .unwrap();
        for i in 0..20 {
            servers.send(&key, &format!(r#"{{"id":{i}}}"#)).unwrap();
        }
        for i in 0..20 {
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                LspEvent::Message { text } => assert_eq!(text, format!(r#"{{"id":{i}}}"#)),
                other => panic!("{other:?}"),
            }
        }
        servers.stop_project("p-on-master");
        let ended = loop {
            match rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
                LspEvent::Exit { .. } => break true,
                _ => continue,
            }
        };
        assert!(ended && servers.running() == 0);
        // Too little memory there: it says so and does not start.
        struct Small(FakeMaster);
        impl Link for Small {
            fn call(
                &self,
                cmd: &str,
                args: Value,
                e: Option<Sink<Value>>,
            ) -> Result<Value, String> {
                if cmd == "machine_room" {
                    Ok(json!([1.0, 0.0]))
                } else {
                    self.0.call(cmd, args, e)
                }
            }
            fn tell(&self, cmd: &str, args: Value) {
                self.0.tell(cmd, args)
            }
        }
        let small = Arc::new(Small(FakeMaster {
            data: data.path().to_path_buf(),
            pipes: crate::pipes::Pipes::default(),
        }));
        let err = servers
            .start_on(small, "p-on-master", "main", "php", Sink::new(|_| true))
            .unwrap_err();
        assert!(err.starts_with("On that machine:"), "{err}");
    }
    type Answer = mpsc::Sender<Result<Value, String>>;

    /// A real master over its line protocol (`qanuvi-master --stdio`), as the app's connection speaks it.
    struct StdioLink {
        child: Mutex<std::process::Child>,
        input: Mutex<ChildStdin>,
        pending: Arc<Mutex<HashMap<u64, Answer>>>,
        streams: Arc<Mutex<HashMap<String, Sink<Value>>>>,
        next: AtomicU64,
    }

    impl StdioLink {
        fn open(exe: &str, data: &str) -> Arc<Self> {
            let mut child = Command::new(exe)
                .args(["--stdio", "--data", data])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let out = child.stdout.take().unwrap();
            let link = Arc::new(Self {
                input: Mutex::new(child.stdin.take().unwrap()),
                child: Mutex::new(child),
                pending: Arc::default(),
                streams: Arc::default(),
                next: AtomicU64::new(0),
            });
            let (pending, streams) = (link.pending.clone(), link.streams.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(name) = msg["stream"].as_str() {
                        let mut all = streams.lock().unwrap();
                        if all
                            .get(name)
                            .is_some_and(|s| s.send(msg["event"].clone()).is_err())
                        {
                            all.remove(name);
                        }
                    } else if let Some(tx) = msg["id"]
                        .as_u64()
                        .and_then(|n| pending.lock().unwrap().remove(&n))
                    {
                        let _ = tx.send(match msg.get("err") {
                            Some(e) => Err(e.as_str().unwrap_or("").to_string()),
                            None => Ok(msg["ok"].clone()),
                        });
                    }
                }
            });
            link
        }

        fn write(&self, id: u64, cmd: &str, args: Value) {
            let mut w = self.input.lock().unwrap();
            let _ = writeln!(w, "{}", json!({"id": id, "cmd": cmd, "args": args}));
            let _ = w.flush();
        }

        /// As the app's disconnect: its streams end, then the connection.
        fn close(&self) {
            self.streams.lock().unwrap().clear();
            let _ = self.child.lock().unwrap().kill();
        }
    }

    impl Link for StdioLink {
        fn call(
            &self,
            cmd: &str,
            mut args: Value,
            events: Option<Sink<Value>>,
        ) -> Result<Value, String> {
            let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(s) = events {
                self.streams.lock().unwrap().insert(format!("s{id}"), s);
                args["onEvent"] = json!(format!("s{id}"));
            }
            let (tx, rx) = mpsc::channel();
            self.pending.lock().unwrap().insert(id, tx);
            self.write(id, cmd, args);
            rx.recv_timeout(std::time::Duration::from_secs(30))
                .map_err(|_| "no answer".to_string())?
        }
        fn tell(&self, cmd: &str, args: Value) {
            let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
            self.write(id, cmd, args);
        }
    }

    /// Live check (migration M1b) against a real master: QANUVI_LIVE_MASTER (its program), QANUVI_LIVE_DATA (a
    /// scratch data folder) and QANUVI_LIVE_PROJECT (a scratch project folder). Run with --ignored.
    #[test]
    #[ignore]
    fn live_runs_a_server_on_a_real_master() {
        let (Ok(exe), Ok(data), Ok(project)) = (
            std::env::var("QANUVI_LIVE_MASTER"),
            std::env::var("QANUVI_LIVE_DATA"),
            std::env::var("QANUVI_LIVE_PROJECT"),
        ) else {
            panic!("set QANUVI_LIVE_MASTER, QANUVI_LIVE_DATA and QANUVI_LIVE_PROJECT");
        };
        let entry = Path::new(&data).join("language-servers/php/node_modules/intelephense/lib");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(
            entry.join("intelephense.js"),
            "console.error('stand-in ready'); process.stdin.pipe(process.stdout);",
        )
        .unwrap();
        let link = StdioLink::open(&exe, &data);
        let p = link
            .call("projects_add", json!({"path": project}), None)
            .unwrap();
        let pid = p["id"].as_str().unwrap().to_string();
        let host = On::new(link.clone()).unwrap();
        let php = status(&host, spec("php").unwrap());
        println!("status on the master: {php:?}");
        assert_eq!(php.found, "qanuvi");
        let servers = Servers::default();
        let (tx, rx) = mpsc::channel();
        let key = servers
            .start_on(
                link.clone(),
                &pid,
                "main",
                "php",
                Sink::new(move |e: LspEvent| tx.send(e).is_ok()),
            )
            .unwrap();
        let big = "x".repeat(100_000);
        for i in 0..30 {
            servers
                .send(
                    &key,
                    &format!(r#"{{"id":{i},"pad":"{}"}}"#, if i == 7 { &big } else { "" }),
                )
                .unwrap();
        }
        let mut got = 0;
        let mut logged = false;
        while got < 30 {
            match rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap() {
                LspEvent::Message { text } => {
                    assert!(
                        text.starts_with(&format!(r#"{{"id":{got},"#)),
                        "message {got} out of order"
                    );
                    got += 1;
                }
                LspEvent::Log { text } => {
                    println!("server log: {text}");
                    logged |= text == "stand-in ready"
                }
                LspEvent::Exit { code } => panic!("ended early: {code:?}"),
            }
        }
        println!("30 messages back in order (one of 100 KB), error output seen: {logged}");
        // The master goes away: the server is told it stopped.
        link.close();
        loop {
            match rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap() {
                LspEvent::Exit { code } => {
                    println!("exit after the connection ended: {code:?}");
                    break;
                }
                _ => continue,
            }
        }
        assert_eq!(servers.running(), 0);
    }
}
