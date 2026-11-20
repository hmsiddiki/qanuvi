//! The user's memory (Milestone 17): facts, decisions, solutions, approved examples, style and rules as separate
//! entries, for every project or for one. Agents get a small relevant slice with their work instead of everything,
//! and can look up more or suggest new entries (which count only once the user approves them).
//!
//! Entries keep an id, change times and a deleted mark rather than being erased, so copies made on two machines can be
//! merged later (Milestone 18).
//!
//! Every entry belongs to one of three parts (24z3 D8): About me and Tools and models are the user's account memory
//! and travel to their other machines; This project entries belong to one project and never leave the machine that has
//! it. Only the user makes an entry active: whatever an agent or Qanuvi adds arrives as a suggestion.

use crate::store::{Db, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const KINDS: [&str; 6] = ["rule", "style", "fact", "decision", "solution", "example"];
/// The parts of the memory: "me" (About me), "project" (This project), "tools" (Tools and models).
pub const PARTS: [&str; 3] = ["me", "project", "tools"];
const MAX_TITLE: usize = 200;
const MAX_BODY: usize = 4000;
/// How much of an entry's text goes into a slice.
const SLICE_BODY: usize = 400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    /// "me", "project" or "tools"; "" (from before the parts, or a file of an older Qanuvi) is read from the scope.
    #[serde(default)]
    pub part: String,
    /// The project id of a This project entry, else "".
    #[serde(default)]
    pub scope: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: String,
    /// "active", or "suggested" (by an agent, waiting for the user).
    #[serde(default = "active")]
    pub status: String,
    /// "user" or "agent".
    #[serde(default = "user")]
    pub source: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub uses: i64,
}

fn active() -> String {
    "active".into()
}

fn user() -> String {
    "user".into()
}

impl Entry {
    /// The entry's part; one without a part (from before the parts) is About me when it was for every project, else
    /// This project.
    pub fn part(&self) -> &str {
        match self.part.as_str() {
            p @ ("me" | "project" | "tools") => p,
            "" if self.scope.is_empty() => "me",
            "" => "project",
            other => other,
        }
    }

    /// About me and Tools and models: the account memory, which travels to the user's other machines.
    pub fn is_account(&self) -> bool {
        matches!(self.part(), "me" | "tools")
    }
}

/// Reads an entry from columns in the order of COLS.
pub(crate) fn from_row(r: &Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: r.get(0)?,
        kind: r.get(1)?,
        part: r.get(2)?,
        scope: r.get(3)?,
        title: r.get(4)?,
        body: r.get(5)?,
        tags: r.get(6)?,
        status: r.get(7)?,
        source: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        uses: r.get(11)?,
    })
}

pub(crate) const COLS: &str =
    "id, kind, part, scope, title, body, tags, status, source, created_at, updated_at, uses";

/// Entries that apply to a project (its own and the ones for every project), or all of them with `None`; suggested
/// ones only with `suggested`.
pub fn list(db: &Db, project: Option<&str>, suggested: bool) -> Result<Vec<Entry>, String> {
    let conn = db.conn();
    let sql = format!(
        "SELECT {COLS} FROM memory WHERE deleted = 0 AND (?1 IS NULL OR scope = '' OR scope = ?1)
         AND (status = 'active' OR ?2) ORDER BY kind, updated_at DESC"
    );
    let mut st = conn.prepare(&sql).map_err(|e| e.to_string())?;
    st.query_map(params![project, suggested], from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn get(db: &Db, id: &str) -> Result<Entry, String> {
    db.conn()
        .query_row(
            &format!("SELECT {COLS} FROM memory WHERE id = ?1 AND deleted = 0"),
            [id],
            from_row,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That memory entry no longer exists.".into())
}

/// The entry as it is kept: a known part, and a project only for This project entries.
fn normalized(e: &Entry) -> Result<Entry, String> {
    let mut out = e.clone();
    out.part = e.part().to_string();
    if !PARTS.contains(&out.part.as_str()) {
        return Err(format!("Unknown part of the memory: {}.", e.part));
    }
    if out.is_account() {
        out.scope = String::new();
    } else if out.scope.trim().is_empty() {
        return Err("Choose the project this entry is for.".into());
    }
    Ok(out)
}

fn check(e: &Entry) -> Result<(), String> {
    if !KINDS.contains(&e.kind.as_str()) {
        return Err(format!("Unknown kind of entry: {}.", e.kind));
    }
    if e.title.trim().is_empty() {
        return Err("Give the entry a title.".into());
    }
    if e.title.chars().count() > MAX_TITLE || e.body.chars().count() > MAX_BODY {
        return Err(format!(
            "An entry has at most {MAX_TITLE} characters of title and {MAX_BODY} of text; split it into smaller ones."
        ));
    }
    if e.scope.len() > 80 {
        return Err("Invalid scope.".into());
    }
    Ok(())
}

fn insert(db: &Db, e: &Entry, status: &str, source: &str) -> Result<Entry, String> {
    let id = uuid::Uuid::new_v4().to_string();
    db.conn()
        .execute(
            "INSERT INTO memory (id, kind, part, scope, title, body, tags, status, source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                id,
                e.kind,
                e.part,
                e.scope,
                e.title.trim(),
                e.body.trim(),
                e.tags.trim(),
                status,
                source,
                now()
            ],
        )
        .map_err(|e| e.to_string())?;
    get(db, &id)
}

/// Adds an entry (without an id) or changes one; answers it as stored.
///
/// A new entry from an agent is always a suggestion: only the user makes an entry active (24z3 D8). An account entry
/// moved to This project is kept under a new id and the old one gets a deleted mark, so the user's other machines drop
/// their copy at the next sync (the new one never leaves this machine).
pub fn save(db: &Db, e: &Entry) -> Result<Entry, String> {
    check(e)?;
    let e = &normalized(e)?;
    let source = if e.source == "agent" { "agent" } else { "user" };
    let status = if e.status == "suggested" || (e.id.is_empty() && source == "agent") {
        "suggested"
    } else {
        "active"
    };
    if e.id.is_empty() {
        return insert(db, e, status, source);
    }
    let before = get(db, &e.id)?;
    if before.is_account() && !e.is_account() {
        let moved = insert(db, e, status, &before.source)?;
        delete(db, &e.id)?;
        return Ok(moved);
    }
    let n = db
        .conn()
        .execute(
            "UPDATE memory SET kind = ?2, part = ?3, scope = ?4, title = ?5, body = ?6, tags = ?7, status = ?8,
             updated_at = ?9 WHERE id = ?1 AND deleted = 0",
            params![
                e.id,
                e.kind,
                e.part,
                e.scope,
                e.title.trim(),
                e.body.trim(),
                e.tags.trim(),
                status,
                now()
            ],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("That memory entry no longer exists.".into());
    }
    get(db, &e.id)
}

/// Marks an entry deleted (kept as a mark so other copies learn of it when they are merged).
pub fn delete(db: &Db, id: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE memory SET deleted = 1, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// An agent's suggestion: stored as "suggested" until the user approves it. `scope` is used only for This project.
pub fn suggest(
    db: &Db,
    kind: &str,
    part: &str,
    scope: &str,
    title: &str,
    body: &str,
) -> Result<Entry, String> {
    save(
        db,
        &Entry {
            id: String::new(),
            kind: kind.into(),
            part: part.into(),
            scope: if part == "project" {
                scope.into()
            } else {
                String::new()
            },
            title: title.into(),
            body: body.into(),
            tags: String::new(),
            status: "suggested".into(),
            source: "agent".into(),
            created_at: 0,
            updated_at: 0,
            uses: 0,
        },
    )
}

const STOP: [&str; 40] = [
    "the", "and", "for", "with", "that", "this", "from", "into", "are", "was", "were", "you",
    "your", "not", "but", "all", "any", "can", "has", "have", "had", "its", "our", "out", "use",
    "using", "when", "then", "than", "them", "they", "will", "would", "should", "could", "about",
    "also", "each", "make", "add",
];

/// Words of a text worth matching on: lower case, at least 3 characters, common words left out.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Entries ranked by how well they match a request: rarer words count more, a match in the title counts double.
pub fn rank<'a>(entries: &'a [Entry], query: &str) -> Vec<(&'a Entry, f64)> {
    let q: HashSet<String> = words(query).into_iter().collect();
    if q.is_empty() {
        return Vec::new();
    }
    let docs: Vec<(HashSet<String>, HashSet<String>)> = entries
        .iter()
        .map(|e| {
            let title: HashSet<String> = words(&format!("{} {}", e.title, e.tags))
                .into_iter()
                .collect();
            let body: HashSet<String> = words(&e.body).into_iter().collect();
            (title, body)
        })
        .collect();
    let mut df: HashMap<&str, usize> = HashMap::new();
    for (t, b) in &docs {
        for w in &q {
            if t.contains(w) || b.contains(w) {
                *df.entry(w.as_str()).or_default() += 1;
            }
        }
    }
    let n = entries.len() as f64;
    let mut out: Vec<(&Entry, f64)> = entries
        .iter()
        .zip(&docs)
        .map(|(e, (t, b))| {
            let score = q
                .iter()
                .map(|w| {
                    let idf = (1.0 + n / *df.get(w.as_str()).unwrap_or(&1) as f64).ln();
                    if t.contains(w) {
                        2.0 * idf
                    } else if b.contains(w) {
                        idf
                    } else {
                        0.0
                    }
                })
                .sum::<f64>();
            (e, score)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

fn line(e: &Entry) -> String {
    let body = e.body.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = if body.chars().count() > SLICE_BODY {
        format!("{}…", body.chars().take(SLICE_BODY).collect::<String>())
    } else {
        body
    };
    if body.is_empty() {
        format!("- [{}] {}", e.kind, e.title)
    } else {
        format!("- [{}] {}: {}", e.kind, e.title, body)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Slice {
    /// The text to give the agent ("" when nothing applies).
    pub text: String,
    pub ids: Vec<String>,
    /// How many entries matched but did not fit.
    pub left_out: usize,
}

/// The part of the memory an agent gets with a request: the rules and style that apply (the project's own first), then
/// the entries that match the request best, within `budget` characters.
/// With `count`, the chosen entries are counted as given to an agent (not for a preview).
pub fn slice(
    db: &Db,
    project: Option<&str>,
    query: &str,
    budget: usize,
    count: bool,
) -> Result<Slice, String> {
    let all = list(db, project, false)?;
    let mut chosen: Vec<&Entry> = Vec::new();
    let mut always: Vec<&Entry> = all
        .iter()
        .filter(|e| e.kind == "rule" || e.kind == "style")
        .collect();
    always.sort_by_key(|e| (e.scope.is_empty(), e.kind != "rule"));
    let others: Vec<Entry> = all
        .iter()
        .filter(|e| e.kind != "rule" && e.kind != "style")
        .cloned()
        .collect();
    let ranked = rank(&others, query);
    let mut used = 0;
    let mut left_out = 0;
    let header = "Notes from the user's memory in Qanuvi. Follow the rules and style; use the rest where it helps:";
    for e in always.into_iter().chain(ranked.iter().map(|(e, _)| *e)) {
        let l = line(e).chars().count() + 1;
        if used + l + header.len() > budget {
            left_out += 1;
            continue;
        }
        used += l;
        chosen.push(e);
    }
    if chosen.is_empty() {
        return Ok(Slice {
            text: String::new(),
            ids: vec![],
            left_out,
        });
    }
    let mut text = vec![header.to_string()];
    text.extend(chosen.iter().map(|e| line(e)));
    if left_out > 0 {
        text.push(format!(
            "({left_out} more entries match; the memory_search tool finds them when Qanuvi's tools are on.)"
        ));
    }
    let ids: Vec<String> = chosen.iter().map(|e| e.id.clone()).collect();
    if count {
        let conn = db.conn();
        let t = now();
        for id in &ids {
            let _ = conn.execute(
                "UPDATE memory SET uses = uses + 1, last_used = ?2 WHERE id = ?1",
                params![id, t],
            );
        }
    }
    Ok(Slice {
        text: text.join("\n"),
        ids,
        left_out,
    })
}

/// Entries matching a search, best first (for the agent's memory_search tool and the Memory view).
pub fn search(
    db: &Db,
    project: Option<&str>,
    query: &str,
    limit: usize,
) -> Result<Vec<Entry>, String> {
    let all = list(db, project, false)?;
    Ok(rank(&all, query)
        .into_iter()
        .take(limit)
        .map(|(e, _)| e.clone())
        .collect())
}

/// The project's own entry with this title, active or suggested (a fact is kept once).
fn same_title(db: &Db, project: &str, title: &str) -> Option<Entry> {
    db.conn()
        .query_row(
            &format!(
                "SELECT {COLS} FROM memory WHERE deleted = 0 AND part = 'project' AND scope = ?1
                 AND lower(trim(title)) = lower(trim(?2)) ORDER BY status = 'active' DESC LIMIT 1"
            ),
            params![project, title],
            from_row,
        )
        .optional()
        .ok()
        .flatten()
}

fn project_fact(project: &str, title: &str, body: &str, status: &str, source: &str) -> Entry {
    Entry {
        id: String::new(),
        kind: "fact".into(),
        part: "project".into(),
        scope: project.into(),
        title: js_cut(title.trim(), MAX_TITLE),
        body: js_cut(body.trim(), MAX_BODY),
        tags: String::new(),
        status: status.into(),
        source: source.into(),
        created_at: 0,
        updated_at: 0,
        uses: 0,
    }
}

fn js_cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// PM a (owner, 2026-11-21): what the owner answered about the project is theirs, so it goes into This project at
/// once, active; an entry with the same title takes the new answer. Never a password or key.
pub fn keep_owner_fact(db: &Db, project: &str, title: &str, body: &str) -> Result<Entry, String> {
    if crate::plan::guidance::looks_secret(body) || crate::plan::guidance::looks_secret(title) {
        return Err("A password or key is not kept in the memory.".into());
    }
    if let Some(mut e) = same_title(db, project, title) {
        e.body = js_cut(body.trim(), MAX_BODY);
        e.status = "active".into();
        return save(db, &e);
    }
    save(db, &project_fact(project, title, body, "active", "user"))
}

/// PM b + e: a fact a run learned about the project, offered as a suggestion; None when the memory has it already
/// (the same title, or the same text under another title).
pub fn suggest_fact(
    db: &Db,
    project: &str,
    title: &str,
    body: &str,
) -> Result<Option<Entry>, String> {
    if title.trim().is_empty() || crate::plan::guidance::looks_secret(&format!("{title} {body}")) {
        return Ok(None);
    }
    if same_title(db, project, title).is_some() {
        return Ok(None);
    }
    let flat = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let text = flat(body);
    if !text.is_empty()
        && list(db, Some(project), true)?
            .iter()
            .any(|e| flat(&e.body) == text)
    {
        return Ok(None);
    }
    save(
        db,
        &project_fact(project, title, body, "suggested", "agent"),
    )
    .map(Some)
}

/// PM b: the owner kept a run's suggestions with one click (the Inbox's "Keep all"); answers how many became active.
pub fn approve(db: &Db, ids: &[String]) -> usize {
    let conn = db.conn();
    let t = now();
    ids.iter()
        .map(|id| {
            conn.execute(
                "UPDATE memory SET status = 'active', updated_at = ?2
                 WHERE id = ?1 AND deleted = 0 AND status = 'suggested'",
                params![id, t],
            )
            .unwrap_or(0)
        })
        .sum()
}

/// PM c: what the memory knows that applies to the project, as lines for the check before a run and the calls that
/// write the owner's questions: This project's entries first, then the account's, within `budget` characters.
pub fn project_lines(db: &Db, project: &str, budget: usize) -> Vec<String> {
    let mut all = list(db, Some(project), false).unwrap_or_default();
    all.sort_by_key(|e| e.is_account());
    let mut used = 0;
    let mut out = vec![];
    for e in &all {
        let l = line(e);
        let n = l.chars().count() + 1;
        if used + n > budget {
            continue;
        }
        used += n;
        out.push(l.trim_start_matches("- ").to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, scope: &str, title: &str, body: &str) -> Entry {
        Entry {
            id: String::new(),
            kind: kind.into(),
            part: String::new(),
            scope: scope.into(),
            title: title.into(),
            body: body.into(),
            tags: String::new(),
            status: "active".into(),
            source: "user".into(),
            created_at: 0,
            updated_at: 0,
            uses: 0,
        }
    }

    #[test]
    fn saves_changes_lists_by_project_and_deletes_with_a_mark() {
        let db = Db::open_in_memory().unwrap();
        let a = save(&db, &entry("rule", "", "Answer in English", "")).unwrap();
        let b = save(
            &db,
            &entry(
                "fact",
                "p1",
                "Shop uses bKash",
                "Payments go through bKash.",
            ),
        )
        .unwrap();
        save(&db, &entry("fact", "p2", "Other project", "x")).unwrap();
        assert_eq!(list(&db, Some("p1"), false).unwrap().len(), 2);
        assert_eq!(list(&db, None, false).unwrap().len(), 3);
        let mut changed = b.clone();
        changed.body = "Payments go through bKash and cards.".into();
        assert_eq!(
            save(&db, &changed).unwrap().body,
            "Payments go through bKash and cards."
        );
        delete(&db, &a.id).unwrap();
        assert!(get(&db, &a.id).is_err());
        let deleted: i64 = db
            .conn()
            .query_row("SELECT deleted FROM memory WHERE id = ?1", [&a.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(deleted, 1, "kept as a mark for merging");
        assert!(save(&db, &entry("wish", "", "x", "")).is_err());
        assert!(save(&db, &entry("fact", "", " ", "")).is_err());
    }

    #[test]
    fn suggestions_wait_for_the_user() {
        let db = Db::open_in_memory().unwrap();
        let s = suggest(
            &db,
            "decision",
            "project",
            "p1",
            "Use integer cents",
            "Money is stored as cents.",
        )
        .unwrap();
        assert_eq!(s.status, "suggested");
        assert!(list(&db, Some("p1"), false).unwrap().is_empty());
        assert_eq!(list(&db, Some("p1"), true).unwrap().len(), 1);
        let mut ok = s.clone();
        ok.status = "active".into();
        save(&db, &ok).unwrap();
        assert_eq!(list(&db, Some("p1"), false).unwrap().len(), 1);
    }

    #[test]
    fn owner_answers_are_kept_at_once_and_run_facts_wait_once_each() {
        let db = Db::open_in_memory().unwrap();
        let a = keep_owner_fact(&db, "p1", "Test site", "https://example.test/shop/").unwrap();
        assert_eq!((a.status.as_str(), a.part.as_str()), ("active", "project"));
        let b = keep_owner_fact(&db, "p1", "test site ", "https://example.test/new/").unwrap();
        assert_eq!(b.id, a.id, "the same title takes the new answer");
        assert_eq!(b.body, "https://example.test/new/");
        assert!(keep_owner_fact(&db, "p1", "Admin", "password Xy7k9qLm2").is_err());
        assert!(
            suggest_fact(&db, "p1", "Test site", "other")
                .unwrap()
                .is_none()
        );
        let s = suggest_fact(&db, "p1", "How tests run", "php tests/run.php")
            .unwrap()
            .unwrap();
        assert_eq!(s.status, "suggested");
        assert!(
            suggest_fact(&db, "p1", "Tests", " php  tests/run.php")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            project_lines(&db, "p1", 1000).len(),
            1,
            "suggestions are not used yet"
        );
        assert_eq!(approve(&db, &[s.id.clone(), "gone".into()]), 1);
        assert_eq!(approve(&db, &[s.id]), 0, "already kept");
        let lines = project_lines(&db, "p1", 1000);
        assert_eq!(lines.len(), 2);
        assert!(
            lines
                .iter()
                .any(|l| l == "[fact] Test site: https://example.test/new/")
        );
        assert!(project_lines(&db, "p2", 1000).is_empty());
    }

    #[test]
    fn a_slice_has_the_rules_then_what_matches_within_the_budget() {
        let db = Db::open_in_memory().unwrap();
        save(
            &db,
            &entry(
                "rule",
                "",
                "Never name an assistant as author",
                "Not in code, docs or commits.",
            ),
        )
        .unwrap();
        save(
            &db,
            &entry("style", "p1", "Plain English", "Short sentences."),
        )
        .unwrap();
        save(
            &db,
            &entry(
                "decision",
                "p1",
                "Money in cents",
                "Prices and totals are integer cents, never floats.",
            ),
        )
        .unwrap();
        save(
            &db,
            &entry(
                "fact",
                "p1",
                "Preview address",
                "The site runs at http://localhost/shop/.",
            ),
        )
        .unwrap();
        save(
            &db,
            &entry("solution", "p2", "Cents elsewhere", "Another project."),
        )
        .unwrap();

        let s = slice(
            &db,
            Some("p1"),
            "Fix the cart total: prices show floats",
            2000,
            true,
        )
        .unwrap();
        let lines: Vec<&str> = s.text.lines().collect();
        assert!(lines[0].starts_with("Notes from the user's memory"));
        assert!(
            lines[1].starts_with("- [style] Plain English"),
            "the project's own first: {lines:?}"
        );
        assert!(lines[2].starts_with("- [rule] Never name"));
        assert!(lines[3].starts_with("- [decision] Money in cents"));
        assert_eq!(
            lines.len(),
            4,
            "the address fact does not match; other projects are left out"
        );

        // Nothing applies: no text at all (no tokens spent).
        let empty = Db::open_in_memory().unwrap();
        assert_eq!(
            slice(&empty, Some("p1"), "anything", 2000, true)
                .unwrap()
                .text,
            ""
        );

        // A small budget keeps what fits and says more exist.
        let small = slice(&db, Some("p1"), "prices floats", 200, false).unwrap();
        assert!(small.left_out > 0);
        assert!(small.text.chars().count() < 400);
        let uses: i64 = db
            .conn()
            .query_row("SELECT MAX(uses) FROM memory", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            uses, 1,
            "entries given to an agent are counted; a preview is not"
        );
    }

    #[test]
    fn every_entry_has_a_part_and_only_this_project_keeps_a_project() {
        let db = Db::open_in_memory().unwrap();
        // Without a part: for every project = About me, for one project = This project.
        assert_eq!(
            save(&db, &entry("fact", "", "I like tea", ""))
                .unwrap()
                .part,
            "me"
        );
        assert_eq!(
            save(&db, &entry("fact", "p1", "Shop", "")).unwrap().part,
            "project"
        );
        // Account parts never keep a project.
        let mut tools = entry("fact", "p1", "Ollama on the GPU", "");
        tools.part = "tools".into();
        let t = save(&db, &tools).unwrap();
        assert_eq!((t.part.as_str(), t.scope.as_str()), ("tools", ""));
        // This project needs its project; an unknown part is refused.
        let mut lost = entry("fact", "", "Which project?", "");
        lost.part = "project".into();
        assert!(save(&db, &lost).is_err());
        let mut odd = entry("fact", "", "x", "");
        odd.part = "brain".into();
        assert!(save(&db, &odd).is_err());
        // Listing a project gives its own and the account entries; another project's are left out.
        save(&db, &entry("fact", "p2", "Other", "")).unwrap();
        assert_eq!(list(&db, Some("p1"), false).unwrap().len(), 3);
    }

    #[test]
    fn a_new_entry_from_an_agent_is_always_a_suggestion() {
        let db = Db::open_in_memory().unwrap();
        let mut e = entry("rule", "", "Always use tabs", "");
        e.source = "agent".into();
        let saved = save(&db, &e).unwrap();
        assert_eq!(
            saved.status, "suggested",
            "an agent cannot make an active entry"
        );
        // The user keeps it: then it is active.
        let mut kept = saved.clone();
        kept.status = "active".into();
        assert_eq!(save(&db, &kept).unwrap().status, "active");
        // Suggest without a project for This project is refused; with one it is kept.
        assert!(suggest(&db, "fact", "project", "", "x", "").is_err());
        let s = suggest(&db, "fact", "tools", "p1", "Uses Fable", "").unwrap();
        assert_eq!(
            (s.part.as_str(), s.scope.as_str(), s.status.as_str()),
            ("tools", "", "suggested")
        );
    }

    #[test]
    fn an_account_entry_moved_to_a_project_leaves_a_deleted_mark() {
        let db = Db::open_in_memory().unwrap();
        let a = save(&db, &entry("fact", "", "Shop uses bKash", "")).unwrap();
        let mut moved = a.clone();
        moved.part = "project".into();
        moved.scope = "p1".into();
        let m = save(&db, &moved).unwrap();
        assert_ne!(m.id, a.id, "kept under a new id");
        assert_eq!((m.part.as_str(), m.scope.as_str()), ("project", "p1"));
        assert!(
            get(&db, &a.id).is_err(),
            "the account copy is marked deleted"
        );
        // A project entry moved to About me keeps its id (it starts to travel).
        let mut back = m.clone();
        back.part = "me".into();
        let b = save(&db, &back).unwrap();
        assert_eq!((b.id.as_str(), b.scope.as_str()), (m.id.as_str(), ""));
    }

    #[test]
    fn ranking_prefers_rare_words_and_titles() {
        let es = vec![
            entry("fact", "", "Cart page", "The cart shows items."),
            entry(
                "fact",
                "",
                "Coupon codes",
                "Coupons are checked in the cart.",
            ),
            entry("fact", "", "Footer", "Nothing about it."),
        ];
        let r = rank(&es, "coupon in cart");
        assert_eq!(r[0].0.title, "Coupon codes");
        assert_eq!(r.len(), 2);
        assert!(rank(&es, "the and for").is_empty());
    }
}
