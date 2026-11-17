//! A machine's own server list (migration M3a): the servers it connects to, kept in its database in the order shown.
//! Once the window kept this list in its localStorage; the first start moves it in, once only. Each entry is the
//! window's server object as JSON (name, colour, lasting id, SSH address or relay details, connect at start); its
//! pairing key is never in it (that stays in the machine's secret store).

use crate::store::{Db, now};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

/// The flag noting the window's old list was moved in.
const IMPORTED: &str = "servers-imported";

/// Largest entry kept (a server is a few hundred bytes).
const MAX_ENTRY: usize = 16 * 1024;

/// Most servers one machine keeps.
const MAX_SERVERS: i64 = 200;

/// The entry's id when it is a server this machine can keep, else why not.
fn check(server: &Value) -> Result<&str, String> {
    let o = server
        .as_object()
        .ok_or_else(|| "A server must be an object.".to_string())?;
    let id = o.get("id").and_then(Value::as_str).unwrap_or("");
    if id.is_empty() || id.len() > 60 || id.chars().any(char::is_control) {
        return Err("A server needs an id of at most 60 characters.".into());
    }
    let target = o.get("target").and_then(Value::as_str).unwrap_or("");
    if target.trim().is_empty() || target.len() > 200 {
        return Err("A server needs its address (at most 200 characters).".into());
    }
    if server.to_string().len() > MAX_ENTRY {
        return Err("This server's details are too large to keep.".into());
    }
    Ok(id)
}

/// Every server, in the order shown. An entry that is not JSON any more is left out.
pub fn list(db: &Db) -> Result<Vec<Value>, String> {
    let conn = db.conn();
    let mut q = conn
        .prepare("SELECT value FROM servers ORDER BY position, id")
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        if let Ok(v) = serde_json::from_str(&row.map_err(|e| e.to_string())?) {
            out.push(v);
        }
    }
    Ok(out)
}

/// Whether the window's old list was moved in already.
pub fn imported(db: &Db) -> Result<bool, String> {
    db.conn()
        .query_row("SELECT 1 FROM flags WHERE key = ?1", [IMPORTED], |_| Ok(()))
        .optional()
        .map(|v| v.is_some())
        .map_err(|e| e.to_string())
}

/// Moves the window's old list in, once only: a second call (another window starting, a later start) keeps what is
/// here. Broken entries and a repeated id are left out. Answers the list now kept.
pub fn import(db: &Db, old: &[Value]) -> Result<Vec<Value>, String> {
    {
        let mut conn = db.conn();
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        let done = tx
            .query_row("SELECT 1 FROM flags WHERE key = ?1", [IMPORTED], |_| Ok(()))
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        if !done {
            let at = now();
            let mut position = 0i64;
            for server in old {
                let Ok(id) = check(server) else { continue };
                if position >= MAX_SERVERS {
                    break;
                }
                let added = tx
                    .execute(
                        "INSERT OR IGNORE INTO servers (id, position, value, updated_at) VALUES (?1, ?2, ?3, ?4)",
                        params![id, position, server.to_string(), at],
                    )
                    .map_err(|e| e.to_string())?;
                position += added as i64;
            }
            tx.execute(
                "INSERT INTO flags (key, value, updated_at) VALUES (?1, '1', ?2)",
                params![IMPORTED, at],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    list(db)
}

/// Keeps a server: a known one keeps its place in the list, a new one goes last.
pub fn save(db: &Db, server: &Value) -> Result<(), String> {
    let id = check(server)?;
    let conn = db.conn();
    let known = conn
        .query_row("SELECT 1 FROM servers WHERE id = ?1", [id], |_| Ok(()))
        .optional()
        .map_err(|e| e.to_string())?
        .is_some();
    if !known {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM servers", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count >= MAX_SERVERS {
            return Err(format!("A machine keeps at most {MAX_SERVERS} servers."));
        }
    }
    conn.execute(
        "INSERT INTO servers (id, position, value, updated_at)
         VALUES (?1, (SELECT COALESCE(MAX(position) + 1, 0) FROM servers), ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![id, server.to_string(), now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Takes a server out of the list. Answers whether it was there.
pub fn remove(db: &Db, id: &str) -> Result<bool, String> {
    db.conn()
        .execute("DELETE FROM servers WHERE id = ?1", [id])
        .map(|n| n > 0)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn server(id: &str, target: &str) -> Value {
        json!({"id": id, "name": "", "target": target, "program": "qanuvi-master", "identity": "", "autoConnect": false})
    }

    #[test]
    fn moves_the_old_list_in_once_only() {
        let db = db();
        assert!(!imported(&db).unwrap());
        let got = import(&db, &[server("a", "root@vps"), server("b", "local")]).unwrap();
        assert_eq!(got, vec![server("a", "root@vps"), server("b", "local")]);
        assert!(imported(&db).unwrap());
        // Another window starting with an older copy changes nothing.
        let again = import(&db, &[server("c", "x@y")]).unwrap();
        assert_eq!(again, got);
        // A server removed after the move does not come back from the old copy.
        assert!(remove(&db, "a").unwrap());
        assert_eq!(
            import(&db, &[server("a", "root@vps")]).unwrap(),
            vec![server("b", "local")]
        );
    }

    #[test]
    fn an_empty_old_list_is_moved_too() {
        let db = db();
        assert!(import(&db, &[]).unwrap().is_empty());
        assert!(imported(&db).unwrap());
        assert!(import(&db, &[server("a", "root@vps")]).unwrap().is_empty());
    }

    #[test]
    fn leaves_out_broken_and_repeated_entries() {
        let db = db();
        let got = import(
            &db,
            &[
                json!("not a server"),
                json!({"id": "", "target": "x"}),
                json!({"id": "n", "target": "  "}),
                json!({"id": "n2"}),
                server("a", "root@vps"),
                server("a", "other@host"),
                server("b", "local"),
            ],
        )
        .unwrap();
        assert_eq!(got, vec![server("a", "root@vps"), server("b", "local")]);
    }

    #[test]
    fn save_keeps_the_place_and_adds_new_ones_last() {
        let db = db();
        import(&db, &[server("a", "root@vps"), server("b", "local")]).unwrap();
        let mut a = server("a", "root@vps");
        a["name"] = json!("VPS");
        save(&db, &a).unwrap();
        save(&db, &server("c", "deploy@host")).unwrap();
        let ids: Vec<String> = list(&db)
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(list(&db).unwrap()[0]["name"], json!("VPS"));
        assert!(!remove(&db, "zz").unwrap());
        assert!(save(&db, &json!({"id": "d"})).is_err());
        assert!(
            save(
                &db,
                &json!({"id": "d", "target": "x", "pad": "y".repeat(MAX_ENTRY)})
            )
            .is_err()
        );
    }
}
