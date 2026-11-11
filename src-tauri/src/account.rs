//! Signing in to Qanuvi with the central Qanuvi account at qanuvi.com (Milestone 19; the site runs on Qlopy and offers
//! Google, Microsoft or GitHub sign-in there): OAuth 2.0 authorization code with PKCE, the way an app
//! on a computer should (RFC 8252). The site's own sign-in page opens in the user's browser (with its two-factor step,
//! if the site has one); Qanuvi never sees the password. The answer comes back to a one-time listener on 127.0.0.1;
//! the refresh token is kept in the system's credential store; the short-lived access token only in memory.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The fixed loopback address Qanuvi listens on for the site's answer. Qlopy matches redirect addresses exactly, so this
/// is the address to register for the Qanuvi app on the site (Settings > API > Apps).
pub const REDIRECT_PORT: u16 = 47213;
pub const REDIRECT: &str = "http://127.0.0.1:47213/callback";
/// The central account site and Qanuvi's app id registered there.
pub const HOME_SITE: &str = "https://qanuvi.com";
pub const CLIENT_ID: &str = "qanuvi-desktop";

/// The account site and app id: qanuvi.com, or a test site given in QANUVI_ACCOUNT_SITE (and QANUVI_ACCOUNT_CLIENT),
/// for trying the sign-in against a local copy of the site.
pub fn home() -> (String, String) {
    let site = std::env::var("QANUVI_ACCOUNT_SITE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| HOME_SITE.into());
    let client = std::env::var("QANUVI_ACCOUNT_CLIENT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| CLIENT_ID.into());
    (site, client)
}
const SCOPES: &str = "users:read";
const WAIT: Duration = Duration::from_secs(300);

/// A random string from an alphabet (for the PKCE verifier and the state).
fn random(len: usize) -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect())
}

/// The PKCE challenge for a verifier: base64url(SHA-256(verifier)) without padding.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The site's address, checked: https, or http only for this computer (a local test site).
pub fn site_url(raw: &str) -> Result<String, String> {
    let u = raw.trim().trim_end_matches('/');
    let lower = u.to_ascii_lowercase();
    let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
        .iter()
        .any(|p| {
            lower == *p
                || lower.starts_with(&format!("{p}/"))
                || lower.starts_with(&format!("{p}:"))
        });
    let ok = (lower.starts_with("https://") && u.len() > 8 || local)
        && u.len() <= 300
        && !u
            .chars()
            .any(|c| c.is_whitespace() || "\"<>\\#?".contains(c));
    if ok {
        Ok(u.to_string())
    } else {
        Err(
            "The site's address must start with https:// (or be http://localhost for a test site)."
                .into(),
        )
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The query parameters of a request line like "GET /callback?code=..&state=.. HTTP/1.1".
pub fn query_of(request_line: &str) -> Vec<(String, String)> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let q = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (dec(k), dec(v))
        })
        .collect()
}

/// The authorize page address for a sign-in.
pub fn authorize_url(site: &str, client_id: &str, state: &str, challenge: &str) -> String {
    format!(
        "{site}/api/qlopy/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        enc(client_id),
        enc(REDIRECT),
        enc(SCOPES),
        enc(state),
        enc(challenge)
    )
}

#[derive(Debug, Clone, Deserialize)]
struct Tokens {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
    refresh_token: String,
}

/// Who is signed in (shown in the app).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Account {
    pub site: String,
    pub user_id: String,
    pub name: String,
    pub email: String,
}

struct Session {
    site: String,
    client_id: String,
    access: String,
    expires: Instant,
}

#[derive(Default)]
pub struct Accounts {
    session: Mutex<Option<Session>>,
    /// Set to stop waiting for a sign-in (the user pressed Cancel).
    cancel: std::sync::atomic::AtomicBool,
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    )
}

fn post_form(url: &str, form: &[(&str, &str)]) -> Result<(u16, Value), String> {
    let mut res = agent()
        .post(url)
        .send_form(form.iter().copied())
        .map_err(|e| format!("The site could not be reached: {e}"))?;
    let status = res.status().as_u16();
    let text = res.body_mut().read_to_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn token_error(v: &Value) -> String {
    let e = v["error_description"]
        .as_str()
        .or(v["message"].as_str())
        .or(v["error"].as_str())
        .unwrap_or("The site refused the sign-in.");
    e.to_string()
}

fn keyring_entry(site: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Qanuvi", &format!("account:{site}")).map_err(|e| e.to_string())
}

/// Waits for the browser to come back to the loopback address with the code (checks the state).
fn wait_for_code(
    listener: &TcpListener,
    state: &str,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + WAIT;
    loop {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("Sign-in cancelled.".into());
        }
        if Instant::now() > deadline {
            return Err("The sign-in was not finished within 5 minutes.".into());
        }
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new(&stream).read_line(&mut line);
        if !line.starts_with("GET /callback") {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        }
        let q = query_of(&line);
        let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let (title, outcome) = if get("state").as_deref() != Some(state) {
            (
                "Sign-in not accepted",
                Err("The answer did not belong to this sign-in.".to_string()),
            )
        } else if let Some(err) = get("error") {
            (
                "Sign-in cancelled",
                Err(get("error_description").unwrap_or(err)),
            )
        } else if let Some(code) = get("code") {
            ("Signed in to Qanuvi", Ok(code))
        } else {
            (
                "Sign-in not accepted",
                Err("The site sent no code.".to_string()),
            )
        };
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>{title}</title><body style=\"font:16px system-ui;background:#141416;color:#e6e6e6;display:grid;place-items:center;height:90vh\"><div><h2>{title}</h2><p>You can close this tab and go back to Qanuvi.</p></div>"
        );
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
            body.len()
        );
        return outcome;
    }
}

impl Accounts {
    /// Signs in: opens the site's sign-in page with `open`, waits for the answer, exchanges the code and reads who
    /// signed in. The refresh token goes to the credential store.
    pub fn sign_in(
        &self,
        site: &str,
        client_id: &str,
        open: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<Account, String> {
        let site = site_url(site)?;
        let client_id = client_id.trim();
        if client_id.is_empty() || client_id.len() > 60 {
            return Err("Give the app's client id from the site (Settings > API > Apps).".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|_| {
            format!(
                "Port {REDIRECT_PORT} on this computer is in use; close what uses it and try again."
            )
        })?;
        let verifier = random(64)?;
        let state = random(24)?;
        open(&authorize_url(
            &site,
            client_id,
            &state,
            &challenge(&verifier),
        ))?;
        self.cancel
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let code = wait_for_code(&listener, &state, &self.cancel)?;
        drop(listener);
        let (status, v) = post_form(
            &format!("{site}/api/qlopy/v1/oauth/token"),
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT),
                ("client_id", client_id),
                ("code_verifier", &verifier),
            ],
        )?;
        if status != 200 {
            return Err(token_error(&v));
        }
        let tokens: Tokens = serde_json::from_value(v)
            .map_err(|_| "The site's answer was not understood.".to_string())?;
        keyring_entry(&site)?
            .set_password(&tokens.refresh_token)
            .map_err(|e| format!("The sign-in could not be kept: {e}"))?;
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
            site: site.clone(),
            client_id: client_id.to_string(),
            access: tokens.access_token,
            expires: Instant::now()
                + Duration::from_secs(tokens.expires_in.saturating_sub(30).max(30)),
        });
        self.me()
    }

    /// A valid access token: the current one, or a new one from the refresh token (which is one-use, so the new refresh
    /// token replaces it).
    fn access(&self, site: &str, client_id: &str) -> Result<String, String> {
        {
            let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = s.as_ref()
                && s.site == site
                && s.expires > Instant::now()
            {
                return Ok(s.access.clone());
            }
        }
        let entry = keyring_entry(site)?;
        let refresh = entry
            .get_password()
            .map_err(|_| "Not signed in.".to_string())?;
        let (status, v) = post_form(
            &format!("{site}/api/qlopy/v1/oauth/token"),
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &refresh),
                ("client_id", client_id),
            ],
        )?;
        if status != 200 {
            // The site no longer accepts this sign-in (revoked, password changed, expired).
            let _ = entry.delete_credential();
            return Err(format!("Signed out by the site: {}", token_error(&v)));
        }
        let tokens: Tokens = serde_json::from_value(v)
            .map_err(|_| "The site's answer was not understood.".to_string())?;
        entry
            .set_password(&tokens.refresh_token)
            .map_err(|e| e.to_string())?;
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
            site: site.to_string(),
            client_id: client_id.to_string(),
            access: tokens.access_token.clone(),
            expires: Instant::now()
                + Duration::from_secs(tokens.expires_in.saturating_sub(30).max(30)),
        });
        Ok(tokens.access_token)
    }

    fn current(&self) -> Result<(String, String), String> {
        let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        s.as_ref()
            .map(|s| (s.site.clone(), s.client_id.clone()))
            .ok_or_else(|| "Not signed in.".to_string())
    }

    /// Stops waiting for a sign-in in the browser.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Who is signed in, asked from the site.
    pub fn me(&self) -> Result<Account, String> {
        let (site, client) = self.current()?;
        self.me_at(&site, &client)
    }

    /// Who is signed in at a site (also after a restart, from the kept sign-in).
    pub fn me_at(&self, site: &str, client_id: &str) -> Result<Account, String> {
        let site = site_url(site)?;
        let token = self.access(&site, client_id.trim())?;
        let mut res = agent()
            .get(&format!("{site}/api/qlopy/v1/users/me"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| format!("The site could not be reached: {e}"))?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status != 200 {
            return Err(token_error(&v));
        }
        let s = |k: &str| v[k].as_str().map(String::from).unwrap_or_default();
        let name = [s("name"), s("display_name"), s("username"), s("slug")]
            .into_iter()
            .find(|x| !x.is_empty())
            .unwrap_or_default();
        Ok(Account {
            site,
            user_id: v["id"]
                .as_i64()
                .map(|i| i.to_string())
                .unwrap_or_else(|| s("id")),
            name,
            email: s("email"),
        })
    }

    /// Signs out: the site forgets the sign-in (the refresh token is revoked), and so does this computer.
    pub fn sign_out(&self, site: &str, client_id: &str) -> Result<(), String> {
        let site = site_url(site)?;
        let entry = keyring_entry(&site)?;
        if let Ok(refresh) = entry.get_password() {
            let _ = post_form(
                &format!("{site}/api/qlopy/v1/oauth/revoke"),
                &[("token", &refresh), ("client_id", client_id.trim())],
            );
        }
        let _ = entry.delete_credential();
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_the_standard_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let v = random(64).unwrap();
        assert_eq!(v.len(), 64);
        assert_ne!(v, random(64).unwrap());
    }

    #[test]
    fn site_addresses_must_be_https_or_this_computer() {
        assert_eq!(
            site_url("https://example.com/").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            site_url(" http://localhost/amirularham ").unwrap(),
            "http://localhost/amirularham"
        );
        assert!(site_url("http://localhost:8080").is_ok());
        assert!(
            site_url("http://example.com").is_err(),
            "plain http elsewhere"
        );
        assert!(site_url("http://localhost.evil.com").is_err());
        assert!(site_url("https://").is_err());
        assert!(site_url("https://example.com/a b").is_err());
    }

    #[test]
    fn the_authorize_address_and_the_callback_are_read_right() {
        let u = authorize_url("https://shop.example", "app 1", "st8", "ch");
        assert!(u.starts_with(
            "https://shop.example/api/qlopy/oauth/authorize?response_type=code&client_id=app%201"
        ));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A47213%2Fcallback"));
        assert!(u.contains("code_challenge=ch&code_challenge_method=S256"));
        let q = query_of("GET /callback?code=abc%2B1&state=st8 HTTP/1.1");
        assert_eq!(
            q,
            vec![
                ("code".into(), "abc+1".into()),
                ("state".into(), "st8".into())
            ]
        );
    }

    /// The loopback listener takes the code only with the right state, and answers the browser with a page.
    #[test]
    fn the_listener_checks_the_state() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let browser = std::thread::spawn(move || {
            use std::io::Read;
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=c0de&state=good HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut page = String::new();
            let _ = s.read_to_string(&mut page);
            page
        });
        assert_eq!(
            wait_for_code(&l, "good", &Default::default()).unwrap(),
            "c0de"
        );
        assert!(browser.join().unwrap().contains("Signed in to Qanuvi"));

        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=c0de&state=forged HTTP/1.1\r\n\r\n")
                .unwrap();
        });
        assert!(wait_for_code(&l, "good", &Default::default()).is_err());
    }

    #[test]
    fn cancel_stops_waiting_for_the_browser() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let started = Instant::now();
        assert_eq!(
            wait_for_code(&l, "s", &cancel).unwrap_err(),
            "Sign-in cancelled."
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            home().0.starts_with("http"),
            "qanuvi.com unless a test site is given"
        );
    }

    /// A stand-in Qlopy site: checks the PKCE verifier against the challenge like Qlopy does, gives tokens (one-use
    /// refresh), answers users/me for the right token, and takes a revoke.
    fn fake_site() -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
        use std::io::Read;
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        let log: std::sync::Arc<Mutex<Vec<String>>> = Default::default();
        let log2 = log.clone();
        std::thread::spawn(move || {
            let mut challenge = String::new();
            let mut refresh = "r1".to_string();
            for stream in l.incoming().flatten() {
                let mut s = stream;
                // The whole request: the headers, then as much body as Content-Length says.
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len = head
                            .lines()
                            .find_map(|l| {
                                let (k, v) = l.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if body.len() >= len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&data).to_string();
                let line = req.lines().next().unwrap_or("").to_string();
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                let form: Vec<(String, String)> = query_of(&format!("POST /?{body} HTTP/1.1"));
                let f = |k: &str| {
                    form.iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                let (status, json) = if let Some(c) = line.strip_prefix("SETCHALLENGE ") {
                    challenge = c.trim().to_string();
                    (200, "{}".to_string())
                } else if line.starts_with("POST /api/qlopy/v1/oauth/token") {
                    log2.lock().unwrap().push(f("grant_type"));
                    if f("grant_type") == "authorization_code"
                        && f("code") == "good-code"
                        && challenge_ok(&f("code_verifier"), &challenge)
                        && f("redirect_uri") == REDIRECT
                    {
                        (200, r#"{"access_token":"a1","token_type":"Bearer","expires_in":1,"refresh_token":"r1"}"#.to_string())
                    } else if f("grant_type") == "refresh_token" && f("refresh_token") == refresh {
                        refresh = "r2".into();
                        (200, r#"{"access_token":"a2","token_type":"Bearer","expires_in":900,"refresh_token":"r2"}"#.to_string())
                    } else {
                        (400, r#"{"error":"invalid_grant","error_description":"The code is not valid."}"#.to_string())
                    }
                } else if line.starts_with("GET /api/qlopy/v1/users/me") {
                    if req.contains("Bearer a1") || req.contains("Bearer a2") {
                        (
                            200,
                            r#"{"id":7,"name":"Test Owner","email":"owner@example.com"}"#
                                .to_string(),
                        )
                    } else {
                        (401, r#"{"message":"Not signed in."}"#.to_string())
                    }
                } else if line.starts_with("POST /api/qlopy/v1/oauth/revoke") {
                    log2.lock().unwrap().push(format!("revoke {}", f("token")));
                    (200, "{}".to_string())
                } else {
                    (404, "{}".to_string())
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
            }
        });
        (base, log)
    }

    fn challenge_ok(verifier: &str, expected: &str) -> bool {
        challenge(verifier) == expected
    }

    /// Signs in against the stand-in site, reads who signed in, refreshes, signs out. Uses the real loopback port
    /// and the system credential store, so it runs only with QANUVI_TEST_KEYRING=1.
    #[test]
    fn signs_in_refreshes_and_signs_out_against_a_test_site() {
        if std::env::var("QANUVI_TEST_KEYRING").is_err() {
            return;
        }
        let (site, log) = fake_site();
        let accounts = Accounts::default();
        let site_for_browser = site.clone();
        let who = accounts
            .sign_in(&site, "qanuvi-test", move |url| {
                // The "browser": the user signs in on the site, which sends them back with a code.
                let q = query_of(&format!(
                    "GET {} HTTP/1.1",
                    &url[url.find("/api").unwrap()..]
                ));
                let get = |k: &str| {
                    q.iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| v.clone())
                        .unwrap()
                };
                assert_eq!(get("code_challenge_method"), "S256");
                assert_eq!(get("redirect_uri"), REDIRECT);
                let mut s =
                    std::net::TcpStream::connect(site_for_browser.trim_start_matches("http://"))
                        .unwrap();
                write!(s, "SETCHALLENGE {}\r\n\r\n", get("code_challenge")).unwrap();
                drop(s);
                std::thread::sleep(Duration::from_millis(100));
                let state = get("state");
                std::thread::spawn(move || {
                    use std::io::Read;
                    let mut c = std::net::TcpStream::connect(("127.0.0.1", REDIRECT_PORT)).unwrap();
                    write!(
                        c,
                        "GET /callback?code=good-code&state={state} HTTP/1.1\r\n\r\n"
                    )
                    .unwrap();
                    let mut page = String::new();
                    let _ = c.read_to_string(&mut page);
                });
                Ok(())
            })
            .unwrap();
        assert_eq!(
            (who.name.as_str(), who.email.as_str(), who.user_id.as_str()),
            ("Test Owner", "owner@example.com", "7")
        );
        // The first access token was short-lived: the next question refreshes it (one-use refresh token).
        std::thread::sleep(Duration::from_millis(1100));
        let fresh = Accounts::default();
        assert_eq!(
            fresh.me_at(&site, "qanuvi-test").unwrap().name,
            "Test Owner",
            "after a restart, from the kept sign-in"
        );
        accounts.sign_out(&site, "qanuvi-test").unwrap();
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["authorization_code", "refresh_token", "revoke r2"]
        );
        assert!(
            keyring_entry(&site).unwrap().get_password().is_err(),
            "nothing kept after signing out"
        );
    }
}
