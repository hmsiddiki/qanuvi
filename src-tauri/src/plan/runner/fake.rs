//! A world from memory for driving the plan runner in tests (ported from `src/lib/stores/testing/fakeCore.ts`):
//! project files, Qanuvi's snapshots, agent turns run by a script, checks, run records, the Inbox, plan data and
//! project settings. The clock only moves when asked to wait, so limit waits take no time.

use crate::agent::{AgentEvent, Usage};
use crate::checks::CheckResult;
use crate::inbox::{InboxItem, NewItem};
use crate::plan::env::{FileText, Flow, RunEnv, TurnEnd, TurnRequest};
use crate::plan_data::Kind;
use crate::projects::ProjectSettings;
use crate::runs::RunRecord;
use crate::snapshots::{Change, Changes, Taken};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What the scripted agent sees of one turn.
pub struct AgentCall<'a> {
    pub prompt: &'a str,
    pub model: &'a str,
    pub agent: &'a str,
    pub mode: &'a str,
    pub title: Option<&'a str>,
    pub session_id: &'a str,
    /// Turns sent to this session so far (1 = the first).
    pub turn: u32,
    pub env: &'a FakeEnv,
}

/// How the scripted turn ended (defaults: idle, $0.01, no final answer). Each turn adds 1000 input and 100 output
/// tokens and 2 seconds of work to its session.
#[derive(Clone, Debug, Default)]
pub struct AgentReply {
    pub status: Option<&'static str>,
    pub cost: Option<f64>,
    pub result: Option<String>,
    pub is_error: bool,
    /// Agent lines sent before the end (a usage line, say).
    pub lines: Vec<String>,
    /// Quiet seconds before the end (the clock moves one second each).
    pub ticks: u32,
}

type AgentScript = Arc<dyn Fn(&AgentCall) -> AgentReply + Send + Sync>;
type CheckScript = Arc<dyn Fn(&str) -> (bool, String) + Send + Sync>;

#[derive(Clone)]
struct File {
    text: String,
    hash: String,
    at: i64,
}

#[derive(Default)]
struct World {
    files: BTreeMap<String, File>,
    snapshots: HashMap<String, BTreeMap<String, String>>,
    turns: HashMap<String, u32>,
    costs: HashMap<String, f64>,
    stopped: Vec<String>,
    /// Messages sent into running turns: session and text.
    sent: Vec<(String, String)>,
    /// Per session: input tokens, output tokens, working milliseconds.
    tokens: HashMap<String, (i64, i64, i64)>,
    runs: BTreeMap<String, RunRecord>,
    settings: ProjectSettings,
    calls: Vec<String>,
    prompts: Vec<(String, String)>,
    requests: Vec<TurnRequest>,
    n: u64,
    clock: i64,
}

pub struct FakeEnv {
    w: Mutex<World>,
    /// The Inbox and plan data are the real modules on a database in memory (their rules are the real ones).
    db: crate::store::Db,
    agent: Mutex<AgentScript>,
    checks: Mutex<CheckScript>,
    /// A core whose diff sees nothing (what git did in a folder it ignores, PLAN-F 2026-11-14).
    pub blind_diff: Mutex<bool>,
    /// Plan folders no git repository sees (reported with each snapshot).
    pub git_blind: Mutex<Vec<String>>,
    /// What the memory slice answers (None: nothing applies).
    pub memory: Mutex<Option<String>>,
}

impl Default for FakeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeEnv {
    pub fn new() -> Self {
        FakeEnv {
            w: Mutex::new(World {
                clock: 1_800_000_000_000,
                ..Default::default()
            }),
            db: crate::store::Db::open_in_memory().expect("in-memory database"),
            agent: Mutex::new(Arc::new(|_| AgentReply::default())),
            checks: Mutex::new(Arc::new(|_| (true, String::new()))),
            blind_diff: Mutex::new(false),
            git_blind: Mutex::new(Vec::new()),
            memory: Mutex::new(None),
        }
    }

    fn w(&self) -> std::sync::MutexGuard<'_, World> {
        self.w.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The scripted agent: edits files through `call.env`, answers how the turn ended.
    pub fn set_agent(&self, f: impl Fn(&AgentCall) -> AgentReply + Send + Sync + 'static) {
        *self.agent.lock().unwrap() = Arc::new(f);
    }

    /// The test command's answer: passed, and its output.
    pub fn set_checks(&self, f: impl Fn(&str) -> (bool, String) + Send + Sync + 'static) {
        *self.checks.lock().unwrap() = Arc::new(f);
    }

    pub fn set_settings(&self, s: ProjectSettings) {
        self.w().settings = s;
    }

    pub fn put(&self, path: &str, text: &str) {
        let mut w = self.w();
        w.n += 1;
        w.clock += 1;
        let (hash, at) = (format!("h{}", w.n), w.clock);
        w.files.insert(
            path.into(),
            File {
                text: text.into(),
                hash,
                at,
            },
        );
    }

    pub fn text(&self, path: &str) -> String {
        self.w()
            .files
            .get(path)
            .map(|f| f.text.clone())
            .unwrap_or_default()
    }

    /// Ticks (or marks) a step by its number in a plan file, as an agent does.
    pub fn mark(&self, file: &str, number: &str, status: char) {
        let line = Regex::new(&format!(
            r"^ ?- \[[ xX!-]\] \*\*{}\.\s",
            regex::escape(number)
        ))
        .unwrap();
        let text = self.text(file);
        let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
        let i = lines
            .iter()
            .position(|l| line.is_match(l))
            .unwrap_or_else(|| panic!("fake env: step {number} is not in {file}"));
        let at = lines[i].find('[').unwrap() + 1;
        lines[i].replace_range(at..at + 1, &status.to_string());
        self.put(file, &lines.join("\n"));
    }

    /// Every command the runner used, in order.
    pub fn calls(&self) -> Vec<String> {
        self.w().calls.clone()
    }

    /// Prompts sent to agents, in order: (model, prompt).
    pub fn prompts(&self) -> Vec<(String, String)> {
        self.w().prompts.clone()
    }

    /// Every turn request, in order.
    pub fn requests(&self) -> Vec<TurnRequest> {
        self.w().requests.clone()
    }

    pub fn stopped(&self) -> Vec<String> {
        self.w().stopped.clone()
    }

    /// Messages sent into running turns, in order: (session, text).
    pub fn sent(&self) -> Vec<(String, String)> {
        self.w().sent.clone()
    }

    pub fn inbox(&self) -> Vec<InboxItem> {
        crate::inbox::list(&self.db).unwrap_or_default()
    }

    /// The owner answers an open Inbox question (by its key).
    pub fn answer(&self, key: &str, answer: Value) {
        crate::inbox::answer_key(&self.db, key, &answer, "owner").unwrap();
    }
    pub fn runs(&self) -> Vec<RunRecord> {
        self.w().runs.values().cloned().collect()
    }

    pub fn settings(&self) -> ProjectSettings {
        self.w().settings.clone()
    }

    fn own(path: &str) -> bool {
        !path.starts_with(".qanuvi/")
    }

    fn call(&self, name: &str) {
        self.w().calls.push(name.into());
    }

    fn take(&self) -> Taken {
        let mut w = self.w();
        w.n += 1;
        let id = format!("snap{}", w.n);
        let files: BTreeMap<String, String> = w
            .files
            .iter()
            .filter(|(p, _)| Self::own(p))
            .map(|(p, f)| (p.clone(), f.text.clone()))
            .collect();
        let count = files.len();
        w.snapshots.insert(id.clone(), files);
        drop(w);
        Taken {
            id,
            files: count,
            stored: 0,
            truncated: false,
            ms: 0,
            git_blind: self.git_blind.lock().unwrap().clone(),
        }
    }
}

impl RunEnv for FakeEnv {
    fn project_id(&self) -> &str {
        "p1"
    }

    fn now_ms(&self) -> i64 {
        self.w().clock
    }

    fn sleep(&self, d: Duration) {
        self.w().clock += d.as_millis() as i64;
    }

    fn read(&self, path: &str) -> Result<FileText, String> {
        self.call("files_read");
        let w = self.w();
        let f = w
            .files
            .get(path)
            .ok_or_else(|| format!("{path} cannot be found."))?;
        Ok(FileText {
            text: f.text.clone(),
            hash: f.hash.clone(),
        })
    }

    fn write(&self, path: &str, text: &str, expected_hash: Option<&str>) -> Result<String, String> {
        self.call("files_write");
        if let (Some(want), Some(f)) = (expected_hash, self.w().files.get(path))
            && f.hash != want
        {
            return Err(format!("{path} changed on disk since it was read."));
        }
        self.put(path, text);
        Ok(self.w().files[path].hash.clone())
    }

    fn log_append(&self, path: &str, text: &str) -> Result<(), String> {
        self.call("files_log_append");
        let now = self.text(path);
        self.put(path, &format!("{now}{text}"));
        Ok(())
    }

    fn changed_since(&self, since_ms: i64) -> Result<Vec<String>, String> {
        self.call("files_changed_since");
        Ok(self
            .w()
            .files
            .iter()
            .filter(|(p, f)| Self::own(p) && f.at >= since_ms)
            .map(|(p, _)| p.clone())
            .collect())
    }

    fn snapshot_take(&self, _also: &[String]) -> Result<Taken, String> {
        self.call("snapshots_take");
        Ok(self.take())
    }

    fn snapshot_changes(
        &self,
        before: &str,
        _also: &[String],
        git: bool,
        skip: Option<&str>,
    ) -> Result<Changes, String> {
        self.call("snapshots_changes");
        let was = self
            .w()
            .snapshots
            .get(before)
            .cloned()
            .ok_or("The snapshot is gone (older than two weeks, or the data folder moved).")?;
        let after = self.take();
        if *self.blind_diff.lock().unwrap() {
            return Ok(Changes {
                changes: vec![],
                unseen: vec![],
                after: after.id,
            });
        }
        let now = self.w().snapshots[&after.id].clone();
        let mut paths: Vec<&String> = was.keys().chain(now.keys()).collect();
        paths.sort();
        paths.dedup();
        let mut changes = Vec::new();
        for p in paths {
            if skip == Some(p.as_str()) {
                continue;
            }
            let (b, n) = (was.get(p), now.get(p));
            if b == n {
                continue;
            }
            let kind = match (b, n) {
                (None, _) => "added",
                (_, None) => "deleted",
                _ => "changed",
            };
            let mut diff = vec![
                format!("--- {p} (before this step)"),
                format!("+++ {p} (after)"),
            ];
            diff.extend(
                b.into_iter()
                    .flat_map(|t| t.split('\n'))
                    .map(|l| format!("-{l}")),
            );
            diff.extend(
                n.into_iter()
                    .flat_map(|t| t.split('\n'))
                    .map(|l| format!("+{l}")),
            );
            changes.push(Change {
                path: p.clone(),
                kind,
                diff: diff.join("\n"),
            });
        }
        let blind = self.git_blind.lock().unwrap().clone();
        let unseen = if git {
            changes
                .iter()
                .map(|c| c.path.clone())
                .filter(|p| blind.iter().any(|d| p.starts_with(&format!("{d}/"))))
                .collect()
        } else {
            vec![]
        };
        Ok(Changes {
            changes,
            unseen,
            after: after.id,
        })
    }

    fn agent_turn(
        &self,
        req: &TurnRequest,
        on_event: &mut dyn FnMut(Option<&AgentEvent>) -> Flow,
    ) -> Result<TurnEnd, String> {
        self.call("agent_start");
        let (id, turn) = {
            let mut w = self.w();
            w.n += 1;
            let id = req
                .session_id
                .clone()
                .unwrap_or_else(|| format!("s{}", w.n));
            let turn = w.turns.get(&id).copied().unwrap_or(0) + 1;
            w.turns.insert(id.clone(), turn);
            w.prompts.push((req.model.clone(), req.prompt.clone()));
            w.requests.push(req.clone());
            (id, turn)
        };
        let script = self.agent.lock().unwrap().clone();
        let reply = script(&AgentCall {
            prompt: &req.prompt,
            model: &req.model,
            agent: &req.agent,
            mode: &req.mode,
            title: req.title.as_deref(),
            session_id: &id,
            turn,
            env: self,
        });
        let (cost, tokens) = {
            let mut w = self.w();
            let c = w.costs.entry(id.clone()).or_insert(0.0);
            *c += reply.cost.unwrap_or(0.01);
            let c = *c;
            let t = w.tokens.entry(id.clone()).or_insert((0, 0, 0));
            *t = (t.0 + 1000, t.1 + 100, t.2 + 2000);
            (c, *t)
        };
        let mut end = TurnEnd {
            session_id: id.clone(),
            status: "running".into(),
            cost_usd: cost,
            input_tokens: tokens.0,
            output_tokens: tokens.1,
            duration_ms: tokens.2,
            ..Default::default()
        };
        let session = |status: &str| AgentEvent::Session {
            session: Box::new(crate::agent::Session {
                id: id.clone(),
                status: status.into(),
                cost_usd: cost,
                input_tokens: tokens.0,
                output_tokens: tokens.1,
                duration_ms: tokens.2,
                model: req.model.clone(),
                mode: req.mode.clone(),
                ..Default::default()
            }),
        };
        // What the runner answered: a message is kept, a stop or an end finishes the turn here.
        let steer = |flow: Flow, end: &mut TurnEnd| -> bool {
            match flow {
                Flow::Go | Flow::Hold => false,
                Flow::Send(text) => {
                    self.w().sent.push((id.clone(), text));
                    false
                }
                Flow::Stop => {
                    self.w().stopped.push(id.clone());
                    end.status = "stopped".into();
                    end.cut_short = true;
                    true
                }
                Flow::End => {
                    end.cut_short = true;
                    true
                }
            }
        };
        if steer(on_event(Some(&session("running"))), &mut end) {
            return Ok(end);
        }
        for line in &reply.lines {
            if steer(
                on_event(Some(&AgentEvent::Line { line: line.clone() })),
                &mut end,
            ) {
                return Ok(end);
            }
        }
        for _ in 0..reply.ticks {
            self.w().clock += 1000;
            if steer(on_event(None), &mut end) {
                return Ok(end);
            }
        }
        if let Some(text) = &reply.result {
            let line =
                serde_json::json!({"type": "result", "result": text, "is_error": reply.is_error})
                    .to_string();
            if steer(on_event(Some(&AgentEvent::Line { line })), &mut end) {
                return Ok(end);
            }
        }
        end.result = reply.result.clone();
        end.is_error = reply.is_error;
        end.status = reply.status.unwrap_or("idle").into();
        let flow = on_event(Some(&session(&end.status)));
        let hold = flow == Flow::Hold;
        if steer(flow, &mut end) {
            return Ok(end);
        }
        // Held at its end (an interrupt): quiet seconds until the runner ends the turn.
        if hold {
            for _ in 0..1000 {
                self.w().clock += 1000;
                if steer(on_event(None), &mut end) {
                    return Ok(end);
                }
            }
        }
        Ok(end)
    }

    fn agent_stop(&self, session_id: &str) {
        self.call("agent_stop");
        self.w().stopped.push(session_id.into());
    }

    fn run_check(&self, command: &str, _timeout: Duration) -> Result<CheckResult, String> {
        self.call("checks_run");
        let script = self.checks.lock().unwrap().clone();
        let (ok, output) = script(command);
        Ok(CheckResult {
            ok,
            exit_code: Some(if ok { 0 } else { 1 }),
            timed_out: false,
            duration_ms: 1,
            output,
        })
    }

    fn usage_since(&self, _since: i64) -> Result<Usage, String> {
        self.call("agent_usage_since");
        Ok(Usage {
            cost_usd: 0.0,
            sessions: 0,
            input_tokens: 0,
            output_tokens: 0,
        })
    }

    fn save_run(&self, id: &str, file: &str, status: &str, data: &str) -> Result<(), String> {
        self.call("runs_save");
        let mut w = self.w();
        let now = w.clock;
        let created = w.runs.get(id).map(|r| r.created_at).unwrap_or(now);
        w.runs.insert(
            id.into(),
            RunRecord {
                id: id.into(),
                file: file.into(),
                status: status.into(),
                data: data.into(),
                created_at: created,
                updated_at: now,
            },
        );
        Ok(())
    }

    fn list_runs(&self) -> Result<Vec<RunRecord>, String> {
        self.call("runs_list");
        Ok(self.runs())
    }

    fn inbox_add(&self, item: &NewItem) -> Result<InboxItem, String> {
        self.call("inbox_add");
        crate::inbox::add_new(&self.db, item).map(|(i, _)| i)
    }

    fn inbox_by_key(&self, key: &str) -> Result<Option<InboxItem>, String> {
        Ok(self
            .inbox()
            .into_iter()
            .filter(|i| i.key.as_deref() == Some(key))
            .max_by_key(|i| i.at))
    }

    fn inbox_resolve(&self, key: &str) -> Result<bool, String> {
        self.call("inbox_resolve");
        crate::inbox::resolve(&self.db, key)
    }

    fn plan_data_get(&self, file: &str, kind: Kind) -> Option<Value> {
        crate::plan_data::get(&self.db, "p1", file, kind)
            .ok()
            .flatten()
    }

    fn plan_data_set(&self, file: &str, kind: Kind, value: &Value) -> Result<(), String> {
        self.call("plan_data_set");
        crate::plan_data::set(&self.db, "p1", file, kind, value).map(|_| ())
    }
    fn memory_slice(&self, _request: &str, _budget: usize) -> Option<String> {
        self.call("memory_slice");
        self.memory.lock().unwrap().clone()
    }

    fn project_settings(&self) -> Result<ProjectSettings, String> {
        Ok(self.settings())
    }

    fn set_project_settings(&self, settings: &ProjectSettings) -> Result<(), String> {
        self.call("projects_update");
        self.w().settings = settings.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::env::{MemoryUse, PlannerAsk, ask_planner, memory_context};
    use serde_json::json;

    const ON: MemoryUse = MemoryUse {
        on: true,
        budget: 4000,
    };

    #[test]
    fn marks_steps_and_snapshots_show_what_changed_without_the_plan_file() {
        let env = FakeEnv::new();
        env.put(
            "PLAN.md",
            "# Plan\n\n- [ ] **97a. Storage**\n- [ ] **97b. Phone**\n",
        );
        let before = env.snapshot_take(&[]).unwrap();
        env.put("src/A.php", "<?php // a\n");
        env.mark("PLAN.md", "97a", 'x');
        assert!(env.text("PLAN.md").contains("- [x] **97a. Storage**"));
        assert!(env.text("PLAN.md").contains("- [ ] **97b. Phone**"));
        let c = env
            .snapshot_changes(&before.id, &[], false, Some("PLAN.md"))
            .unwrap();
        assert_eq!(
            c.changes
                .iter()
                .map(|c| (c.path.as_str(), c.kind))
                .collect::<Vec<_>>(),
            vec![("src/A.php", "added")]
        );
        assert!(c.changes[0].diff.contains("+<?php // a"));
        *env.blind_diff.lock().unwrap() = true;
        assert!(
            env.snapshot_changes(&c.after, &[], false, None)
                .unwrap()
                .changes
                .is_empty()
        );
    }

    #[test]
    fn git_blind_folders_come_back_as_unseen_only_with_git() {
        let env = FakeEnv::new();
        *env.git_blind.lock().unwrap() = vec!["content/plugins/shop".into()];
        let before = env.snapshot_take(&[]).unwrap();
        assert_eq!(before.git_blind, vec!["content/plugins/shop".to_string()]);
        env.put("content/plugins/shop/src/S.php", "x");
        assert_eq!(
            env.snapshot_changes(&before.id, &[], true, None)
                .unwrap()
                .unseen,
            vec!["content/plugins/shop/src/S.php".to_string()]
        );
        assert!(
            env.snapshot_changes(&before.id, &[], false, None)
                .unwrap()
                .unseen
                .is_empty()
        );
    }

    #[test]
    fn a_hash_mismatch_refuses_the_write() {
        let env = FakeEnv::new();
        env.put("a.txt", "1");
        let h = env.read("a.txt").unwrap().hash;
        env.put("a.txt", "2");
        assert!(
            env.write("a.txt", "3", Some(&h))
                .unwrap_err()
                .contains("changed on disk")
        );
    }

    #[test]
    fn turns_count_per_session_and_costs_add_up() {
        let env = FakeEnv::new();
        env.set_agent(|c| AgentReply {
            cost: Some(0.5),
            result: Some(format!(
                "turn {} {} {} {}",
                c.turn, c.session_id, c.model, c.agent
            )),
            ..Default::default()
        });
        let first = env
            .agent_turn(
                &TurnRequest {
                    prompt: "a".into(),
                    ..Default::default()
                },
                &mut |_| Flow::Go,
            )
            .unwrap();
        let again = TurnRequest {
            session_id: Some(first.session_id.clone()),
            prompt: "b".into(),
            model: "m".into(),
            agent: "claude-code".into(),
            ..Default::default()
        };
        let second = env.agent_turn(&again, &mut |_| Flow::Go).unwrap();
        assert_eq!(
            second.result.as_deref(),
            Some(format!("turn 2 {} m claude-code", first.session_id).as_str())
        );
        assert_eq!(second.cost_usd, 1.0);
        assert_eq!(env.prompts().len(), 2);
    }

    #[test]
    fn the_planner_gets_a_read_only_session_with_memory_and_it_is_stopped_after() {
        let env = FakeEnv::new();
        *env.memory.lock().unwrap() = Some("Rule: PHP 8.2".into());
        env.set_agent(|c| {
            assert_eq!(c.mode, "plan");
            assert_eq!(c.title, Some("Decider: review step 3"));
            AgentReply {
                result: Some(r#"{"ok": true}"#.into()),
                cost: Some(0.002),
                ..Default::default()
            }
        });
        let ask = PlannerAsk {
            prompt: "Review".into(),
            model: "m".into(),
            title: "Decider: review step 3".into(),
            lean: true,
            ..Default::default()
        };
        let a = ask_planner(&env, ON, &ask, &mut |_| {}).unwrap();
        assert_eq!(a.text, r#"{"ok": true}"#);
        assert_eq!(a.cost_usd, 0.002);
        assert_eq!(env.requests()[0].context.as_deref(), Some("Rule: PHP 8.2"));
        assert_eq!(env.stopped(), vec![a.session_id]);
        // Memory off: no slice asked for.
        let off = MemoryUse {
            on: false,
            budget: 4000,
        };
        ask_planner(&env, off, &ask, &mut |_| {}).unwrap();
        assert_eq!(env.requests()[1].context, None);
        assert_eq!(
            env.calls().iter().filter(|c| *c == "memory_slice").count(),
            1
        );
    }

    #[test]
    fn planner_errors_and_unfinished_turns_say_so() {
        let env = FakeEnv::new();
        let ask = PlannerAsk {
            prompt: "x".into(),
            ..Default::default()
        };
        env.set_agent(|_| AgentReply {
            result: Some(String::new()),
            is_error: true,
            ..Default::default()
        });
        assert_eq!(
            ask_planner(&env, ON, &ask, &mut |_| {}).unwrap_err(),
            "The planner stopped with an error."
        );
        env.set_agent(|_| AgentReply {
            result: Some("over the limit".into()),
            is_error: true,
            ..Default::default()
        });
        assert_eq!(
            ask_planner(&env, ON, &ask, &mut |_| {}).unwrap_err(),
            "over the limit"
        );
        env.set_agent(|_| AgentReply {
            status: Some("failed"),
            ..Default::default()
        });
        assert_eq!(
            ask_planner(&env, ON, &ask, &mut |_| {}).unwrap_err(),
            "The planner did not finish."
        );
        // Stopped, but it had answered: the answer counts.
        env.set_agent(|_| AgentReply {
            status: Some("stopped"),
            result: Some("fine".into()),
            ..Default::default()
        });
        assert_eq!(
            ask_planner(&env, ON, &ask, &mut |_| {}).unwrap().text,
            "fine"
        );
    }

    #[test]
    fn an_empty_memory_slice_gives_no_context() {
        let env = FakeEnv::new();
        *env.memory.lock().unwrap() = Some(String::new());
        assert_eq!(memory_context(&env, ON, "x"), None);
    }

    #[test]
    fn inbox_questions_by_key_are_replaced_answered_and_resolved() {
        let env = FakeEnv::new();
        let q = NewItem {
            kind_of: "answer".into(),
            kind: "run".into(),
            title: "Step 1 needs you".into(),
            text: String::new(),
            key: Some("hold:p1:PLAN.md:1".into()),
            open: None,
            form: Some(json!({"choices": [{"id": "go", "label": "Try again"}]})),
            source: None,
        };
        env.inbox_add(&q).unwrap();
        env.inbox_add(&q).unwrap();
        assert_eq!(env.inbox().len(), 1);
        env.answer(
            "hold:p1:PLAN.md:1",
            json!({"choice": "go", "text": "key added"}),
        );
        let got = env.inbox_by_key("hold:p1:PLAN.md:1").unwrap().unwrap();
        assert_eq!(got.status, "answered");
        // Asked again while answered: the answer stays.
        env.inbox_add(&q).unwrap();
        assert_eq!(
            env.inbox_by_key("hold:p1:PLAN.md:1")
                .unwrap()
                .unwrap()
                .status,
            "answered"
        );
        assert!(env.inbox_resolve("hold:p1:PLAN.md:1").unwrap());
        assert!(env.inbox().is_empty());
    }
}
