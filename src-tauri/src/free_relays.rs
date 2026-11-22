//! 24rl F1: the free relay list. Volunteers run the same relay program as Qanuvi's; the owner checks each one and
//! puts it into relays.json on the update channel, signed with the update key (relays.json.sig), so nobody can slip
//! a relay in. A list that does not match its signature is refused; the last good copy stays in use.
//!
//! A free relay is only ever used after the user picks it and says yes (the window asks). Messages stay sealed end
//! to end there too; the relay sees who connects, when, from where and how much.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

/// One relay on the list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeRelay {
    pub url: String,
    pub name: String,
    /// Who runs it (as they asked to be named).
    #[serde(default)]
    pub run_by: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Default, Deserialize)]
struct List {
    #[serde(default)]
    relays: Vec<FreeRelay>,
}

/// A relay as the window lists it: tested just now (milliseconds to connect, or why it could not).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tested {
    #[serde(flatten)]
    pub relay: FreeRelay,
    pub ms: Option<u64>,
    pub error: String,
}

/// What `free_relays` answers: the relays, best first; `error` when the list could not be fetched (an older copy
/// may still be listed).
#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub relays: Vec<Tested>,
    pub error: String,
}

const CACHE: &str = "free-relays.json";

/// Reads a list and checks it against its signature (base64 minisign text, as the channel's .sig files). Entries
/// that are not a wss:// relay address are left out; Qanuvi's own relay is not a free relay.
pub fn read_list(bytes: &[u8], signature: &str, key: &str) -> Result<Vec<FreeRelay>, String> {
    crate::master_update::verify(bytes, signature, key)
        .map_err(|_| "The free relay list's signature is wrong, so it was refused.".to_string())?;
    let list: List = serde_json::from_slice(bytes)
        .map_err(|e| format!("The free relay list does not read: {e}"))?;
    let mut seen = Vec::new();
    Ok(list
        .relays
        .into_iter()
        .filter_map(|mut r| {
            let url = crate::relay_client::check_relay_url(&r.url).ok()?;
            if !url.starts_with("wss://")
                || url == crate::relay_client::DEFAULT_RELAY
                || seen.contains(&url)
            {
                return None;
            }
            seen.push(url.clone());
            r.url = url;
            r.name = r.name.trim().chars().take(40).collect();
            if r.name.is_empty() {
                r.name = r.url.trim_start_matches("wss://").to_string();
            }
            Some(r)
        })
        .collect())
}

/// The list's address: beside latest.json on the update channel.
fn list_url() -> String {
    let latest = crate::master_update::channel_url();
    match latest.rsplit_once('/') {
        Some((dir, _)) if !latest.is_empty() => format!("{dir}/relays.json"),
        _ => String::new(),
    }
}

/// Fetches and checks the list, keeping it in `data` when good; on any failure the last good copy (checked again).
pub fn load(data: &Path) -> (Vec<FreeRelay>, String) {
    let key = crate::master_update::trusted_key();
    let cache = data.join(CACHE);
    let fresh = (|| -> Result<Vec<FreeRelay>, String> {
        let url = list_url();
        if url.is_empty() {
            return Err("This build has no update address for the free relay list.".into());
        }
        // No list on the channel (yet): no free relays, and nothing wrong.
        let Some(bytes) = crate::master_update::fetch_bytes(&url)? else {
            let _ = std::fs::remove_file(&cache);
            return Ok(Vec::new());
        };
        let sig = crate::master_update::fetch_bytes(&format!("{url}.sig"))?
            .ok_or("The free relay list has no signature, so it was refused.")?;
        let sig = String::from_utf8_lossy(&sig).to_string();
        let relays = read_list(&bytes, &sig, &key)?;
        let kept = serde_json::json!({"list": String::from_utf8_lossy(&bytes), "sig": sig});
        let _ = std::fs::write(&cache, kept.to_string());
        Ok(relays)
    })();
    match fresh {
        Ok(r) => (r, String::new()),
        Err(e) => {
            let old = std::fs::read_to_string(&cache)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| {
                    read_list(v["list"].as_str()?.as_bytes(), v["sig"].as_str()?, &key).ok()
                })
                .unwrap_or_default();
            (old, e)
        }
    }
}

/// How long a relay takes to answer a connection (TLS and WebSocket); nothing is sent, so it registers nothing.
fn probe(url: &str) -> Result<u64, String> {
    let started = Instant::now();
    let link = crate::relay_client::Link::connect(url)?;
    drop(link);
    Ok(started.elapsed().as_millis() as u64)
}

/// The list, each relay tested at once (side by side, up to 10 s), the fastest first and the unreachable last.
pub fn list_tested(data: &Path) -> Answer {
    let (relays, error) = load(data);
    let tested = test_all(relays, probe);
    Answer {
        relays: tested,
        error,
    }
}

fn test_all(relays: Vec<FreeRelay>, probe: fn(&str) -> Result<u64, String>) -> Vec<Tested> {
    let (tx, rx) = std::sync::mpsc::channel();
    for (i, r) in relays.iter().enumerate() {
        let (tx, url) = (tx.clone(), r.url.clone());
        std::thread::spawn(move || {
            let _ = tx.send((i, probe(&url)));
        });
    }
    drop(tx);
    let mut results: Vec<Option<Result<u64, String>>> = vec![None; relays.len()];
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((i, r)) => results[i] = Some(r),
            Err(_) => break,
        }
    }
    let mut out: Vec<Tested> = relays
        .into_iter()
        .zip(results)
        .map(|(relay, r)| match r {
            Some(Ok(ms)) => Tested {
                relay,
                ms: Some(ms),
                error: String::new(),
            },
            Some(Err(e)) => Tested {
                relay,
                ms: None,
                error: e,
            },
            None => Tested {
                relay,
                ms: None,
                error: "No answer within 10 seconds.".into(),
            },
        })
        .collect();
    out.sort_by_key(|t| t.ms.unwrap_or(u64::MAX));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed with a throwaway key made for this test (tauri signer), never the update key.
    const KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDg3MENGQzhFNzU3QzkzOTQKUldTVWszeDFqdndNaC9SY09UZ2I3Zk55UE04b1AvcG9pYklaMjlzNVFtY0hkeVlFYWFCaGpFVC8K";
    const LIST: &str = r#"{"relays":[{"url":"wss://relay.example.org","name":"Example","runBy":"Test volunteer","region":"EU","note":"test list"}]}"#;
    const SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTVWszeDFqdndNaDk3bVlsN1pZRzY1aUJ3cXVQc0dzOTZBWnZQS0h4WWZVemhZVHI3MWRQMFlDcW5NZDI1ZXFHL041OENCQW1XTXFGNGJZdU42M3hmNHR2QVRtR245VkFjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk1Mjg3NjMyCWZpbGU6cmVsYXlzLmpzb24Kd2xHSS9FNmoyTGU1TytDUURkSDRxNW41VDNtbSt5VVM3Qmc2dTZYeUJ5ZDNRQU82R1Z3YU94QW1NdTV2TkEzM2ZMVEdHeTBQSUhXMkRFZXRsdUlXQVE9PQo=";

    #[test]
    fn a_signed_list_reads_and_a_changed_or_unsigned_one_is_refused() {
        let relays = read_list(LIST.as_bytes(), SIG, KEY).unwrap();
        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].url, "wss://relay.example.org");
        assert_eq!(relays[0].run_by, "Test volunteer");
        let changed = LIST.replace("example.org", "attacker.net");
        assert!(read_list(changed.as_bytes(), SIG, KEY).is_err());
        assert!(read_list(LIST.as_bytes(), "", KEY).is_err());
        // The app's own key did not sign it.
        assert!(read_list(LIST.as_bytes(), SIG, &crate::master_update::public_key()).is_err());
    }

    #[test]
    fn the_fastest_relays_come_first_and_the_unreachable_last() {
        let r = |url: &str| FreeRelay {
            url: url.into(),
            name: url.into(),
            run_by: String::new(),
            region: String::new(),
            note: String::new(),
        };
        fn fake(url: &str) -> Result<u64, String> {
            match url {
                "wss://a" => Ok(300),
                "wss://b" => Ok(40),
                _ => Err("refused".into()),
            }
        }
        let out = test_all(vec![r("wss://c"), r("wss://a"), r("wss://b")], fake);
        let order: Vec<&str> = out.iter().map(|t| t.relay.url.as_str()).collect();
        assert_eq!(order, ["wss://b", "wss://a", "wss://c"]);
        assert_eq!(out[2].error, "refused");
    }
}
