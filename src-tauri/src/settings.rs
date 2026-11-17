//! A machine's settings (24z3 Part A3): what the Settings page shows, kept in this machine's database so a window
//! showing the machine (This PC, or a master over the connection) reads and changes the machine's own. One JSON value
//! per top-level key, so a change to one key never overwrites another window's change to a different one. The
//! window's own things (its sign-in; the server list, which has its own table in servers.rs) are never sent here.

use crate::store::{Db, now};
use rusqlite::params;
use serde_json::{Map, Value};

/// Keys a window keeps for itself; a machine refuses them.
const WINDOW_KEYS: [&str; 2] = ["masters", "account"];

/// Largest value kept for one key (the Settings page is a few kilobytes; this leaves room).
const MAX_VALUE: usize = 1 << 20;

fn check_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Err(format!("Not a settings key: {key:?}."));
    }
    if WINDOW_KEYS.contains(&key) {
        return Err(format!("{key} belongs to the window, not to a machine."));
    }
    Ok(())
}

/// Every kept key and its value. A value that is not JSON any more is left out.
pub fn get_all(db: &Db) -> Result<Map<String, Value>, String> {
    let conn = db.conn();
    let mut q = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut out = Map::new();
    for row in rows {
        let (key, text) = row.map_err(|e| e.to_string())?;
        if let Ok(v) = serde_json::from_str(&text) {
            out.insert(key, v);
        }
    }
    Ok(out)
}

/// Keeps several keys at once (null removes one). Nothing is kept when any key is refused.
pub fn set_many(db: &Db, values: &Map<String, Value>) -> Result<(), String> {
    let mut texts = Vec::with_capacity(values.len());
    for (key, v) in values {
        check_key(key)?;
        let text = if v.is_null() {
            None
        } else {
            Some(v.to_string())
        };
        if text.as_ref().is_some_and(|t| t.len() > MAX_VALUE) {
            return Err(format!("The setting {key} is too large to keep."));
        }
        texts.push((key, text));
    }
    let mut conn = db.conn();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let at = now();
    for (key, text) in texts {
        match text {
            Some(t) => tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key, t, at],
            ),
            None => tx.execute("DELETE FROM settings WHERE key = ?1", params![key]),
        }
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())
}

/// The `values` argument of `settings_set`: an object of keys.
pub fn values_arg(v: &Value) -> Result<&Map<String, Value>, String> {
    v.as_object()
        .ok_or_else(|| "settings_set needs an object of values.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn keeps_changes_and_removes_with_null() {
        let db = db();
        assert!(get_all(&db).unwrap().is_empty());
        let v = json!({"team": {"decider": "a"}, "approveMinutes": 15});
        set_many(&db, values_arg(&v).unwrap()).unwrap();
        set_many(&db, json!({"approveMinutes": 30}).as_object().unwrap()).unwrap();
        let all = get_all(&db).unwrap();
        assert_eq!(all["team"], json!({"decider": "a"}));
        assert_eq!(all["approveMinutes"], json!(30));
        set_many(&db, json!({"team": null}).as_object().unwrap()).unwrap();
        assert!(!get_all(&db).unwrap().contains_key("team"));
    }

    #[test]
    fn refuses_window_keys_and_odd_keys_and_keeps_nothing_then() {
        let db = db();
        for bad in [
            json!({"ai": true, "masters": []}),
            json!({"ai": true, "account": {}}),
            json!({"ai": true, "a b": 1}),
            json!({"ai": true, "": 1}),
        ] {
            assert!(set_many(&db, bad.as_object().unwrap()).is_err());
        }
        assert!(get_all(&db).unwrap().is_empty());
        assert!(values_arg(&json!([1])).is_err());
    }
}
