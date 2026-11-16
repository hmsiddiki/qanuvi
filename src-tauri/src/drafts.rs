//! Unsaved editor text kept on its machine (24z3 Part B2). When the window's switcher hides a machine, its open files
//! leave the window; any file with unsaved changes is kept here, on the machine the file belongs to, and given back
//! when the window shows that machine again. Nothing asks the owner: the text simply comes back.

use crate::store::{Db, now};
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// The longest text kept for one file (a bigger unsaved file is not something an editor tab holds).
const MAX_TEXT: usize = 20 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// The project the file belongs to; empty for an agent's own configuration file.
    pub project_id: String,
    /// The file's path in the project (an agent configuration file: its tab key).
    pub path: String,
    pub text: String,
    /// The file's fingerprint when the unsaved changes began, so a file changed since shows a conflict on save.
    pub hash: String,
    /// What reopens an agent configuration file (JSON written by the window); empty for a project file.
    #[serde(default)]
    pub meta: String,
    #[serde(default)]
    pub updated_at: i64,
}

/// Keeps a draft, replacing the one kept before for the same file.
pub fn put(db: &Db, d: &Draft) -> Result<(), String> {
    if d.path.is_empty() {
        return Err("A draft needs a file.".into());
    }
    if d.text.len() > MAX_TEXT {
        return Err("This unsaved text is too large to keep.".into());
    }
    db.conn()
        .execute(
            "INSERT INTO drafts (project_id, path, text, hash, meta, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (project_id, path) DO UPDATE SET text = ?3, hash = ?4, meta = ?5, updated_at = ?6",
            params![d.project_id, d.path, d.text, d.hash, d.meta, now()],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The drafts of one project (empty: the agent configuration files'), oldest first.
pub fn list(db: &Db, project_id: &str) -> Result<Vec<Draft>, String> {
    let conn = db.conn();
    let mut q = conn
        .prepare(
            "SELECT project_id, path, text, hash, meta, updated_at FROM drafts WHERE project_id = ?1
             ORDER BY updated_at, path",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map(params![project_id], |r| {
            Ok(Draft {
                project_id: r.get(0)?,
                path: r.get(1)?,
                text: r.get(2)?,
                hash: r.get(3)?,
                meta: r.get(4)?,
                updated_at: r.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

/// Drops a draft once it is back in an editor (or the file was saved or closed).
pub fn drop(db: &Db, project_id: &str, path: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "DELETE FROM drafts WHERE project_id = ?1 AND path = ?2",
            params![project_id, path],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Drops every draft of a project (the project was removed from the list).
pub fn drop_project(db: &Db, project_id: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "DELETE FROM drafts WHERE project_id = ?1",
            params![project_id],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(project: &str, path: &str, text: &str) -> Draft {
        Draft {
            project_id: project.into(),
            path: path.into(),
            text: text.into(),
            hash: "h1".into(),
            meta: String::new(),
            updated_at: 0,
        }
    }

    #[test]
    fn keeps_replaces_lists_and_drops_drafts_per_project() {
        let db = Db::open_in_memory().unwrap();
        put(&db, &draft("p1", "a.js", "one")).unwrap();
        put(&db, &draft("p1", "b.js", "two")).unwrap();
        put(&db, &draft("p2", "a.js", "other")).unwrap();
        put(&db, &draft("p1", "a.js", "one, changed")).unwrap();

        let p1 = list(&db, "p1").unwrap();
        assert_eq!(p1.len(), 2);
        assert_eq!(
            p1.iter().find(|d| d.path == "a.js").unwrap().text,
            "one, changed"
        );
        assert_eq!(list(&db, "p2").unwrap()[0].text, "other");

        drop(&db, "p1", "a.js").unwrap();
        assert_eq!(list(&db, "p1").unwrap().len(), 1);
        drop_project(&db, "p1").unwrap();
        assert!(list(&db, "p1").unwrap().is_empty());
        assert_eq!(list(&db, "p2").unwrap().len(), 1);
    }

    #[test]
    fn keeps_an_agent_configuration_file_with_its_details() {
        let db = Db::open_in_memory().unwrap();
        let mut d = draft("", "config::claude-code::settings::", "{}");
        d.meta =
            r#"{"provider":"claude-code","id":"settings","projectId":null,"label":"Settings"}"#
                .into();
        put(&db, &d).unwrap();
        let back = list(&db, "").unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].meta, d.meta);
    }

    #[test]
    fn refuses_a_draft_without_a_file() {
        let db = Db::open_in_memory().unwrap();
        assert!(put(&db, &draft("p1", "", "x")).is_err());
    }
}
