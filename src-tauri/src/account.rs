//! Signing in to Qanuvi with the central Qanuvi account at qanuvi.com (Milestone 19; the site runs on Qlopy and offers
//! Google, Microsoft or GitHub sign-in there): OAuth 2.0 authorization code with PKCE, the way an app
//! on a computer should (RFC 8252). The site's own sign-in page opens in the user's browser (with its two-factor step,
//! if the site has one); Qanuvi never sees the password. The short-lived access token lives only in memory.
//!
//! Each machine has its own account (migration M2): the sign-in is kept by the machine it belongs to, in that
//! machine's store (the system's credential store in a desktop build; in a server build, `account-sign-in.json` in its
//! data folder, readable by its own user only, never copied into backups). A browser sign-in for a master is split in
//! three: the master makes the PKCE verifier and the sign-in address (`begin`), the window opens the address in the
//! browser on its own computer and catches the answer on 127.0.0.1 (`catch_code`), and hands the one-time code to the
//! master, which alone trades it for tokens (`finish`). A server can also sign in with an application password made
//! on the site (`qanuvi-master login`), sent as HTTP Basic, the way the site's REST API takes them.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The fixed loopback address Qanuvi listens on for the site's answer. Qlopy matches redirect addresses exactly, so this
/// is the address to register for the Qanuvi app on the site (Settings > API > Apps).
pub const REDIRECT_PORT: u16 = 47213;
pub const REDIRECT: &str = "http://127.0.0.1:47213/callback";
/// The central account site and Qanuvi's app id registered there.
pub const HOME_SITE: &str = "https://qanuvi.com";
pub const CLIENT_ID: &str = "qanuvi-desktop";
/// A server build's kept sign-ins, in its data folder.
pub const KEPT_FILE: &str = "account-sign-in.json";

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

/// Who is signed in (shown in the app), and how: "browser" (the site's sign-in page) or "password" (an application
/// password given to `qanuvi-master login`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Account {
    pub site: String,
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub via: String,
}

/// A browser sign-in begun on this machine: the address the window opens, and the state its answer must carry.
#[derive(Debug, Clone, Serialize)]
pub struct Begun {
    pub url: String,
    pub state: String,
}

/// A kept sign-in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Kept {
    /// From the browser: the one-use refresh token.
    Browser { refresh: String },
    /// An application password made on the site, with the login name it belongs to.
    Password { login: String, password: String },
}

impl Kept {
    /// A kept text: JSON, or a bare refresh token (how This PC kept it before M2).
    #[cfg_attr(not(any(feature = "desktop", test)), allow(dead_code))]
    fn read(text: &str) -> Option<Kept> {
        let t = text.trim();
        if t.is_empty() {
            return None;
        }
        serde_json::from_str(t).ok().or_else(|| {
            (!t.starts_with('{')).then(|| Kept::Browser {
                refresh: t.to_string(),
            })
        })
    }

    fn via(&self) -> &'static str {
        match self {
            Kept::Browser { .. } => "browser",
            Kept::Password { .. } => "password",
        }
    }
}

/// Where a machine keeps its sign-in.
enum Keep {
    /// The system's credential store, under `<prefix><site>`.
    #[cfg(feature = "desktop")]
    Store(String),
    /// A file of the machine's own, readable by its user only (a server build; a desktop build only in tests).
    #[cfg_attr(all(feature = "desktop", not(test)), allow(dead_code))]
    File(PathBuf),
}

impl Keep {
    #[cfg(feature = "desktop")]
    fn entry(prefix: &str, site: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("Qanuvi", &format!("{prefix}{site}")).map_err(|e| e.to_string())
    }

    fn all(file: &Path) -> BTreeMap<String, Kept> {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_all(file: &Path, all: &BTreeMap<String, Kept>) -> Result<(), String> {
        if all.is_empty() {
            return match std::fs::remove_file(file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let text = serde_json::to_string_pretty(all).map_err(|e| e.to_string())?;
        let tmp = file.with_extension("json.new");
        let _ = std::fs::remove_file(&tmp);
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes())
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        std::fs::rename(&tmp, file).map_err(|e| e.to_string())
    }

    fn read(&self, site: &str) -> Option<Kept> {
        match self {
            #[cfg(feature = "desktop")]
            Keep::Store(prefix) => {
                Kept::read(&Self::entry(prefix, site).ok()?.get_password().ok()?)
            }
            Keep::File(file) => Self::all(file).remove(site),
        }
    }

    fn write(&self, site: &str, kept: &Kept) -> Result<(), String> {
        match self {
            #[cfg(feature = "desktop")]
            Keep::Store(prefix) => Self::entry(prefix, site)?
                .set_password(&serde_json::to_string(kept).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string()),
            Keep::File(file) => {
                let mut all = Self::all(file);
                all.insert(site.to_string(), kept.clone());
                Self::save_all(file, &all)
            }
        }
    }

    fn forget(&self, site: &str) -> Result<(), String> {
        match self {
            #[cfg(feature = "desktop")]
            Keep::Store(prefix) => match Self::entry(prefix, site)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Keep::File(file) => {
                let mut all = Self::all(file);
                if all.remove(site).is_none() {
                    return Ok(());
                }
                Self::save_all(file, &all)
            }
        }
    }
}

struct Session {
    site: String,
    /// The refresh token kept with it: when the kept sign-in changes (signed out, or `qanuvi-master login` while the
    /// service runs), this session is not used any more.
    refresh: String,
    access: String,
    expires: Instant,
}

/// A browser sign-in waiting for its code.
struct Pending {
    site: String,
    client_id: String,
    state: String,
    verifier: String,
    at: Instant,
}

pub struct Accounts {
    keep: Keep,
    session: Mutex<Option<Session>>,
    pending: Mutex<Option<Pending>>,
    /// Set to stop waiting for a sign-in (the user pressed Cancel; the window's own wait).
    cancel: std::sync::atomic::AtomicBool,
}

/// The account site's HTTP client. It trusts what the system trusts (its certificate store), so a site behind a
/// company's or a developer's own certificate authority works as it does in the browser.
fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
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

/// The HTTP Basic header for an application password.
fn basic(login: &str, password: &str) -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{login}:{password}"))
    )
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
    fn with(keep: Keep) -> Self {
        Self {
            keep,
            session: Mutex::default(),
            pending: Mutex::default(),
            cancel: Default::default(),
        }
    }

    /// This PC's account: the credential store under the names it always had (`account:<site>`).
    pub fn this_pc(dir: &Path) -> Self {
        #[cfg(feature = "desktop")]
        {
            let _ = dir;
            Self::with(Keep::Store("account:".into()))
        }
        #[cfg(not(feature = "desktop"))]
        Self::with(Keep::File(dir.join(KEPT_FILE)))
    }

    /// A master's own account: in a desktop build the credential store under names of its own
    /// (`master-account:<machine id>:<site>`, so a master on the same computer never shares This PC's sign-in); in a
    /// server build its own file.
    pub fn master(dir: &Path, machine_id: &str) -> Self {
        #[cfg(feature = "desktop")]
        {
            let _ = dir;
            Self::with(Keep::Store(format!("master-account:{machine_id}:")))
        }
        #[cfg(not(feature = "desktop"))]
        {
            let _ = machine_id;
            Self::with(Keep::File(dir.join(KEPT_FILE)))
        }
    }

    /// An account kept in a given file (tests).
    #[cfg(test)]
    fn in_file(file: PathBuf) -> Self {
        Self::with(Keep::File(file))
    }

    /// Begins a browser sign-in on this machine: the verifier stays here; the window opens the address.
    pub fn begin(&self, site: &str, client_id: &str) -> Result<Begun, String> {
        let site = site_url(site)?;
        let client_id = client_id.trim();
        if client_id.is_empty() || client_id.len() > 60 {
            return Err("Give the app's client id from the site (Settings > API > Apps).".into());
        }
        let verifier = random(64)?;
        let state = random(24)?;
        let url = authorize_url(&site, client_id, &state, &challenge(&verifier));
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pending {
            site,
            client_id: client_id.to_string(),
            state: state.clone(),
            verifier,
            at: Instant::now(),
        });
        Ok(Begun { url, state })
    }

    /// The window's part of a browser sign-in: opens the address with `open` and waits on this computer's loopback
    /// address for the site's answer; gives back the one-time code.
    pub fn catch_code(
        &self,
        url: &str,
        state: &str,
        open: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<String, String> {
        let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|_| {
            format!(
                "Port {REDIRECT_PORT} on this computer is in use; close what uses it and try again."
            )
        })?;
        self.cancel
            .store(false, std::sync::atomic::Ordering::SeqCst);
        open(url)?;
        wait_for_code(&listener, state, &self.cancel)
    }

    /// Ends a browser sign-in begun here: trades the code for tokens and keeps the sign-in.
    pub fn finish(&self, state: &str, code: &str) -> Result<Account, String> {
        let p = {
            let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
            match pending.take() {
                Some(p) if p.state == state && p.at.elapsed() < WAIT + Duration::from_secs(30) => p,
                other => {
                    *pending = other;
                    return Err(
                        "This sign-in was not started on this machine, or it is too old. Sign in again."
                            .into(),
                    );
                }
            }
        };
        let code = code.trim();
        if code.is_empty() || code.len() > 2000 {
            return Err("The site sent no code.".into());
        }
        let (status, v) = post_form(
            &format!("{}/api/qlopy/v1/oauth/token", p.site),
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", REDIRECT),
                ("client_id", &p.client_id),
                ("code_verifier", &p.verifier),
            ],
        )?;
        if status != 200 {
            return Err(token_error(&v));
        }
        let tokens: Tokens = serde_json::from_value(v)
            .map_err(|_| "The site's answer was not understood.".to_string())?;
        self.keep
            .write(
                &p.site,
                &Kept::Browser {
                    refresh: tokens.refresh_token.clone(),
                },
            )
            .map_err(|e| format!("The sign-in could not be kept: {e}"))?;
        self.set_session(&p.site, &tokens);
        self.me_at(&p.site, &p.client_id)
    }

    /// Signs in with an application password made on the site: checked against the site first, then kept.
    pub fn login(&self, site: &str, login: &str, password: &str) -> Result<Account, String> {
        let site = site_url(site)?;
        let (login, password) = (login.trim(), password.trim());
        if login.is_empty() || password.is_empty() {
            return Err("Give the login name and the application password.".into());
        }
        if login.contains(':') || login.len() > 200 || password.len() > 500 {
            return Err("That login name or application password is not valid.".into());
        }
        let who = self.ask_me(&site, &basic(login, password), "password")?;
        self.keep
            .write(
                &site,
                &Kept::Password {
                    login: login.into(),
                    password: password.into(),
                },
            )
            .map_err(|e| format!("The sign-in could not be kept: {e}"))?;
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(who)
    }

    fn set_session(&self, site: &str, tokens: &Tokens) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
            site: site.to_string(),
            refresh: tokens.refresh_token.clone(),
            access: tokens.access_token.clone(),
            expires: Instant::now()
                + Duration::from_secs(tokens.expires_in.saturating_sub(30).max(30)),
        });
    }

    /// A valid access token for a browser sign-in: the current one, or a new one from the refresh token (which is
    /// one-use, so the new refresh token replaces it).
    fn access(&self, site: &str, client_id: &str, refresh: &str) -> Result<String, String> {
        {
            let s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = s.as_ref()
                && s.site == site
                && s.refresh == refresh
                && s.expires > Instant::now()
            {
                return Ok(s.access.clone());
            }
        }
        let (status, v) = post_form(
            &format!("{site}/api/qlopy/v1/oauth/token"),
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
                ("client_id", client_id),
            ],
        )?;
        if status != 200 {
            // The site no longer accepts this sign-in (revoked, password changed, expired).
            let _ = self.keep.forget(site);
            return Err(format!("Signed out by the site: {}", token_error(&v)));
        }
        let tokens: Tokens = serde_json::from_value(v)
            .map_err(|_| "The site's answer was not understood.".to_string())?;
        self.keep.write(
            site,
            &Kept::Browser {
                refresh: tokens.refresh_token.clone(),
            },
        )?;
        self.set_session(site, &tokens);
        Ok(tokens.access_token)
    }

    /// Asks the site who an authorization header belongs to.
    fn ask_me(&self, site: &str, authorization: &str, via: &str) -> Result<Account, String> {
        let mut res = agent()
            .get(&format!("{site}/api/qlopy/v1/users/me"))
            .header("Authorization", authorization)
            .call()
            .map_err(|e| format!("The site could not be reached: {e}"))?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status != 200 {
            let why = token_error(&v);
            return Err(if status == 401 && via == "password" {
                format!("The site did not accept this login name and application password: {why}")
            } else {
                why
            });
        }
        let s = |k: &str| v[k].as_str().map(String::from).unwrap_or_default();
        let name = [s("name"), s("display_name"), s("username"), s("slug")]
            .into_iter()
            .find(|x| !x.is_empty())
            .unwrap_or_default();
        Ok(Account {
            site: site.to_string(),
            user_id: v["id"]
                .as_i64()
                .map(|i| i.to_string())
                .unwrap_or_else(|| s("id")),
            name,
            email: s("email"),
            via: via.to_string(),
        })
    }

    /// Stops waiting for a sign-in in the browser.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Who is signed in at a site (also after a restart, from the kept sign-in).
    pub fn me_at(&self, site: &str, client_id: &str) -> Result<Account, String> {
        let site = site_url(site)?;
        match self.keep.read(&site) {
            None => Err("Not signed in.".into()),
            Some(Kept::Browser { refresh }) => {
                let token = self.access(&site, client_id.trim(), &refresh)?;
                self.ask_me(&site, &format!("Bearer {token}"), "browser")
            }
            Some(Kept::Password { login, password }) => {
                match self.ask_me(&site, &basic(&login, &password), "password") {
                    // The site no longer takes this application password (removed there).
                    Err(e) if e.starts_with("The site did not accept") => {
                        let _ = self.keep.forget(&site);
                        Err(format!("Signed out by the site: {e}"))
                    }
                    r => r,
                }
            }
        }
    }

    /// Signs out: a browser sign-in is revoked at the site; an application password stays on the site until it is
    /// removed there. Either way this machine forgets it.
    pub fn sign_out(&self, site: &str, client_id: &str) -> Result<(), String> {
        let site = site_url(site)?;
        if let Some(Kept::Browser { refresh }) = self.keep.read(&site) {
            let _ = post_form(
                &format!("{site}/api/qlopy/v1/oauth/revoke"),
                &[("token", &refresh), ("client_id", client_id.trim())],
            );
        }
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.keep.forget(&site)
    }

    /// How the kept sign-in at a site was made, without asking the site ("" when none is kept).
    pub fn kept_via(&self, site: &str) -> &'static str {
        site_url(site)
            .ok()
            .and_then(|s| self.keep.read(&s))
            .map(|k| k.via())
            .unwrap_or("")
    }
}

/// Reads a line from the terminal without showing it (an application password).
pub fn read_hidden(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let quiet = hidden::Quiet::start();
    let mut line = String::new();
    let r = std::io::stdin().read_line(&mut line);
    drop(quiet);
    eprintln!();
    r.map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

#[cfg(unix)]
mod hidden {
    /// Turns the terminal's echo off while it lives (nothing happens when the input is not a terminal).
    pub struct Quiet(bool);

    impl Quiet {
        pub fn start() -> Self {
            let tty = std::process::Command::new("stty")
                .arg("-echo")
                .stdin(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            Self(tty)
        }
    }

    impl Drop for Quiet {
        fn drop(&mut self) {
            if self.0 {
                let _ = std::process::Command::new("stty")
                    .arg("echo")
                    .stdin(std::process::Stdio::inherit())
                    .status();
            }
        }
    }
}

#[cfg(windows)]
mod hidden {
    use std::ffi::c_void;

    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const ENABLE_ECHO_INPUT: u32 = 0x0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
    }

    /// Turns the console's echo off while it lives (nothing happens when the input is not a console).
    pub struct Quiet(Option<(*mut c_void, u32)>);

    impl Quiet {
        pub fn start() -> Self {
            // SAFETY: the standard input handle of this process, read and set with the console's own calls.
            unsafe {
                let h = GetStdHandle(STD_INPUT_HANDLE);
                let mut mode = 0u32;
                if h.is_null() || GetConsoleMode(h, &mut mode) == 0 {
                    return Self(None);
                }
                if SetConsoleMode(h, mode & !ENABLE_ECHO_INPUT) == 0 {
                    return Self(None);
                }
                Self(Some((h, mode)))
            }
        }
    }

    impl Drop for Quiet {
        fn drop(&mut self) {
            if let Some((h, mode)) = self.0 {
                // SAFETY: puts back the mode read in `start` on the same handle.
                unsafe {
                    SetConsoleMode(h, mode);
                }
            }
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod hidden {
    pub struct Quiet;

    impl Quiet {
        pub fn start() -> Self {
            Quiet
        }
    }
}

/// `qanuvi-master login`: signs a master in with an application password. Reads the login name and the password from
/// the terminal (the password not shown), or the password from standard input with `--password-stdin`.
pub fn cli_login(
    accounts: &Accounts,
    site: Option<&str>,
    password_stdin: bool,
) -> Result<(), String> {
    let (home_site, _) = home();
    let site = site_url(site.unwrap_or(&home_site))?;
    eprintln!(
        "Sign this machine in to {site} with an application password (make one on the site under Machine passwords)."
    );
    eprint!("Login name: ");
    let _ = std::io::stderr().flush();
    let mut login = String::new();
    let password = if password_stdin {
        // The login name comes first on its own line, then the password.
        std::io::stdin()
            .read_line(&mut login)
            .map_err(|e| e.to_string())?;
        eprintln!();
        let mut p = String::new();
        std::io::stdin()
            .read_line(&mut p)
            .map_err(|e| e.to_string())?;
        p
    } else {
        std::io::stdin()
            .read_line(&mut login)
            .map_err(|e| e.to_string())?;
        read_hidden("Application password: ")?
    };
    let who = accounts.login(&site, login.trim(), password.trim())?;
    println!(
        "Signed in as {}{} on {site}.",
        if who.name.is_empty() {
            "this account"
        } else {
            &who.name
        },
        if who.email.is_empty() {
            String::new()
        } else {
            format!(" ({})", who.email)
        }
    );
    Ok(())
}

/// `qanuvi-master logout`: forgets the master's sign-in (a browser sign-in is revoked at the site too).
pub fn cli_logout(accounts: &Accounts, site: Option<&str>) -> Result<(), String> {
    let (home_site, client) = home();
    let site = site_url(site.unwrap_or(&home_site))?;
    let via = accounts.kept_via(&site);
    accounts.sign_out(&site, &client)?;
    println!(
        "{}",
        match via {
            "" => format!("Not signed in to {site}."),
            "password" => format!(
                "Signed out of {site}. The application password stays on the site until you remove it there."
            ),
            _ => format!("Signed out of {site}."),
        }
    );
    Ok(())
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
                    if req.contains("Bearer a1")
                        || req.contains("Bearer a2")
                        || req.contains(&basic("owner", "good-pw"))
                    {
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

    /// The browser sign-in in its three parts against the stand-in site, kept in a file (a server's store): reads who
    /// signed in, refreshes, signs out.
    #[test]
    fn signs_in_refreshes_and_signs_out_against_a_test_site() {
        let (site, log) = fake_site();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(KEPT_FILE);
        // The master's part: it makes the address; the window (here: this test) opens it and catches the code.
        let master = Accounts::in_file(file.clone());
        let begun = master.begin(&site, "qanuvi-test").unwrap();
        let q = query_of(&format!(
            "GET {} HTTP/1.1",
            &begun.url[begun.url.find("/api").unwrap()..]
        ));
        let get = |k: &str| {
            q.iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(get("code_challenge_method"), "S256");
        assert_eq!(get("redirect_uri"), REDIRECT);
        assert_eq!(get("state"), begun.state);
        let mut s = std::net::TcpStream::connect(site.trim_start_matches("http://")).unwrap();
        write!(s, "SETCHALLENGE {}\r\n\r\n", get("code_challenge")).unwrap();
        drop(s);
        std::thread::sleep(Duration::from_millis(100));

        // A code with another sign-in's state is refused, and the real one still finishes afterwards.
        assert!(master.finish("forged", "good-code").is_err());
        assert!(
            Accounts::in_file(file.clone())
                .finish(&begun.state, "good-code")
                .is_err(),
            "only the machine that began it can finish it"
        );
        let who = master.finish(&begun.state, "good-code").unwrap();
        assert_eq!(
            (
                who.name.as_str(),
                who.email.as_str(),
                who.user_id.as_str(),
                who.via.as_str()
            ),
            ("Test Owner", "owner@example.com", "7", "browser")
        );
        assert!(
            master.finish(&begun.state, "good-code").is_err(),
            "a sign-in finishes once"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // The first access token was short-lived: the next question refreshes it (one-use refresh token).
        std::thread::sleep(Duration::from_millis(1100));
        let fresh = Accounts::in_file(file.clone());
        assert_eq!(
            fresh.me_at(&site, "qanuvi-test").unwrap().name,
            "Test Owner",
            "after a restart, from the kept sign-in"
        );
        // Signing out revokes the newest refresh token, read from the file.
        master.sign_out(&site, "qanuvi-test").unwrap();
        let calls = log.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["authorization_code", "refresh_token", "revoke r2"]
        );
        assert!(!file.exists(), "nothing kept after signing out");
        assert_eq!(
            fresh.me_at(&site, "qanuvi-test").unwrap_err(),
            "Not signed in."
        );
    }

    /// `qanuvi-master login`: an application password is checked at the site before it is kept, sent as HTTP Basic,
    /// and signing out forgets it here without a revoke (it stays on the site until removed there).
    #[test]
    fn an_application_password_signs_a_master_in() {
        let (site, log) = fake_site();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(KEPT_FILE);
        let a = Accounts::in_file(file.clone());
        let e = a.login(&site, "owner", "wrong").unwrap_err();
        assert!(e.starts_with("The site did not accept"), "{e}");
        assert!(!file.exists(), "a refused password is not kept");
        assert!(a.login(&site, "own:er", "x").is_err());
        let who = a.login(&site, "owner", " good-pw ").unwrap();
        assert_eq!(
            (who.name.as_str(), who.via.as_str()),
            ("Test Owner", "password")
        );
        assert_eq!(a.kept_via(&site), "password");
        assert_eq!(
            Accounts::in_file(file.clone())
                .me_at(&site, "x")
                .unwrap()
                .via,
            "password",
            "after a restart"
        );
        a.sign_out(&site, "x").unwrap();
        assert!(!file.exists());
        assert!(
            !log.lock().unwrap().iter().any(|c| c.starts_with("revoke")),
            "an application password is not revoked from here"
        );
    }

    #[test]
    fn a_bare_refresh_token_from_before_reads_as_a_browser_sign_in() {
        assert_eq!(
            Kept::read("r-old"),
            Some(Kept::Browser {
                refresh: "r-old".into()
            })
        );
        let k = Kept::Password {
            login: "a".into(),
            password: "b".into(),
        };
        assert_eq!(Kept::read(&serde_json::to_string(&k).unwrap()), Some(k));
        assert_eq!(Kept::read("{broken"), None);
        assert_eq!(Kept::read("  "), None);
    }

    #[test]
    fn the_kept_file_holds_one_sign_in_per_site() {
        let dir = tempfile::tempdir().unwrap();
        let keep = Keep::File(dir.path().join(KEPT_FILE));
        let a = Kept::Browser {
            refresh: "r".into(),
        };
        let b = Kept::Password {
            login: "o".into(),
            password: "p".into(),
        };
        keep.write("https://a.example", &a).unwrap();
        keep.write("https://b.example", &b).unwrap();
        assert_eq!(keep.read("https://a.example"), Some(a));
        keep.forget("https://a.example").unwrap();
        keep.forget("https://a.example").unwrap();
        assert_eq!(keep.read("https://a.example"), None);
        assert_eq!(keep.read("https://b.example"), Some(b));
    }
}
