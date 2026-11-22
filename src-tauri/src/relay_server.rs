//! The relay server (Milestone 20): `qanuvi-relay`, a small program for a VPS or hosting. Masters and handlers both
//! connect out to it over WebSocket; it matches them up and passes their messages along. Everything between a master
//! and a handler is sealed end to end (relay.rs), so the relay sees only ids and sealed text. It keeps nothing but the
//! masters' ids and a hash of each master's secret (so nobody else can take a master's place); put it behind a TLS
//! proxy (nginx, Caddy) for wss://.
//!
//! 24rl R1: every link is a small tokio task that sleeps until something comes in, something goes out or its ping
//! timer fires (it used to be a thread waking every 50 ms), so idle links cost almost nothing and the relay is no
//! longer limited by how many threads the system allows.

use crate::relay::{Msg, new_id};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tungstenite::{Message, WebSocket};

const SLOT_LIFE: Duration = Duration::from_secs(600);
const MAX_MESSAGE: usize = 4 << 20;
/// 24rl R1: bytes read from a link at a time (tungstenite's default is 128 KB per link: 25 GB for 100k links).
const READ_CHUNK: usize = 4 << 10;
/// 24rl R1: a link whose outbox holds more than this (it reads too slowly or not at all) is ended, so one stuck
/// reader cannot fill the relay's memory.
pub const MAX_QUEUED: usize = 64 << 20;
/// How long a new connection has for its WebSocket handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// rl1: every connection is pinged this often, both ways, so a link that died quietly (wifi, sleep, a router that
/// forgot it) is found.
pub const PING_EVERY: Duration = Duration::from_secs(30);
/// rl1: a connection nothing came from for this long is gone; its sessions end on the other side too.
pub const SILENT: Duration = Duration::from_secs(90);

/// A link's outbox: counted in bytes and cut at `limit`.
#[derive(Clone)]
struct Outbox {
    tx: UnboundedSender<String>,
    queued: Arc<AtomicUsize>,
    behind: Arc<AtomicBool>,
    /// Woken once when the outbox goes over its limit, also while the link is in the middle of a send.
    stuck: Arc<tokio::sync::Notify>,
    limit: usize,
}

impl Outbox {
    fn new(limit: usize) -> (Self, UnboundedReceiver<String>) {
        let (tx, rx) = unbounded_channel();
        let out = Self {
            tx,
            queued: Default::default(),
            behind: Default::default(),
            stuck: Default::default(),
            limit,
        };
        (out, rx)
    }

    /// Queues a line; false when the link is gone or too far behind.
    fn send(&self, line: String) -> bool {
        if self.behind.load(Ordering::SeqCst) {
            return false;
        }
        let n = line.len();
        if self.queued.fetch_add(n, Ordering::SeqCst) + n > self.limit {
            self.queued.fetch_sub(n, Ordering::SeqCst);
            if !self.behind.swap(true, Ordering::SeqCst) {
                self.stuck.notify_one();
            }
            return false;
        }
        self.tx.send(line).is_ok()
    }
}

impl State {
    fn add_session(&mut self, session: u64, handler: u64, master: &str) {
        let Some(master_conn) = self.masters.get(master).map(|(c, _)| *c) else {
            return;
        };
        self.sessions
            .insert(session, (handler, master.to_string(), master_conn));
        self.by_conn.entry(handler).or_default().push(session);
        self.by_conn.entry(master_conn).or_default().push(session);
    }

    fn drop_session(&mut self, session: u64) -> Option<(u64, String, u64)> {
        let gone = self.sessions.remove(&session)?;
        for conn in [gone.0, gone.2] {
            if let Some(list) = self.by_conn.get_mut(&conn) {
                list.retain(|s| *s != session);
                if list.is_empty() {
                    self.by_conn.remove(&conn);
                }
            }
        }
        Some(gone)
    }
}

#[derive(Default)]
struct State {
    /// Masters online: id -> its connection.
    masters: HashMap<String, (u64, Outbox)>,
    /// Every connection's outbox, by connection number.
    conns: HashMap<u64, Outbox>,
    /// Open pairing slots: slot -> (master id, until).
    slots: HashMap<String, (String, Instant)>,
    /// Handlers in the middle of pairing: handler connection -> master id.
    pairing: HashMap<u64, String>,
    /// Sessions: id -> (handler connection, master id, master connection).
    sessions: HashMap<u64, (u64, String, u64)>,
    /// R4: each connection's sessions (handler and master side), so a link that ends closes its own sessions without
    /// looking through all of them (100k links ending at once took 80 s of full CPU that way).
    by_conn: HashMap<u64, Vec<u64>>,
    /// Registered masters: id -> hash of their secret (kept in a file).
    registered: HashMap<String, String>,
    /// 24rl F3: today's relayed bytes per master id (memory only: a relay restart starts the day's count again).
    usage: HashMap<String, Use>,
    /// The day `usage` was last cleared of other days.
    pruned: i64,
}

/// 24rl F3: one master's use of the relay today.
struct Use {
    /// UTC day number (unix seconds / 86400).
    day: i64,
    bytes: u64,
    /// Over the cap: when the next message may pass (the links are slowed to `slow_to`).
    next_free: Instant,
    /// When the master was last told (Usage), and at what level (0 under 80%, 1 near, 2 over).
    told: Option<(Instant, u8)>,
}

/// 24rl F3: a master is told its use at most this often while it changes (and at once when it gets near or over).
const TELL_EVERY: Duration = Duration::from_secs(60);
/// 24rl F3: the speed a master over its daily cap is slowed to, unless the relay is told another (--slow-to).
pub const SLOW_TO: u64 = 256 << 10;

fn today() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        / 86_400
}

pub struct Relay {
    state: Mutex<State>,
    next: AtomicU64,
    file: Option<PathBuf>,
    ping_every: Duration,
    silent: Duration,
    max_queued: usize,
    /// 24rl R2: the relay's own TLS address, told to apps (Welcome, Accept).
    direct: Option<String>,
    /// Tests: masters' connections are cut (as when the relay restarts or their network drops).
    cut_masters: AtomicBool,
    cut_now: tokio::sync::Notify,
    /// Test relays write no log lines (rl3).
    quiet: bool,
    /// 24rl F3: relayed bytes a master may use a day at full speed (None = no cap), and the speed after (bytes/s).
    cap: Option<u64>,
    slow_to: u64,
}

fn hash(secret: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(secret.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn text(m: &Msg) -> String {
    serde_json::to_string(m).unwrap_or_default()
}

impl Relay {
    pub fn new(data: Option<&Path>) -> Arc<Self> {
        let file = data.map(|d| d.join("masters.json"));
        let registered = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Arc::new(Self {
            state: Mutex::new(State {
                registered,
                ..State::default()
            }),
            next: AtomicU64::new(1),
            file,
            ping_every: PING_EVERY,
            silent: SILENT,
            max_queued: MAX_QUEUED,
            direct: None,
            cut_masters: Default::default(),
            cut_now: Default::default(),
            quiet: false,
            cap: None,
            slow_to: SLOW_TO,
        })
    }

    /// 24rl F3: counts a Data message against its master's daily share (both ways) and answers when it may pass:
    /// None at once, or a moment to wait for when the master is over its cap. Tells the master its use when due.
    fn charge(&self, me: u64, msg: &Msg, master_of: &Option<String>) -> Option<Instant> {
        let Msg::Data { session, body, .. } = msg else {
            return None;
        };
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (handler, master, master_conn) = st.sessions.get(session).cloned()?;
        if master_of.as_ref() != Some(&master) && me != handler {
            return None;
        }
        let now = Instant::now();
        let day = today();
        let u = st.usage.entry(master).or_insert(Use {
            day,
            bytes: 0,
            next_free: now,
            told: None,
        });
        if u.day != day {
            *u = Use {
                day,
                bytes: 0,
                next_free: now,
                told: None,
            };
        }
        u.bytes += body.len() as u64;
        let wait = match self.cap {
            Some(cap) if u.bytes > cap => {
                let start = u.next_free.max(now);
                u.next_free =
                    start + Duration::from_secs_f64(body.len() as f64 / self.slow_to.max(1) as f64);
                (start > now).then_some(start)
            }
            _ => None,
        };
        let usage = Self::usage_due(u, self.cap, now);
        if let Some(m) = usage
            && let Some(tx) = st.conns.get(&master_conn)
        {
            tx.send(text(&m));
        }
        wait
    }

    /// 24rl F3: the Usage message for a master when it is due: its level changed (near 80%, over), or a minute passed.
    /// A relay with no cap says nothing (no share to keep an eye on, and no extra messages).
    fn usage_due(u: &mut Use, cap: Option<u64>, now: Instant) -> Option<Msg> {
        cap?;
        let level = Self::level(u, cap);
        if let Some((at, was)) = u.told
            && was == level
            && now.duration_since(at) < TELL_EVERY
        {
            return None;
        }
        u.told = Some((now, level));
        Some(Self::usage_msg(u, cap))
    }

    /// 0 under 80% of the cap (or no cap), 1 near it, 2 over it.
    fn level(u: &Use, cap: Option<u64>) -> u8 {
        match cap {
            Some(c) if u.bytes > c => 2,
            Some(c) if u.bytes * 5 >= c * 4 => 1,
            _ => 0,
        }
    }

    fn usage_msg(u: &Use, cap: Option<u64>) -> Msg {
        let over = cap.is_some_and(|c| u.bytes > c);
        Msg::Usage {
            used: u.bytes,
            cap: cap.unwrap_or(0),
            slowed_until: if over { (u.day + 1) * 86_400 } else { 0 },
        }
    }

    /// Tests: cut every master's connection (and keep cutting new ones) while `on`.
    pub fn cut_masters(&self, on: bool) {
        self.cut_masters.store(on, Ordering::SeqCst);
        if on {
            self.cut_now.notify_waiters();
        }
    }

    fn save(&self, st: &State) {
        if let Some(f) = &self.file {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, serde_json::to_string(&st.registered).unwrap_or_default());
        }
    }

    /// Handles one message from connection `me`; `role` remembers what the connection is.
    fn handle(&self, me: u64, msg: Msg, master_of: &mut Option<String>) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let reply = |st: &State, to: u64, m: Msg| {
            if let Some(tx) = st.conns.get(&to) {
                let _ = tx.send(text(&m));
            }
        };
        let to_master = |st: &State, id: &str, m: Msg| -> bool {
            match st.masters.get(id) {
                Some((_, tx)) => tx.send(text(&m)),
                None => false,
            }
        };
        match msg {
            Msg::HelloMaster { master_id, secret } => {
                let (id, secret) = if master_id.is_empty() || secret.is_empty() {
                    match (new_id(), new_id()) {
                        (Ok(i), Ok(s)) => (i, s),
                        _ => return,
                    }
                } else {
                    (master_id, secret)
                };
                match st.registered.get(&id) {
                    Some(h) if *h != hash(&secret) => {
                        reply(
                            &st,
                            me,
                            Msg::Error {
                                message: "This master id belongs to another computer.".into(),
                            },
                        );
                        return;
                    }
                    Some(_) => {}
                    None => {
                        st.registered.insert(id.clone(), hash(&secret));
                        self.save(&st);
                    }
                }
                if let Some(tx) = st.conns.get(&me).cloned() {
                    st.masters.insert(id.clone(), (me, tx));
                }
                *master_of = Some(id.clone());
                // 24rl F3: other days' counts go once a day (not on every hello: 100k at once after a restart).
                let day = today();
                if st.pruned != day {
                    st.usage.retain(|_, u| u.day == day);
                    st.pruned = day;
                }
                let usage = match st.usage.get_mut(&id) {
                    Some(u) if u.day == day && self.cap.is_some() => {
                        u.told = Some((Instant::now(), Self::level(u, self.cap)));
                        Some(Self::usage_msg(u, self.cap))
                    }
                    _ => None,
                };
                reply(
                    &st,
                    me,
                    Msg::Welcome {
                        master_id: id,
                        secret,
                        direct: self.direct.clone(),
                    },
                );
                // A master coming back learns today's use at once (a new one: with its first relayed message).
                if let Some(m) = usage {
                    reply(&st, me, m);
                }
            }
            Msg::PairOpen { slot } => {
                let Some(id) = master_of.clone() else { return };
                let now = Instant::now();
                st.slots.retain(|_, (_, until)| *until > now);
                if st.slots.get(&slot).is_some_and(|(m, _)| *m != id) {
                    reply(
                        &st,
                        me,
                        Msg::Error {
                            message: "That pairing code is taken; make a new one.".into(),
                        },
                    );
                    return;
                }
                st.slots.insert(slot, (id, now + SLOT_LIFE));
            }
            Msg::PairJoin { slot, spake, .. } => {
                let now = Instant::now();
                // One try per code.
                match st.slots.remove(&slot) {
                    Some((master, until)) if until > now => {
                        st.pairing.insert(me, master.clone());
                        if !to_master(
                            &st,
                            &master,
                            Msg::PairJoin {
                                slot,
                                handler: me,
                                spake,
                            },
                        ) {
                            reply(
                                &st,
                                me,
                                Msg::Error {
                                    message: "That computer is not online.".into(),
                                },
                            );
                        }
                    }
                    _ => reply(
                        &st,
                        me,
                        Msg::Error {
                            message: "No computer is waiting for that code (it may have expired)."
                                .into(),
                        },
                    ),
                }
            }
            Msg::PairReply {
                handler,
                spake,
                body,
            } => {
                if master_of.is_some() && st.pairing.get(&handler) == master_of.as_ref() {
                    reply(
                        &st,
                        handler,
                        Msg::PairReply {
                            handler,
                            spake,
                            body,
                        },
                    );
                }
            }
            Msg::PairConfirm { slot, body, .. } => {
                if let Some(master) = st.pairing.get(&me).cloned() {
                    to_master(
                        &st,
                        &master,
                        Msg::PairConfirm {
                            slot,
                            handler: me,
                            body,
                        },
                    );
                }
            }
            Msg::PairDone { handler, ok } => {
                if master_of.is_some() && st.pairing.get(&handler) == master_of.as_ref() {
                    st.pairing.remove(&handler);
                    reply(&st, handler, Msg::PairDone { handler, ok });
                }
            }
            Msg::Connect {
                master_id,
                pairing,
                nonce,
                ..
            } => {
                let session = self.next.fetch_add(1, Ordering::SeqCst);
                if st.masters.contains_key(&master_id) {
                    st.add_session(session, me, &master_id);
                    to_master(
                        &st,
                        &master_id,
                        Msg::Connect {
                            master_id: master_id.clone(),
                            pairing,
                            nonce,
                            session,
                        },
                    );
                } else {
                    reply(
                        &st,
                        me,
                        Msg::Reject {
                            session,
                            reason: "That computer is not online.".into(),
                        },
                    );
                }
            }
            Msg::Accept { session, .. } | Msg::Reject { session, .. } if master_of.is_some() => {
                if let Some((handler, m, _)) = st.sessions.get(&session).cloned()
                    && Some(&m) == master_of.as_ref()
                {
                    let rejected = matches!(msg, Msg::Reject { .. });
                    // R2: the handler learns the relay's own TLS address with its first session.
                    let msg = match msg {
                        Msg::Accept { session, nonce, .. } => Msg::Accept {
                            session,
                            nonce,
                            direct: self.direct.clone(),
                        },
                        other => other,
                    };
                    reply(&st, handler, msg);
                    if rejected {
                        st.drop_session(session);
                    }
                }
            }
            Msg::Data { session, .. } | Msg::Close { session } => {
                let Some((handler, master, _)) = st.sessions.get(&session).cloned() else {
                    return;
                };
                let closing = matches!(msg, Msg::Close { .. });
                if master_of.as_ref() == Some(&master) {
                    reply(&st, handler, msg);
                } else if me == handler {
                    to_master(&st, &master, msg);
                }
                if closing {
                    st.drop_session(session);
                }
            }
            _ => {}
        }
    }

    /// A connection ended: forget it and close the sessions it was part of; answers how many sessions ended.
    fn gone(&self, me: u64, master_of: &Option<String>) -> usize {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.conns.remove(&me);
        st.pairing.remove(&me);
        if let Some(id) = master_of
            && st.masters.get(id).is_some_and(|(c, _)| *c == me)
        {
            st.masters.remove(id);
        }
        let mine = st.by_conn.remove(&me).unwrap_or_default();
        let mut count = 0;
        for session in mine {
            let Some((handler, _, master_conn)) = st.drop_session(session) else {
                continue;
            };
            count += 1;
            let other = if handler == me { master_conn } else { handler };
            if other != me
                && let Some(tx) = st.conns.get(&other)
            {
                tx.send(text(&Msg::Close { session }));
            }
        }
        count
    }

    /// How often a link wakes by itself: to ping, and to see whether the other side went silent.
    fn tick(&self) -> Duration {
        self.ping_every
            .min(self.silent / 3)
            .max(Duration::from_millis(10))
    }

    /// Serves one WebSocket connection until it ends.
    async fn serve_conn<S>(self: Arc<Self>, stream: S, mut from: String)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE))
            .read_buffer_size(READ_CHUNK)
            // Each message goes out at once instead of waiting in a 128 KB buffer.
            .write_buffer_size(0);
        // rl3: who it is (behind nginx the socket's address is the proxy's, so the forwarded one is used).
        // The error type is tungstenite's own (an HTTP answer), not ours to make smaller.
        #[allow(clippy::result_large_err)]
        let callback = |req: &tungstenite::handshake::server::Request,
                        res: tungstenite::handshake::server::Response| {
            let header = |name: &str| {
                req.headers()
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.split(',').next().unwrap_or("").trim().to_string())
                    .filter(|v| !v.is_empty())
            };
            if let Some(real) = header("x-real-ip").or_else(|| header("x-forwarded-for")) {
                from = real;
            }
            Ok(res)
        };
        let shake = tokio_tungstenite::accept_hdr_async_with_config(stream, callback, Some(config));
        let ws = match tokio::time::timeout(HANDSHAKE, shake).await {
            Ok(Ok(ws)) => ws,
            Ok(Err(e)) => {
                self.note(&format!("handshake failed: {e}"));
                return;
            }
            Err(_) => {
                self.note("handshake failed: none within 10 s");
                return;
            }
        };
        let me = self.next.fetch_add(1, Ordering::SeqCst);
        let opened = Instant::now();
        self.note(&format!("link {me} opened from {from}"));
        let (outbox, mut rx) = Outbox::new(self.max_queued);
        let queued = outbox.queued.clone();
        let stuck = outbox.stuck.clone();
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .conns
            .insert(me, outbox);
        let (mut sink, mut incoming) = ws.split();
        let mut master_of = None;
        let mut heard = Instant::now();
        let mut pinged = Instant::now();
        let every = self.tick();
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let why: String = loop {
            if master_of.is_some() && self.cut_masters.load(Ordering::SeqCst) {
                break "cut by a test".into();
            }
            tokio::select! {
                got = incoming.next() => match got {
                    Some(Ok(Message::Text(text))) => {
                        heard = Instant::now();
                        if let Ok(msg) = serde_json::from_str::<Msg>(text.as_str()) {
                            // 24rl F3: over its daily cap, a master's messages wait their turn here (this link
                            // reads nothing meanwhile, so its sender is slowed too); at most a few seconds each.
                            if let Some(at) = self.charge(me, &msg, &master_of) {
                                tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
                            }
                            self.handle(me, msg, &mut master_of);
                        }
                    }
                    Some(Ok(Message::Close(frame))) => break closed_why(frame),
                    // A ping (answered by the WebSocket itself), a pong or another frame: the other side is there.
                    Some(Ok(_)) => heard = Instant::now(),
                    Some(Err(e)) => break format!("read failed: {e}"),
                    None => break "closed by the other side".into(),
                },
                out = rx.recv() => match out {
                    Some(line) => {
                        let n = line.len();
                        let sent = tokio::select! {
                            sent = sink.send(Message::text(line)) => sent,
                            _ = stuck.notified() => break behind(self.max_queued),
                        };
                        queued.fetch_sub(n, Ordering::SeqCst);
                        if let Err(e) = sent {
                            break format!("message not sent: {e}");
                        }
                    }
                    None => break "outbox gone".into(),
                },
                _ = stuck.notified() => break behind(self.max_queued),
                _ = tick.tick() => {
                    // rl1: something (a pong at least) must come back; a quiet link is gone, and its sessions get a
                    // Close.
                    if heard.elapsed() > self.silent {
                        break format!("silent for {} s", heard.elapsed().as_secs());
                    }
                    // R4: idle links cost only their pings (100k idle links: about 28% of one core with a ping
                    // each way per 30 s, 0% at 300 s). Apps ping the relay themselves, so a link heard from lately
                    // is not pinged back on top; one that only reads (older apps) still is.
                    if pinged.elapsed() >= self.ping_every && heard.elapsed() >= self.ping_every {
                        pinged = Instant::now();
                        if let Err(e) = sink.send(Message::Ping(Vec::new().into())).await {
                            break format!("ping not sent: {e}");
                        }
                    }
                }
                _ = self.cut_now.notified(), if master_of.is_some() => {}
            }
        };
        // A goodbye at most 2 s long: a link that stopped reading would never take it (its socket is full).
        let _ = tokio::time::timeout(Duration::from_secs(2), sink.close()).await;
        let role = match &master_of {
            Some(id) => format!("master {}", id.chars().take(8).collect::<String>()),
            None => "handler".into(),
        };
        let heard_ago = heard.elapsed().as_secs();
        let sessions = self.gone(me, &master_of);
        self.note(&format!(
            "link {me} ({role}, from {from}) ended after {} s, last heard {heard_ago} s ago, \
             {sessions} session(s) closed: {why}",
            opened.elapsed().as_secs()
        ));
    }

    /// Takes connections until the program stops.
    async fn accept_loop(self: Arc<Self>, listener: std::net::TcpListener) -> Result<(), String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
        let mut last_error: Option<Instant> = None;
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let _ = stream.set_nodelay(true);
                    tokio::spawn(self.clone().serve_conn(stream, peer.to_string()));
                }
                Err(e) => {
                    // Out of open files and the like: wait a little instead of spinning; logged once a minute.
                    if last_error.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                        last_error = Some(Instant::now());
                        self.note(&format!("accept failed: {e}"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    /// 24rl R2: takes connections on the relay's own TLS port until the program stops. The certificate in use is the
    /// one `certs` holds at the moment of each handshake, so a renewed one is used from the next link on.
    async fn tls_accept_loop(
        self: Arc<Self>,
        listener: std::net::TcpListener,
        certs: Arc<Certs>,
    ) -> Result<(), String> {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
        let mut last_error: Option<Instant> = None;
        loop {
            match listener.accept().await {
                Ok((stream, peer)) => {
                    let _ = stream.set_nodelay(true);
                    let acceptor = tokio_rustls::TlsAcceptor::from(certs.current());
                    let relay = self.clone();
                    tokio::spawn(async move {
                        match tokio::time::timeout(HANDSHAKE, acceptor.accept(stream)).await {
                            Ok(Ok(tls)) => relay.serve_conn(tls, peer.to_string()).await,
                            Ok(Err(e)) => relay.note(&format!("TLS handshake failed: {e}")),
                            Err(_) => relay.note("TLS handshake failed: none within 10 s"),
                        }
                    });
                }
                Err(e) => {
                    if last_error.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                        last_error = Some(Instant::now());
                        self.note(&format!("accept failed (TLS port): {e}"));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    /// 24rl R2: looks at the certificate files every `every` and swaps in a renewed certificate (links already
    /// open keep theirs). A file that does not load leaves the one in use and is logged.
    async fn watch_certs(self: Arc<Self>, certs: Arc<Certs>, every: Duration) {
        loop {
            tokio::time::sleep(every).await;
            match certs.reload_if_changed() {
                Ok(true) => self.note("TLS certificate renewed: new links get the new one"),
                Ok(false) => {}
                Err(e) => self.note(&format!("TLS certificate not renewed: {e}")),
            }
        }
    }

    /// rl3: one line in the relay's log (stderr: journald on the VPS) with the time, so link ends can be lined up
    /// with nginx's log and the masters' own errors. Test relays keep quiet.
    fn note(&self, line: &str) {
        if self.quiet {
            return;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        eprintln!("{h:02}:{m:02}:{s:02} UTC {line}");
    }
}

/// 24rl R1: why a link that stopped reading was ended.
fn behind(limit: usize) -> String {
    format!(
        "too far behind: more than {} MB waiting to be sent",
        limit >> 20
    )
}

/// Why the other side closed (rl3 words, the same for the relay and its clients).
fn closed_why(frame: Option<tungstenite::protocol::CloseFrame>) -> String {
    match frame {
        Some(f) if f.reason.is_empty() => {
            format!("closed by the other side ({})", u16::from(f.code))
        }
        Some(f) => format!(
            "closed by the other side ({}: {})",
            u16::from(f.code),
            f.reason
        ),
        None => "closed by the other side".into(),
    }
}

/// The connection ended, and why (rl3: the relay logs it).
#[derive(Debug)]
pub struct Gone(pub String);

/// What one read brought.
pub enum Frame {
    Text(String),
    /// A ping, a pong or another frame: the other side is there.
    Other,
    /// Nothing within the short wait.
    Nothing,
}

/// The next text message, None after a short wait, Err when the connection is gone.
pub fn read<S: std::io::Read + std::io::Write>(
    ws: &mut WebSocket<S>,
) -> Result<Option<String>, Gone> {
    Ok(match read_frame(ws)? {
        Frame::Text(t) => Some(t),
        _ => None,
    })
}

/// The next frame of any kind (a ping read here is answered by the WebSocket itself).
pub fn read_frame<S: std::io::Read + std::io::Write>(ws: &mut WebSocket<S>) -> Result<Frame, Gone> {
    match ws.read() {
        Ok(Message::Text(t)) => Ok(Frame::Text(t.as_str().to_string())),
        Ok(Message::Close(frame)) => Err(Gone(match frame {
            Some(f) if f.reason.is_empty() => {
                format!("closed by the other side ({})", u16::from(f.code))
            }
            Some(f) => format!(
                "closed by the other side ({}: {})",
                u16::from(f.code),
                f.reason
            ),
            None => "closed by the other side".into(),
        })),
        Ok(_) => Ok(Frame::Other),
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(Frame::Nothing)
        }
        Err(e) => Err(Gone(format!("read failed: {e}"))),
    }
}

/// 24rl R2: the relay's own TLS certificate and key (PEM files, as Let's Encrypt and ISPConfig write them).
#[derive(Clone, Debug)]
pub struct TlsFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// Relay kit K1: the certificate the relay made for itself in `data`/tls-self (made on first use and kept, so its
/// pin and the relay's address never change). For a relay with only an IP address and no domain: apps check it by
/// the pin in the address (relay_client::split_pin), not by a certificate authority.
pub fn self_made(data: &Path) -> Result<TlsFiles, String> {
    let dir = data.join("tls-self");
    let files = TlsFiles {
        cert: dir.join("cert.pem"),
        key: dir.join("key.pem"),
    };
    if files.cert.is_file() && files.key.is_file() {
        return Ok(files);
    }
    let fail = |e: &dyn std::fmt::Display| {
        format!(
            "The relay could not make its certificate in {}: {e}",
            dir.display()
        )
    };
    std::fs::create_dir_all(&dir).map_err(|e| fail(&e))?;
    let made = rcgen::generate_simple_self_signed(vec!["qanuvi-relay".to_string()])
        .map_err(|e| fail(&e))?;
    let key_tmp = dir.join("key.pem.new");
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut file = o.open(&key_tmp).map_err(|e| fail(&e))?;
        std::io::Write::write_all(&mut file, made.signing_key.serialize_pem().as_bytes())
            .map_err(|e| fail(&e))?;
    }
    std::fs::write(dir.join("cert.pem.new"), made.cert.pem()).map_err(|e| fail(&e))?;
    std::fs::rename(&key_tmp, &files.key).map_err(|e| fail(&e))?;
    std::fs::rename(dir.join("cert.pem.new"), &files.cert).map_err(|e| fail(&e))?;
    Ok(files)
}

/// The pin of the first certificate in a PEM file (what goes after #pin= in the relay's address).
pub fn pin_of_file(cert: &Path) -> Result<String, String> {
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::pem::PemObject;
    let der = CertificateDer::pem_file_iter(cert)
        .ok()
        .and_then(|mut it| it.next())
        .and_then(|c| c.ok())
        .ok_or_else(|| format!("{} holds no certificate.", cert.display()))?;
    Ok(crate::relay_client::pin_of(der.as_ref()))
}

/// The certificate in use, and when its files were last changed.
pub struct Certs {
    files: TlsFiles,
    current: std::sync::RwLock<Arc<rustls::ServerConfig>>,
    stamp: Mutex<Option<(std::time::SystemTime, std::time::SystemTime)>>,
}

fn stamp_of(files: &TlsFiles) -> Option<(std::time::SystemTime, std::time::SystemTime)> {
    let changed = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    Some((changed(&files.cert)?, changed(&files.key)?))
}

fn load_tls(files: &TlsFiles) -> Result<Arc<rustls::ServerConfig>, String> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let shown = |p: &Path| p.display().to_string();
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&files.cert)
        .and_then(|it| it.collect::<Result<_, _>>())
        .map_err(|e| {
            format!(
                "The certificate {} could not be read: {e}",
                shown(&files.cert)
            )
        })?;
    if chain.is_empty() {
        return Err(format!("{} holds no certificate.", shown(&files.cert)));
    }
    let key = PrivateKeyDer::from_pem_file(&files.key)
        .map_err(|e| format!("The key {} could not be read: {e}", shown(&files.key)))?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_no_client_auth()
    .with_single_cert(chain, key)
    .map_err(|e| format!("The certificate and key do not go together: {e}"))?;
    Ok(Arc::new(config))
}

impl Certs {
    pub fn load(files: TlsFiles) -> Result<Arc<Self>, String> {
        let config = load_tls(&files)?;
        Ok(Arc::new(Self {
            stamp: Mutex::new(stamp_of(&files)),
            current: std::sync::RwLock::new(config),
            files,
        }))
    }

    fn current(&self) -> Arc<rustls::ServerConfig> {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Loads the files again when they changed; true when a new certificate is in use.
    fn reload_if_changed(&self) -> Result<bool, String> {
        let now = stamp_of(&self.files);
        let mut stamp = self.stamp.lock().unwrap_or_else(|e| e.into_inner());
        if now.is_none() || now == *stamp {
            return Ok(false);
        }
        let config = load_tls(&self.files)?;
        *self.current.write().unwrap_or_else(|e| e.into_inner()) = config;
        *stamp = now;
        Ok(true)
    }
}

/// How often the certificate files are looked at (a renewal is picked up within this time).
pub const CERT_CHECK: Duration = Duration::from_secs(600);

/// What the relay serves (qanuvi-relay's flags).
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Plain WebSocket, for a TLS proxy in front (nginx): for example 127.0.0.1:47300.
    pub listen: Option<String>,
    /// 24rl R2: the relay's own TLS port, for example 0.0.0.0:47443.
    pub tls_listen: Option<String>,
    pub tls: Option<TlsFiles>,
    /// Relay kit K1: no certificate given, so the relay uses one it made itself (`--tls-self`, kept in `data`).
    pub tls_self: bool,
    /// The address apps use for the TLS port (told to them in Welcome and Accept), for example
    /// wss://relay.qanuvi.com:47443.
    pub direct: Option<String>,
    pub data: Option<PathBuf>,
    /// How often each link is pinged (default 30 s); a link silent for three of these is gone.
    pub ping_every: Option<Duration>,
    /// 24rl F3: relayed bytes a master may use a day at full speed (--cap-per-day, in MB); None = no cap.
    pub cap_per_day: Option<u64>,
    /// 24rl F3: the speed over the cap, bytes a second (--slow-to, in KB/s); None = SLOW_TO.
    pub slow_to: Option<u64>,
}

/// Runs the relay as `options` says until it is stopped.
pub fn run_with(options: &Options) -> Result<(), String> {
    if options.listen.is_none() && options.tls_listen.is_none() {
        return Err("Nothing to listen on: give --listen, --tls-listen or both.".into());
    }
    let plain = options.listen.as_deref().map(bind).transpose()?;
    let tls = match options.tls_listen.as_deref() {
        Some(addr) => {
            let files = match (&options.tls, options.tls_self, &options.data) {
                (Some(files), _, _) => files.clone(),
                (None, true, Some(data)) => self_made(data)?,
                (None, true, None) => {
                    return Err(
                        "--tls-self needs --data <folder> (the certificate is kept there).".into(),
                    );
                }
                (None, false, _) => {
                    return Err("--tls-listen needs --cert and --key, or --tls-self.".into());
                }
            };
            Some((bind(addr)?, Certs::load(files)?))
        }
        None => None,
    };
    let mut relay = Relay::new(options.data.as_deref());
    if let Some(r) = Arc::get_mut(&mut relay) {
        r.direct = options.direct.clone();
        r.cap = options.cap_per_day;
        r.slow_to = options.slow_to.unwrap_or(SLOW_TO);
        if let Some(p) = options.ping_every {
            r.ping_every = p;
            r.silent = p * 3;
        }
    }
    runtime()?.block_on(async move {
        let mut tasks = Vec::new();
        if let Some((listener, certs)) = tls {
            tokio::spawn(relay.clone().watch_certs(certs.clone(), CERT_CHECK));
            tasks.push(tokio::spawn(relay.clone().tls_accept_loop(listener, certs)));
        }
        if let Some(listener) = plain {
            tasks.push(tokio::spawn(relay.clone().accept_loop(listener)));
        }
        for t in tasks {
            t.await.map_err(|e| e.to_string())??;
        }
        Ok(())
    })
}

/// R4: listens with a queue of 4096 waiting connections (std's default of 128 turned away most of 100k devices
/// coming back at once after a restart; the system may cap it lower, net.core.somaxconn).
fn bind(addr: &str) -> Result<std::net::TcpListener, String> {
    use std::net::ToSocketAddrs;
    let err = |e: std::io::Error| format!("Could not listen on {addr}: {e}");
    let at = addr
        .to_socket_addrs()
        .map_err(err)?
        .next()
        .ok_or_else(|| format!("Could not listen on {addr}: no address"))?;
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(at),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(err)?;
    #[cfg(unix)]
    socket.set_reuse_address(true).map_err(err)?;
    // Relay kit: [::] takes IPv4 links too (one listener for both; a server with only a public IPv6 address).
    if at.is_ipv6() && at.ip().is_unspecified() {
        socket.set_only_v6(false).map_err(err)?;
    }
    socket.bind(&at.into()).map_err(err)?;
    socket.listen(4096).map_err(err)?;
    Ok(socket.into())
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("qanuvi-relay")
        .build()
        .map_err(|e| format!("Could not start the relay: {e}"))
}

/// Runs the relay on an address like 127.0.0.1:47300 until it is stopped.
pub fn run(addr: &str, data: Option<&Path>) -> Result<(), String> {
    run_with(&Options {
        listen: Some(addr.into()),
        data: data.map(Path::to_path_buf),
        ..Options::default()
    })
}

/// Starts a relay on a free local port (for tests); answers its ws:// address.
pub fn start_local() -> Result<String, String> {
    start_local_timed(PING_EVERY, SILENT)
}

/// A test relay with its own ping and silence times (rl1 tests).
pub fn start_local_timed(ping_every: Duration, silent: Duration) -> Result<String, String> {
    Ok(start_local_relay(ping_every, silent)?.0)
}

/// A test relay and its handle (rl1 tests cut its masters off).
pub fn start_local_relay(
    ping_every: Duration,
    silent: Duration,
) -> Result<(String, Arc<Relay>), String> {
    start_test_relay(ping_every, silent, MAX_QUEUED)
}

/// 24rl F3: a test relay with a daily cap (bytes) and the speed over it (bytes a second).
pub fn start_local_capped(cap: u64, slow_to: u64) -> Result<(String, Arc<Relay>), String> {
    start_test_relay_with(PING_EVERY, SILENT, MAX_QUEUED, Some((cap, slow_to)))
}

fn start_test_relay(
    ping_every: Duration,
    silent: Duration,
    max_queued: usize,
) -> Result<(String, Arc<Relay>), String> {
    start_test_relay_with(ping_every, silent, max_queued, None)
}

fn start_test_relay_with(
    ping_every: Duration,
    silent: Duration,
    max_queued: usize,
    cap: Option<(u64, u64)>,
) -> Result<(String, Arc<Relay>), String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let mut relay = Relay::new(None);
    if let Some(r) = Arc::get_mut(&mut relay) {
        r.ping_every = ping_every;
        r.silent = silent;
        r.max_queued = max_queued;
        r.quiet = true;
        if let Some((c, slow)) = cap {
            r.cap = Some(c);
            r.slow_to = slow;
        }
    }
    let handle = relay.clone();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = rt.block_on(relay.accept_loop(listener));
    });
    Ok((format!("ws://127.0.0.1:{port}"), handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    type Client = WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

    fn link(relay: &str) -> Client {
        let (ws, _) = tungstenite::connect(relay).unwrap();
        if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_ref() {
            s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        }
        ws
    }

    fn say(ws: &mut Client, m: &Msg) {
        ws.send(Message::text(text(m))).unwrap();
    }

    /// The next message within `secs`; None when nothing came or the link ended.
    fn next(ws: &mut Client, secs: u64) -> Option<Msg> {
        let until = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < until {
            match read(ws) {
                Ok(Some(t)) => return serde_json::from_str(&t).ok(),
                Ok(None) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// A master and a handler with one session open between them.
    fn pair_up(relay: &str) -> (Client, Client, u64) {
        let mut master = link(relay);
        say(
            &mut master,
            &Msg::HelloMaster {
                master_id: String::new(),
                secret: String::new(),
            },
        );
        let Some(Msg::Welcome { master_id, .. }) = next(&mut master, 5) else {
            panic!("no welcome");
        };
        let mut handler = link(relay);
        say(
            &mut handler,
            &Msg::Connect {
                master_id,
                pairing: "p".into(),
                nonce: "n".into(),
                session: 0,
            },
        );
        let Some(Msg::Connect { session, .. }) = next(&mut master, 5) else {
            panic!("no connect at the master");
        };
        say(
            &mut master,
            &Msg::Accept {
                session,
                nonce: "m".into(),
                direct: None,
            },
        );
        assert!(matches!(next(&mut handler, 5), Some(Msg::Accept { .. })));
        (master, handler, session)
    }

    #[test]
    fn many_idle_links_need_no_thread_each_and_messages_still_pass() {
        // The old relay ran a thread per link (the service's task limit was its ceiling): 1,000 links here would
        // be 1,000 threads. Now they all sit on the runtime's two threads, and a pair opened last still talks.
        let (relay, _) = start_local_relay(PING_EVERY, SILENT).unwrap();
        let idle: Vec<Client> = (0..1000).map(|_| link(&relay)).collect();
        let (mut master, mut handler, session) = pair_up(&relay);
        say(
            &mut handler,
            &Msg::Data {
                session,
                body: "hello".into(),
                more: false,
            },
        );
        assert_eq!(
            next(&mut master, 5),
            Some(Msg::Data {
                session,
                body: "hello".into(),
                more: false
            })
        );
        drop(idle);
    }

    #[test]
    fn a_link_that_stops_reading_is_ended_at_its_outbox_limit_and_the_others_stay() {
        let (relay, _) = start_test_relay(PING_EVERY, SILENT, 2 << 20).unwrap();
        let (mut master, mut handler, session) = pair_up(&relay);
        // The handler reads nothing while the master sends far more than the socket and the outbox hold.
        let body = "x".repeat(512 << 10);
        let mut closed = false;
        for _ in 0..400 {
            if master
                .send(Message::text(text(&Msg::Data {
                    session,
                    body: body.clone(),
                    more: false,
                })))
                .is_err()
            {
                break;
            }
            if let Ok(Some(t)) = read(&mut master)
                && matches!(serde_json::from_str(&t), Ok(Msg::Close { .. }))
            {
                closed = true;
                break;
            }
        }
        if !closed {
            closed = matches!(next(&mut master, 10), Some(Msg::Close { .. }));
        }
        assert!(closed, "the master was never told the session ended");
        // The handler's link is gone; the master's stays and can open a new session.
        let mut gone = false;
        let until = Instant::now() + Duration::from_secs(20);
        while Instant::now() < until {
            if read(&mut handler).is_err() {
                gone = true;
                break;
            }
        }
        assert!(gone, "the stuck handler's link was not ended");
        let mut other = link(&relay);
        say(
            &mut other,
            &Msg::Connect {
                master_id: String::new(),
                pairing: "p".into(),
                nonce: "n".into(),
                session: 0,
            },
        );
        assert!(matches!(next(&mut other, 5), Some(Msg::Reject { .. })));
        say(
            &mut master,
            &Msg::PairOpen {
                slot: "SLOT".into(),
            },
        );
        assert!(read(&mut master).is_ok(), "the master's link ended too");
    }

    /// R2: a test certificate for "localhost" written as PEM files; answers its DER (for the client to trust).
    fn make_cert(cert: &Path, key: &Path) -> rustls::pki_types::CertificateDer<'static> {
        let made = rcgen::generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .unwrap();
        std::fs::write(cert, made.cert.pem()).unwrap();
        std::fs::write(key, made.signing_key.serialize_pem()).unwrap();
        made.cert.der().clone()
    }

    /// R2: a test relay on a plain port and its own TLS port at once (the TLS address told to apps), with its
    /// certificate files looked at every 50 ms.
    fn start_both(files: TlsFiles) -> (String, u16, Arc<Relay>) {
        let plain = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let tls = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let (pport, tport) = (
            plain.local_addr().unwrap().port(),
            tls.local_addr().unwrap().port(),
        );
        let mut relay = Relay::new(None);
        if let Some(r) = Arc::get_mut(&mut relay) {
            r.quiet = true;
            r.direct = Some(format!("wss://127.0.0.1:{tport}"));
        }
        let certs = Certs::load(files).unwrap();
        let handle = relay.clone();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        std::thread::spawn(move || {
            rt.block_on(async move {
                tokio::spawn(
                    relay
                        .clone()
                        .watch_certs(certs.clone(), Duration::from_millis(50)),
                );
                tokio::spawn(relay.clone().tls_accept_loop(tls, certs));
                let _ = relay.accept_loop(plain).await;
            })
        });
        (format!("ws://127.0.0.1:{pport}"), tport, handle)
    }

    /// A wss link to the test relay's TLS port, trusting only `trust`; answers it and the certificate it was shown.
    fn tls_link(
        port: u16,
        trust: &[rustls::pki_types::CertificateDer<'static>],
    ) -> (Client, Vec<u8>) {
        let mut roots = rustls::RootCertStore::empty();
        for c in trust {
            roots.add(c.clone()).unwrap();
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let tcp = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (ws, _) = tungstenite::client_tls_with_config(
            format!("wss://localhost:{port}"),
            tcp,
            None,
            Some(tungstenite::Connector::Rustls(Arc::new(config))),
        )
        .unwrap();
        let shown = match ws.get_ref() {
            tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                s.get_ref()
                    .set_read_timeout(Some(Duration::from_millis(50)))
                    .unwrap();
                s.conn.peer_certificates().unwrap()[0].to_vec()
            }
            _ => panic!("not TLS"),
        };
        (ws, shown)
    }

    fn hello_master(ws: &mut Client) -> (String, Option<String>) {
        say(
            ws,
            &Msg::HelloMaster {
                master_id: String::new(),
                secret: String::new(),
            },
        );
        match next(ws, 5) {
            Some(Msg::Welcome {
                master_id, direct, ..
            }) => (master_id, direct),
            other => panic!("no welcome: {other:?}"),
        }
    }

    /// The handler asks for the master, the master accepts; answers the session and what the handler was told.
    fn open_session(master: &mut Client, handler: &mut Client, master_id: &str) -> (u64, Msg) {
        say(
            handler,
            &Msg::Connect {
                master_id: master_id.into(),
                pairing: "p".into(),
                nonce: "n".into(),
                session: 0,
            },
        );
        let Some(Msg::Connect { session, .. }) = next(master, 5) else {
            panic!("no connect at the master");
        };
        say(
            master,
            &Msg::Accept {
                session,
                nonce: "m".into(),
                direct: None,
            },
        );
        (session, next(handler, 5).expect("an answer"))
    }

    /// 24rl F3: the handler sends 40 KB to its master; answers how long it took and the Usage messages the master got.
    fn send_40kb(relay: &str) -> (Duration, Vec<(u64, u64, i64)>) {
        let mut master = link(relay);
        let (id, _) = hello_master(&mut master);
        let mut handler = link(relay);
        let (session, _) = open_session(&mut master, &mut handler, &id);
        let body = "x".repeat(4 << 10);
        let started = Instant::now();
        for _ in 0..10 {
            say(
                &mut handler,
                &Msg::Data {
                    session,
                    body: body.clone(),
                    more: false,
                },
            );
        }
        let (mut got, mut usage) = (0, Vec::new());
        let until = Instant::now() + Duration::from_secs(15);
        while got < 10 && Instant::now() < until {
            match next(&mut master, 1) {
                Some(Msg::Data { .. }) => got += 1,
                Some(Msg::Usage {
                    used,
                    cap,
                    slowed_until,
                }) => usage.push((used, cap, slowed_until)),
                _ => {}
            }
        }
        assert_eq!(got, 10, "not every message came through");
        (started.elapsed(), usage)
    }

    #[test]
    fn a_master_over_its_daily_cap_is_slowed_not_cut_and_is_told() {
        // No cap: 40 KB passes at once, and nothing is said about use.
        let open = start_local().unwrap();
        let (took, usage) = send_40kb(&open);
        assert!(
            took < Duration::from_millis(800),
            "no cap, yet slow: {took:?}"
        );
        assert!(usage.is_empty(), "{usage:?}");

        // A 10 KB cap, 20 KB/s after it: the 30 KB over the cap take about 1.5 s, and all of it arrives.
        let (capped, _) = start_local_capped(10 << 10, 20 << 10).unwrap();
        let (took, usage) = send_40kb(&capped);
        assert!(
            took >= Duration::from_millis(1200),
            "over the cap, yet fast: {took:?}"
        );
        assert!(took < Duration::from_secs(6), "slowed too far: {took:?}");
        // Told when near (80%) and when over, with the cap and the end of the slow time (next UTC midnight).
        assert!(
            usage.iter().any(|(used, cap, slowed)| *cap == 10 << 10
                && *used * 5 >= *cap * 4
                && *slowed == 0),
            "{usage:?}"
        );
        let midnight = (today() + 1) * 86_400;
        assert!(
            usage.iter().any(|(_, _, slowed)| *slowed == midnight),
            "{usage:?}"
        );
    }

    #[test]
    fn the_own_tls_port_and_the_plain_one_work_at_once_and_apps_are_told_the_tls_address() {
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles {
            cert: dir.path().join("cert.pem"),
            key: dir.path().join("key.pem"),
        };
        let der = make_cert(&files.cert, &files.key);
        let (plain, tport, _) = start_both(files);
        let (mut master, _) = tls_link(tport, std::slice::from_ref(&der));
        let (id, direct) = hello_master(&mut master);
        assert_eq!(direct, Some(format!("wss://127.0.0.1:{tport}")));
        let mut handler = link(&plain);
        let (session, told) = open_session(&mut master, &mut handler, &id);
        assert_eq!(
            told,
            Msg::Accept {
                session,
                nonce: "m".into(),
                direct: Some(format!("wss://127.0.0.1:{tport}")),
            }
        );
        let data = |body: &str| Msg::Data {
            session,
            body: body.into(),
            more: false,
        };
        say(&mut handler, &data("up"));
        assert_eq!(next(&mut master, 5), Some(data("up")));
        say(&mut master, &data("down"));
        assert_eq!(next(&mut handler, 5), Some(data("down")));
    }

    #[test]
    fn a_renewed_certificate_is_used_for_new_links_and_open_links_stay() {
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles {
            cert: dir.path().join("cert.pem"),
            key: dir.path().join("key.pem"),
        };
        let old = make_cert(&files.cert, &files.key);
        let (plain, tport, _) = start_both(files.clone());
        let (mut master, shown) = tls_link(tport, std::slice::from_ref(&old));
        assert_eq!(shown, old.to_vec());
        let (id, _) = hello_master(&mut master);
        // Renewed on disk (as ISPConfig does it, then the copy unit); a later change time on both files.
        let new = make_cert(&files.cert, &files.key);
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        for f in [&files.cert, &files.key] {
            std::fs::File::options()
                .write(true)
                .open(f)
                .unwrap()
                .set_modified(later)
                .unwrap();
        }
        let both = [old.clone(), new.clone()];
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let (_, shown) = tls_link(tport, &both);
            if shown == new.to_vec() {
                break;
            }
            assert!(
                Instant::now() < until,
                "the renewed certificate was never used"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        // The link opened before the renewal still works.
        let mut handler = link(&plain);
        let (_, told) = open_session(&mut master, &mut handler, &id);
        assert!(matches!(told, Msg::Accept { .. }), "{told:?}");
    }

    #[test]
    fn a_bad_renewal_keeps_the_certificate_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles {
            cert: dir.path().join("cert.pem"),
            key: dir.path().join("key.pem"),
        };
        make_cert(&files.cert, &files.key);
        let certs = Certs::load(files.clone()).unwrap();
        let before = certs.current();
        std::fs::write(&files.key, "not a key").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&files.key)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        let e = certs.reload_if_changed().unwrap_err();
        assert!(e.contains("could not be read"), "{e}");
        assert!(Arc::ptr_eq(&before, &certs.current()));
    }

    #[test]
    fn old_apps_read_the_new_messages_and_new_relays_send_old_ones_unchanged() {
        // An older app reads Welcome with the new field as before (no unknown-field refusal).
        let m: Msg = serde_json::from_str(
            r#"{"t":"welcome","master_id":"a","secret":"b","direct":"wss://x:47443","later":1}"#,
        )
        .unwrap();
        assert!(matches!(m, Msg::Welcome { .. }));
        // Without a TLS address the JSON is exactly the old one.
        let plain = text(&Msg::Welcome {
            master_id: "a".into(),
            secret: "b".into(),
            direct: None,
        });
        assert_eq!(plain, r#"{"t":"welcome","master_id":"a","secret":"b"}"#);
        let accept = text(&Msg::Accept {
            session: 1,
            nonce: "n".into(),
            direct: None,
        });
        assert_eq!(accept, r#"{"t":"accept","session":1,"nonce":"n"}"#);
    }

    /// R3: a plain test relay that tells apps a TLS address of the test's choosing.
    fn start_plain_telling(direct: String) -> String {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut relay = Relay::new(None);
        if let Some(r) = Arc::get_mut(&mut relay) {
            r.quiet = true;
            r.direct = Some(direct);
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        std::thread::spawn(move || {
            let _ = rt.block_on(relay.accept_loop(listener));
        });
        format!("ws://127.0.0.1:{port}")
    }

    /// R3: a master's link through the app's own connect; answers what the relay said in Welcome.
    fn app_welcome(relay: &str) -> Msg {
        let mut link = crate::relay_client::Link::connect(relay).unwrap();
        link.send(&Msg::HelloMaster {
            master_id: String::new(),
            secret: String::new(),
        })
        .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if let Some(m @ Msg::Welcome { .. }) = link.poll().unwrap() {
                return m;
            }
        }
        panic!("no welcome");
    }

    #[test]
    fn apps_use_the_relays_own_port_once_it_told_them() {
        use crate::relay_client::{OWN_PORT, THROUGH_SAVED, road_of};
        let dir = tempfile::tempdir().unwrap();
        let files = TlsFiles {
            cert: dir.path().join("cert.pem"),
            key: dir.path().join("key.pem"),
        };
        let der = make_cert(&files.cert, &files.key);
        crate::relay_client::TEST_ROOTS.lock().unwrap().push(der);
        let (plain, tport, _) = start_both(files);
        // First link: nothing known yet, so the saved address; the relay tells its own port.
        app_welcome(&plain);
        let road = road_of(&plain);
        assert_eq!(road.last, THROUGH_SAVED);
        assert_eq!(road.direct, format!("wss://127.0.0.1:{tport}"));
        // Next link goes to the own port, and works there.
        let told = app_welcome(&plain);
        assert!(matches!(told, Msg::Welcome { .. }));
        assert_eq!(road_of(&plain).last, OWN_PORT);
    }

    #[test]
    fn an_own_port_that_does_not_answer_costs_a_few_seconds_once_then_the_saved_address_is_used_straight_away()
     {
        use crate::relay_client::{THROUGH_SAVED, road_of};
        // A port that takes the connection and never answers (as a network that swallows it).
        let silent = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let sport = silent.local_addr().unwrap().port();
        let relay = start_plain_telling(format!("wss://127.0.0.1:{sport}"));
        app_welcome(&relay);
        assert_eq!(road_of(&relay).direct, format!("wss://127.0.0.1:{sport}"));
        let t = Instant::now();
        app_welcome(&relay);
        let first = t.elapsed();
        assert!(
            first >= Duration::from_millis(2500) && first < Duration::from_secs(8),
            "{first:?}"
        );
        let road = road_of(&relay);
        assert_eq!(road.last, THROUGH_SAVED);
        assert!(road.skip_until > crate::store::now() + 3600);
        let t = Instant::now();
        app_welcome(&relay);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        drop(silent);
    }

    #[test]
    fn a_relay_that_tells_no_own_port_is_used_as_before_and_a_foreign_host_is_not_taken() {
        use crate::relay_client::{THROUGH_SAVED, road_of};
        let relay = start_local().unwrap();
        app_welcome(&relay);
        assert_eq!(road_of(&relay).direct, "");
        assert_eq!(road_of(&relay).last, THROUGH_SAVED);
        // A relay cannot send this computer to another host.
        let other = start_plain_telling("wss://elsewhere.example:47443".into());
        app_welcome(&other);
        assert_eq!(road_of(&other).direct, "");
    }

    #[test]
    fn many_links_ending_at_once_each_close_only_their_own_sessions_quickly() {
        // R4: 100k links ending together took 80 s of full CPU, each one looking through every session. 20,000
        // here: the old way is 400 million looks (seconds); now each link finds its own.
        let relay = Relay::new(None);
        let n: u64 = 20_000;
        {
            let mut st = relay.state.lock().unwrap();
            for i in 0..n {
                let (master, handler) = (2 * i + 1, 2 * i + 2);
                for c in [master, handler] {
                    let (out, rx) = Outbox::new(MAX_QUEUED);
                    std::mem::forget(rx);
                    st.conns.insert(c, out);
                }
                let id = format!("m{i}");
                let tx = st.conns[&master].clone();
                st.masters.insert(id.clone(), (master, tx));
                st.add_session(1_000_000 + i, handler, &id);
            }
        }
        let t = Instant::now();
        for i in 0..n {
            assert_eq!(relay.gone(2 * i + 2, &None), 1);
        }
        assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
        let st = relay.state.lock().unwrap();
        assert!(st.sessions.is_empty() && st.by_conn.is_empty());
        // Each master was told its session ended.
        assert_eq!(
            st.conns[&1].queued.load(Ordering::SeqCst),
            text(&Msg::Close { session: 1_000_000 }).len()
        );
    }

    #[test]
    fn a_link_that_pings_the_relay_itself_is_not_pinged_back_and_a_quiet_one_is() {
        let (relay, _) =
            start_local_relay(Duration::from_millis(100), Duration::from_secs(5)).unwrap();
        let count_pings = |talks: bool| {
            let mut ws = link(&relay);
            let (mut pings, until) = (0, Instant::now() + Duration::from_millis(1200));
            let mut sent = Instant::now();
            while Instant::now() < until {
                if talks && sent.elapsed() >= Duration::from_millis(40) {
                    sent = Instant::now();
                    ws.send(Message::Ping(Vec::new().into())).unwrap();
                }
                if let Ok(Message::Ping(_)) = ws.read() {
                    pings += 1;
                }
            }
            pings
        };
        assert_eq!(count_pings(true), 0);
        assert!(count_pings(false) >= 5);
    }

    /// Relay kit K1/K2: a relay with no domain makes its own certificate once and keeps it; apps reach it only by the
    /// pin in its address (right pin connects, a wrong or a missing pin is refused).
    #[test]
    fn a_self_made_certificate_is_kept_and_apps_reach_it_only_by_its_pin() {
        let dir = tempfile::tempdir().unwrap();
        let files = self_made(dir.path()).unwrap();
        let pin = pin_of_file(&files.cert).unwrap();
        let again = self_made(dir.path()).unwrap();
        assert_eq!(
            pin_of_file(&again.cert).unwrap(),
            pin,
            "made once, then kept"
        );
        let (_plain, port, _relay) = start_both(files);
        let right = format!("wss://127.0.0.1:{port}#pin={pin}");
        assert!(crate::relay_client::check_relay_url(&right).is_ok());
        assert!(
            crate::relay_client::Link::connect(&right).is_ok(),
            "the right pin connects"
        );
        let wrong = format!("wss://127.0.0.1:{port}#pin={}", "A".repeat(43));
        let e = crate::relay_client::Link::connect(&wrong)
            .err()
            .expect("a wrong pin is refused");
        assert!(e.contains("not the relay you set up"), "{e}");
        assert!(
            crate::relay_client::Link::connect(&format!("wss://127.0.0.1:{port}")).is_err(),
            "without the pin a self-made certificate is not trusted"
        );
    }

    /// Relay kit: one listener on [::] takes IPv4 and IPv6 links, and a pinned IPv6 address (brackets) connects.
    #[test]
    fn a_dual_stack_listener_takes_both_and_a_pinned_ipv6_address_connects() {
        let dir = tempfile::tempdir().unwrap();
        let files = self_made(dir.path()).unwrap();
        let pin = pin_of_file(&files.cert).unwrap();
        let Ok(listener) = bind("[::]:0") else {
            return; // no IPv6 on this machine
        };
        let port = listener.local_addr().unwrap().port();
        let certs = Certs::load(files).unwrap();
        let relay = Relay::new(None);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        std::thread::spawn(move || {
            rt.block_on(async move {
                let _ = relay.tls_accept_loop(listener, certs).await;
            })
        });
        for host in ["[::1]", "127.0.0.1"] {
            let addr = format!("wss://{host}:{port}#pin={pin}");
            let link = crate::relay_client::Link::connect(&addr);
            assert!(link.is_ok(), "{addr}: {:?}", link.err());
        }
    }
}
