//! Records of plan runs (a task list run step by step, Milestone 9). The interface owns the record's shape; the core
//! keeps it, checks it is JSON of a sane size, and lists a project's runs.

use crate::store::{Db, now};
use rusqlite::params;
use serde::Serialize;

pub const MAX_RECORD_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunRecord {
    pub id: String,
    pub file: String,
    pub status: String,
    /// The run as the interface saved it (JSON text).
    pub data: String,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn save(
    db: &Db,
    project_id: &str,
    id: &str,
    file: &str,
    status: &str,
    data: &str,
) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 || file.is_empty() || status.is_empty() || status.len() > 20 {
        return Err("Invalid run record.".into());
    }
    if data.len() > MAX_RECORD_BYTES {
        return Err("The run record is too large.".into());
    }
    serde_json::from_str::<serde_json::Value>(data)
        .map_err(|e| format!("The run record is not JSON: {e}"))?;
    db.conn()
        .execute(
            "INSERT INTO plan_runs (id, project_id, file, status, data, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET status = excluded.status, data = excluded.data, updated_at = excluded.updated_at",
            params![id, project_id, file, status, data, now()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list(db: &Db, project_id: &str) -> Result<Vec<RunRecord>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT id, file, status, data, created_at, updated_at FROM plan_runs
             WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT 50",
        )
        .map_err(|e| e.to_string())?;

    stmt.query_map([project_id], |r| {
        Ok(RunRecord {
            id: r.get(0)?,
            file: r.get(1)?,
            status: r.get(2)?,
            data: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// The latest runs of every project (for learning which model does which work well).
pub fn list_all(db: &Db, limit: usize) -> Result<Vec<RunRecord>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT id, file, status, data, created_at, updated_at FROM plan_runs
             ORDER BY updated_at DESC LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    stmt.query_map([limit.min(2000) as i64], |r| {
        Ok(RunRecord {
            id: r.get(0)?,
            file: r.get(1)?,
            status: r.get(2)?,
            data: r.get(3)?,
            created_at: r.get(4)?,
            updated_at: r.get(5)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

/// A run that was going when the app closed cannot go on by itself.
pub fn mark_interrupted(db: &Db) {
    mark_interrupted_because(db, None);
}

/// Why the last master stopped, as its runs say it ("Run stopped · Stopped by the update…", with Resume). M5a: an
/// update, the tray's Quit and a restart say so; M5b: a crash (its watcher says), a shutdown (the computer started
/// again after the master's last state note), or an end some other way.
pub fn stop_note(why: &str) -> Option<&'static str> {
    match why {
        "update" => {
            Some("Stopped by the update to a new version of Qanuvi. Resume goes on from here.")
        }
        "quit" => Some("Stopped when Qanuvi was quit. Resume goes on from here."),
        // s10: the Quit question's "Go on next time"; the next start resumes it by itself (resume_cut_off).
        "quit-later" => Some(
            "Stopped when Qanuvi was quit, to go on when it starts again. Resume goes on from here.",
        ),
        "restart" => Some("Stopped when Qanuvi restarted. Resume goes on from here."),
        "crash" => Some("Stopped when Qanuvi's background part crashed. Resume goes on from here."),
        "shutdown" => Some("Stopped when the computer shut down. Resume goes on from here."),
        "ended" => Some(
            "Stopped when Qanuvi's background part ended unexpectedly. Resume goes on from here.",
        ),
        _ => None,
    }
}

/// Marks the runs that were going as stopped, with why in their message (and `stoppedBy`) when it is known.
/// Answers the projects whose run was going.
pub fn mark_interrupted_because(db: &Db, why: Option<&str>) -> Vec<String> {
    let projects: Vec<String> = db
        .conn()
        .prepare("SELECT DISTINCT project_id FROM plan_runs WHERE status = 'running'")
        .and_then(|mut q| {
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_default();
    if let Some((why, note)) = why.and_then(|w| stop_note(w).map(|n| (w, n))) {
        let _ = db.conn().execute(
            "UPDATE plan_runs SET data = json_set(data, '$.message', ?1, '$.stoppedBy', ?2) \
             WHERE status = 'running' AND json_valid(data)",
            [note, why],
        );
    }
    let _ = db.conn().execute(
        "UPDATE plan_runs SET status = 'stopped' WHERE status = 'running'",
        [],
    );
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_updates_lists_and_cleans_up_runs() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        save(
            &db,
            &project.id,
            "r1",
            "TASKLIST.md",
            "running",
            "{\"steps\":[]}",
        )
        .unwrap();
        save(
            &db,
            &project.id,
            "r1",
            "TASKLIST.md",
            "done",
            "{\"steps\":[1]}",
        )
        .unwrap();
        let runs = list(&db, &project.id).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "done");
        assert_eq!(runs[0].data, "{\"steps\":[1]}");

        assert!(save(&db, &project.id, "r2", "T.md", "running", "not json").is_err());
        assert!(save(&db, &project.id, "", "T.md", "running", "{}").is_err());
        let big = format!("\"{}\"", "x".repeat(MAX_RECORD_BYTES));
        assert!(save(&db, &project.id, "r3", "T.md", "running", &big).is_err());

        assert_eq!(list_all(&db, 10).unwrap().len(), 1);
        save(&db, &project.id, "r4", "T.md", "running", "{}").unwrap();
        assert_eq!(list_all(&db, 1).unwrap().len(), 1);
        mark_interrupted(&db);
        assert!(
            list(&db, &project.id)
                .unwrap()
                .iter()
                .all(|r| r.status != "running")
        );

        crate::projects::remove(&db, &project.id).unwrap();
        assert!(list(&db, &project.id).unwrap().is_empty());
    }

    /// M5a: a run stopped by an update says so afterwards; one already finished keeps its own message.
    #[test]
    fn a_run_stopped_by_an_update_says_so() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        save(
            &db,
            &project.id,
            "r1",
            "T.md",
            "running",
            r#"{"status":"running","message":"Step 2"}"#,
        )
        .unwrap();
        save(
            &db,
            &project.id,
            "r2",
            "U.md",
            "done",
            r#"{"status":"done","message":"All done"}"#,
        )
        .unwrap();
        // M5b: the projects whose run was going come back (an auto-resume after a shutdown needs them).
        let cut = mark_interrupted_because(&db, Some("update"));
        assert_eq!(cut, vec![project.id.clone()]);
        let runs = list(&db, &project.id).unwrap();
        let r1 = runs.iter().find(|r| r.id == "r1").unwrap();
        assert_eq!(r1.status, "stopped");
        let data: serde_json::Value = serde_json::from_str(&r1.data).unwrap();
        assert_eq!(data["message"], stop_note("update").unwrap());
        assert_eq!(data["stoppedBy"], "update");
        let r2 = runs.iter().find(|r| r.id == "r2").unwrap();
        assert!(r2.data.contains("All done"));
        // Unknown or no reason: stopped, its message as it was.
        save(
            &db,
            &project.id,
            "r3",
            "V.md",
            "running",
            r#"{"message":"Step 1"}"#,
        )
        .unwrap();
        mark_interrupted_because(&db, Some("nonsense"));
        let r3 = list(&db, &project.id)
            .unwrap()
            .into_iter()
            .find(|r| r.id == "r3")
            .unwrap();
        assert_eq!(r3.status, "stopped");
        assert!(r3.data.contains("Step 1"));
    }
}
