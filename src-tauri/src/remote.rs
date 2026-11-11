//! The desktop's side of a master without a window (Milestones 16 and 20): connects to `qanuvi-master --stdio` through
//! SSH, or to a paired computer through the relay, and passes the window's calls for that master's projects to it. "This PC" starts the master program next to the app,
//! with its own data folder (for trying the feature without a server).

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;
use tauri::ipc::Channel;

type Reply = Result<Value, String>;

pub struct Connection {
    /// The SSH or local master process (none for a relay connection).
    child: Mutex<Option<Child>>,
    /// Where lines to the master go: the process's input, or the relay session's outbox.
    stdin: Mutex<Box<dyn Write + Send>>,
    closed: AtomicBool,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    streams: Mutex<HashMap<String, Channel<Value>>>,
    next: AtomicU64,
    pub info: Mutex<Value>,
}

#[derive(Default)]
pub struct Remotes {
    map: Mutex<HashMap<String, Arc<Connection>>>,
}

/// Checks an SSH target ("user@host", "host", "host:port" is not SSH syntax; use an SSH config alias for ports).
pub fn check_target(target: &str) -> Result<(), String> {
    let ok = !target.is_empty()
        && target.len() <= 200
        && !target.starts_with('-')
        && target
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@.-_".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "\"{target}\" is not an SSH address. Use user@host, a host name, or an alias from your SSH config."
        ))
    }
}

/// Checks the program to start on the server (a path or a name, no shell syntax).
pub fn check_command(command: &str) -> Result<(), String> {
    let ok = !command.is_empty()
        && command.len() <= 300
        && command
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "\"{command}\" is not a program path (letters, digits and / . _ - ~ only)."
        ))
    }
}

/// The command that reaches the master: SSH with keys only (Qanuvi cannot answer a password prompt), or the master
/// program next to the app for "This PC".
/// The SSH key file to use, with `~` for the home folder; it must exist.
pub fn key_file(path: &str) -> Result<std::path::PathBuf, String> {
    let p = path.trim();
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let full = match p.strip_prefix("~") {
        Some(rest) => std::path::PathBuf::from(home).join(rest.trim_start_matches(['/', '\\'])),
        None => std::path::PathBuf::from(p),
    };
    if p.starts_with('-') || !full.is_file() {
        return Err(format!("The SSH key file {p} was not found."));
    }
    Ok(full)
}

pub fn command_for(
    target: &str,
    program: &str,
    identity: Option<&str>,
    local_data: &std::path::Path,
) -> Result<Command, String> {
    if target == "local" || target == "self" {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let name = if cfg!(windows) {
            "qanuvi-master.exe"
        } else {
            "qanuvi-master"
        };
        let master = exe.with_file_name(name);
        if !master.is_file() {
            return Err(format!(
                "The master program was not found next to Qanuvi ({}).",
                master.display()
            ));
        }
        let mut c = Command::new(master);
        c.arg("--stdio").arg("--data").arg(local_data);
        return Ok(c);
    }
    check_target(target)?;
    check_command(program)?;
    let mut c = Command::new("ssh");
    // A key chosen for this server (only that key is offered).
    if let Some(k) = identity.map(str::trim).filter(|k| !k.is_empty()) {
        c.arg("-i")
            .arg(key_file(k)?)
            .args(["-o", "IdentitiesOnly=yes"]);
    }
    c.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ConnectTimeout=15",
        target,
        program,
        "--stdio",
    ]);
    Ok(c)
}

impl Connection {
    fn send(&self, msg: &Value) -> Result<(), String> {
        let mut w = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(w, "{msg}")
            .and_then(|_| w.flush())
            .map_err(|_| "The connection to the master has ended.".to_string())
    }

    /// Sends a command and waits for its answer (as long as it takes: a test run can be long).
    pub fn call(&self, cmd: &str, mut args: Value, events: Option<Channel<Value>>) -> Reply {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(ch) = events {
            let name = format!("s{id}");
            self.streams
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.clone(), ch);
            if !args.is_object() {
                args = json!({});
            }
            args["onEvent"] = json!(name);
        }
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        self.send(&json!({"id": id, "cmd": cmd, "args": args}))?;
        rx.recv()
            .unwrap_or_else(|_| Err("The connection to the master has ended.".into()))
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(mut c) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// One line from the master: an answer to a call, or an event for a stream.
    fn on_line(&self, line: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(stream) = msg["stream"].as_str() {
            let streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ch) = streams.get(stream) {
                let _ = ch.send(msg["event"].clone());
            }
        } else if let Some(n) = msg["id"].as_u64() {
            let tx = self
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&n);
            if let Some(tx) = tx {
                let reply = match msg.get("err") {
                    Some(e) => Err(e.as_str().unwrap_or("Error").to_string()),
                    None => Ok(msg["ok"].clone()),
                };
                let _ = tx.send(reply);
            }
        }
    }

    fn ended(&self, app: &tauri::AppHandle, id: &str) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let _ = app.emit("remote-closed", json!({"id": id}));
    }
}

/// How to reach a master.
pub enum Transport {
    /// SSH, or the master program next to the app.
    Process(Command),
    /// A paired computer through the relay.
    Relay(crate::relay_client::Paired),
}

/// Sends whole lines to the relay session's thread.
struct LineSender {
    tx: mpsc::Sender<String>,
    buf: Vec<u8>,
}

impl Write for LineSender {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(b);
        while let Some(i) = self.buf.iter().position(|c| *c == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            self.tx
                .send(String::from_utf8_lossy(&line[..line.len() - 1]).to_string())
                .map_err(|_| std::io::Error::other("the session ended"))?;
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Remotes {
    pub fn get(&self, id: &str) -> Result<Arc<Connection>, String> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| "Not connected to this master.".into())
    }

    /// Connects (or answers the existing connection) and returns the master's hello.
    pub fn connect(&self, app: tauri::AppHandle, id: &str, transport: Transport) -> Reply {
        if let Ok(c) = self.get(id) {
            return Ok(c.info.lock().unwrap_or_else(|e| e.into_inner()).clone());
        }
        let errors = Arc::new(Mutex::new(String::new()));
        let conn = match transport {
            Transport::Process(command) => self.start_process(&app, id, command, errors.clone())?,
            Transport::Relay(paired) => self.start_relay(&app, id, &paired)?,
        };
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string(), conn.clone());
        let (tx, rx) = mpsc::channel();
        {
            let conn = conn.clone();
            std::thread::spawn(move || {
                let _ = tx.send(conn.call("hello", json!({}), None));
            });
        }
        let hello = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| Err("The master did not answer in 30 seconds.".into()));
        match hello {
            Ok(info) => {
                *conn.info.lock().unwrap_or_else(|e| e.into_inner()) = info.clone();
                Ok(info)
            }
            Err(e) => {
                self.disconnect(id);
                std::thread::sleep(Duration::from_millis(200));
                let said = errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .trim()
                    .to_string();
                Err(explain(if said.is_empty() { &e } else { &said }))
            }
        }
    }

    fn start_process(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        mut command: Command,
        errors: Arc<Mutex<String>>,
    ) -> Result<Arc<Connection>, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Could not start the connection: {e}"))?;
        let stdin = child.stdin.take().ok_or("No input to the connection.")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("No output from the connection.")?;
        let mut stderr = child.stderr.take().ok_or("No error output.")?;
        let conn = Arc::new(Connection {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(Box::new(stdin)),
            closed: AtomicBool::new(false),
            pending: Mutex::default(),
            streams: Mutex::default(),
            next: AtomicU64::new(0),
            info: Mutex::new(Value::Null),
        });
        // What SSH or the master says on its error output explains a failed connection.
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            *errors.lock().unwrap_or_else(|e| e.into_inner()) = buf;
        });
        let (c, id, app) = (conn.clone(), id.to_string(), app.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                c.on_line(&line);
            }
            c.ended(&app, &id);
        });
        Ok(conn)
    }

    /// A paired computer through the relay: an encrypted session carries the same lines as SSH.
    fn start_relay(
        &self,
        app: &tauri::AppHandle,
        id: &str,
        paired: &crate::relay_client::Paired,
    ) -> Result<Arc<Connection>, String> {
        let mut conversation = crate::relay_client::open_session(paired)?;
        let (tx, rx) = mpsc::channel::<String>();
        let conn = Arc::new(Connection {
            child: Mutex::new(None),
            stdin: Mutex::new(Box::new(LineSender {
                tx,
                buf: Vec::new(),
            })),
            closed: AtomicBool::new(false),
            pending: Mutex::default(),
            streams: Mutex::default(),
            next: AtomicU64::new(0),
            info: Mutex::new(Value::Null),
        });
        let (c, id, app) = (conn.clone(), id.to_string(), app.clone());
        std::thread::spawn(move || {
            'session: loop {
                if c.closed.load(Ordering::SeqCst) {
                    conversation.close();
                    break;
                }
                match conversation.poll_line() {
                    Ok(Some(line)) => c.on_line(&line),
                    Ok(None) => {}
                    Err(_) => break,
                }
                loop {
                    match rx.try_recv() {
                        Ok(line) => {
                            if conversation.send_line(&line).is_err() {
                                break 'session;
                            }
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            conversation.close();
                            break 'session;
                        }
                    }
                }
            }
            c.ended(&app, &id);
        });
        Ok(conn)
    }

    /// The masters connected now.
    pub fn connected(&self) -> Vec<String> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    pub fn disconnect(&self, id: &str) {
        let conn = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if let Some(c) = conn {
            c.close();
        }
    }

    pub fn disconnect_all(&self) {
        let all: Vec<_> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .collect();
        for (_, c) in all {
            c.close();
        }
    }
}

/// Plain words for the usual SSH failures.
pub fn explain(text: &str) -> String {
    let t = text.to_lowercase();
    let hint = if t.contains("permission denied") {
        "SSH refused the sign-in. Qanuvi signs in with SSH keys only: add your public key to the server (ssh-copy-id) and check that `ssh <address>` works without a password."
    } else if t.contains("could not resolve") || t.contains("name or service not known") {
        "The server's name was not found. Check the address."
    } else if t.contains("timed out") || t.contains("connection refused") {
        "The server did not answer on its SSH port."
    } else if t.contains("not found") || t.contains("no such file") {
        "The master program was not found on the server. Install qanuvi-master there, or give its full path."
    } else if t.contains("host key verification failed") {
        "The server is not known yet: connect once with `ssh <address>` in a terminal to accept its key."
    } else {
        ""
    };
    let first = text.lines().last().unwrap_or(text).trim();
    if hint.is_empty() {
        first.to_string()
    } else {
        format!("{hint} ({first})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_ssh_addresses_and_programs() {
        assert!(check_target("deploy@vps.example.com").is_ok());
        assert!(check_target("myvps").is_ok());
        assert!(check_target("-oProxyCommand=x").is_err());
        assert!(check_target("host; rm -rf ~").is_err());
        assert!(check_command("~/.local/bin/qanuvi-master").is_ok());
        assert!(check_command("qanuvi-master && evil").is_err());
    }

    #[test]
    fn finds_the_key_file_and_refuses_odd_ones() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("vps_key");
        std::fs::write(&key, "k").unwrap();
        assert_eq!(key_file(&key.to_string_lossy()).unwrap(), key);
        assert!(key_file("-oProxyCommand=x").is_err());
        assert!(key_file(&dir.path().join("missing").to_string_lossy()).is_err());
        let cmd = command_for(
            "root@host",
            "qanuvi-master",
            Some(&key.to_string_lossy()),
            dir.path(),
        )
        .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "-i");
        assert!(args.contains(&"IdentitiesOnly=yes".to_string()));
        assert_eq!(
            &args[args.len() - 3..],
            ["root@host", "qanuvi-master", "--stdio"]
        );
    }

    #[test]
    fn explains_the_usual_failures() {
        assert!(explain("user@h: Permission denied (publickey).").starts_with("SSH refused"));
        assert!(
            explain("bash: qanuvi-master: command not found").starts_with("The master program")
        );
        assert_eq!(explain("something else"), "something else");
    }
}
