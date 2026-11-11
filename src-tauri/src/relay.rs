//! Reaching a computer from anywhere through a relay (Milestone 20): the shared parts. The master (the computer being
//! reached) and the handler (the laptop or phone reaching it) both connect out to a small relay server, which only
//! passes messages along: everything between them is encrypted end to end, so the relay cannot read or change it.
//!
//! Pairing (once per device): the master shows a code like `K7QF-29MX-T4HD`. The first part tells the relay which
//! master is pairing; the rest is a secret the relay never sees. Both sides run SPAKE2 with it, which gives them the
//! same key without sending anything a listener (the relay included) could use to find the code or the key. Each later
//! connection takes fresh keys from that pairing key and random values from both sides, and every message is sealed
//! with XChaCha20-Poly1305 and numbered, so a message cannot be read, changed, replayed or reordered.

use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Letters and digits that are not easily mixed up when read aloud or typed (no 0/O, 1/I/L).
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

pub fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn random_code(len: usize) -> Result<String, String> {
    let mut b = vec![0u8; len];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b.iter()
        .map(|x| CODE_ALPHABET[(*x as usize) % CODE_ALPHABET.len()] as char)
        .collect())
}

pub fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

pub fn unb64(s: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| "Bad message from the relay.".to_string())
}

/// A pairing code: a public slot (to find the master at the relay) and a secret (for SPAKE2).
#[derive(Debug, Clone, PartialEq)]
pub struct Code {
    pub slot: String,
    pub secret: String,
}

impl Code {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            slot: random_code(4)?,
            secret: random_code(8)?,
        })
    }

    /// As shown to the user: `SLOT-SECR-ET12`.
    pub fn shown(&self) -> String {
        format!("{}-{}-{}", self.slot, &self.secret[..4], &self.secret[4..])
    }

    /// Reads a typed code (spaces, dashes and lower case are fine).
    pub fn parse(typed: &str) -> Result<Self, String> {
        let clean: String = typed
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        if clean.len() != 12 || !clean.bytes().all(|b| CODE_ALPHABET.contains(&b)) {
            return Err(
                "That is not a pairing code (12 letters and digits, like K7QF-29MX-T4HD).".into(),
            );
        }
        Ok(Self {
            slot: clean[..4].into(),
            secret: clean[4..].into(),
        })
    }
}

/// One side of the pairing exchange.
pub struct PairStart {
    state: Spake2<Ed25519Group>,
    /// The message to send to the other side.
    pub message: Vec<u8>,
}

pub fn pair_start(code: &Code) -> PairStart {
    let (state, message) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.secret.as_bytes()),
        &Identity::new(format!("qanuvi-pair:{}", code.slot).as_bytes()),
    );
    PairStart { state, message }
}

/// The pairing key from the other side's message. Both sides get the same key only with the same code.
pub fn pair_finish(start: PairStart, other: &[u8]) -> Result<[u8; 32], String> {
    let shared = start
        .state
        .finish(other)
        .map_err(|_| "The pairing failed.".to_string())?;
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(b"qanuvi pairing"), &shared)
        .expand(b"pairing key", &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

/// Seals with a key and a nonce made from a number (each key is used for one direction only, so numbers never
/// repeat under the same key).
fn seal_with(key: &[u8; 32], counter: u64, plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 24];
    nonce[..8].copy_from_slice(&counter.to_le_bytes());
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    let sealed = cipher
        .encrypt(&XNonce::from(nonce), plain)
        .map_err(|_| "Encryption failed.".to_string())?;
    let mut out = counter.to_le_bytes().to_vec();
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn open_with(key: &[u8; 32], expected: u64, data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 8 + 16 {
        return Err("A message was cut short.".into());
    }
    let counter = u64::from_le_bytes(data[..8].try_into().expect("8 bytes"));
    if counter != expected {
        return Err("A message came out of order or twice.".into());
    }
    let mut nonce = [0u8; 24];
    nonce[..8].copy_from_slice(&data[..8]);
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    cipher
        .decrypt(&XNonce::from(nonce), &data[8..])
        .map_err(|_| "A message could not be opened (wrong key, or it was changed).".into())
}

/// A one-off sealed message under the pairing key (used once while pairing, with its own derived key).
pub fn seal_once(pairing_key: &[u8; 32], label: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    seal_with(&derive(pairing_key, &[], label)?, 0, plain)
}

pub fn open_once(pairing_key: &[u8; 32], label: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    open_with(&derive(pairing_key, &[], label)?, 0, data)
}

fn derive(key: &[u8; 32], salt: &[u8], label: &str) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Hkdf::<Sha256>::new(Some(salt), key)
        .expand(label.as_bytes(), &mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// An encrypted connection between a handler and a master: fresh keys for each direction from the pairing key and
/// both sides' random values; messages numbered from 0 in each direction.
pub struct Session {
    send_key: [u8; 32],
    recv_key: [u8; 32],
    sent: u64,
    received: u64,
}

impl Session {
    /// `master` says which side this is (the two directions get different keys).
    pub fn new(
        pairing_key: &[u8; 32],
        handler_nonce: &[u8],
        master_nonce: &[u8],
        master: bool,
    ) -> Result<Self, String> {
        let mut salt = handler_nonce.to_vec();
        salt.extend_from_slice(master_nonce);
        let to_master = derive(pairing_key, &salt, "handler to master")?;
        let to_handler = derive(pairing_key, &salt, "master to handler")?;
        let (send_key, recv_key) = if master {
            (to_handler, to_master)
        } else {
            (to_master, to_handler)
        };
        Ok(Self {
            send_key,
            recv_key,
            sent: 0,
            received: 0,
        })
    }

    pub fn seal(&mut self, plain: &[u8]) -> Result<Vec<u8>, String> {
        let out = seal_with(&self.send_key, self.sent, plain)?;
        self.sent += 1;
        Ok(out)
    }

    pub fn open(&mut self, data: &[u8]) -> Result<Vec<u8>, String> {
        let out = open_with(&self.recv_key, self.received, data)?;
        self.received += 1;
        Ok(out)
    }
}

/// Messages over the relay (JSON text frames). The relay reads only `t`, the ids and the slot; everything else it
/// passes on as it is. `body` fields are sealed (base64).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    // Master <-> relay
    /// A master says who it is (a new one leaves the id empty and gets one).
    HelloMaster {
        #[serde(default)]
        master_id: String,
        #[serde(default)]
        secret: String,
    },
    Welcome {
        master_id: String,
        secret: String,
    },
    /// The master opens a pairing slot (for 10 minutes, one try).
    PairOpen {
        slot: String,
    },
    // Handler -> relay -> master
    PairJoin {
        slot: String,
        #[serde(default)]
        handler: u64,
        spake: String,
    },
    // Master -> relay -> handler: its SPAKE2 message and, sealed with the new key, who it is.
    PairReply {
        handler: u64,
        spake: String,
        body: String,
    },
    // Handler -> relay -> master: sealed, the handler's name and the pairing id (proves the handler has the key too).
    PairConfirm {
        slot: String,
        #[serde(default)]
        handler: u64,
        body: String,
    },
    PairDone {
        handler: u64,
        ok: bool,
    },
    /// A handler asks for a connection to a paired master.
    Connect {
        master_id: String,
        pairing: String,
        nonce: String,
        #[serde(default)]
        session: u64,
    },
    Accept {
        session: u64,
        nonce: String,
    },
    Reject {
        session: u64,
        reason: String,
    },
    /// A sealed piece of the conversation (the same JSON lines as over SSH, Milestone 16).
    Data {
        session: u64,
        body: String,
    },
    Close {
        session: u64,
    },
    /// Anything the relay wants to say (master not online, slot taken).
    Error {
        message: String,
    },
}

/// What the master tells a pairing handler about itself (sealed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MasterCard {
    pub master_id: String,
    pub name: String,
    pub pairing: String,
}

/// What the handler tells the master (sealed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandlerCard {
    pub name: String,
    pub pairing: String,
}

pub fn new_id() -> Result<String, String> {
    let b = random_bytes::<12>()?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_shown_and_read_back() {
        let c = Code::new().unwrap();
        let shown = c.shown();
        assert_eq!(shown.len(), 14);
        assert_eq!(
            Code::parse(&shown.to_lowercase().replace('-', " ")).unwrap(),
            c
        );
        assert!(Code::parse("K7QF-29MX").is_err());
        assert!(
            Code::parse("K7QF-29MX-T4H0").is_err(),
            "0 is not in the alphabet"
        );
    }

    #[test]
    fn the_same_code_gives_the_same_key_and_another_code_does_not() {
        let code = Code::parse("K7QF-29MX-T4HD").unwrap();
        let a = pair_start(&code);
        let b = pair_start(&code);
        let (ma, mb) = (a.message.clone(), b.message.clone());
        let ka = pair_finish(a, &mb).unwrap();
        let kb = pair_finish(b, &ma).unwrap();
        assert_eq!(ka, kb);

        let wrong = Code::parse("K7QF-29MX-T4HE").unwrap();
        let a = pair_start(&code);
        let b = pair_start(&wrong);
        let (ma, mb) = (a.message.clone(), b.message.clone());
        assert_ne!(pair_finish(a, &mb).unwrap(), pair_finish(b, &ma).unwrap());
    }

    #[test]
    fn sessions_seal_both_ways_and_refuse_replays_and_changes() {
        let key = [7u8; 32];
        let (hn, mn) = ([1u8; 16], [2u8; 16]);
        let mut h = Session::new(&key, &hn, &mn, false).unwrap();
        let mut m = Session::new(&key, &hn, &mn, true).unwrap();
        let one = h.seal(b"git_status").unwrap();
        let two = h.seal(b"files_read").unwrap();
        assert_eq!(m.open(&one).unwrap(), b"git_status");
        assert!(m.open(&one).is_err(), "a replay is refused");
        let mut changed = two.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        let mut m2 = Session::new(&key, &hn, &mn, true).unwrap();
        m2.open(&one).unwrap();
        assert!(m2.open(&changed).is_err(), "a changed message is refused");
        assert_eq!(m.open(&two).unwrap(), b"files_read");
        let back = m.seal(b"answer").unwrap();
        assert_eq!(h.open(&back).unwrap(), b"answer");
        // Other random values: other keys.
        let mut other = Session::new(&key, &[3u8; 16], &mn, true).unwrap();
        let fresh = Session::new(&key, &hn, &mn, false)
            .unwrap()
            .seal(b"x")
            .unwrap();
        assert!(other.open(&fresh).is_err());
    }

    #[test]
    fn messages_have_a_type_tag() {
        let m = Msg::Data {
            session: 3,
            body: "abc".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(j, r#"{"t":"data","session":3,"body":"abc"}"#);
        assert_eq!(serde_json::from_str::<Msg>(&j).unwrap(), m);
    }
}
