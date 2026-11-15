//! Model providers' keys on this machine (Milestone 24; per machine since 24z3 D1). The desktop build keeps them in
//! the system's credential store; a server build (no credential store) reads `QANUVI_KEY_<PROVIDER>` from its
//! environment, and the window cannot add or remove them there. Either build also takes the environment variable when
//! the store has none. The key itself never goes to the window.
//!
//! A master that runs as its own program on a desktop keeps its keys under names of its own (`master-models:<id>`), so
//! a master on the same computer as the window never shares This PC's keys (`models:<id>`).

use std::sync::atomic::{AtomicBool, Ordering};

static MASTER: AtomicBool = AtomicBool::new(false);

/// Called once by a master at start-up: its keys get its own names in the credential store.
pub fn use_master_names() {
    MASTER.store(true, Ordering::Relaxed);
}

/// Whether this program is a master with names of its own (a master on This PC's own data is not).
pub fn is_master() -> bool {
    MASTER.load(Ordering::Relaxed)
}

fn entry(endpoint: &str) -> String {
    entry_for(MASTER.load(Ordering::Relaxed), endpoint)
}

fn entry_for(master: bool, endpoint: &str) -> String {
    if master {
        format!("master-models:{endpoint}")
    } else {
        format!("models:{endpoint}")
    }
}

/// Where this build keeps model keys: "store" (the credential store) or "environment" (a server).
pub fn kind() -> &'static str {
    crate::secrets::kind()
}

/// The environment variable that holds a provider's key: `QANUVI_KEY_OPENROUTER` for `openrouter`.
pub fn env_name(endpoint: &str) -> String {
    format!("QANUVI_KEY_{}", endpoint.to_uppercase().replace('-', "_"))
}

/// A provider's key: the credential store, or the environment variable.
pub fn get(endpoint: &str) -> Option<String> {
    crate::secrets::store::read(&entry(endpoint))
        .or_else(|| crate::brain::env_key(endpoint))
        .filter(|k| !k.trim().is_empty())
}

/// Whether a provider has a key.
pub fn has(endpoint: &str) -> bool {
    get(endpoint).is_some()
}

/// What a server says when the window tries to save or forget a key.
pub fn env_only(endpoint: &str) -> String {
    format!(
        "This server reads model keys from its environment. Set (or remove) {} where qanuvi-master starts, for example in its systemd unit, then restart it.",
        env_name(endpoint)
    )
}

/// Keeps a provider's key (an empty key forgets it). A server build cannot, and says where to set it.
pub fn set(endpoint: &str, key: &str) -> Result<(), String> {
    if kind() == "environment" {
        return Err(env_only(endpoint));
    }
    if key.trim().is_empty() {
        return crate::secrets::store::delete(&entry(endpoint));
    }
    crate::secrets::store::write(&entry(endpoint), key.trim())
        .map_err(|e| format!("The key could not be kept: {e}"))
}

/// The endpoint with this machine's key for it.
pub fn with_key(mut ep: crate::llm::Endpoint) -> crate::llm::Endpoint {
    ep.key = get(&ep.id);
    ep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_master_keeps_its_keys_under_names_of_its_own() {
        assert_eq!(entry_for(false, "openrouter"), "models:openrouter");
        assert_eq!(entry_for(true, "openrouter"), "master-models:openrouter");
    }

    #[test]
    fn the_environment_name_matches_the_agents_one() {
        assert_eq!(env_name("open-router"), "QANUVI_KEY_OPEN_ROUTER");
        let name = env_name("d1test-env");
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var(&name, "k1") };
        assert_eq!(crate::brain::env_key("d1test-env").as_deref(), Some("k1"));
        assert!(has("d1test-env"));
        unsafe { std::env::remove_var(&name) };
    }

    #[test]
    fn a_server_says_where_to_set_a_key() {
        let t = env_only("openrouter");
        assert!(t.contains("QANUVI_KEY_OPENROUTER") && t.contains("restart"));
    }
}
