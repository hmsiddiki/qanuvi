//! sl1 S3: direct links on the same network. Two paired computers at home reach each other in a few ms instead of
//! going out to the relay and back (~120 ms a call). Nothing is set up by anyone: once a relay session is open the app
//! asks for a direct road inside it (sealed, so the relay sees nothing), the master answers with its private-network
//! addresses and port, and the app tries them. Only a paired app gets in: both sides prove they hold the pairing key
//! before a line passes. The relay session stays open, quiet, and takes the lines again the moment the direct link
//! drops.

use crate::relay::{Session, b64, random_bytes, unb64};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The first port a master listens on for direct links; the next ones are tried when it is taken. The installer's
/// private-network firewall rule covers PORT..PORT+PORTS.
pub const PORT: u16 = 47444;
pub const PORTS: u16 = 11;
/// The app's ask inside a relay session, and the start of the master's answer (an Offer as JSON after it). Neither is
/// JSON, so an older master's core leaves the ask, and an older app never asks.
pub const ASK: &str = "\u{2}direct?";
pub const OFFER: &str = "\u{2}direct ";
/// How long a quiet direct link waits before it pings, and how many quiet waits end it.
const QUIET: Duration = Duration::from_secs(15);
const QUIET_TIMES: u32 = 3;
const TRY_LIMIT: Duration = Duration::from_millis(300);
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(3);
const HELLO_MAX: usize = 4096;
const FRAME_MAX: usize = (512 << 20) + 1024;
const PROOF_APP: &[u8] = b"qanuvi direct link: app";
const PROOF_MASTER: &[u8] = b"qanuvi direct link: master";
/// Failed tries from one address before it is held off, and for how long.
const FAILS: usize = 5;
const HOLD_OFF: Duration = Duration::from_secs(60);

/// Where a master can be reached directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Offer {
    pub port: u16,
    pub addrs: Vec<String>,
}

/// A private-network (or this computer's own) address: the only kind offered and the only kind let in.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_private() || a.is_loopback(),
        IpAddr::V6(a) => {
            a.is_loopback()
                || (a.segments()[0] & 0xfe00) == 0xfc00
                || a.to_ipv4_mapped()
                    .is_some_and(|m| m.is_private() || m.is_loopback())
        }
    }
}

/// This computer's private-network addresses (the ones its traffic leaves from; no packet is sent to find them).
pub fn local_addresses() -> Vec<IpAddr> {
    let mut out = Vec::new();
    for (bind, to) in [("0.0.0.0:0", "198.51.100.1:9"), ("[::]:0", "[fd00::1]:9")] {
        if let Ok(s) = UdpSocket::bind(bind)
            && s.connect(to).is_ok()
            && let Ok(a) = s.local_addr()
            && private(a.ip())
            && !a.ip().is_loopback()
            && !out.contains(&a.ip())
        {
            out.push(a.ip());
        }
    }
    if cfg!(test) || loopback_only() {
        out.push(IpAddr::from([127, 0, 0, 1]));
    }
    if loopback_only() {
        out.retain(|a| a.is_loopback());
    }
    out
}

/// For checks with two masters on one computer (scripts/sl1-measure.mjs): direct links stay on this computer's own
/// address, so no firewall is asked about a scratch program.
fn loopback_only() -> bool {
    std::env::var_os("QANUVI_DIRECT_LOOPBACK").is_some()
}

fn write_frame(w: &mut impl Write, b: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4 + b.len());
    buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
    buf.extend_from_slice(b);
    w.write_all(&buf)?;
    w.flush()
}

fn read_frame(r: &mut impl Read, max: usize) -> std::io::Result<Vec<u8>> {
    let mut n = [0u8; 4];
    r.read_exact(&mut n)?;
    let n = u32::from_be_bytes(n) as usize;
    if n > max {
        return Err(std::io::Error::other("frame too long"));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(b)
}

/// The sending half of a direct link (one line a frame, packed when long, sealed).
pub struct Out {
    id: u64,
    stream: TcpStream,
    crypto: Session,
}

impl Out {
    pub fn send_line(&mut self, line: &[u8]) -> Result<(), String> {
        let packed = crate::relay_client::pack(line);
        let sealed = self.crypto.seal(packed.as_deref().unwrap_or(line))?;
        write_frame(&mut self.stream, &sealed).map_err(|e| e.to_string())
    }

    /// Ends the link both ways (its reader on either side hears the end at once).
    pub fn close(self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn ping(&mut self) -> Result<(), String> {
        self.send_line(b"")
    }
}

/// Where a session's lines go directly while a direct link is up (None: over the relay).
pub type Slot = Arc<Mutex<Option<Out>>>;

/// Sends a line on the direct link when there is one; false means the caller sends it over the relay (a link that
/// failed just now is taken out of the slot first).
pub fn try_send(slot: &Slot, line: &[u8]) -> bool {
    let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
    match s.as_mut().map(|o| o.send_line(line)) {
        Some(Ok(())) => true,
        Some(Err(_)) => {
            *s = None;
            false
        }
        None => false,
    }
}

pub fn is_up(slot: &Slot) -> bool {
    slot.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Reads a direct link's lines until it ends (pinging while it is quiet), then takes it out of the slot if it is
/// still the one there. Empty lines are pings and are not passed on.
fn read_lines(
    mut stream: TcpStream,
    mut crypto: Session,
    slot: Slot,
    id: u64,
    mut on_line: impl FnMut(Vec<u8>),
) {
    let _ = stream.set_read_timeout(Some(QUIET));
    let mut quiet = 0;
    loop {
        match read_frame(&mut stream, FRAME_MAX) {
            Ok(f) => {
                quiet = 0;
                let Ok(plain) = crypto.open(&f) else { break };
                if plain.is_empty() {
                    continue;
                }
                match crate::relay_client::unpack(plain) {
                    Ok(line) => on_line(line),
                    Err(_) => break,
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                quiet += 1;
                if quiet >= QUIET_TIMES {
                    break;
                }
                let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(o) = s.as_mut().filter(|o| o.id == id)
                    && o.ping().is_err()
                {
                    *s = None;
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    let mut s = slot.lock().unwrap_or_else(|e| e.into_inner());
    if s.as_ref().is_some_and(|o| o.id == id) {
        *s = None;
    }
}

#[derive(Serialize, Deserialize)]
struct Hello {
    pairing: String,
    session: u64,
    nonce: String,
}

#[derive(Serialize, Deserialize)]
struct HelloBack {
    nonce: String,
}

fn new_id() -> u64 {
    random_bytes::<8>()
        .map(u64::from_le_bytes)
        .unwrap_or_default()
}

// The app's side -------------------------------------------------------------------------------------------------------

/// Tries the master's addresses one by one (TRY_LIMIT each) and proves the link both ways. On success the link is
/// put into `slot` and its lines go to `on_line` (on a thread of its own) until it ends; `on_end` is then told.
pub fn connect(
    offer: &Offer,
    key: &[u8; 32],
    pairing: &str,
    session: u64,
    slot: &Slot,
    on_line: impl FnMut(Vec<u8>) + Send + 'static,
    on_end: impl FnOnce() + Send + 'static,
) -> Result<SocketAddr, String> {
    let mut last = "no address offered".to_string();
    for a in &offer.addrs {
        let Ok(ip) = a.parse::<IpAddr>() else {
            continue;
        };
        if !private(ip) {
            continue;
        }
        let addr = SocketAddr::new(ip, offer.port);
        let stream = match TcpStream::connect_timeout(&addr, TRY_LIMIT) {
            Ok(s) => s,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        match prove_app(&stream, key, pairing, session) {
            Ok((out, inn)) => {
                let id = new_id();
                let reader = stream.try_clone().map_err(|e| e.to_string())?;
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Out {
                    id,
                    stream,
                    crypto: out,
                });
                let slot = slot.clone();
                std::thread::spawn(move || {
                    read_lines(reader, inn, slot, id, on_line);
                    on_end();
                });
                return Ok(addr);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn prove_app(
    stream: &TcpStream,
    key: &[u8; 32],
    pairing: &str,
    session: u64,
) -> Result<(Session, Session), String> {
    let mut s = stream;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_LIMIT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_LIMIT));
    let mine = random_bytes::<16>()?;
    let hello = Hello {
        pairing: pairing.to_string(),
        session,
        nonce: b64(&mine),
    };
    let io = |e: std::io::Error| e.to_string();
    write_frame(
        &mut s,
        serde_json::to_string(&hello).unwrap_or_default().as_bytes(),
    )
    .map_err(io)?;
    let back: HelloBack = serde_json::from_slice(&read_frame(&mut s, HELLO_MAX).map_err(io)?)
        .map_err(|_| "The master did not take the direct link.".to_string())?;
    let theirs = unb64(&back.nonce)?;
    let mut out = Session::new(key, &mine, &theirs, false)?;
    let mut inn = Session::new(key, &mine, &theirs, false)?;
    write_frame(&mut s, &out.seal(PROOF_APP)?).map_err(io)?;
    let proof = inn.open(&read_frame(&mut s, HELLO_MAX).map_err(io)?)?;
    if proof != PROOF_MASTER {
        return Err("The other side could not prove it is the paired computer.".into());
    }
    let _ = stream.set_write_timeout(None);
    Ok((out, inn))
}

// The master's side ----------------------------------------------------------------------------------------------------

/// What a master hands a proven direct link: the pairing key to prove it with, where its lines go in, and the slot
/// the session's answers go out through.
pub struct Attach {
    pub key: [u8; 32],
    pub input: Sender<Vec<u8>>,
    pub slot: Slot,
}

/// A master's open relay sessions, as the direct listener needs them.
pub trait Sessions: Send + Sync {
    /// The session `session` when it is open and belongs to `pairing`.
    fn attach(&self, session: u64, pairing: &str) -> Option<Attach>;
}

/// Listens for direct links from PORT on (port 0 = any free one, for tests). Answers the port it got, None when no
/// port could be had (then no direct road is offered and everything stays on the relay).
pub fn serve(sessions: Arc<dyn Sessions>, from: u16) -> Option<u16> {
    let tries: Vec<u16> = if from == 0 {
        vec![0]
    } else {
        (from..from.saturating_add(PORTS)).collect()
    };
    let (v4, port) = tries.into_iter().find_map(|p| {
        let any = if loopback_only() {
            "127.0.0.1"
        } else {
            "0.0.0.0"
        };
        let l = TcpListener::bind((any, p)).ok()?;
        let port = l.local_addr().ok()?.port();
        Some((l, port))
    })?;
    let fails: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>> = Arc::default();
    let mut listeners = vec![v4];
    // IPv6 on the same port when there is any (home networks with ULA addresses).
    if !loopback_only()
        && let Ok(l) = TcpListener::bind(("::", port))
    {
        listeners.push(l);
    }
    for l in listeners {
        let (sessions, fails) = (sessions.clone(), fails.clone());
        std::thread::spawn(move || {
            for stream in l.incoming().map_while(Result::ok) {
                let Ok(peer) = stream.peer_addr() else {
                    continue;
                };
                if !private(peer.ip()) || held_off(&fails, peer.ip()) {
                    continue;
                }
                let (sessions, fails) = (sessions.clone(), fails.clone());
                std::thread::spawn(move || {
                    if prove_master(stream, sessions.as_ref()).is_err() {
                        let mut f = fails.lock().unwrap_or_else(|e| e.into_inner());
                        f.entry(peer.ip()).or_default().push(Instant::now());
                    }
                });
            }
        });
    }
    Some(port)
}

fn held_off(fails: &Mutex<HashMap<IpAddr, Vec<Instant>>>, ip: IpAddr) -> bool {
    let mut f = fails.lock().unwrap_or_else(|e| e.into_inner());
    let list = f.entry(ip).or_default();
    list.retain(|t| t.elapsed() < HOLD_OFF);
    list.len() >= FAILS
}

fn prove_master(stream: TcpStream, sessions: &dyn Sessions) -> Result<(), String> {
    let mut s = &stream;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_LIMIT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_LIMIT));
    let io = |e: std::io::Error| e.to_string();
    let hello: Hello = serde_json::from_slice(&read_frame(&mut s, HELLO_MAX).map_err(io)?)
        .map_err(|_| "not a direct link".to_string())?;
    let attach = sessions
        .attach(hello.session, &hello.pairing)
        .ok_or("no such session")?;
    let theirs = unb64(&hello.nonce)?;
    let mine = random_bytes::<16>()?;
    let back = HelloBack { nonce: b64(&mine) };
    write_frame(
        &mut s,
        serde_json::to_string(&back).unwrap_or_default().as_bytes(),
    )
    .map_err(io)?;
    let mut inn = Session::new(&attach.key, &theirs, &mine, true)?;
    let mut out = Session::new(&attach.key, &theirs, &mine, true)?;
    if inn.open(&read_frame(&mut s, HELLO_MAX).map_err(io)?)? != PROOF_APP {
        return Err("the app could not prove the pairing".into());
    }
    write_frame(&mut s, &out.seal(PROOF_MASTER)?).map_err(io)?;
    let _ = stream.set_write_timeout(None);
    let id = new_id();
    let reader = stream.try_clone().map_err(|e| e.to_string())?;
    *attach.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Out {
        id,
        stream,
        crypto: out,
    });
    let input = attach.input;
    std::thread::spawn(move || {
        read_lines(reader, inn, attach.slot, id, move |mut line| {
            line.push(b'\n');
            let _ = input.send(line);
        });
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    struct One {
        key: [u8; 32],
        input: Sender<Vec<u8>>,
        slot: Slot,
    }

    impl Sessions for One {
        fn attach(&self, session: u64, pairing: &str) -> Option<Attach> {
            (session == 5 && pairing == "p1").then(|| Attach {
                key: self.key,
                input: self.input.clone(),
                slot: self.slot.clone(),
            })
        }
    }

    #[test]
    fn only_private_addresses_count() {
        for a in [
            "192.168.0.230",
            "10.1.2.3",
            "172.16.0.9",
            "127.0.0.1",
            "fd12::1",
            "::1",
        ] {
            assert!(private(a.parse().unwrap()), "{a}");
        }
        for a in ["8.8.8.8", "165.99.219.195", "2001:4860::8888", "172.32.0.1"] {
            assert!(!private(a.parse().unwrap()), "{a}");
        }
        assert!(local_addresses().iter().all(|a| private(*a)));
    }

    #[test]
    fn a_paired_app_links_directly_both_ways_and_others_are_refused() {
        let key = [9u8; 32];
        let (input, from_app) = channel();
        let master_slot: Slot = Arc::default();
        let port = serve(
            Arc::new(One {
                key,
                input,
                slot: master_slot.clone(),
            }),
            0,
        )
        .unwrap();
        let offer = Offer {
            port,
            addrs: vec!["127.0.0.1".into()],
        };
        // The wrong key, an unknown pairing, or a session not open: no link, nothing in the slots.
        let app_slot: Slot = Arc::default();
        assert!(connect(&offer, &[1u8; 32], "p1", 5, &app_slot, |_| {}, || {}).is_err());
        assert!(connect(&offer, &key, "p2", 5, &app_slot, |_| {}, || {}).is_err());
        assert!(connect(&offer, &key, "p1", 6, &app_slot, |_| {}, || {}).is_err());
        assert!(!is_up(&app_slot) && !is_up(&master_slot));
        // The paired app: lines go both ways (a long one packed), pings are not passed on.
        let (heard_tx, heard) = channel();
        let (ended_tx, ended) = channel();
        connect(
            &offer,
            &key,
            "p1",
            5,
            &app_slot,
            move |l| {
                let _ = heard_tx.send(l);
            },
            move || {
                let _ = ended_tx.send(());
            },
        )
        .unwrap();
        assert!(is_up(&app_slot));
        let wait = Duration::from_secs(5);
        let mut i = 0;
        while !is_up(&master_slot) {
            i += 1;
            assert!(i < 100, "the master did not take the link");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(try_send(&app_slot, br#"{"id":1,"cmd":"hello"}"#));
        assert_eq!(
            from_app.recv_timeout(wait).unwrap(),
            b"{\"id\":1,\"cmd\":\"hello\"}\n"
        );
        let long: Vec<u8> = (0..3000)
            .flat_map(|i| format!("{{\"line\":{i}}}").into_bytes())
            .collect();
        assert!(try_send(&master_slot, &long));
        assert_eq!(heard.recv_timeout(wait).unwrap(), long);
        assert!(app_slot.lock().unwrap().as_mut().unwrap().ping().is_ok());
        assert!(try_send(&master_slot, b"after"));
        assert_eq!(heard.recv_timeout(wait).unwrap(), b"after");
        // The master's side goes: the app hears the end, its slot empties, and sends fall back to the relay.
        master_slot.lock().unwrap().take().unwrap().close();
        ended.recv_timeout(wait).unwrap();
        assert!(!is_up(&app_slot));
        assert!(!try_send(&app_slot, b"x"));
    }
}
