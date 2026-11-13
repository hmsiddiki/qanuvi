//! The Inbox (24u part 4): one list of what Qanuvi has to tell the owner, kept in the database so every window, the
//! runner and later the phone see the same items. An item is for your information, needs an answer, or tells about a
//! problem. Answer items carry their form; the first answer anywhere closes the item everywhere.

use crate::store::Db;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Items kept besides open questions (an open question is never dropped to make room).
pub const MAX_ITEMS: i64 = 100;
const MAX_FIELD: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    /// Milliseconds since 1970.
    pub at: i64,
    /// "info", "answer" or "problem".
    #[serde(rename = "type")]
    pub kind_of: String,
    /// What it is about: "run", "sign-in", "knowledge", "chats".
    pub kind: String,
    pub title: String,
    pub text: String,
    pub read: bool,
    /// Items with the same key replace each other (the newest stays).
    pub key: Option<String>,
    /// Where the item's button leads (JSON written by the interface).
    pub open: Option<serde_json::Value>,
    /// The answer form (JSON written by the interface), for "answer" items.
    pub form: Option<serde_json::Value>,
    /// What the item belongs to (project, plan file, run, step), JSON written by the interface.
    pub source: Option<serde_json::Value>,
    /// "open", "answered" or "closed".
    pub status: String,
    pub answer: Option<serde_json::Value>,
    /// Which window or device answered (a window label, later "phone").
    pub answered_by: Option<String>,
    pub answered_at: Option<i64>,
}

/// A new item as the interface sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewItem {
    #[serde(rename = "type", default = "info")]
    pub kind_of: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    pub key: Option<String>,
    pub open: Option<serde_json::Value>,
    pub form: Option<serde_json::Value>,
    pub source: Option<serde_json::Value>,
}

fn info() -> String {
    "info".into()
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn json_text(v: &Option<serde_json::Value>) -> Option<String> {
    v.as_ref().filter(|v| !v.is_null()).map(|v| v.to_string())
}

fn parse(s: Option<String>) -> Option<serde_json::Value> {
    s.and_then(|s| serde_json::from_str(&s).ok())
}

const COLUMNS: &str = "id, at, type, kind, title, text, read, key, open, form, source, status, answer, answered_by, answered_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<InboxItem> {
    Ok(InboxItem {
        id: r.get(0)?,
        at: r.get(1)?,
        kind_of: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        text: r.get(5)?,
        read: r.get::<_, i64>(6)? != 0,
        key: r.get(7)?,
        open: parse(r.get(8)?),
        form: parse(r.get(9)?),
        source: parse(r.get(10)?),
        status: r.get(11)?,
        answer: parse(r.get(12)?),
        answered_by: r.get(13)?,
        answered_at: r.get(14)?,
    })
}

fn check(item: &NewItem) -> Result<(), String> {
    if !matches!(item.kind_of.as_str(), "info" | "answer" | "problem") {
        return Err("Unknown Inbox item type.".into());
    }
    if item.kind.is_empty() || item.kind.len() > 40 || item.title.is_empty() {
        return Err("Invalid Inbox item.".into());
    }
    let size = item.title.len()
        + item.text.len()
        + [&item.open, &item.form, &item.source]
            .iter()
            .map(|v| json_text(v).map_or(0, |s| s.len()))
            .sum::<usize>();
    if size > MAX_FIELD {
        return Err("The Inbox item is too large.".into());
    }
    if item.kind_of == "answer" && item.form.is_none() {
        return Err("A question needs its answer form.".into());
    }
    Ok(())
}

pub fn get(db: &Db, id: &str) -> Result<Option<InboxItem>, String> {
    db.conn()
        .query_row(
            &format!("SELECT {COLUMNS} FROM inbox WHERE id = ?1"),
            [id],
            row,
        )
        .optional()
        .map_err(|e| e.to_string())
}

/// Newest first; open questions first of all.
pub fn list(db: &Db) -> Result<Vec<InboxItem>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM inbox
             ORDER BY (type = 'answer' AND status = 'open') DESC, at DESC LIMIT 300"
        ))
        .map_err(|e| e.to_string())?;
    stmt.query_map([], row)
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}

/// Adds an item. One with the same key is replaced; it stays read when nothing in it changed, and an answered one
/// keeps its answer when the same question comes again (the first answer stands).
pub fn add(db: &Db, item: &NewItem) -> Result<InboxItem, String> {
    add_new(db, item).map(|(i, _)| i)
}

/// Adds an item and says whether it is new (false: the same question was asked again, nothing to tell).
pub fn add_new(db: &Db, item: &NewItem) -> Result<(InboxItem, bool), String> {
    check(item)?;
    let conn = db.conn();
    let same: Option<InboxItem> = match &item.key {
        Some(k) => conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM inbox WHERE key = ?1 ORDER BY at DESC LIMIT 1"),
                [k],
                row,
            )
            .optional()
            .map_err(|e| e.to_string())?,
        None => None,
    };
    let unchanged = same
        .as_ref()
        .is_some_and(|s| s.title == item.title && s.text == item.text && s.kind_of == item.kind_of);
    if let Some(s) = &same
        && unchanged
        && s.kind_of == "answer"
        && s.status != "closed"
    {
        // Asked again while open or answered: nothing new to tell.
        return Ok((s.clone(), false));
    }
    let read = same.as_ref().is_some_and(|s| s.read) && unchanged;
    if let Some(k) = &item.key {
        conn.execute("DELETE FROM inbox WHERE key = ?1", [k])
            .map_err(|e| e.to_string())?;
    }
    let new = InboxItem {
        id: uuid::Uuid::new_v4().to_string(),
        at: now_ms(),
        kind_of: item.kind_of.clone(),
        kind: item.kind.clone(),
        title: item.title.clone(),
        text: item.text.clone(),
        read,
        key: item.key.clone(),
        open: item.open.clone(),
        form: item.form.clone(),
        source: item.source.clone(),
        status: "open".into(),
        answer: None,
        answered_by: None,
        answered_at: None,
    };
    insert(&conn, &new)?;
    prune(&conn)?;
    Ok((new, true))
}

fn insert(conn: &rusqlite::Connection, i: &InboxItem) -> Result<usize, String> {
    conn.execute(
        &format!("INSERT OR IGNORE INTO inbox ({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"),
        params![
            i.id,
            i.at,
            i.kind_of,
            i.kind,
            i.title,
            i.text,
            i.read as i64,
            i.key,
            json_text(&i.open),
            json_text(&i.form),
            json_text(&i.source),
            i.status,
            json_text(&i.answer),
            i.answered_by,
            i.answered_at,
        ],
    )
    .map_err(|e| e.to_string())
}

/// Keeps the newest MAX_ITEMS items besides open questions.
fn prune(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute(
        "DELETE FROM inbox WHERE NOT (type = 'answer' AND status = 'open') AND id NOT IN (
             SELECT id FROM inbox WHERE NOT (type = 'answer' AND status = 'open') ORDER BY at DESC LIMIT ?1)",
        [MAX_ITEMS],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Answers an open question. Only the first answer counts: returns the answered item, or None when it was already
/// answered or closed (by another window or device) or is gone.
pub fn answer(
    db: &Db,
    id: &str,
    answer: &serde_json::Value,
    by: &str,
) -> Result<Option<InboxItem>, String> {
    let text = answer.to_string();
    if text.len() > MAX_FIELD {
        return Err("The answer is too long.".into());
    }
    let changed = db
        .conn()
        .execute(
            "UPDATE inbox SET status = 'answered', answer = ?2, answered_by = ?3, answered_at = ?4, read = 1
             WHERE id = ?1 AND type = 'answer' AND status = 'open'",
            params![id, text, by, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Ok(None);
    }
    get(db, id)
}

/// Answers the open question with this key (the run panel answered it), so it closes everywhere.
pub fn answer_key(
    db: &Db,
    key: &str,
    answer: &serde_json::Value,
    by: &str,
) -> Result<Option<InboxItem>, String> {
    let id: Option<String> = db
        .conn()
        .query_row(
            "SELECT id FROM inbox WHERE key = ?1 AND type = 'answer' AND status = 'open' LIMIT 1",
            [key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    match id {
        Some(id) => self::answer(db, &id, answer, by),
        None => Ok(None),
    }
}

/// The question is no longer asked (its step got done another way): it closes without an answer.
pub fn close(db: &Db, id: &str) -> Result<bool, String> {
    let n = db
        .conn()
        .execute(
            "UPDATE inbox SET status = 'closed', read = 1 WHERE id = ?1 AND status = 'open'",
            [id],
        )
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// Removes the items with this key (the problem they told about is gone).
pub fn resolve(db: &Db, key: &str) -> Result<bool, String> {
    let n = db
        .conn()
        .execute("DELETE FROM inbox WHERE key = ?1", [key])
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

pub fn mark_read(db: &Db, id: Option<&str>) -> Result<bool, String> {
    let conn = db.conn();
    let n = match id {
        Some(id) => conn.execute("UPDATE inbox SET read = 1 WHERE id = ?1 AND read = 0", [id]),
        // "All seen" leaves open questions unread: they still need the owner.
        None => conn.execute(
            "UPDATE inbox SET read = 1 WHERE read = 0 AND NOT (type = 'answer' AND status = 'open')",
            [],
        ),
    }
    .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// Removes an item; an open question can only be answered or closed, not removed.
pub fn remove(db: &Db, id: &str) -> Result<bool, String> {
    let n = db
        .conn()
        .execute(
            "DELETE FROM inbox WHERE id = ?1 AND NOT (type = 'answer' AND status = 'open')",
            [id],
        )
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// Clears everything except open questions.
pub fn clear(db: &Db) -> Result<bool, String> {
    let n = db
        .conn()
        .execute(
            "DELETE FROM inbox WHERE NOT (type = 'answer' AND status = 'open')",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// An item of the old per-window Inbox (kept in the window's local storage before 24u part 4).
#[derive(Debug, Clone, Deserialize)]
pub struct OldItem {
    pub id: String,
    pub at: i64,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub read: bool,
    pub open: Option<serde_json::Value>,
    pub key: Option<String>,
}

/// Brings the old per-window items in once (items already there stay); returns how many came in.
pub fn import(db: &Db, old: &[OldItem]) -> Result<usize, String> {
    let conn = db.conn();
    let mut n = 0;
    for o in old.iter().take(MAX_ITEMS as usize) {
        if o.id.is_empty() || o.kind.is_empty() || o.title.is_empty() {
            continue;
        }
        if let Some(k) = &o.key {
            let there: i64 = conn
                .query_row("SELECT COUNT(*) FROM inbox WHERE key = ?1", [k], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if there > 0 {
                continue;
            }
        }
        let item = InboxItem {
            id: o.id.clone(),
            at: o.at,
            kind_of: "info".into(),
            kind: o.kind.clone(),
            title: o.title.clone(),
            text: o.text.clone(),
            read: o.read,
            key: o.key.clone(),
            open: o.open.clone(),
            form: None,
            source: None,
            status: "open".into(),
            answer: None,
            answered_by: None,
            answered_at: None,
        };
        n += insert(&conn, &item)?;
    }
    prune(&conn)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn question(key: &str) -> NewItem {
        NewItem {
            kind_of: "answer".into(),
            kind: "run".into(),
            title: format!("Step {key} needs you"),
            text: "it pushes to the server".into(),
            key: Some(format!("hold:p:PLAN.md:{key}")),
            open: None,
            form: Some(json!({"choices": [{"id": "go", "label": "Go ahead"}], "text": true})),
            source: Some(json!({"projectId": "p", "file": "PLAN.md", "step": key})),
        }
    }

    fn note(title: &str) -> NewItem {
        NewItem {
            kind_of: "info".into(),
            kind: "run".into(),
            title: title.into(),
            text: String::new(),
            key: None,
            open: Some(json!({"label": "Open the plan", "view": "plan"})),
            form: None,
            source: None,
        }
    }

    #[test]
    fn the_first_answer_closes_the_question_everywhere() {
        let db = Db::open_in_memory().unwrap();
        let q = add(&db, &question("1c")).unwrap();
        let a = answer(&db, &q.id, &json!({"choice": "go", "text": "yes"}), "main")
            .unwrap()
            .unwrap();
        assert_eq!(a.status, "answered");
        assert_eq!(a.answered_by.as_deref(), Some("main"));
        assert!(
            answer(&db, &q.id, &json!({"choice": "skip"}), "window-2")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            get(&db, &q.id).unwrap().unwrap().answer,
            Some(json!({"choice": "go", "text": "yes"}))
        );
    }

    #[test]
    fn the_same_question_again_keeps_the_item_and_its_answer() {
        let db = Db::open_in_memory().unwrap();
        let q = add(&db, &question("1c")).unwrap();
        assert_eq!(add(&db, &question("1c")).unwrap().id, q.id);
        answer(&db, &q.id, &json!({"choice": "go"}), "main").unwrap();
        let again = add(&db, &question("1c")).unwrap();
        assert_eq!(again.status, "answered");
        assert_eq!(list(&db).unwrap().len(), 1);
        // Closed questions asked again open anew.
        let db = Db::open_in_memory().unwrap();
        let q = add(&db, &question("1d")).unwrap();
        assert!(close(&db, &q.id).unwrap());
        let anew = add(&db, &question("1d")).unwrap();
        assert_ne!(anew.id, q.id);
        assert_eq!(anew.status, "open");
    }

    #[test]
    fn answer_by_key_and_open_questions_are_not_removed_or_pruned() {
        let db = Db::open_in_memory().unwrap();
        let q = add(&db, &question("2a")).unwrap();
        assert!(!remove(&db, &q.id).unwrap());
        assert!(!clear(&db).unwrap());
        mark_read(&db, None).unwrap();
        assert!(!get(&db, &q.id).unwrap().unwrap().read);
        for i in 0..(MAX_ITEMS + 5) {
            add(&db, &note(&format!("note {i}"))).unwrap();
        }
        let all = list(&db).unwrap();
        assert_eq!(all.len() as i64, MAX_ITEMS + 1);
        assert_eq!(all[0].id, q.id, "open questions come first");
        assert!(
            answer_key(&db, "hold:p:PLAN.md:2a", &json!({"choice": "skip"}), "main")
                .unwrap()
                .is_some()
        );
        assert!(
            answer_key(&db, "hold:p:PLAN.md:2a", &json!({"choice": "go"}), "main")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn keyed_notes_replace_each_other_and_stay_read_when_unchanged() {
        let db = Db::open_in_memory().unwrap();
        let mut n = note("Sign in again");
        n.key = Some("sign-in:x".into());
        let first = add(&db, &n).unwrap();
        mark_read(&db, Some(&first.id)).unwrap();
        let second = add(&db, &n).unwrap();
        assert!(second.read);
        n.text = "changed".into();
        assert!(!add(&db, &n).unwrap().read);
        assert_eq!(list(&db).unwrap().len(), 1);
        assert!(resolve(&db, "sign-in:x").unwrap());
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn checks_items_and_imports_old_ones_once() {
        let db = Db::open_in_memory().unwrap();
        let mut bad = question("x");
        bad.form = None;
        assert!(add(&db, &bad).is_err());
        let mut bad = note("x");
        bad.kind_of = "shout".into();
        assert!(add(&db, &bad).is_err());
        let old = vec![OldItem {
            id: "a1".into(),
            at: 5,
            kind: "run".into(),
            title: "PLAN done".into(),
            text: String::new(),
            read: true,
            open: None,
            key: None,
        }];
        assert_eq!(import(&db, &old).unwrap(), 1);
        import(&db, &old).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
    }
}
