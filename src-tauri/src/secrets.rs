//! Project secrets: logins, passwords and keys a project's work needs (a test database login, for example). The values
//! live in the system's credential store, one entry per project, and never go back to the window, into plans, chats,
//! memory or Qanuvi's database. Every agent session and the project's test command get them as environment variables,
//! so every step and every model has them without anyone seeing or typing them again.

use std::collections::BTreeMap;

fn entry(project_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Qanuvi", &format!("secrets:{project_id}")).map_err(|e| e.to_string())
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
    entry(project_id)
        .and_then(|e| e.get_password().map_err(|e| e.to_string()))
        .map(|t| parse(&t))
        .unwrap_or_default()
}

fn store(project_id: &str, all: &BTreeMap<String, String>) -> Result<(), String> {
    let e = entry(project_id)?;
    if all.is_empty() {
        return match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        };
    }
    let text = serde_json::to_string(all).map_err(|e| e.to_string())?;
    e.set_password(&text).map_err(|e| e.to_string())
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
    store(project_id, &all)
}

pub fn remove(project_id: &str, name: &str) -> Result<(), String> {
    let mut all = load(project_id);
    all.remove(name);
    store(project_id, &all)
}

/// Name and value pairs for a process's environment.
pub fn env(project_id: &str) -> Vec<(String, String)> {
    load(project_id)
        .into_iter()
        .filter(|(k, _)| valid_name(k))
        .collect()
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
