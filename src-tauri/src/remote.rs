//! One machine's connections to other masters (Milestones 16 and 20, migration M3b): connects to
//! `qanuvi-master --stdio` through SSH, or to a paired computer through the relay, and passes calls to it. The desktop
//! app uses it for This PC's servers ("local" starts the master program next to the app, with its own data folder, for
//! trying the feature without a server); a master uses the same code for the servers it connects to itself.

use crate::sink::Sink;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

type Reply = Result<Value, String>;

/// Told the id of a connection that ended by itself (network gone, the other master restarted).
pub type OnEnd = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Connection {
    /// The SSH or local master process (none for a relay connection).
    child: Mutex<Option<Child>>,
    /// Where lines to the master go: the process's input, or the relay session's outbox.
    stdin: Mutex<Box<dyn Write + Send>>,
    closed: AtomicBool,
    pending: Mutex<HashMap<u64, mpsc::Sender<Reply>>>,
    streams: Mutex<HashMap<String, Sink<Value>>>,
    next: AtomicU64,
    pub info: Mutex<Value>,
}

#[derive(Default)]
pub struct Remotes {
    map: Mutex<HashMap<String, Arc<Connection>>>,
    /// Local ports that reach a master's localhost port (migration M1), by machine key and its port.
    forwards: Mutex<HashMap<(String, u16), crate::pipes::Forward>>,
    /// This machine's lasting id: the first entry of every path's `seen` list (migration M3c).
    me: Mutex<String>,
    /// The connection a path goes through when its first machine is not connected here (M5c part 2: the app's
    /// link to This PC's master, which holds This PC's server connections).
    through: Mutex<Option<String>>,
    /// The machines a connect is under way to (their hello not in yet).
    starting: Mutex<std::collections::HashSet<String>>,
}

/// How many machines a path may hold below the window (owner, M3: at most 3 levels).
pub const MAX_DEPTH: usize = 3;

pub fn too_deep() -> String {
    format!(
        "That machine is more than {MAX_DEPTH} steps away. Qanuvi reaches machines at most {MAX_DEPTH} steps below the window."
    )
}

pub fn a_loop() -> String {
    "That path comes back to a machine it has already passed, so the call is refused.".into()
}

/// A machine's key in the window: the server ids along its path joined with "/" ("vps1" is a server of this
/// machine, "vps1/vps2" a server of vps1). Server ids are UUIDs, so "/" never appears inside one.
/// The app's link to This PC's own master (migration M5a; remote.ts THIS_PC): every call about This PC goes on it.
pub const THIS_PC: &str = "this-computer";

pub fn split_path(key: &str) -> Result<Vec<String>, String> {
    let parts: Vec<String> = key.split('/').map(str::to_string).collect();
    if parts.iter().any(|p| p.trim().is_empty()) {
        return Err(format!("\"{key}\" is not a machine's path."));
    }
    if parts.len() > MAX_DEPTH {
        return Err(too_deep());
    }
    Ok(parts)
}

/// The way to a machine along a path (migration M3c): this machine's connection to the first one, and the rest of
/// the path for it to pass on. A call to the end of the path goes as it is; any other goes as `remote_call` on the
/// first machine with the rest of the path in `via` and the machines passed so far in `seen`.
pub struct Routed {
    conn: Arc<Connection>,
    rest: Vec<String>,
    seen: Vec<String>,
}

impl Routed {
    fn wrap(&self, cmd: &str, args: Value, streams: bool) -> (String, Value) {
        match self.rest.split_first() {
            None => (cmd.to_string(), args),
            Some((next, via)) => (
                "remote_call".to_string(),
                json!({"master": next, "via": via, "seen": self.seen, "cmd": cmd, "args": args, "streams": streams}),
            ),
        }
    }
}

impl crate::pipes::Link for Routed {
    fn call(&self, cmd: &str, args: Value, events: Option<Sink<Value>>) -> Reply {
        let (cmd, args) = self.wrap(cmd, args, events.is_some());
        self.conn.call(&cmd, args, events)
    }
    fn tell(&self, cmd: &str, args: Value) {
        let (cmd, args) = self.wrap(cmd, args, false);
        self.conn.tell(&cmd, args)
    }
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
    pub fn call(&self, cmd: &str, mut args: Value, events: Option<Sink<Value>>) -> Reply {
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

    /// Sends a command without waiting for its answer (the answer is dropped when it comes).
    pub fn tell(&self, cmd: &str, args: Value) {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.send(&json!({"id": id, "cmd": cmd, "args": args}));
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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
            let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
            // A stream nobody listens to any more is forgotten (a port link that closed, a window that went).
            if let Some(sink) = streams.get(stream)
                && sink.send(msg["event"].clone()).is_err()
            {
                streams.remove(stream);
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

    fn ended(&self, on_end: &OnEnd, id: &str) {
        // What it streamed ends too (a language server there is told it stopped).
        self.streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        // Closed on purpose (disconnect, removed): nobody needs telling.
        if !self.closed.load(Ordering::SeqCst) {
            on_end(id);
        }
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
    /// Sets this machine's lasting id (the start of every path it sends calls along).
    pub fn set_me(&self, machine_id: &str) {
        *self.me.lock().unwrap_or_else(|e| e.into_inner()) = machine_id.to_string();
    }

    /// The way to a machine by its key, with the machines the call has passed already (`seen`, this one last).
    /// Refused: a path too long, a next machine already passed (a loop), or a next machine too old to pass calls on.
    pub fn route(&self, key: &str, seen: Vec<String>) -> Result<Routed, String> {
        let mut rest = split_path(key)?;
        let first = rest.remove(0);
        let conn = self.get(&first)?;
        let info = conn.info.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(id) = info["machineId"].as_str()
            && seen.iter().any(|s| s == id)
        {
            return Err(a_loop());
        }
        if !rest.is_empty() && info["forwards"] != true {
            return Err(
                "That machine runs an older Qanuvi that cannot pass calls on to its servers. Update it first."
                    .into(),
            );
        }
        Ok(Routed { conn, rest, seen })
    }

    /// A connection that sends nothing, with the hello given (tests: a connection still waiting has `Null`).
    #[cfg(test)]
    pub fn add_idle(&self, id: &str, info: Value) {
        self.map
            .lock()
            .unwrap()
            .insert(id.to_string(), tests::idle_connection(info));
    }

    /// Paths whose first machine is not connected here go through the connection `id` (M5c part 2).
    pub fn set_through(&self, id: &str) {
        *self.through.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.to_string());
    }

    /// The connection a key's path goes through (connected yet or not) when its first machine is not connected
    /// here; none when it starts here.
    pub fn through_for(&self, key: &str) -> Option<String> {
        let through = self
            .through
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        let first = key.split('/').next().unwrap_or("");
        if first == through || self.get(first).is_ok() {
            return None;
        }
        Some(through)
    }

    /// The way to a machine by its key, starting here.
    pub fn link(&self, key: &str) -> Result<Arc<dyn crate::pipes::Link>, String> {
        // M5c part 2: one of This PC's servers, held by This PC's master. That master is this machine, so the path
        // starts there with nothing passed yet, as on the window's own link to it.
        if let Some(through) = self.through_for(key) {
            let rest = split_path(key)?;
            let conn = self.get(&through)?;
            if conn.info.lock().unwrap_or_else(|e| e.into_inner())["forwards"] != true {
                return Err(
                    "This computer's background part is too old to reach its servers. Restart Qanuvi.".into(),
                );
            }
            return Ok(Arc::new(Routed {
                conn,
                rest,
                seen: vec![],
            }));
        }
        let me = self.me.lock().unwrap_or_else(|e| e.into_inner()).clone();
        // The app's link to This PC's own master (M5a) is this machine, not a step away from it: the path starts
        // there, so a way on that comes back here is still a loop. Any other link to this machine is one.
        let own = split_path(key)
            .ok()
            .filter(|p| p[0] == THIS_PC)
            .and_then(|p| self.get(&p[0]).ok())
            .is_some_and(|c| {
                c.info.lock().unwrap_or_else(|e| e.into_inner())["machineId"].as_str() == Some(&me)
            });
        let seen = if me.is_empty() || own {
            vec![]
        } else {
            vec![me]
        };
        Ok(Arc::new(self.route(key, seen)?))
    }

    pub fn get(&self, id: &str) -> Result<Arc<Connection>, String> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
            .ok_or_else(|| "Not connected to this master.".into())
    }

    /// Connects (or answers the existing connection) and returns the master's hello. A connect to a machine that is
    /// being connected already waits for that one (M5c part 2: the window asks This PC's master while its start-up
    /// connect is under way), so it never answers before the hello is in, nor makes a second connection.
    pub fn connect(&self, on_end: OnEnd, id: &str, transport: Transport) -> Reply {
        let since = std::time::Instant::now();
        loop {
            {
                let mut starting = self.starting.lock().unwrap_or_else(|e| e.into_inner());
                if !starting.contains(id) {
                    if let Ok(c) = self.get(id) {
                        return Ok(c.info.lock().unwrap_or_else(|e| e.into_inner()).clone());
                    }
                    starting.insert(id.to_string());
                    break;
                }
            }
            if since.elapsed() > Duration::from_secs(35) {
                return Err("It is still being connected; try again in a moment.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let result = self.connect_new(on_end, id, transport);
        self.starting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        result
    }

    fn connect_new(&self, on_end: OnEnd, id: &str, transport: Transport) -> Reply {
        let errors = Arc::new(Mutex::new(String::new()));
        let conn = match transport {
            Transport::Process(command) => {
                self.start_process(&on_end, id, command, errors.clone())?
            }
            Transport::Relay(paired) => self.start_relay(&on_end, id, &paired)?,
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
        on_end: &OnEnd,
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
        let (c, id, on_end) = (conn.clone(), id.to_string(), on_end.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                c.on_line(&line);
            }
            c.ended(&on_end, &id);
        });
        Ok(conn)
    }

    /// A paired computer through the relay: an encrypted session carries the same lines as SSH.
    fn start_relay(
        &self,
        on_end: &OnEnd,
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
        let (c, id, on_end) = (conn.clone(), id.to_string(), on_end.clone());
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
            c.ended(&on_end, &id);
        });
        Ok(conn)
    }

    /// A local port that reaches `port` on the master's localhost (the same one again when asked twice). It ends
    /// when the master is disconnected.
    pub fn forward(&self, id: &str, port: u16) -> Result<u16, String> {
        let link = self.link(id)?;
        let mut all = self.forwards.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = all.get(&(id.to_string(), port)) {
            return Ok(f.local);
        }
        let f = crate::pipes::forward(link, port)?;
        let local = f.local;
        all.insert((id.to_string(), port), f);
        Ok(local)
    }

    pub fn unforward(&self, id: &str, port: u16) {
        self.forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(id.to_string(), port));
    }

    /// What each connected master said when it answered (its hello), by id.
    pub fn infos(&self) -> Vec<(String, Value)> {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, c)| {
                (
                    id.clone(),
                    c.info.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                )
            })
            .collect()
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

    /// Ends a connection, with the port links to it and to every machine reached through it. A nested key
    /// ("vps1/vps2") only drops the port links: the connection to it is vps1's own.
    pub fn disconnect(&self, id: &str) {
        let below = format!("{id}/");
        self.forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(m, _), _| m != id && !m.starts_with(&below));
        if id.contains('/') {
            return;
        }
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
        self.forwards
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
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

impl crate::pipes::Link for Connection {
    fn call(&self, cmd: &str, args: Value, events: Option<Sink<Value>>) -> Reply {
        Connection::call(self, cmd, args, events)
    }
    fn tell(&self, cmd: &str, args: Value) {
        Connection::tell(self, cmd, args)
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

    pub(super) fn idle_connection(info: Value) -> Arc<Connection> {
        Arc::new(Connection {
            child: Mutex::new(None),
            stdin: Mutex::new(Box::new(std::io::sink())),
            closed: AtomicBool::new(false),
            pending: Mutex::default(),
            streams: Mutex::default(),
            next: AtomicU64::new(0),
            info: Mutex::new(info),
        })
    }

    #[test]
    fn a_path_holds_at_most_three_machines() {
        assert_eq!(split_path("a").unwrap(), ["a"]);
        assert_eq!(split_path("a/b/c").unwrap(), ["a", "b", "c"]);
        assert_eq!(split_path("a/b/c/d").unwrap_err(), too_deep());
        assert!(split_path("a//b").is_err());
        assert!(split_path("").is_err());
    }

    #[test]
    fn a_call_along_a_path_goes_as_remote_call_with_the_rest() {
        let r = Routed {
            conn: idle_connection(json!({})),
            rest: vec!["b".into(), "c".into()],
            seen: vec!["pc".into()],
        };
        let (cmd, args) = r.wrap("projects_list", json!({"x": 1}), true);
        assert_eq!(cmd, "remote_call");
        assert_eq!(
            args,
            json!({"master": "b", "via": ["c"], "seen": ["pc"], "cmd": "projects_list", "args": {"x": 1}, "streams": true})
        );
        let end = Routed {
            conn: idle_connection(json!({})),
            rest: vec![],
            seen: vec![],
        };
        assert_eq!(
            end.wrap("hello", json!({}), false),
            ("hello".into(), json!({}))
        );
    }

    #[test]
    fn a_route_refuses_loops_and_old_masters() {
        let remotes = Remotes::default();
        remotes.set_me("pc");
        let add = |id: &str, info: Value| {
            remotes
                .map
                .lock()
                .unwrap()
                .insert(id.to_string(), idle_connection(info));
        };
        add("back", json!({"machineId": "pc", "forwards": true}));
        add("old", json!({"machineId": "o"}));
        add("new", json!({"machineId": "n", "forwards": true}));
        assert_eq!(remotes.link("back").err().unwrap(), a_loop());
        assert!(remotes.link("old").is_ok());
        assert!(
            remotes
                .link("old/x")
                .err()
                .unwrap()
                .contains("older Qanuvi")
        );
        assert!(remotes.link("new/x/y").is_ok());
        assert_eq!(remotes.link("new/x/y/z").err().unwrap(), too_deep());
        assert!(remotes.link("gone/x").is_err());
        // M5a: the app's own link to This PC's master is no loop; a way on from there back here still is.
        add(THIS_PC, json!({"machineId": "pc", "forwards": true}));
        assert!(remotes.link(THIS_PC).is_ok());
        assert!(remotes.link(&format!("{THIS_PC}/x")).is_ok());
        assert_eq!(remotes.link("back").err().unwrap(), a_loop());
    }

    // M5c part 2: a path whose first machine is not connected here goes through This PC's master, the whole path
    // passed on to it with nothing seen yet; one connected here (the test master next to the app) stays direct.
    #[test]
    fn this_pcs_servers_go_through_its_master() {
        let remotes = Remotes::default();
        remotes.set_me("pc");
        assert!(remotes.through_for("vps").is_none());
        remotes.set_through(THIS_PC);
        assert_eq!(remotes.through_for("vps/x").as_deref(), Some(THIS_PC));
        assert!(remotes.through_for(THIS_PC).is_none());
        // Not connected to This PC's master yet: the caller makes that link first.
        assert!(remotes.link("vps").is_err());
        let add = |id: &str, info: Value| {
            remotes
                .map
                .lock()
                .unwrap()
                .insert(id.to_string(), idle_connection(info));
        };
        add(THIS_PC, json!({"machineId": "pc", "forwards": true}));
        add("local", json!({"machineId": "t", "forwards": true}));
        assert!(remotes.through_for("local/x").is_none());
        let via = Routed {
            conn: remotes.get(THIS_PC).unwrap(),
            rest: split_path("vps/x/y").unwrap(),
            seen: vec![],
        };
        assert_eq!(
            via.wrap("hello", json!({}), false),
            (
                "remote_call".into(),
                json!({"master": "vps", "via": ["x", "y"], "seen": [], "cmd": "hello", "args": {}, "streams": false})
            )
        );
        assert!(remotes.link("vps/x/y").is_ok());
        assert_eq!(remotes.link("vps/x/y/z").err().unwrap(), too_deep());
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
