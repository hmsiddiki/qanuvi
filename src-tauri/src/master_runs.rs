// Plan runs on the master (24z R2d): the same run loop as the desktop, on the master's own projects. A run goes on
// when the desktop disconnects; a connection that wants its news calls `plan_run_watch` (or passes `onEvent` to
// `plan_run_start`) and gets `{"kind": "view", "view": …}` and `{"kind": "agent", "sessionId": …, "event": …}` on
// that stream until it goes away. API keys are the master's own (model_keys.rs), as for agent_start.

use super::{Master, Streams, opt, s, to};
use crate::agent::AgentEvent;
use crate::plan::env::{CoreEnv, Notify};
use crate::plan::finishing::FinishLevel;
use crate::plan::holdask::AnswerChoice;
use crate::plan::live::{RunStart, RunView, Tell, finish_pass, last_run};
use crate::plan::run::run_plan;
use crate::sink::Sink;
use crate::{projects, runs};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The connections that follow each project's run; one that no longer listens is dropped.
#[derive(Default)]
pub struct Watchers(Mutex<HashMap<String, Vec<Sink<Value>>>>);

impl Watchers {
    pub fn add(&self, project_id: &str, sink: Sink<Value>) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(project_id.to_string())
            .or_default()
            .push(sink);
    }

    fn send(&self, project_id: &str, v: Value) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(list) = map.get_mut(project_id) {
            list.retain(|sink| sink.send(v.clone()).is_ok());
        }
    }
}

impl Tell for Watchers {
    fn view(&self, view: &RunView) {
        self.send(&view.project_id, json!({"kind": "view", "view": view}));
    }

    fn agent(&self, project_id: &str, session_id: Option<&str>, event: &AgentEvent) {
        self.send(
            project_id,
            json!({"kind": "agent", "sessionId": session_id, "event": event}),
        );
    }
}

fn start_of(args: &Value) -> Result<RunStart, String> {
    let mut start: RunStart = serde_json::from_value(args["start"].clone())
        .map_err(|e| format!("The run's start is not valid: {e}"))?;
    // The master's own keys (its credential store or QANUVI_KEY_…): the desktop's are never sent to it.
    for setup in start.turns.setups.values_mut() {
        *setup = crate::brain::with_keys(setup, crate::model_keys::get)?;
    }
    Ok(start)
}

/// Where a project's last run start is kept (M5b).
fn start_file(dir: &std::path::Path, project_id: &str) -> Option<std::path::PathBuf> {
    let plain = !project_id.is_empty()
        && project_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| dir.join("run-starts").join(format!("{project_id}.json")))
}

impl Master {
    pub(super) fn keep_start(&self, project_id: &str, start: &Value) {
        if let Some(f) = start_file(&self.dir, project_id) {
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(&self.dir));
            let _ = std::fs::write(f, start.to_string());
        }
    }

    /// M5b: after a shutdown, with "Start Qanuvi when Windows starts" on, This PC's runs that were cut off go on by
    /// themselves (half a minute after start, so its start-up commands answer first), each said in the Inbox. With
    /// the setting off, the window shows the stop with Resume and offers the setting.
    pub fn resume_cut_off(self: &Arc<Self>) {
        let (why, projects) =
            std::mem::take(&mut *self.cut_off.lock().unwrap_or_else(|e| e.into_inner()));
        if why != "shutdown" || projects.is_empty() || !self.app_data {
            return;
        }
        if !crate::sign_in_start::is_on().unwrap_or(false) {
            return;
        }
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let Some(m) = me.upgrade() else { return };
            for pid in projects {
                m.resume_one(&pid);
            }
        });
    }

    /// Starts a cut-off run again with its kept start and says so in the Inbox.
    pub(super) fn resume_one(&self, project_id: &str) {
        let name = projects::get(&self.db, project_id)
            .map(|p| p.name)
            .unwrap_or_else(|_| "a project".into());
        let Some(mut start) = start_file(&self.dir, project_id)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return;
        };
        // A new start on the plan: its done steps stay done; a step that was cut off starts again.
        start["resume"] = Value::Null;
        // Named by the run that was cut off (its kept start may not say).
        let file = runs::list(&self.db, project_id)
            .ok()
            .and_then(|l| l.into_iter().next())
            .map(|r| r.file)
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "its plan".into());
        let no_stream = |_: &str| Sink::new(|_: Value| true);
        let args = json!({"projectId": project_id, "start": start});
        let (title, text, kind_of) = match self.plan_run("plan_run_start", &args, &no_stream) {
            Ok(_) => (
                format!("Plan run resumed on its own: {name}"),
                format!(
                    "The run of {file} was cut off when the computer shut down. Qanuvi started with the computer and \
                     the run goes on from where it stopped."
                ),
                "info",
            ),
            Err(e) => (
                format!("Plan run not resumed: {name}"),
                format!(
                    "The run of {file} was cut off when the computer shut down and could not go on by itself: {}. \
                     Open the plan and choose Resume.",
                    e.trim_end_matches('.')
                ),
                "problem",
            ),
        };
        let item = crate::inbox::NewItem {
            kind_of: kind_of.into(),
            kind: "run-resumed".into(),
            title,
            text,
            key: None,
            open: None,
            form: None,
            source: None,
        };
        if let Ok((added, new)) = crate::inbox::add_new(&self.db, &item) {
            (self.inbox_teller())(&json!({"change": "add", "item": added, "new": new}));
        }
    }

    fn me(&self) -> Result<Arc<Master>, String> {
        self.me
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| "The master is not shared yet.".to_string())
    }

    pub(super) fn run_env(&self, project_id: &str) -> Result<CoreEnv<'_>, String> {
        let watchers = self.watchers.clone();
        let tell_inbox = self.inbox_teller();
        let plan_data_watchers = self.plan_data_watchers.clone();
        let pid = project_id.to_string();
        // What the window hears as events on the desktop goes to the watching connections here; Inbox changes also go
        // to the connections that follow the Inbox (a desktop that does not watch this run still sees its questions).
        let notify: Notify = Arc::new(move |event: &str, value: Value| {
            if event == "qanuvi-inbox" {
                tell_inbox(&value);
            }
            // The run's plan data reaches every window that follows it, not only those watching this run.
            if event == "qanuvi-plan-data" {
                plan_data_watchers.send(&value);
            }
            watchers.send(
                &pid,
                json!({"kind": "event", "event": event, "value": value}),
            );
        });
        Ok(CoreEnv {
            db: self.db.clone(),
            agents: &self.agents,
            indexes: &self.indexes,
            project: projects::get(&self.db, project_id)?,
            notify,
        })
    }

    fn watch(&self, project_id: &str, args: &Value, stream: Streams) {
        if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
            self.watchers.add(project_id, stream(&name));
        }
    }

    /// The `plan_run_*` commands.
    pub(super) fn plan_run(
        &self,
        cmd: &str,
        args: &Value,
        stream: Streams,
    ) -> Result<Value, String> {
        let pid = s(args, "projectId");
        let sh = || self.runs.shared(&pid);
        match cmd {
            "plan_run_start" => {
                projects::get(&self.db, &pid)?;
                let setup = start_of(args)?.setup();
                // Said to the caller at once (the window or terminal), not as a run that ends straight away.
                if let Some(why) = crate::agent::missing(&setup.provider) {
                    return Err(why);
                }
                // M5b: kept so a run cut off by a shutdown can go on by itself after the restart (no keys in it).
                self.keep_start(&pid, &args["start"]);
                self.watch(&pid, args, stream);
                let me = self.me()?;
                let p = pid.clone();
                // y9-1: a start without a window (a terminal) asks for the check before the run; the window makes
                // its own in the Run panel and sends what it settled.
                let check_first = args["start"]["checkFirst"].as_bool() == Some(true);
                to(self.runs.start_run(&pid, self.watchers.clone(), move |sh| {
                    let env = me.run_env(&p)?;
                    let mut setup = setup;
                    if check_first {
                        let c = crate::plan::headless_check::check_before_run(&env, &setup).map_err(|e| {
                            format!("The check before the run did not answer ({e}). Start it again, or with --no-check.")
                        })?;
                        let o = &mut setup.options;
                        o.wait_before.get_or_insert_with(Vec::new).extend(c.wait_before);
                        o.extra_checks.get_or_insert_with(Vec::new).extend(c.extra_checks);
                    }
                    run_plan(&env, sh, setup)
                }))
            }
            "plan_run_finish" => {
                let start = start_of(args)?;
                if let Some(why) = crate::agent::missing(&start.provider) {
                    return Err(why);
                }
                let level: FinishLevel =
                    serde_json::from_value(args["level"].clone()).map_err(|e| e.to_string())?;
                let run = match sh().run().filter(|r| r.project_id == pid) {
                    Some(r) => r,
                    None => {
                        let file = (!start.file.is_empty()).then_some(start.file.as_str());
                        last_run(&runs::list(&self.db, &pid)?, file)
                            .ok_or("This project has no run to finish.")?
                    }
                };
                self.watch(&pid, args, stream);
                let me = self.me()?;
                let p = pid.clone();
                let setup = start.setup();
                to(self
                    .runs
                    .start_finish(&pid, self.watchers.clone(), move |sh| {
                        Ok(finish_pass(&me.run_env(&p)?, sh, setup, run, level))
                    }))
            }
            "plan_run_watch" | "plan_run_state" => {
                if cmd == "plan_run_watch" {
                    self.watch(&pid, args, stream);
                }
                if sh().run().is_none()
                    && let Some(r) = last_run(&runs::list(&self.db, &pid)?, None)
                {
                    self.runs.show_last(&pid, r);
                }
                to(Ok(self.runs.view(&pid)))
            }
            "plan_run_active" => to(Ok(self.runs.active())),
            "plan_run_stop" => {
                sh().stop();
                Ok(Value::Null)
            }
            "plan_run_pause" => {
                sh().pause();
                Ok(Value::Null)
            }
            "plan_run_finish_without" => {
                sh().finish_without();
                Ok(Value::Null)
            }
            "plan_run_answer" => {
                let choice: AnswerChoice =
                    serde_json::from_value(args["choice"].clone()).map_err(|e| e.to_string())?;
                sh().answer(
                    &s(args, "step"),
                    choice,
                    &opt(args, "text").unwrap_or_default(),
                );
                Ok(Value::Null)
            }
            "plan_run_message" => {
                sh().owner_message(&s(args, "text"));
                Ok(Value::Null)
            }
            "plan_run_tell" => {
                sh().controls.tell_agent(&s(args, "text"));
                Ok(Value::Null)
            }
            "plan_run_interrupt" => {
                sh().controls.interrupt();
                Ok(Value::Null)
            }
            "plan_run_model" => {
                sh().controls.switch_model(&s(args, "model"));
                Ok(Value::Null)
            }
            "plan_run_mode" => {
                sh().controls.set_mode(&s(args, "mode"));
                Ok(Value::Null)
            }
            // Y10 d7: what earlier checks in this project missed, for the window's check before the run.
            "plan_run_check_lessons" => {
                let env = self.run_env(&pid)?;
                to(Ok(crate::plan::check_more::lessons(&env)))
            }
            // Y10 d1-d5: the window's "Check and start" hands the check's answer here: what the steps need is tried
            // on this machine (holds for what is missing), and grants, eyes, the end check and promises are kept.
            "plan_run_check_settle" => {
                use crate::plan::env::RunEnv;
                use crate::plan::preflight::{RiskChoice, parse_preflight};
                let env = self.run_env(&pid)?;
                let file = s(args, "file");
                let found = parse_preflight(&s(args, "answer"));
                let choices: HashMap<usize, RiskChoice> = args["risky"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| {
                                Some((k.parse().ok()?, serde_json::from_value(v.clone()).ok()?))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let text = env.read(&file)?.text;
                let steps = crate::plan::tasklist::parse_plan(&text).steps;
                let settings = env.project_settings().unwrap_or_default();
                let plan_rules = crate::run_rules::plan_rules(&text);
                let recipe = crate::test_build::plan_test_build(&text)
                    .filter(crate::test_build::TestBuild::is_set)
                    .unwrap_or(settings.test_build);
                let first = s(args, "firstStep");
                let mut holds = crate::plan::check_more::probe(&env, &found, &steps, &first);
                for h in crate::plan::check_more::probe_test_build(&env, &recipe, &steps) {
                    if !holds.iter().any(|x| x.step == h.step) {
                        holds.push(h);
                    }
                }
                // The real end-to-end check the run rules refuse waits for the owner (shown on the card first).
                if let Some(h) = crate::plan::check_more::end_check_hold(
                    &env,
                    &found,
                    &steps,
                    &s(args, "mode"),
                    plan_rules.as_ref(),
                ) {
                    crate::plan::check_more::add_end_check_hold(&mut holds, h);
                }
                // Shown on the card first (apply false: only tried), kept once the owner starts.
                let applied = if args["apply"].as_bool() == Some(false) {
                    crate::plan::check_more::Applied::default()
                } else {
                    crate::plan::check_more::apply(
                        &env,
                        &file,
                        &found,
                        &choices,
                        &s(args, "mode"),
                        plan_rules.as_ref(),
                    )
                };
                Ok(json!({
                    "holds": holds,
                    "notes": applied.notes,
                    "said": applied.said,
                }))
            }
            _ => Err(format!("This master does not know the command {cmd}.")),
        }
    }
}
