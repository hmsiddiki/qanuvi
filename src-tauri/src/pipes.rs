//! The two pipes a master lends to a viewer (migration M1, docs/MIGRATION-VIEWER.md).
//!
//! - A program pipe runs a program on the master's machine and streams its input and output over the connection.
//!   The bytes go through untouched: the master knows nothing about what the program speaks (a language server, a
//!   debug adapter), so the viewer frames and reads it.
//! - A port link reaches a port on the master's localhost. A viewer's [`forward`] listens on a free local port and
//!   links every connection to it through the master (the preview of a dev server on a shown master).
//!
//! Both end with the connection that opened them. The master answers each command on its own thread, so writes carry
//! a sequence number and go to the program or port strictly in that order.

use crate::sink::Sink;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Bytes read at a time from a program or a socket.
const CHUNK: usize = 32 * 1024;
/// Writes that may wait for an earlier one (beyond that the viewer is broken, and the pipe ends).
const MAX_HELD: usize = 4096;

/// Writes in sequence order, holding any that arrive early.
struct Ordered {
    next: u64,
    held: BTreeMap<u64, Vec<u8>>,
    w: Box<dyn Write + Send>,
}

impl Ordered {
    fn new(w: Box<dyn Write + Send>) -> Self {
        Self {
            next: 0,
            held: BTreeMap::new(),
            w,
        }
    }

    fn put(&mut self, seq: u64, bytes: Vec<u8>) -> std::io::Result<()> {
        if seq < self.next {
            return Ok(());
        }
        if self.held.len() >= MAX_HELD {
            return Err(std::io::Error::other("too many writes out of order"));
        }
        self.held.insert(seq, bytes);
        let mut wrote = false;
        while let Some(b) = self.held.remove(&self.next) {
            self.w.write_all(&b)?;
            self.next += 1;
            wrote = true;
        }
        if wrote {
            self.w.flush()?;
        }
        Ok(())
    }
}

enum Kind {
    Program {
        child: Child,
        _job: crate::browser::Job,
    },
    Port {
        socket: TcpStream,
    },
}

struct Open {
    kind: Kind,
    input: Arc<Mutex<Ordered>>,
}

impl Open {
    fn end(self) {
        match self.kind {
            Kind::Program { mut child, .. } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Kind::Port { socket } => {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }
}

/// The program pipes and port links a master has open, by id ("pipe-…" / "port-…").
#[derive(Default, Clone)]
pub struct Pipes {
    map: Arc<Mutex<HashMap<String, Open>>>,
}

fn hide(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window; below-normal priority, so a busy tool never slows the machine's own work.
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

pub fn is_pipe(id: &str) -> bool {
    id.starts_with("pipe-") || id.starts_with("port-")
}

impl Pipes {
    /// Runs `program` in `cwd`. Its output goes to `sink` as `{kind: "out", data}` (base64), its error output as
    /// `{kind: "err", text}` lines, and its end as `{kind: "exit", code}`.
    pub fn start(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
        sink: Sink<Value>,
    ) -> Result<String, String> {
        if program.trim().is_empty() {
            return Err("Which program?".into());
        }
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start {program} on this machine: {e}"))?;
        let job = crate::browser::Job::for_child(&child);
        let stdin = child.stdin.take().ok_or("The program has no input.")?;
        let mut stdout = child.stdout.take().ok_or("The program has no output.")?;
        let stderr = child.stderr.take();
        let id = format!("pipe-{}", uuid::Uuid::new_v4());
        self.lock().insert(
            id.clone(),
            Open {
                kind: Kind::Program { child, _job: job },
                input: Arc::new(Mutex::new(Ordered::new(Box::new(stdin)))),
            },
        );
        if let Some(err) = stderr {
            let s = sink.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if s.send(json!({"kind": "err", "text": line})).is_err() {
                        break;
                    }
                }
            });
        }
        let (map, key) = (self.map.clone(), id.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0u8; CHUNK];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let event = json!({"kind": "out", "data": B64.encode(&buf[..n])});
                        if sink.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
            let open = map.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
            let code = open.and_then(|o| match o.kind {
                Kind::Program { mut child, .. } => {
                    let _ = child.kill();
                    child.wait().ok().and_then(|s| s.code())
                }
                Kind::Port { .. } => None,
            });
            let _ = sink.send(json!({"kind": "exit", "code": code}));
        });
        Ok(id)
    }

    /// Opens a link to `port` on this machine's localhost. What the port sends goes to `sink` as
    /// `{kind: "data", data}` (base64); its end as `{kind: "closed"}`.
    pub fn open_port(&self, port: u16, sink: Sink<Value>) -> Result<String, String> {
        if port == 0 {
            return Err("Which port?".into());
        }
        let addrs = [
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port)),
        ];
        let socket = addrs
            .iter()
            .find_map(|a| TcpStream::connect_timeout(a, Duration::from_secs(5)).ok())
            .ok_or_else(|| format!("Nothing answers on port {port} of this machine."))?;
        let _ = socket.set_nodelay(true);
        let mut reader = socket.try_clone().map_err(|e| e.to_string())?;
        let writer = socket.try_clone().map_err(|e| e.to_string())?;
        let id = format!("port-{}", uuid::Uuid::new_v4());
        self.lock().insert(
            id.clone(),
            Open {
                kind: Kind::Port { socket },
                input: Arc::new(Mutex::new(Ordered::new(Box::new(writer)))),
            },
        );
        let (map, key) = (self.map.clone(), id.clone());
        std::thread::spawn(move || {
            let mut buf = vec![0u8; CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let event = json!({"kind": "data", "data": B64.encode(&buf[..n])});
                        if sink.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
            let open = map.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
            if let Some(o) = open {
                o.end();
            }
            let _ = sink.send(json!({"kind": "closed"}));
        });
        Ok(id)
    }

    /// Writes base64 `data` to a program's input or a port, as write number `seq` (from 0) of that pipe.
    pub fn send(&self, id: &str, seq: u64, data: &str) -> Result<(), String> {
        let bytes = B64
            .decode(data)
            .map_err(|_| "The data is not base64.".to_string())?;
        // The map is not held while writing: a slow program only holds up its own pipe.
        let input = self
            .lock()
            .get(id)
            .map(|o| o.input.clone())
            .ok_or("That pipe has ended.")?;
        let wrote = input
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(seq, bytes);
        wrote.map_err(|e| {
            self.stop(id);
            format!("That pipe has ended: {e}")
        })
    }

    /// Ends a program pipe or a port link (its end event still follows).
    pub fn stop(&self, id: &str) {
        let open = self.lock().remove(id);
        if let Some(o) = open {
            o.end();
        }
    }

    pub fn running(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Open>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A connection to a master, as the viewer side of a port forward needs it.
pub trait Link: Send + Sync + 'static {
    /// Sends a command and waits for its answer; `events` gets the command's stream.
    fn call(&self, cmd: &str, args: Value, events: Option<Sink<Value>>) -> Result<Value, String>;
    /// Sends a command without waiting for its answer.
    fn tell(&self, cmd: &str, args: Value);
}

/// A local port that reaches a port on a master's localhost. It stops when dropped.
pub struct Forward {
    pub local: u16,
    pub remote: u16,
    stop: Arc<AtomicBool>,
    sockets: Arc<Mutex<Vec<TcpStream>>>,
}

impl Drop for Forward {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop so it sees the stop.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from(([127, 0, 0, 1], self.local)),
            Duration::from_millis(300),
        );
        for s in self
            .sockets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}

/// Listens on any free port of this computer's 127.0.0.1 (so it never clashes with its own dev servers) and links
/// each connection to `remote` on the master's localhost.
pub fn forward(link: Arc<dyn Link>, remote: u16) -> Result<Forward, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let sockets: Arc<Mutex<Vec<TcpStream>>> = Arc::default();
    let (stopped, open) = (stop.clone(), sockets.clone());
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            let Ok(socket) = socket else { continue };
            if let Ok(s) = socket.try_clone() {
                let mut all = open.lock().unwrap_or_else(|e| e.into_inner());
                all.retain(|s| s.peer_addr().is_ok());
                all.push(s);
            }
            let link = link.clone();
            std::thread::spawn(move || carry(link, socket, remote));
        }
    });
    Ok(Forward {
        local,
        remote,
        stop,
        sockets,
    })
}

/// Carries one local connection through the master until either side ends.
fn carry(link: Arc<dyn Link>, socket: TcpStream, remote: u16) {
    let _ = socket.set_nodelay(true);
    let Ok(to_local) = socket.try_clone() else {
        return;
    };
    let ended = Arc::new(AtomicBool::new(false));
    let done = ended.clone();
    let events = Sink::new(move |e: Value| match e["kind"].as_str() {
        Some("data") => {
            let bytes = B64
                .decode(e["data"].as_str().unwrap_or(""))
                .unwrap_or_default();
            if (&to_local).write_all(&bytes).is_err() {
                done.store(true, Ordering::SeqCst);
                let _ = to_local.shutdown(Shutdown::Both);
                return false;
            }
            true
        }
        Some("closed") => {
            done.store(true, Ordering::SeqCst);
            let _ = to_local.shutdown(Shutdown::Both);
            false
        }
        _ => true,
    });
    let id = match link.call("port_open", json!({"port": remote}), Some(events)) {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        Err(_) => {
            let _ = socket.shutdown(Shutdown::Both);
            return;
        }
    };
    let mut seq = 0u64;
    let mut from_local = socket;
    let mut buf = vec![0u8; CHUNK];
    loop {
        match from_local.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                link.tell(
                    "port_send",
                    json!({"id": id, "seq": seq, "data": B64.encode(&buf[..n])}),
                );
                seq += 1;
            }
        }
    }
    if !ended.load(Ordering::SeqCst) {
        link.tell("port_close", json!({"id": id}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn writes_in_sequence_order_whatever_order_they_arrive_in() {
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        struct W(Arc<Mutex<Vec<u8>>>);
        impl Write for W {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut o = Ordered::new(Box::new(W(out.clone())));
        o.put(2, b"c".to_vec()).unwrap();
        o.put(0, b"a".to_vec()).unwrap();
        assert_eq!(&*out.lock().unwrap(), b"a");
        o.put(1, b"b".to_vec()).unwrap();
        o.put(1, b"x".to_vec()).unwrap();
        assert_eq!(&*out.lock().unwrap(), b"abc");
    }

    /// An echo server on a free port of this computer.
    fn echo_server() -> u16 {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in l.incoming().map_while(Result::ok) {
                std::thread::spawn(move || {
                    let mut r = s.try_clone().unwrap();
                    let mut w = s;
                    let _ = std::io::copy(&mut r, &mut w);
                });
            }
        });
        port
    }

    #[test]
    fn links_a_port_on_this_machine_and_ends_it() {
        let port = echo_server();
        let pipes = Pipes::default();
        let (tx, rx) = mpsc::channel();
        let sink = Sink::new(move |e: Value| tx.send(e).is_ok());
        let id = pipes.open_port(port, sink).unwrap();
        pipes.send(&id, 1, &B64.encode(b" world")).unwrap();
        pipes.send(&id, 0, &B64.encode(b"hello")).unwrap();
        let mut got = Vec::new();
        while got.len() < 11 {
            let e = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(e["kind"], "data");
            got.extend(B64.decode(e["data"].as_str().unwrap()).unwrap());
        }
        assert_eq!(got, b"hello world");
        pipes.stop(&id);
        let e = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(e["kind"], "closed");
        assert_eq!(pipes.running(), 0);
        assert!(pipes.send(&id, 2, "").is_err());
        assert!(pipes.open_port(0, Sink::new(|_| true)).is_err());
    }
}
