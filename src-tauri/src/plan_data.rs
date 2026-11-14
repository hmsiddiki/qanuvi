//! What a plan keeps between runs (24z R2a): the decider's model picks, the plan state (holds, answers, closings,
//! decisions, extra checks), the plan's notes and its run settings. Once in the window's localStorage; now in the
//! database, so a run in the core (and later in qanuvi-master) reads and writes the same as the window. One JSON value
//! per project, plan file and kind; the window keeps a copy and follows changes through an event.

use crate::plan::picks::SavedPicks;
use crate::plan::planstate::PlanState;
use crate::store::Db;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The plan's notes kept at most (the newest ones), as the window did.
pub const NOTES_KEPT: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The decider's model picks ("Pick models" in the Plan view).
    Picks,
    /// Holds, answers, closings, decisions and extra checks (24u parts 2 and 3).
    State,
    /// The owner's instructions for every run of the plan.
    Notes,
    /// The Run dialog's settings for this plan.
    Run,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Picks => "picks",
            Kind::State => "state",
            Kind::Notes => "notes",
            Kind::Run => "run",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "picks" => Kind::Picks,
            "state" => Kind::State,
            "notes" => Kind::Notes,
            "run" => Kind::Run,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub project_id: String,
    pub file: String,
    pub kind: Kind,
    pub value: Value,
    #[serde(default)]
    pub updated_at: i64,
}

/// A value that means "nothing kept": the row goes instead.
fn is_empty(kind: Kind, v: &Value) -> bool {
    match (kind, v) {
        (_, Value::Null) => true,
        (Kind::Notes, Value::Array(a)) => a.is_empty(),
        (Kind::State, v) => PlanState::from_json(v).is_empty(),
        _ => false,
    }
}

pub fn list(db: &Db) -> Result<Vec<Entry>, String> {
    let conn = db.conn();
    let mut q = conn
        .prepare("SELECT project_id, file, kind, value, updated_at FROM plan_data ORDER BY project_id, file, kind")
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (project_id, file, kind, value, updated_at) = row.map_err(|e| e.to_string())?;
        // A kind this version does not know, or a value that is not JSON, is left alone.
        let (Some(kind), Ok(value)) = (Kind::parse(&kind), serde_json::from_str(&value)) else {
            continue;
        };
        out.push(Entry {
            project_id,
            file,
            kind,
            value,
            updated_at,
        });
    }
    Ok(out)
}

pub fn get(db: &Db, project: &str, file: &str, kind: Kind) -> Result<Option<Value>, String> {
    let text: Option<String> = db
        .conn()
        .query_row(
            "SELECT value FROM plan_data WHERE project_id = ?1 AND file = ?2 AND kind = ?3",
            params![project, file, kind.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

/// Keeps a value (an empty one removes the row). Answers the entry as kept, or None when it was removed.
pub fn set(
    db: &Db,
    project: &str,
    file: &str,
    kind: Kind,
    value: &Value,
) -> Result<Option<Entry>, String> {
    if is_empty(kind, value) {
        remove(db, project, file, kind)?;
        return Ok(None);
    }
    let value = match (kind, value) {
        (Kind::Notes, Value::Array(a)) if a.len() > NOTES_KEPT => {
            Value::Array(a[a.len() - NOTES_KEPT..].to_vec())
        }
        _ => value.clone(),
    };
    let now = crate::store::now();
    db.conn()
        .execute(
            "INSERT INTO plan_data (project_id, file, kind, value, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (project_id, file, kind) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![project, file, kind.as_str(), value.to_string(), now],
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(Entry {
        project_id: project.into(),
        file: file.into(),
        kind,
        value,
        updated_at: now,
    }))
}

pub fn remove(db: &Db, project: &str, file: &str, kind: Kind) -> Result<bool, String> {
    let n = db
        .conn()
        .execute(
            "DELETE FROM plan_data WHERE project_id = ?1 AND file = ?2 AND kind = ?3",
            params![project, file, kind.as_str()],
        )
        .map_err(|e| e.to_string())?;
    Ok(n > 0)
}

/// Everything kept for a project's plans goes (the project left the list).
pub fn forget_project(db: &Db, project: &str) -> Result<(), String> {
    db.conn()
        .execute("DELETE FROM plan_data WHERE project_id = ?1", [project])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Brings in what a window kept in localStorage, once: what the database has already wins. Projects of a master
/// (not in this computer's list) are kept too, as localStorage kept them. Answers how many came in.
pub fn import(db: &Db, old: &[Entry]) -> Result<usize, String> {
    let mut n = 0;
    for e in old {
        if is_empty(e.kind, &e.value) || get(db, &e.project_id, &e.file, e.kind)?.is_some() {
            continue;
        }
        set(db, &e.project_id, &e.file, e.kind, &e.value)?;
        n += 1;
    }
    Ok(n)
}

// ---------- Typed, for the runner in the core ----------

pub fn load_state(db: &Db, project: &str, file: &str) -> PlanState {
    get(db, project, file, Kind::State)
        .ok()
        .flatten()
        .map(|v| PlanState::from_json(&v))
        .unwrap_or_default()
}

pub fn save_state(db: &Db, project: &str, file: &str, state: &PlanState) -> Result<(), String> {
    let v = serde_json::to_value(state).map_err(|e| e.to_string())?;
    set(db, project, file, Kind::State, &v).map(|_| ())
}

pub fn load_picks(db: &Db, project: &str, file: &str) -> Option<SavedPicks> {
    let v = get(db, project, file, Kind::Picks).ok().flatten()?;
    serde_json::from_value(v).ok()
}

pub fn save_picks(db: &Db, project: &str, file: &str, picks: &SavedPicks) -> Result<(), String> {
    let v = serde_json::to_value(picks).map_err(|e| e.to_string())?;
    set(db, project, file, Kind::Picks, &v).map(|_| ())
}

pub fn load_notes(db: &Db, project: &str, file: &str) -> Vec<String> {
    match get(db, project, file, Kind::Notes).ok().flatten() {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn save_notes(db: &Db, project: &str, file: &str, notes: &[String]) -> Result<(), String> {
    set(db, project, file, Kind::Notes, &serde_json::json!(notes)).map(|_| ())
}

/// The plan's run settings as the Run dialog kept them (an object; empty when none).
pub fn load_run_settings(db: &Db, project: &str, file: &str) -> serde_json::Map<String, Value> {
    match get(db, project, file, Kind::Run).ok().flatten() {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

/// Changes some of the run settings (a model switch while the plan runs), keeping the rest.
pub fn patch_run_settings(
    db: &Db,
    project: &str,
    file: &str,
    patch: &serde_json::Map<String, Value>,
) -> Result<Option<Entry>, String> {
    let mut now = load_run_settings(db, project, file);
    for (k, v) in patch {
        now.insert(k.clone(), v.clone());
    }
    set(db, project, file, Kind::Run, &Value::Object(now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (Db, tempfile::TempDir, String) {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let p = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        (db, dir, p.id)
    }

    #[test]
    fn keeps_replaces_and_removes_values_per_plan_and_kind() {
        let (db, _dir, p) = setup();
        assert!(get(&db, &p, "PLAN.md", Kind::Run).unwrap().is_none());
        set(&db, &p, "PLAN.md", Kind::Run, &json!({"budget": 2})).unwrap();
        set(&db, &p, "PLAN-F.md", Kind::Run, &json!({"budget": 5})).unwrap();
        set(&db, &p, "PLAN.md", Kind::Run, &json!({"budget": 3})).unwrap();
        assert_eq!(
            get(&db, &p, "PLAN.md", Kind::Run).unwrap(),
            Some(json!({"budget": 3}))
        );
        assert_eq!(
            get(&db, &p, "PLAN-F.md", Kind::Run).unwrap(),
            Some(json!({"budget": 5}))
        );
        assert!(get(&db, &p, "PLAN.md", Kind::Notes).unwrap().is_none());
        assert_eq!(list(&db).unwrap().len(), 2);
        assert!(
            set(&db, &p, "PLAN.md", Kind::Run, &Value::Null)
                .unwrap()
                .is_none()
        );
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn an_empty_state_or_note_list_removes_the_row() {
        let (db, _dir, p) = setup();
        save_notes(&db, &p, "PLAN.md", &["use the staging site".into()]).unwrap();
        save_state(
            &db,
            &p,
            "PLAN.md",
            &PlanState {
                checks: vec![crate::plan::preflight::StepWhy {
                    step: "3".into(),
                    why: "risky".into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list(&db).unwrap().len(), 2);
        save_notes(&db, &p, "PLAN.md", &[]).unwrap();
        save_state(&db, &p, "PLAN.md", &PlanState::default()).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }

    #[test]
    fn notes_keep_the_newest_sixty() {
        let (db, _dir, p) = setup();
        let notes: Vec<String> = (0..70).map(|i| format!("note {i}")).collect();
        save_notes(&db, &p, "PLAN.md", &notes).unwrap();
        let kept = load_notes(&db, &p, "PLAN.md");
        assert_eq!(kept.len(), NOTES_KEPT);
        assert_eq!(kept[0], "note 10");
    }

    #[test]
    fn typed_state_and_picks_round_trip() {
        let (db, _dir, p) = setup();
        let state = PlanState::from_json(&json!({
            "holds": [{"step": "97a", "why": "blocked", "by": "default", "at": 1}],
            "checks": [{"step": "97b", "why": "touches payments"}]
        }));
        assert!(!state.is_empty());
        save_state(&db, &p, "PLAN-F.md", &state).unwrap();
        assert_eq!(load_state(&db, &p, "PLAN-F.md"), state);
        let picks: SavedPicks = serde_json::from_value(json!({
            "at": 1, "providerId": "claude-code", "freeFirst": true,
            "picks": {"1": {"title": "Schema", "worker": "free", "model": "qanuvi:qwen", "why": "small"}}
        }))
        .unwrap();
        save_picks(&db, &p, "PLAN.md", &picks).unwrap();
        assert_eq!(load_picks(&db, &p, "PLAN.md"), Some(picks));
        assert!(load_picks(&db, &p, "OTHER.md").is_none());
    }

    #[test]
    fn run_settings_patch_keeps_the_rest() {
        let (db, _dir, p) = setup();
        set(
            &db,
            &p,
            "PLAN.md",
            Kind::Run,
            &json!({"oneModel": "a", "budget": 2}),
        )
        .unwrap();
        let patch = json!({"oneModel": "b"});
        patch_run_settings(&db, &p, "PLAN.md", patch.as_object().unwrap()).unwrap();
        assert_eq!(
            Value::Object(load_run_settings(&db, &p, "PLAN.md")),
            json!({"oneModel": "b", "budget": 2})
        );
    }

    #[test]
    fn import_brings_old_values_once_and_the_database_wins() {
        let (db, _dir, p) = setup();
        set(&db, &p, "PLAN.md", Kind::Run, &json!({"budget": 9})).unwrap();
        let old = vec![
            Entry {
                project_id: p.clone(),
                file: "PLAN.md".into(),
                kind: Kind::Run,
                value: json!({"budget": 1}),
                updated_at: 0,
            },
            Entry {
                project_id: p.clone(),
                file: "PLAN.md".into(),
                kind: Kind::Notes,
                value: json!(["a"]),
                updated_at: 0,
            },
            Entry {
                project_id: p.clone(),
                file: "PLAN.md".into(),
                kind: Kind::State,
                value: json!({}),
                updated_at: 0,
            },
            Entry {
                project_id: "on-a-master".into(),
                file: "PLAN.md".into(),
                kind: Kind::Notes,
                value: json!(["b"]),
                updated_at: 0,
            },
        ];
        assert_eq!(import(&db, &old).unwrap(), 2);
        assert_eq!(
            get(&db, &p, "PLAN.md", Kind::Run).unwrap(),
            Some(json!({"budget": 9}))
        );
        assert_eq!(load_notes(&db, &p, "PLAN.md"), vec!["a".to_string()]);
        assert_eq!(
            load_notes(&db, "on-a-master", "PLAN.md"),
            vec!["b".to_string()]
        );
        assert_eq!(import(&db, &old).unwrap(), 0);
    }

    #[test]
    fn rows_go_with_their_project() {
        let (db, _dir, p) = setup();
        save_notes(&db, &p, "PLAN.md", &["a".into()]).unwrap();
        crate::projects::remove(&db, &p).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }
}
