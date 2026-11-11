//! Qanuvi's remote knowledge (24c): setup guides, free-provider lists and the like, published by the Qanuvi team as
//! one JSON file with an Ed25519 signature next to it (`<file>.sig`, base64). Qanuvi only takes a file whose signature
//! matches the public key built in here; the interface then shows what changed and uses it after the user's yes.

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use std::time::Duration;

/// The public key that signs Qanuvi's knowledge files (base64). The private key stays with the publisher.
pub const PUBLIC_KEY: &str = "ZVkoC8pbZs5CvTyph593L5rYW4NUKdR/c0rTsXUOfaY=";

/// Where the knowledge file is published (the test host until qanuvi.com is up).
pub const DEFAULT_URL: &str = "https://qlopy.top/qanuvi-updates/knowledge.json";

/// Knowledge files stay small; anything bigger is refused.
const MAX_BYTES: u64 = 1024 * 1024;

/// A new key pair: (private key, public key), both base64.
pub fn keygen() -> Result<(String, String), String> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| format!("No randomness: {e}"))?;
    let key = SigningKey::from_bytes(&seed);
    Ok((B64.encode(seed), B64.encode(key.verifying_key().to_bytes())))
}

/// The signature (base64) of `body` with the private key (base64).
pub fn sign(body: &[u8], secret_b64: &str) -> Result<String, String> {
    let seed: [u8; 32] = B64
        .decode(secret_b64.trim())
        .map_err(|_| "The private key is not base64.".to_string())?
        .try_into()
        .map_err(|_| "The private key is not 32 bytes.".to_string())?;
    Ok(B64.encode(SigningKey::from_bytes(&seed).sign(body).to_bytes()))
}

/// Whether `sig_b64` is a valid signature of `body` by the public key `key_b64`.
pub fn verify(body: &[u8], sig_b64: &str, key_b64: &str) -> Result<(), String> {
    let key: [u8; 32] = B64
        .decode(key_b64.trim())
        .map_err(|_| "The public key is not base64.".to_string())?
        .try_into()
        .map_err(|_| "The public key is not 32 bytes.".to_string())?;
    let key =
        VerifyingKey::from_bytes(&key).map_err(|_| "The public key is not valid.".to_string())?;
    let sig: [u8; 64] = B64
        .decode(sig_b64.trim())
        .map_err(|_| "The signature is not base64.".to_string())?
        .try_into()
        .map_err(|_| "The signature is not 64 bytes.".to_string())?;
    key.verify(body, &Signature::from_bytes(&sig)).map_err(|_| {
        "The signature does not match: the file was not published by Qanuvi, or it was changed."
            .into()
    })
}

fn get(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>, String> {
    let mut res = agent
        .get(url)
        .call()
        .map_err(|e| format!("{url} could not be reached: {e}"))?;
    let status = res.status().as_u16();
    if status != 200 {
        return Err(format!("{url} answered {status}."));
    }
    res.body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| format!("{url} could not be read: {e}"))
}

/// Fetches the knowledge file and its signature over HTTPS, checks the signature with `key_b64`, and returns the text.
pub fn fetch(url: &str, key_b64: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("Knowledge is only fetched over https.".into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .build(),
    );
    let body = get(&agent, url)?;
    let sig = get(&agent, &format!("{url}.sig"))?;
    verify(&body, &String::from_utf8_lossy(&sig), key_b64)?;
    String::from_utf8(body).map_err(|_| "The knowledge file is not UTF-8 text.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_file_checks_out_and_a_changed_one_does_not() {
        let (secret, public) = keygen().unwrap();
        let body = br#"{"format":1,"version":1}"#;
        let sig = sign(body, &secret).unwrap();
        assert!(verify(body, &sig, &public).is_ok());
        assert!(verify(br#"{"format":1,"version":2}"#, &sig, &public).is_err());
        let (_, other) = keygen().unwrap();
        assert!(verify(body, &sig, &other).is_err());
        assert!(verify(body, "not base64!", &public).is_err());
    }

    #[test]
    fn the_built_in_key_is_a_real_key() {
        let raw = B64.decode(PUBLIC_KEY).expect("base64");
        let raw: [u8; 32] = raw.try_into().expect("32 bytes");
        assert!(VerifyingKey::from_bytes(&raw).is_ok());
    }

    #[test]
    fn only_https_is_fetched() {
        assert!(fetch("http://example.com/k.json", PUBLIC_KEY).is_err());
    }
}
