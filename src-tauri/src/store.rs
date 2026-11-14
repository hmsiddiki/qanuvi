//! Local data in SQLite: projects, agent sessions and their events.

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// The database; clones share one connection.
#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>);

impl Db {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Self(Arc::new(Mutex::new(conn))))
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the data intact; keep using it.
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE projects (
                 id TEXT PRIMARY KEY,
                 name TEXT NOT NULL,
                 path TEXT NOT NULL UNIQUE,
                 kind TEXT NOT NULL,
                 tags TEXT NOT NULL,
                 settings TEXT NOT NULL,
                 added_at INTEGER NOT NULL,
                 last_opened_at INTEGER NOT NULL
             );
             CREATE TABLE sessions (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 title TEXT NOT NULL,
                 agent TEXT NOT NULL,
                 model TEXT NOT NULL,
                 mode TEXT NOT NULL,
                 status TEXT NOT NULL,
                 agent_session_id TEXT,
                 input_tokens INTEGER NOT NULL DEFAULT 0,
                 output_tokens INTEGER NOT NULL DEFAULT 0,
                 cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                 cost_usd REAL NOT NULL DEFAULT 0,
                 duration_ms INTEGER NOT NULL DEFAULT 0,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE INDEX sessions_by_project ON sessions(project_id, updated_at);
             CREATE TABLE session_events (
                 session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                 seq INTEGER NOT NULL,
                 line TEXT NOT NULL,
                 PRIMARY KEY (session_id, seq)
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    if version < 2 {
        // Plan runs (Milestone 9): the run's record is kept as JSON written by the interface.
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE plan_runs (
                 id TEXT PRIMARY KEY,
                 project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                 file TEXT NOT NULL,
                 status TEXT NOT NULL,
                 data TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL
             );
             CREATE INDEX plan_runs_by_project ON plan_runs(project_id, updated_at);
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    if version < 3 {
        // The user's memory (Milestone 17): separate entries, kept with a deleted mark for merging copies later.
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE memory (
                 id TEXT PRIMARY KEY,
                 kind TEXT NOT NULL,
                 scope TEXT NOT NULL DEFAULT '',
                 title TEXT NOT NULL,
                 body TEXT NOT NULL DEFAULT '',
                 tags TEXT NOT NULL DEFAULT '',
                 status TEXT NOT NULL DEFAULT 'active',
                 source TEXT NOT NULL DEFAULT 'user',
                 created_at INTEGER NOT NULL,
                 updated_at INTEGER NOT NULL,
                 deleted INTEGER NOT NULL DEFAULT 0,
                 uses INTEGER NOT NULL DEFAULT 0,
                 last_used INTEGER
             );
             CREATE INDEX memory_by_scope ON memory(scope, deleted);
             PRAGMA user_version = 3;
             COMMIT;",
        )?;
    }
    if version < 4 {
        // The chat's timeline (24n): when each line arrived (seconds; 0 for lines from before).
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE session_events ADD COLUMN at INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 4;
             COMMIT;",
        )?;
    }
    if version < 5 {
        // Chat handover (24s): a session that goes on from an earlier one names it; a chat can turn handover off.
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE sessions ADD COLUMN continues_from TEXT;
             ALTER TABLE sessions ADD COLUMN handover_off INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 5;
             COMMIT;",
        )?;
    }
    if version < 6 {
        // Bookmarks (24s part 2): a starred chat sits in the Saved group and is not deleted while starred.
        conn.execute_batch(
            "BEGIN;
             ALTER TABLE sessions ADD COLUMN saved INTEGER NOT NULL DEFAULT 0;
             PRAGMA user_version = 6;
             COMMIT;",
        )?;
    }
    if version < 7 {
        // Plan logins were stored in plain inside step requests: mask the copies already kept.
        let rows: Vec<(String, i64, String)> = conn
            .prepare("SELECT session_id, seq, line FROM session_events WHERE line LIKE ?1")?
            .query_map([format!("%{}%", crate::agent::PRIVATE_HEAD)], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        conn.execute_batch("BEGIN;")?;
        for (session, seq, line) in rows {
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let Some(text) = v["text"].as_str() else {
                continue;
            };
            let masked = crate::agent::mask_private(text);
            if masked != text {
                v["text"] = serde_json::Value::String(masked);
                conn.execute(
                    "UPDATE session_events SET line = ?1 WHERE session_id = ?2 AND seq = ?3",
                    rusqlite::params![v.to_string(), session, seq],
                )?;
            }
        }
        conn.execute_batch("PRAGMA user_version = 7; COMMIT;")?;
    }
    if version < 8 {
        // The Inbox (24u part 4): one list for every window, the runner and later the phone; questions carry their form.
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE inbox (
                 id TEXT PRIMARY KEY,
                 at INTEGER NOT NULL,
                 type TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 title TEXT NOT NULL,
                 text TEXT NOT NULL DEFAULT '',
                 read INTEGER NOT NULL DEFAULT 0,
                 key TEXT,
                 open TEXT,
                 form TEXT,
                 source TEXT,
                 status TEXT NOT NULL DEFAULT 'open',
                 answer TEXT,
                 answered_by TEXT,
                 answered_at INTEGER
             );
             CREATE INDEX inbox_by_key ON inbox(key);
             CREATE INDEX inbox_by_at ON inbox(at);
             PRAGMA user_version = 8;
             COMMIT;",
        )?;
    }
    if version < 9 {
        // What a plan keeps between runs (24z R2a), once in the window's localStorage: the decider's model picks, the
        // plan state (holds, answers, closings, decisions, extra checks), the plan's notes and its run settings. In the
        // core, so a run going on without the window reads and writes the same. No reference to projects: a master's
        // projects are not in this computer's list, and their window copy is kept here as localStorage kept it.
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE plan_data (
                 project_id TEXT NOT NULL,
                 file TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 value TEXT NOT NULL,
                 updated_at INTEGER NOT NULL,
                 PRIMARY KEY (project_id, file, kind)
             );
             PRAGMA user_version = 9;
             COMMIT;",
        )?;
    }
    Ok(())
}

/// Seconds since 1970, used for all timestamps.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_a_new_database_and_is_safe_to_run_again() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 9);
        migrate(&conn).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('projects','sessions','session_events')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 3);
    }
}
