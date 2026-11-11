//! The relay server (Milestone 20): `qanuvi-relay`, a small program for a VPS or hosting. Masters and handlers both
//! connect out to it over WebSocket; it matches them up and passes their messages along. Everything between a master
//! and a handler is sealed end to end (relay.rs), so the relay sees only ids and sealed text. It keeps nothing but the
//! masters' ids and a hash of each master's secret (so nobody else can take a master's place); put it behind a TLS
//! proxy (nginx, Caddy) for wss://.

use crate::relay::{Msg, new_id};
use std::collections::HashMap;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const SLOT_LIFE: Duration = Duration::from_secs(600);
const MAX_MESSAGE: usize = 4 << 20;

#[derive(Default)]
struct State {
    /// Masters online: id -> its connection.
    masters: HashMap<String, (u64, Sender<String>)>,
    /// Every connection's outbox, by connection number.
    conns: HashMap<u64, Sender<String>>,
    /// Open pairing slots: slot -> (master id, until).
    slots: HashMap<String, (String, Instant)>,
    /// Handlers in the middle of pairing: handler connection -> master id.
    pairing: HashMap<u64, String>,
    /// Sessions: id -> (handler connection, master id).
    sessions: HashMap<u64, (u64, String)>,
    /// Registered masters: id -> hash of their secret (kept in a file).
    registered: HashMap<String, String>,
}

pub struct Relay {
    state: Mutex<State>,
    next: AtomicU64,
    file: Option<PathBuf>,
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
        })
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
                Some((_, tx)) => tx.send(text(&m)).is_ok(),
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
                reply(
                    &st,
                    me,
                    Msg::Welcome {
                        master_id: id,
                        secret,
                    },
                );
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
                    st.sessions.insert(session, (me, master_id.clone()));
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
                if let Some((handler, m)) = st.sessions.get(&session).cloned()
                    && Some(&m) == master_of.as_ref()
                {
                    let rejected = matches!(msg, Msg::Reject { .. });
                    reply(&st, handler, msg);
                    if rejected {
                        st.sessions.remove(&session);
                    }
                }
            }
            Msg::Data { session, .. } | Msg::Close { session } => {
                let Some((handler, master)) = st.sessions.get(&session).cloned() else {
                    return;
                };
                let closing = matches!(msg, Msg::Close { .. });
                if master_of.as_ref() == Some(&master) {
                    reply(&st, handler, msg);
                } else if me == handler {
                    to_master(&st, &master, msg);
                }
                if closing {
                    st.sessions.remove(&session);
                }
            }
            _ => {}
        }
    }

    /// A connection ended: forget it and close the sessions it was part of.
    fn gone(&self, me: u64, master_of: &Option<String>) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.conns.remove(&me);
        st.pairing.remove(&me);
        if let Some(id) = master_of
            && st.masters.get(id).is_some_and(|(c, _)| *c == me)
        {
            st.masters.remove(id);
        }
        let ended: Vec<(u64, u64, String)> = st
            .sessions
            .iter()
            .filter(|(_, (h, m))| *h == me || Some(m) == master_of.as_ref())
            .map(|(s, (h, m))| (*s, *h, m.clone()))
            .collect();
        for (session, handler, master) in ended {
            st.sessions.remove(&session);
            let close = text(&Msg::Close { session });
            if handler != me
                && let Some(tx) = st.conns.get(&handler)
            {
                let _ = tx.send(close.clone());
            }
            if Some(&master) != master_of.as_ref()
                && let Some((_, tx)) = st.masters.get(&master)
            {
                let _ = tx.send(close);
            }
        }
    }

    /// Serves one WebSocket connection until it ends.
    pub fn serve_conn(self: &Arc<Self>, stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let config = tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE))
            .max_frame_size(Some(MAX_MESSAGE));
        let Ok(mut ws) = tungstenite::accept_with_config(stream, Some(config)) else {
            return;
        };
        let _ = ws
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(50)));
        let me = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx): (Sender<String>, Receiver<String>) = channel();
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .conns
            .insert(me, tx);
        let mut master_of = None;
        'conn: loop {
            match read(&mut ws) {
                Ok(Some(text)) => {
                    if let Ok(msg) = serde_json::from_str::<Msg>(&text) {
                        self.handle(me, msg, &mut master_of);
                    }
                }
                Ok(None) => {}
                Err(Gone) => break,
            }
            while let Ok(out) = rx.try_recv() {
                if ws.send(Message::text(out)).is_err() {
                    break 'conn;
                }
            }
        }
        self.gone(me, &master_of);
    }
}

/// The connection ended.
#[derive(Debug)]
pub struct Gone;

/// The next text message, None after a short wait, Err when the connection is gone.
pub fn read<S: std::io::Read + std::io::Write>(
    ws: &mut WebSocket<S>,
) -> Result<Option<String>, Gone> {
    match ws.read() {
        Ok(Message::Text(t)) => Ok(Some(t.as_str().to_string())),
        Ok(Message::Close(_)) => Err(Gone),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(Gone),
    }
}

/// Runs the relay on an address like 127.0.0.1:47300 until it is stopped.
pub fn run(addr: &str, data: Option<&Path>) -> Result<(), String> {
    let listener =
        TcpListener::bind(addr).map_err(|e| format!("Could not listen on {addr}: {e}"))?;
    let relay = Relay::new(data);
    for stream in listener.incoming().flatten() {
        let relay = relay.clone();
        std::thread::spawn(move || relay.serve_conn(stream));
    }
    Ok(())
}

/// Starts a relay on a free local port (for tests); answers its ws:// address.
pub fn start_local() -> Result<String, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let relay = Relay::new(None);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let relay = relay.clone();
            std::thread::spawn(move || relay.serve_conn(stream));
        }
    });
    Ok(format!("ws://127.0.0.1:{port}"))
}
