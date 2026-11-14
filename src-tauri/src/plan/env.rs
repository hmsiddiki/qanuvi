//! What the plan runner needs from the world (24z R2a), as one trait: the project's files, Qanuvi's snapshots, agent
//! turns, checks, run records, the Inbox, what a plan keeps between runs and the user's memory. The core answers it
//! (`CoreEnv`, used on the desktop and later in qanuvi-master); the scenario tests answer it from memory
//! (`runner::fake`), so the run loop is tested end to end without a window, an agent or a disk.
//!
//! Built here on top of it: the memory slice a request gets and the planner call (a read-only turn whose final
//! answer is the result), both as the window did them.

use crate::agent::{AgentEvent, Usage};
use crate::checks::CheckResult;
use crate::inbox::{InboxItem, NewItem};
use crate::plan_data::Kind;
use crate::projects::ProjectSettings;
use crate::runs::RunRecord;
use crate::snapshots::{Changes, Taken};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub struct FileText {
    pub text: String,
    pub hash: String,
}

/// One message to an agent: a new session (no `session_id`) or the next turn of one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnRequest {
    pub session_id: Option<String>,
    pub prompt: String,
    pub model: String,
    /// The permission mode ("auto", "plan" for read-only, ...).
    pub mode: String,
    /// The agent ("claude-code", "qanuvi", ...).
    pub agent: String,
    pub base_url: Option<String>,
    pub effort: Option<String>,
    pub lean: bool,
    pub index_tools: bool,
    pub browser_tools: bool,
    /// Given before the prompt (the memory slice, a work packet).
    pub context: Option<String>,
    /// The Qanuvi agent's setup, with its keys filled in.
    pub setup: Option<String>,
    /// A new session's title in the agent list ("Decider: review step 3").
    pub title: Option<String>,
}

/// How a turn ended.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnEnd {
    pub session_id: String,
    /// The session's status at the end: "idle" (answered), "failed", "stopped", "ended".
    pub status: String,
    /// The session's cost so far (all its turns), US dollars.
    pub cost_usd: f64,
    /// The turn's final answer, when it gave one.
    pub result: Option<String>,
    /// The final answer was an error.
    pub is_error: bool,
}

/// A session status that ends a turn.
pub fn turn_over(status: &str) -> bool {
    matches!(status, "idle" | "failed" | "stopped" | "ended")
}

/// Reads one agent line: the turn's final answer and whether it was an error.
pub fn result_of(line: &str) -> Option<(String, bool)> {
    let msg: Value = serde_json::from_str(line).ok()?;
    if msg.get("type")?.as_str()? != "result" {
        return None;
    }
    let text = msg
        .get("result")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some((
        text,
        msg.get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ))
}

pub trait RunEnv: Send + Sync {
    fn project_id(&self) -> &str;
    /// Milliseconds since 1970.
    fn now_ms(&self) -> i64;
    fn sleep(&self, d: Duration);

    // The project's files (paths from the project's folder).
    fn read(&self, path: &str) -> Result<FileText, String>;
    /// Writes a file; with `expected_hash`, only when nobody changed it since it was read. Answers the new hash.
    fn write(&self, path: &str, text: &str, expected_hash: Option<&str>) -> Result<String, String>;
    fn log_append(&self, path: &str, text: &str) -> Result<(), String>;
    /// Files changed at or after a moment (milliseconds), Qanuvi's own folder left out.
    fn changed_since(&self, since_ms: i64) -> Result<Vec<String>, String>;

    // Qanuvi's snapshots (git optional).
    fn snapshot_take(&self, also: &[String]) -> Result<Taken, String>;
    fn snapshot_changes(
        &self,
        before: &str,
        also: &[String],
        git: bool,
        skip: Option<&str>,
    ) -> Result<Changes, String>;

    /// Sends a message and waits until the turn is over; every event goes to `on_event` on the way.
    fn agent_turn(
        &self,
        req: &TurnRequest,
        on_event: &mut dyn FnMut(&AgentEvent),
    ) -> Result<TurnEnd, String>;
    fn agent_stop(&self, session_id: &str);

    /// Runs a command in the project's folder (the test command, a step's check).
    fn run_check(&self, command: &str, timeout: Duration) -> Result<CheckResult, String>;
    /// What all agent sessions cost since a moment (seconds since 1970), for the monthly budget.
    fn usage_since(&self, since: i64) -> Result<Usage, String>;

    fn save_run(&self, id: &str, file: &str, status: &str, data: &str) -> Result<(), String>;
    fn list_runs(&self) -> Result<Vec<RunRecord>, String>;

    fn inbox_add(&self, item: &NewItem) -> Result<InboxItem, String>;
    /// The newest item with this key (a held step's question and its answer).
    fn inbox_by_key(&self, key: &str) -> Result<Option<InboxItem>, String>;
    /// Questions with this key are settled (the step got done, or it runs again).
    fn inbox_resolve(&self, key: &str) -> Result<bool, String>;

    fn plan_data_get(&self, file: &str, kind: Kind) -> Option<Value>;
    fn plan_data_set(&self, file: &str, kind: Kind, value: &Value) -> Result<(), String>;

    /// The part of the user's memory for a request, at most `budget` characters; None when nothing applies.
    fn memory_slice(&self, request: &str, budget: usize) -> Option<String>;

    /// The project's settings now (the test command, where plan step changes come from).
    fn project_settings(&self) -> Result<ProjectSettings, String>;
    /// Changes them (the owner's answer "Use Qanuvi's snapshots" to the changes-from question).
    fn set_project_settings(&self, settings: &ProjectSettings) -> Result<(), String>;
}

/// Whether requests get the user's memory, and how much (Settings > Memory, sent with the run's start).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryUse {
    pub on: bool,
    pub budget: usize,
}

/// The memory slice for a request, as the window's `memoryContext` gave it.
pub fn memory_context(env: &dyn RunEnv, memory: MemoryUse, request: &str) -> Option<String> {
    if !memory.on {
        return None;
    }
    env.memory_slice(request, memory.budget)
        .filter(|t| !t.is_empty())
}

/// A question to the planner (the decider's reviews, a rewrite into smaller steps, a finishing pass).
#[derive(Clone, Debug, Default)]
pub struct PlannerAsk {
    pub prompt: String,
    pub model: String,
    /// The session's title in the agent list ("Decider: review step 3").
    pub title: String,
    pub agent: String,
    pub base_url: Option<String>,
    pub effort: Option<String>,
    pub lean: bool,
    pub index_tools: bool,
    pub setup: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannerAnswer {
    pub text: String,
    pub cost_usd: f64,
    pub session_id: String,
}

/// Asks the planner and answers its final text. The session is read-only, gets the memory slice for the question,
/// and is stopped afterwards (as `planner.ask` in the window).
pub fn ask_planner(
    env: &dyn RunEnv,
    memory: MemoryUse,
    ask: &PlannerAsk,
    on_event: &mut dyn FnMut(&AgentEvent),
) -> Result<PlannerAnswer, String> {
    let req = TurnRequest {
        session_id: None,
        prompt: ask.prompt.clone(),
        model: ask.model.clone(),
        mode: "plan".into(),
        agent: ask.agent.clone(),
        base_url: ask.base_url.clone(),
        effort: ask.effort.clone(),
        lean: ask.lean,
        index_tools: ask.index_tools,
        browser_tools: false,
        context: memory_context(env, memory, &ask.prompt),
        setup: ask.setup.clone(),
        title: Some(ask.title.clone()).filter(|t| !t.is_empty()),
    };
    let end = env.agent_turn(&req, on_event);
    if let Ok(e) = &end {
        env.agent_stop(&e.session_id);
    }
    let end = end?;
    let text = end.result.clone().unwrap_or_default();
    if end.is_error {
        return Err(if text.is_empty() {
            "The planner stopped with an error.".into()
        } else {
            text
        });
    }
    if end.status != "idle" && text.is_empty() {
        return Err("The planner did not finish.".into());
    }
    Ok(PlannerAnswer {
        text,
        cost_usd: end.cost_usd,
        session_id: end.session_id,
    })
}

// ---------- The core's answer ----------

/// Tells the windows (or a master's stream) about a change: the event name and its payload.
pub type Notify = std::sync::Arc<dyn Fn(&str, Value) + Send + Sync>;

/// The runner's world in the core: the database, the agents and the project's folder.
pub struct CoreEnv<'a> {
    pub db: crate::store::Db,
    pub agents: &'a crate::agent::Agents,
    pub project: crate::projects::Project,
    pub notify: Notify,
}

impl CoreEnv<'_> {
    fn root(&self) -> std::path::PathBuf {
        std::path::PathBuf::from(&self.project.path)
    }
}

impl RunEnv for CoreEnv<'_> {
    fn project_id(&self) -> &str {
        &self.project.id
    }

    fn now_ms(&self) -> i64 {
        crate::inbox::now_ms()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }

    fn read(&self, path: &str) -> Result<FileText, String> {
        let f = crate::files::read_file(&self.root(), path)?;
        Ok(FileText {
            text: f.text,
            hash: f.hash,
        })
    }

    fn write(&self, path: &str, text: &str, expected_hash: Option<&str>) -> Result<String, String> {
        crate::files::write_file(&self.root(), path, text, expected_hash)
    }

    fn log_append(&self, path: &str, text: &str) -> Result<(), String> {
        crate::files::log_append(&self.root(), path, text)
    }

    fn changed_since(&self, since_ms: i64) -> Result<Vec<String>, String> {
        let since = std::time::UNIX_EPOCH + Duration::from_millis(since_ms.max(0) as u64);
        crate::files::changed_since(&self.root(), since, 500)
    }

    fn snapshot_take(&self, also: &[String]) -> Result<Taken, String> {
        crate::snapshots::take(&self.root(), also)
    }

    fn snapshot_changes(
        &self,
        before: &str,
        also: &[String],
        git: bool,
        skip: Option<&str>,
    ) -> Result<Changes, String> {
        let skip = skip.unwrap_or("");
        crate::snapshots::changes(&self.root(), before, also, git, &|p| {
            !skip.is_empty() && p == skip
        })
    }

    fn agent_turn(
        &self,
        req: &TurnRequest,
        on_event: &mut dyn FnMut(&AgentEvent),
    ) -> Result<TurnEnd, String> {
        let (tx, rx) = std::sync::mpsc::channel::<AgentEvent>();
        let sink = crate::sink::Sink::new(move |e| tx.send(e).is_ok());
        let started = self.agents.start(
            &self.db,
            &self.project,
            req.session_id.as_deref(),
            &req.prompt,
            &req.model,
            &req.mode,
            req.base_url.as_deref(),
            req.effort.as_deref(),
            req.lean,
            req.index_tools,
            req.browser_tools,
            req.context.as_deref(),
            &[],
            if req.agent.is_empty() {
                "claude-code"
            } else {
                &req.agent
            },
            req.setup.as_deref(),
            sink,
        )?;
        if let Some(title) = &req.title {
            let _ = crate::agent::rename(&self.db, &started.id, title);
        }
        let mut end = TurnEnd {
            session_id: started.id.clone(),
            status: started.status.clone(),
            cost_usd: started.cost_usd,
            ..Default::default()
        };
        // Events until the turn is over; a closed channel means the session's process is gone.
        while let Ok(event) = rx.recv() {
            on_event(&event);
            match &event {
                AgentEvent::Line { line } => {
                    if let Some((text, is_error)) = result_of(line) {
                        end.result = Some(text);
                        end.is_error = is_error;
                    }
                }
                AgentEvent::Session { session } => {
                    end.status = session.status.clone();
                    end.cost_usd = session.cost_usd;
                    if turn_over(&session.status) {
                        break;
                    }
                }
                AgentEvent::Exit { .. } => {
                    if !turn_over(&end.status) {
                        end.status = "ended".into();
                    }
                    break;
                }
                AgentEvent::Error { .. } => {}
            }
        }
        Ok(end)
    }

    fn agent_stop(&self, session_id: &str) {
        self.agents.stop(session_id);
    }

    fn run_check(&self, command: &str, timeout: Duration) -> Result<CheckResult, String> {
        crate::checks::run(&self.root(), command, timeout)
    }

    fn usage_since(&self, since: i64) -> Result<Usage, String> {
        crate::agent::usage_since(&self.db, since)
    }

    fn save_run(&self, id: &str, file: &str, status: &str, data: &str) -> Result<(), String> {
        crate::runs::save(&self.db, &self.project.id, id, file, status, data)
    }

    fn list_runs(&self) -> Result<Vec<RunRecord>, String> {
        crate::runs::list(&self.db, &self.project.id)
    }

    fn inbox_add(&self, item: &NewItem) -> Result<InboxItem, String> {
        let (added, _) = crate::inbox::add_new(&self.db, item)?;
        (self.notify)(
            "qanuvi-inbox",
            serde_json::json!({"change": "add", "item": added}),
        );
        Ok(added)
    }

    fn inbox_by_key(&self, key: &str) -> Result<Option<InboxItem>, String> {
        Ok(crate::inbox::list(&self.db)?
            .into_iter()
            .filter(|i| i.key.as_deref() == Some(key))
            .max_by_key(|i| i.at))
    }

    fn inbox_resolve(&self, key: &str) -> Result<bool, String> {
        let gone = crate::inbox::resolve(&self.db, key)?;
        if gone {
            (self.notify)(
                "qanuvi-inbox",
                serde_json::json!({"change": "resolve", "item": null}),
            );
        }
        Ok(gone)
    }

    fn plan_data_get(&self, file: &str, kind: Kind) -> Option<Value> {
        crate::plan_data::get(&self.db, &self.project.id, file, kind)
            .ok()
            .flatten()
    }

    fn plan_data_set(&self, file: &str, kind: Kind, value: &Value) -> Result<(), String> {
        let kept = crate::plan_data::set(&self.db, &self.project.id, file, kind, value)?;
        (self.notify)(
            "qanuvi-plan-data",
            serde_json::json!({"project_id": self.project.id, "file": file, "kind": kind, "value": kept.map(|e| e.value)}),
        );
        Ok(())
    }

    fn project_settings(&self) -> Result<ProjectSettings, String> {
        Ok(crate::projects::get(&self.db, &self.project.id)?.settings)
    }

    fn set_project_settings(&self, settings: &ProjectSettings) -> Result<(), String> {
        let now = crate::projects::get(&self.db, &self.project.id)?;
        crate::projects::update(&self.db, &now.id, &now.name, settings).map(|_| ())
    }

    fn memory_slice(&self, request: &str, budget: usize) -> Option<String> {
        crate::memory::slice(&self.db, Some(&self.project.id), request, budget, true)
            .ok()
            .map(|s| s.text)
    }
}
