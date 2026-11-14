//! Project secrets: logins, passwords and keys a project's work needs (a test database login, for example). The values
//! live in the system's credential store, one entry per project, and never go back to the window, into plans, chats,
//! memory or Qanuvi's database. Every agent session and the project's test command get them as environment variables,
//! so every step and every model has them without anyone seeing or typing them again.

use std::collections::BTreeMap;

/// The credential store under the three calls this file needs. The desktop build uses the system's store (keyring);
/// a server build without the desktop feature has none: nothing is read, saving says so, deleting is a no-op.
#[cfg(feature = "desktop")]
mod store {
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
mod store {
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
    let mut all = load(project_id);
    all.insert(name.to_string(), value.to_string());
    save(project_id, &all)
}

pub fn remove(project_id: &str, name: &str) -> Result<(), String> {
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
