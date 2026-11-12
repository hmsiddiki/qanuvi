//! Language servers (15i): the same servers VS Code uses, spoken to over the Language Server Protocol. Nothing is
//! bundled. A server is installed on demand, after the user's yes, into Qanuvi's data folder (npm servers) or through
//! the tool the language already has (rust-analyzer through rustup). One is found on PATH when the user installed it
//! already. A server starts on the first file of its language, runs at below-normal priority, ends with Qanuvi (a job
//! object on Windows), and stops when its project closes or after a while unused (the window decides that).
//!
//! The window speaks the protocol itself (src/lib/lsp). This side only frames the messages: `Content-Length` headers
//! out, whole JSON messages back.

use crate::sink::Sink;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

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

/// Where Qanuvi's own copy of an npm server lives.
fn own_dir(spec: &Spec) -> PathBuf {
    crate::paths::data_dir()
        .join("language-servers")
        .join(spec.id)
}

fn own_entry(spec: &Spec) -> Option<PathBuf> {
    if spec.npm.is_empty() {
        return None;
    }
    let p = own_dir(spec).join("node_modules").join(spec.entry);
    p.is_file().then_some(p)
}

/// rustup puts a rust-analyzer proxy on PATH even when the component is missing; asking it for its version tells.
fn rust_analyzer_works(program: &Path) -> bool {
    let mut cmd = Command::new(program);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide(&mut cmd);
    cmd.status().is_ok_and(|s| s.success())
}

/// How a server starts, if it can: the program and its arguments.
pub fn command(spec: &Spec) -> Option<(PathBuf, Vec<String>)> {
    let args: Vec<String> = spec.args.iter().map(|a| a.to_string()).collect();
    if let Some(entry) = own_entry(spec)
        && let Some(node) = crate::local_servers::find("node")
    {
        let mut all = vec![entry.to_string_lossy().into_owned()];
        all.extend(args);
        return Some((node, all));
    }
    let program = crate::local_servers::find(spec.program)?;
    if spec.id == "rust" && !rust_analyzer_works(&program) {
        return None;
    }
    Some((program, args))
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub id: &'static str,
    pub label: &'static str,
    pub serves: &'static str,
    /// "qanuvi" (installed by Qanuvi), "path" (found on this computer), "" (not installed).
    pub found: &'static str,
    /// What installing does, in words; empty when Qanuvi cannot install it here.
    pub install: String,
    /// Why it cannot be installed here (no Node.js, no rustup).
    pub missing: String,
}

pub fn status(spec: &'static Spec) -> Status {
    let found = if own_entry(spec).is_some() {
        "qanuvi"
    } else if command(spec).is_some() {
        "path"
    } else {
        ""
    };
    let (install, missing) = if spec.npm.is_empty() {
        if crate::local_servers::find("rustup").is_some() {
            (
                "Runs: rustup component add rust-analyzer".to_string(),
                String::new(),
            )
        } else {
            (
                String::new(),
                "rust-analyzer comes with rustup; install Rust from rustup.rs first.".to_string(),
            )
        }
    } else if crate::local_servers::find("npm").is_some() {
        (
            format!(
                "Downloads {} from npm into Qanuvi's data folder (nothing is installed for the whole computer).",
                spec.npm.join(" and ")
            ),
            String::new(),
        )
    } else {
        (
            String::new(),
            "This server runs on Node.js; install Node.js from nodejs.org first.".to_string(),
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

pub fn statuses() -> Vec<Status> {
    SPECS.iter().map(status).collect()
}

/// Installs a server (blocking; npm can take a minute). Only called after the user said yes in the window.
pub fn install(spec: &'static Spec) -> Result<Status, String> {
    let mut cmd = if spec.npm.is_empty() {
        let rustup = crate::local_servers::find("rustup")
            .ok_or("rust-analyzer comes with rustup; install Rust from rustup.rs first.")?;
        let mut c = Command::new(rustup);
        c.args(["component", "add", "rust-analyzer"]);
        c
    } else {
        let npm = crate::local_servers::find("npm")
            .ok_or("This server runs on Node.js; install Node.js from nodejs.org first.")?;
        let dir = own_dir(spec);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not make {}: {e}", dir.display()))?;
        // A package.json of its own, so npm keeps the install in this folder and never looks above it.
        let pj = dir.join("package.json");
        if !pj.is_file() {
            std::fs::write(&pj, "{\"private\":true}\n").map_err(|e| e.to_string())?;
        }
        let mut c = Command::new(npm);
        c.args([
            "install",
            "--no-audit",
            "--no-fund",
            "--omit=dev",
            "--prefix",
        ])
        .arg(&dir)
        .args(spec.npm)
        .current_dir(&dir);
        c
    };
    cmd.stdin(Stdio::null());
    hide(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("Could not start the install: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return Err(format!(
            "Installing {} failed:\n{}",
            spec.label,
            tail.join("\n")
        ));
    }
    let st = status(spec);
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

struct Running {
    project: String,
    owner: String,
    stdin: Arc<Mutex<ChildStdin>>,
    child: Child,
    _job: crate::browser::Job,
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

impl Servers {
    /// Starts a server for a project folder. Its messages go to `sink` until it ends.
    pub fn start(
        &self,
        project: &str,
        owner: &str,
        root: &Path,
        id: &str,
        sink: Sink<LspEvent>,
    ) -> Result<String, String> {
        let spec = spec(id)?;
        let (program, args) = command(spec)
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
        let map = self.map.clone();
        let k = key.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Some(text) = read_message(&mut r) {
                if sink.send(LspEvent::Message { text }).is_err() {
                    break;
                }
            }
            let code = map
                .lock()
                .unwrap()
                .remove(&k)
                .and_then(|mut run| {
                    let _ = run.child.kill();
                    run.child.wait().ok()
                })
                .and_then(|s| s.code());
            let _ = sink.send(LspEvent::Exit { code });
        });
        self.map.lock().unwrap().insert(
            key.clone(),
            Running {
                project: project.into(),
                owner: owner.into(),
                stdin: Arc::new(Mutex::new(stdin)),
                child,
                _job: job,
            },
        );
        Ok(key)
    }

    pub fn send(&self, key: &str, text: &str) -> Result<(), String> {
        // The map is not held while writing: a slow server only holds up its own messages.
        let stdin = self
            .map
            .lock()
            .unwrap()
            .get(key)
            .map(|r| r.stdin.clone())
            .ok_or("That language server is not running.")?;
        let mut w = stdin.lock().unwrap();
        w.write_all(&frame(text))
            .and_then(|_| w.flush())
            .map_err(|e| format!("The language server stopped: {e}"))
    }

    pub fn stop(&self, key: &str) {
        let run = self.map.lock().unwrap().remove(key);
        if let Some(mut run) = run {
            let _ = run.child.kill();
            let _ = run.child.wait();
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
            let st = status(s);
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
}
