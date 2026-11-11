//! Both ends of a relay connection (Milestone 20). A handler (the laptop or phone) pairs with a code and opens
//! encrypted sessions; a master (the computer being reached) stays connected to the relay, answers pairings, and serves
//! each session with the same commands as over SSH (master.rs), keeping an activity log and a way to stop everything.

use crate::relay::{
    Code, HandlerCard, MasterCard, Msg, Session, b64, new_id, open_once, pair_finish, pair_start,
    random_bytes, seal_once, unb64,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// A WebSocket to the relay.
pub struct Link {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
}

pub fn check_relay_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let lower = u.to_ascii_lowercase();
    let local = lower.starts_with("ws://127.0.0.1") || lower.starts_with("ws://localhost");
    if (lower.starts_with("wss://") && u.len() > 6 || local)
        && u.len() <= 300
        && !u.chars().any(|c| c.is_whitespace() || "\"<>\\".contains(c))
    {
        Ok(u.to_string())
    } else {
        Err(
            "The relay address must start with wss:// (ws:// only for this computer, for testing)."
                .into(),
        )
    }
}

impl Link {
    pub fn connect(url: &str) -> Result<Self, String> {
        let url = check_relay_url(url)?;
        let (ws, _) = tungstenite::connect(url.as_str())
            .map_err(|e| format!("The relay could not be reached: {e}"))?;
        let timeout = Some(Duration::from_millis(50));
        match ws.get_ref() {
            MaybeTlsStream::Plain(s) => s.set_read_timeout(timeout).map_err(|e| e.to_string())?,
            MaybeTlsStream::Rustls(s) => s
                .get_ref()
                .set_read_timeout(timeout)
                .map_err(|e| e.to_string())?,
            _ => {}
        }
        Ok(Self { ws })
    }

    pub fn send(&mut self, m: &Msg) -> Result<(), String> {
        self.ws
            .send(Message::text(serde_json::to_string(m).unwrap_or_default()))
            .map_err(|_| "The connection to the relay ended.".to_string())
    }

    /// The next message, or None after a short wait.
    pub fn poll(&mut self) -> Result<Option<Msg>, String> {
        match crate::relay_server::read(&mut self.ws) {
            Ok(Some(t)) => Ok(serde_json::from_str(&t).ok()),
            Ok(None) => Ok(None),
            Err(crate::relay_server::Gone) => Err("The connection to the relay ended.".into()),
        }
    }

    /// Waits for a message the test picks (relay errors stop the wait).
    fn wait(&mut self, secs: u64, mut pick: impl FnMut(&Msg) -> bool) -> Result<Msg, String> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if let Some(m) = self.poll()? {
                if let Msg::Error { message } = &m {
                    return Err(message.clone());
                }
                if pick(&m) {
                    return Ok(m);
                }
            }
        }
        Err("The relay did not answer in time.".into())
    }
}

// The handler's side ---------------------------------------------------------------------------------------------------

/// A computer this device is paired with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paired {
    pub relay: String,
    pub master_id: String,
    pub master_name: String,
    pub pairing: String,
    /// The pairing key (base64); kept in the credential store by the desktop app.
    pub key: String,
}

/// Pairs with the computer showing `code`.
pub fn pair(relay: &str, code: &str, my_name: &str) -> Result<Paired, String> {
    let code = Code::parse(code)?;
    let mut link = Link::connect(relay)?;
    let start = pair_start(&code);
    link.send(&Msg::PairJoin {
        slot: code.slot.clone(),
        handler: 0,
        spake: b64(&start.message),
    })?;
    let Msg::PairReply { spake, body, .. } =
        link.wait(30, |m| matches!(m, Msg::PairReply { .. }))?
    else {
        unreachable!()
    };
    let key = pair_finish(start, &unb64(&spake)?)?;
    let card: MasterCard = open_once(&key, "master card", &unb64(&body)?)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("The code was not right (or not this computer's).")?;
    let mine = HandlerCard {
        name: my_name.chars().take(60).collect(),
        pairing: card.pairing.clone(),
    };
    link.send(&Msg::PairConfirm {
        slot: code.slot,
        handler: 0,
        body: b64(&seal_once(
            &key,
            "handler card",
            &serde_json::to_vec(&mine).unwrap_or_default(),
        )?),
    })?;
    let Msg::PairDone { ok, .. } = link.wait(30, |m| matches!(m, Msg::PairDone { .. }))? else {
        unreachable!()
    };
    if !ok {
        return Err("The other computer refused the pairing.".into());
    }
    Ok(Paired {
        relay: check_relay_url(relay)?,
        master_id: card.master_id,
        master_name: card.name,
        pairing: card.pairing,
        key: crate::relay::b64(&key),
    })
}

/// An encrypted session with a paired master, carrying the same lines as over SSH.
pub struct Conversation {
    link: Link,
    session: u64,
    crypto: Session,
}

pub fn open_session(p: &Paired) -> Result<Conversation, String> {
    let key: [u8; 32] = unb64(&p.key)?
        .try_into()
        .map_err(|_| "The pairing key is damaged; pair again.".to_string())?;
    let mut link = Link::connect(&p.relay)?;
    let nonce = random_bytes::<16>()?;
    link.send(&Msg::Connect {
        master_id: p.master_id.clone(),
        pairing: p.pairing.clone(),
        nonce: b64(&nonce),
        session: 0,
    })?;
    match link.wait(30, |m| matches!(m, Msg::Accept { .. } | Msg::Reject { .. }))? {
        Msg::Accept {
            session,
            nonce: theirs,
        } => {
            let crypto = Session::new(&key, &nonce, &unb64(&theirs)?, false)?;
            Ok(Conversation {
                link,
                session,
                crypto,
            })
        }
        Msg::Reject { reason, .. } => Err(reason),
        _ => unreachable!(),
    }
}

impl Conversation {
    pub fn send_line(&mut self, line: &str) -> Result<(), String> {
        let body = b64(&self.crypto.seal(line.as_bytes())?);
        self.link.send(&Msg::Data {
            session: self.session,
            body,
        })
    }

    /// The next line from the master, None after a short wait; Err when the session ended.
    pub fn poll_line(&mut self) -> Result<Option<String>, String> {
        match self.link.poll()? {
            Some(Msg::Data { session, body }) if session == self.session => {
                let plain = self.crypto.open(&unb64(&body)?)?;
                Ok(Some(String::from_utf8_lossy(&plain).to_string()))
            }
            Some(Msg::Close { session }) if session == self.session => {
                Err("The other computer ended the connection.".into())
            }
            _ => Ok(None),
        }
    }

    pub fn close(&mut self) {
        let _ = self.link.send(&Msg::Close {
            session: self.session,
        });
    }
}

// The master's side ----------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub pairing: String,
    pub name: String,
    pub added: i64,
    /// The pairing key (base64). The file is readable by this user only.
    pub key: String,
}

/// What a master keeps about the relay (relay.json in its data folder).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub relay: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub master_id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Activity {
    pub at: i64,
    pub device: String,
    pub what: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Live {
    pub session: u64,
    pub device: String,
    pub since: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Status {
    pub relay: String,
    pub enabled: bool,
    pub connected: bool,
    pub name: String,
    pub devices: Vec<(String, String, i64)>,
    pub sessions: Vec<Live>,
    /// A pairing code waiting, with seconds left.
    pub code: Option<(String, u64)>,
    pub error: String,
}

struct Pending {
    code: Code,
    until: Instant,
    /// After the handler joined: its connection, the key, and the pairing id.
    joined: Option<(u64, [u8; 32], String)>,
}

struct LiveSession {
    device: String,
    since: i64,
    input: Sender<Vec<u8>>,
    crypto: Arc<Mutex<Session>>,
}

#[derive(Default)]
struct Inner {
    config: Config,
    connected: bool,
    error: String,
    pending: Option<Pending>,
    sessions: HashMap<u64, LiveSession>,
    activity: Vec<Activity>,
    /// Bumped to make the running link reconnect or stop (settings changed).
    generation: u64,
}

/// The master's relay side, shared by its link thread, its sessions and the commands that manage it.
pub struct RelayMaster {
    inner: Mutex<Inner>,
    outbox: Mutex<Option<Sender<Msg>>>,
    file: PathBuf,
}

/// Reads lines from a channel (a session's decrypted input) as a stream.
struct ChannelReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for ChannelReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(b) => {
                    self.buf = b;
                    self.pos = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Seals each line a session writes and sends it through the link.
struct SealingWriter {
    session: u64,
    crypto: Arc<Mutex<Session>>,
    outbox: Sender<Msg>,
    pending: Vec<u8>,
}

impl Write for SealingWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(b);
        while let Some(i) = self.pending.iter().position(|c| *c == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=i).collect();
            let sealed = self
                .crypto
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .seal(&line[..line.len() - 1])
                .map_err(std::io::Error::other)?;
            self.outbox
                .send(Msg::Data {
                    session: self.session,
                    body: b64(&sealed),
                })
                .map_err(|_| std::io::Error::other("link gone"))?;
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn now_secs() -> i64 {
    crate::store::now()
}

impl RelayMaster {
    pub fn open(data: &Path) -> Arc<Self> {
        let file = data.join("relay.json");
        let config: Config = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Arc::new(Self {
            inner: Mutex::new(Inner {
                config,
                ..Inner::default()
            }),
            outbox: Mutex::new(None),
            file,
        })
    }

    fn save(&self, c: &Config) {
        let tmp = self.file.with_extension("tmp");
        if std::fs::write(&tmp, "").is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            if std::fs::write(&tmp, serde_json::to_string_pretty(c).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(&tmp, &self.file);
            }
        }
    }

    fn log(&self, device: &str, what: &str) {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.activity.push(Activity {
            at: now_secs(),
            device: device.into(),
            what: what.into(),
        });
        let n = i.activity.len();
        if n > 300 {
            i.activity.drain(..n - 300);
        }
    }

    pub fn status(&self) -> Status {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        Status {
            relay: i.config.relay.clone(),
            enabled: i.config.enabled,
            connected: i.connected,
            name: i.config.name.clone(),
            devices: i
                .config
                .devices
                .iter()
                .map(|d| (d.pairing.clone(), d.name.clone(), d.added))
                .collect(),
            sessions: i
                .sessions
                .iter()
                .map(|(s, l)| Live {
                    session: *s,
                    device: l.device.clone(),
                    since: l.since,
                })
                .collect(),
            code: i
                .pending
                .as_ref()
                .filter(|p| p.until > now)
                .map(|p| (p.code.shown(), (p.until - now).as_secs())),
            error: i.error.clone(),
        }
    }

    pub fn activity(&self) -> Vec<Activity> {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.activity.iter().rev().take(100).cloned().collect()
    }

    /// Turns remote access on (with a relay address and this computer's name) or off.
    pub fn configure(
        self: &Arc<Self>,
        relay: &str,
        name: &str,
        enabled: bool,
        master: Arc<crate::master::Master>,
    ) -> Result<(), String> {
        let relay = if enabled {
            check_relay_url(relay)?
        } else {
            relay.trim().to_string()
        };
        {
            let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if i.config.relay != relay {
                // Another relay: it gives this computer a new id.
                i.config.master_id.clear();
                i.config.secret.clear();
            }
            i.config.relay = relay;
            i.config.name = name.chars().take(60).collect();
            i.config.enabled = enabled;
            i.generation += 1;
            let c = i.config.clone();
            drop(i);
            self.save(&c);
        }
        if !enabled {
            self.stop_sessions();
        }
        self.start(master);
        Ok(())
    }

    /// Starts the link (when remote access is on); a running one notices a newer generation and gives way.
    pub fn start(self: &Arc<Self>, master: Arc<crate::master::Master>) {
        let (enabled, generation) = {
            let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (i.config.enabled && !i.config.relay.is_empty(), i.generation)
        };
        if !enabled {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || me.run(generation, master));
    }

    fn current(&self, generation: u64) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .generation
            == generation
    }

    /// Keeps a link to the relay while remote access is on, reconnecting after a drop.
    fn run(self: Arc<Self>, generation: u64, master: Arc<crate::master::Master>) {
        let mut wait = Duration::from_secs(1);
        while self.current(generation) {
            match self.connected_loop(generation, &master) {
                Ok(()) => return,
                Err(e) => {
                    let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    i.connected = false;
                    i.error = e;
                }
            }
            std::thread::sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(60));
        }
    }

    fn connected_loop(
        &self,
        generation: u64,
        master: &Arc<crate::master::Master>,
    ) -> Result<(), String> {
        let (relay, id, secret) = {
            let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            (
                i.config.relay.clone(),
                i.config.master_id.clone(),
                i.config.secret.clone(),
            )
        };
        let mut link = Link::connect(&relay)?;
        link.send(&Msg::HelloMaster {
            master_id: id,
            secret,
        })?;
        let (tx, rx) = channel::<Msg>();
        *self.outbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx.clone());
        loop {
            if !self.current(generation) {
                return Ok(());
            }
            if let Some(m) = link.poll()? {
                self.on_message(m, &tx, master)?;
            }
            while let Ok(out) = rx.try_recv() {
                link.send(&out)?;
            }
        }
    }

    fn on_message(
        &self,
        m: Msg,
        tx: &Sender<Msg>,
        master: &Arc<crate::master::Master>,
    ) -> Result<(), String> {
        match m {
            Msg::Welcome { master_id, secret } => {
                let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                i.connected = true;
                i.error.clear();
                if i.config.master_id != master_id || i.config.secret != secret {
                    i.config.master_id = master_id;
                    i.config.secret = secret;
                    let c = i.config.clone();
                    drop(i);
                    self.save(&c);
                }
            }
            Msg::Error { message } => {
                self.inner.lock().unwrap_or_else(|e| e.into_inner()).error = message;
            }
            Msg::PairJoin {
                slot,
                handler,
                spake,
            } => {
                let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let (id, name) = (i.config.master_id.clone(), i.config.name.clone());
                let Some(p) = i.pending.as_mut().filter(|p| {
                    p.code.slot == slot && p.until > Instant::now() && p.joined.is_none()
                }) else {
                    return Ok(());
                };
                let start = pair_start(&p.code);
                let message = start.message.clone();
                let key = pair_finish(start, &unb64(&spake)?)?;
                let pairing = new_id()?;
                let card = MasterCard {
                    master_id: id,
                    name,
                    pairing: pairing.clone(),
                };
                p.joined = Some((handler, key, pairing));
                let body = b64(&seal_once(
                    &key,
                    "master card",
                    &serde_json::to_vec(&card).unwrap_or_default(),
                )?);
                let _ = tx.send(Msg::PairReply {
                    handler,
                    spake: b64(&message),
                    body,
                });
            }
            Msg::PairConfirm { handler, body, .. } => {
                let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let Some((h, key, pairing)) = i.pending.as_ref().and_then(|p| p.joined.clone())
                else {
                    return Ok(());
                };
                if h != handler {
                    return Ok(());
                }
                let card: Option<HandlerCard> = open_once(&key, "handler card", &unb64(&body)?)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .filter(|c: &HandlerCard| c.pairing == pairing);
                let ok = card.is_some();
                if let Some(c) = card {
                    i.config.devices.push(Device {
                        pairing,
                        name: if c.name.trim().is_empty() {
                            "A device".into()
                        } else {
                            c.name
                        },
                        added: now_secs(),
                        key: b64(&key),
                    });
                    let cfg = i.config.clone();
                    i.pending = None;
                    drop(i);
                    self.save(&cfg);
                }
                let _ = tx.send(Msg::PairDone { handler, ok });
            }
            Msg::Connect {
                pairing,
                nonce,
                session,
                ..
            } => {
                let device = {
                    let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                    i.config
                        .devices
                        .iter()
                        .find(|d| d.pairing == pairing)
                        .cloned()
                };
                let Some(device) = device else {
                    let _ = tx.send(Msg::Reject {
                        session,
                        reason: "This device is no longer paired with that computer.".into(),
                    });
                    return Ok(());
                };
                let key: [u8; 32] = match unb64(&device.key).ok().and_then(|k| k.try_into().ok()) {
                    Some(k) => k,
                    None => return Ok(()),
                };
                let mine = random_bytes::<16>()?;
                let crypto = Arc::new(Mutex::new(Session::new(
                    &key,
                    &unb64(&nonce)?,
                    &mine,
                    true,
                )?));
                let (input, rx) = channel::<Vec<u8>>();
                self.inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .sessions
                    .insert(
                        session,
                        LiveSession {
                            device: device.name.clone(),
                            since: now_secs(),
                            input,
                            crypto: crypto.clone(),
                        },
                    );
                self.log(&device.name, "connected");
                let _ = tx.send(Msg::Accept {
                    session,
                    nonce: b64(&mine),
                });
                let out: Arc<Mutex<Box<dyn Write + Send>>> =
                    Arc::new(Mutex::new(Box::new(SealingWriter {
                        session,
                        crypto,
                        outbox: tx.clone(),
                        pending: Vec::new(),
                    })));
                let master = master.clone();
                std::thread::spawn(move || {
                    let reader = std::io::BufReader::new(ChannelReader {
                        rx,
                        buf: Vec::new(),
                        pos: 0,
                    });
                    crate::master::handle_connection(master, reader, out);
                });
            }
            Msg::Data { session, body } => {
                let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                let Some(live) = i.sessions.get(&session) else {
                    return Ok(());
                };
                let plain = live
                    .crypto
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .open(&unb64(&body)?);
                match plain {
                    Ok(mut line) => {
                        let device = live.device.clone();
                        // The activity log keeps which commands came in, not their contents.
                        let cmd = serde_json::from_slice::<serde_json::Value>(&line)
                            .ok()
                            .and_then(|v| v["cmd"].as_str().map(String::from))
                            .unwrap_or_default();
                        line.push(b'\n');
                        let _ = live.input.send(line);
                        drop(i);
                        if !cmd.is_empty() && !QUIET.contains(&cmd.as_str()) {
                            self.log(&device, &cmd);
                        }
                    }
                    Err(_) => {
                        // A message that does not open: end the session.
                        i.sessions.remove(&session);
                        let _ = tx.send(Msg::Close { session });
                    }
                }
            }
            Msg::Close { session } => {
                let ended = self
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .sessions
                    .remove(&session);
                if let Some(s) = ended {
                    self.log(&s.device, "disconnected");
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Makes a pairing code (good for 10 minutes, one try).
    pub fn pair_start(&self) -> Result<String, String> {
        let code = Code::new()?;
        let shown = code.shown();
        let slot = code.slot.clone();
        {
            let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !i.connected {
                return Err(
                    "Turn on remote access and wait until this computer is connected to the relay."
                        .into(),
                );
            }
            i.pending = Some(Pending {
                code,
                until: Instant::now() + Duration::from_secs(600),
                joined: None,
            });
        }
        self.send(Msg::PairOpen { slot })?;
        Ok(shown)
    }

    fn send(&self, m: Msg) -> Result<(), String> {
        self.outbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .ok_or("Not connected to the relay.")?
            .send(m)
            .map_err(|_| "Not connected to the relay.".into())
    }

    pub fn remove_device(&self, pairing: &str) {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        i.config.devices.retain(|d| d.pairing != pairing);
        let c = i.config.clone();
        drop(i);
        self.save(&c);
    }

    /// Ends every remote session now (the "Stop" button).
    pub fn stop_sessions(&self) {
        let ended: Vec<(u64, String)> = {
            let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            i.sessions.drain().map(|(s, l)| (s, l.device)).collect()
        };
        for (s, device) in ended {
            let _ = self.send(Msg::Close { session: s });
            self.log(&device, "stopped from this computer");
        }
    }
}

/// Commands that come in all the time and are not worth a line in the activity log.
const QUIET: [&str; 6] = [
    "hello",
    "git_status",
    "files_list_dir",
    "agent_sessions",
    "term_resize",
    "browser_agent_view",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_addresses_must_be_wss_or_this_computer() {
        assert!(check_relay_url("wss://relay.qanuvi.com").is_ok());
        assert!(check_relay_url("ws://127.0.0.1:47300").is_ok());
        assert!(check_relay_url("ws://relay.example.com").is_err());
        assert!(check_relay_url("https://relay.example.com").is_err());
    }

    /// A relay, a master and a handler on this computer: pairing, a session with real commands, the activity log,
    /// a wrong code, an unknown pairing, and stopping.
    #[test]
    fn pairs_and_works_end_to_end_through_a_relay() {
        let relay = crate::relay_server::start_local().unwrap();
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("cart.js"), "export const total = 1;\n").unwrap();
        let master = crate::master::Master::open(data.path()).unwrap().shared();
        let rm = RelayMaster::open(data.path());
        rm.configure(&relay, "Home PC", true, master.clone())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !rm.status().connected {
            assert!(
                Instant::now() < deadline,
                "the master did not reach the relay: {:?}",
                rm.status()
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        // A wrong code does not pair (and uses up the slot).
        let code = rm.pair_start().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let wrong = format!(
            "{}{}",
            &code[..code.len() - 1],
            if code.ends_with('2') { "3" } else { "2" }
        );
        assert!(pair(&relay, &wrong, "Laptop").is_err());
        assert!(rm.status().devices.is_empty());

        let code = rm.pair_start().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let paired = pair(&relay, &code, "Laptop").unwrap();
        assert_eq!(paired.master_name, "Home PC");
        assert_eq!(rm.status().devices.len(), 1);

        // A session: the same commands as over SSH, sealed end to end.
        let mut c = open_session(&paired).unwrap();
        c.send_line(&serde_json::json!({"id": 1, "cmd": "projects_add", "args": {"path": project.path().to_string_lossy()}}).to_string())
            .unwrap();
        let mut answer = None;
        let deadline = Instant::now() + Duration::from_secs(10);
        while answer.is_none() && Instant::now() < deadline {
            answer = c.poll_line().unwrap();
        }
        let v: serde_json::Value = serde_json::from_str(&answer.unwrap()).unwrap();
        assert_eq!(v["id"], 1);
        assert!(v["ok"]["id"].is_string(), "{v}");
        assert_eq!(rm.status().sessions.len(), 1);
        assert!(
            rm.activity()
                .iter()
                .any(|a| a.device == "Laptop" && a.what == "projects_add")
        );

        // A pairing the master does not know is refused.
        let mut stranger = paired.clone();
        stranger.pairing = "unknown".into();
        assert!(open_session(&stranger).is_err());

        // Stop ends the session for the handler.
        rm.stop_sessions();
        let deadline = Instant::now() + Duration::from_secs(10);
        let ended = loop {
            match c.poll_line() {
                Err(_) => break true,
                Ok(_) if Instant::now() > deadline => break false,
                Ok(_) => {}
            }
        };
        assert!(ended);
        assert!(rm.status().sessions.is_empty());

        // Removing the device: it can no longer connect.
        rm.remove_device(&paired.pairing);
        assert!(open_session(&paired).is_err());
        rm.configure(&relay, "Home PC", false, master).unwrap();
    }
}
