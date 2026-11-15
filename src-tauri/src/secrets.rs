//! Project secrets: logins, passwords and keys a project's work needs (a test database login, for example). The values
//! live in the system's credential store, one entry per project, and never go back to the window, into plans, chats,
//! memory or Qanuvi's database. Every agent session and the project's test command get them as environment variables,
//! so every step and every model has them without anyone seeing or typing them again. A server build (no credential
//! store, 24z3 A3) reads them from its own environment instead: `QANUVI_SECRET_DB_PASSWORD` gives `DB_PASSWORD` to
//! every project's work there; the window cannot add or remove them, and says where to set them.

use std::collections::BTreeMap;

/// The credential store under the three calls this file needs. The desktop build uses the system's store (keyring);
/// a server build without the desktop feature has none: nothing is read, saving says so, deleting is a no-op.
#[cfg(feature = "desktop")]
pub(crate) mod store {
    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("Qanuvi", key).map_err(|e| e.to_string())
    }

    pub fn read(key: &str) -> Option<String> {
        entry(key).ok()?.get_password().ok()
    }

    pub fn write(key: &str, text: &str) -> Result<(), String> {
        entry(key)?.set_password(text).map_err(|e| e.to_string())
    }

    pub fn delete(key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(feature = "desktop"))]
pub(crate) mod store {
    pub fn read(_key: &str) -> Option<String> {
        None
    }

    pub fn write(_key: &str, _text: &str) -> Result<(), String> {
        Err("This build has no credential store, so secrets cannot be saved here.".into())
    }

    pub fn delete(_key: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The prefix of a server build's secrets in its environment.
pub const ENV_PREFIX: &str = "QANUVI_SECRET_";

/// Where this build keeps project secrets: "store" (the system's credential store) or "environment" (a server).
pub fn kind() -> &'static str {
    if cfg!(feature = "desktop") {
        "store"
    } else {
        "environment"
    }
}

/// A server's secrets from its environment: each `QANUVI_SECRET_<NAME>` with a value, as `<NAME>`.
pub fn from_env(vars: impl Iterator<Item = (String, String)>) -> BTreeMap<String, String> {
    vars.filter_map(|(k, v)| {
        let name = k.strip_prefix(ENV_PREFIX)?;
        (valid_name(name) && !v.is_empty()).then(|| (name.to_string(), v))
    })
    .collect()
}

/// What a server says when the window tries to add or remove a secret.
pub fn env_only(name: &str) -> String {
    format!(
        "This server keeps secrets in its environment. Set (or remove) {ENV_PREFIX}{name} where qanuvi-master          starts, for example in its systemd unit, then restart it."
    )
}

fn key(project_id: &str) -> String {
    format!("secrets:{project_id}")
}

/// An environment variable name: letters, digits and underscores, not starting with a digit.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The stored secrets of a project, read from the credential's text (empty when unreadable).
pub fn parse(text: &str) -> BTreeMap<String, String> {
    serde_json::from_str(text).unwrap_or_default()
}

fn load(project_id: &str) -> BTreeMap<String, String> {
    if kind() == "environment" {
        return from_env(std::env::vars());
    }
    store::read(&key(project_id))
        .map(|t| parse(&t))
        .unwrap_or_default()
}

fn save(project_id: &str, all: &BTreeMap<String, String>) -> Result<(), String> {
    if all.is_empty() {
        return store::delete(&key(project_id));
    }
    let text = serde_json::to_string(all).map_err(|e| e.to_string())?;
    store::write(&key(project_id), &text)
}

/// The names only (what the window shows).
pub fn names(project_id: &str) -> Vec<String> {
    load(project_id).into_keys().collect()
}

pub fn set(project_id: &str, name: &str, value: &str) -> Result<(), String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("A secret's name is like an environment variable: letters, digits and _ (for example DB_PASSWORD).".into());
    }
    if value.is_empty() {
        return Err("The secret has no value.".into());
    }
    if kind() == "environment" {
        return Err(env_only(name));
    }
    let mut all = load(project_id);
    all.insert(name.to_string(), value.to_string());
    save(project_id, &all)
}

pub fn remove(project_id: &str, name: &str) -> Result<(), String> {
    if kind() == "environment" {
        return Err(env_only(name));
    }
    let mut all = load(project_id);
    all.remove(name);
    save(project_id, &all)
}

/// Name and value pairs for a process's environment.
pub fn env(project_id: &str) -> Vec<(String, String)> {
    load(project_id)
        .into_iter()
        .filter(|(k, _)| valid_name(k))
        .collect()
}

/// Private details the owner gave during a plan's run (a login typed into a step): kept for that plan, in the
/// credential store, so every later step and run of the plan has them; never in files or Qanuvi's database.
fn plan_key(project_id: &str, file: &str) -> String {
    format!("plan:{project_id}:{file}")
}

pub fn plan_private(project_id: &str, file: &str) -> Vec<String> {
    store::read(&plan_key(project_id, file))
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn plan_private_add(project_id: &str, file: &str, text: &str) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let mut all = plan_private(project_id, file);
    if !all.iter().any(|t| t == text) {
        all.push(text.chars().take(500).collect());
    }
    // The credential store holds a few kilobytes: the newest details win.
    while serde_json::to_string(&all).map(|s| s.len()).unwrap_or(0) > 2000 && all.len() > 1 {
        all.remove(0);
    }
    let t = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    store::write(&plan_key(project_id, file), &t)
}

pub fn plan_private_clear(project_id: &str, file: &str) -> Result<(), String> {
    store::delete(&plan_key(project_id, file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_reads_its_secrets_from_its_environment() {
        let vars = [
            ("QANUVI_SECRET_DB_PASSWORD", "pw"),
            ("QANUVI_SECRET_EMPTY", ""),
            ("QANUVI_SECRET_9BAD", "x"),
            ("QANUVI_SECRET_", "x"),
            ("QANUVI_KEY_OPENROUTER", "not a project secret"),
            ("PATH", "/usr/bin"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let got = from_env(vars.into_iter());
        assert_eq!(
            got.into_iter().collect::<Vec<_>>(),
            vec![("DB_PASSWORD".to_string(), "pw".to_string())]
        );
        assert!(env_only("DB_PASSWORD").contains("QANUVI_SECRET_DB_PASSWORD"));
        assert_eq!(
            kind(),
            if cfg!(feature = "desktop") {
                "store"
            } else {
                "environment"
            }
        );
    }

    #[test]
    fn names_are_environment_variable_names_and_bad_text_reads_as_none() {
        assert!(valid_name("QCOMMERCE_TEST_PASSWORD"));
        assert!(valid_name("_x1"));
        assert!(!valid_name("1ABC"));
        assert!(!valid_name("DB PASS"));
        assert!(!valid_name("A=B"));
        assert!(!valid_name(""));
        assert!(parse("not json").is_empty());
        let m = parse(r#"{"B":"2","A":"1"}"#);
        assert_eq!(m.keys().collect::<Vec<_>>(), ["A", "B"]);
    }
}
