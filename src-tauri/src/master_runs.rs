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

impl Master {
    fn me(&self) -> Result<Arc<Master>, String> {
        self.me
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| "The master is not shared yet.".to_string())
    }

    pub(super) fn run_env(&self, project_id: &str) -> Result<CoreEnv<'_>, String> {
        let watchers = self.watchers.clone();
        let inbox_watchers = self.inbox_watchers.clone();
        let pid = project_id.to_string();
        // What the window hears as events on the desktop goes to the watching connections here; Inbox changes also go
        // to the connections that follow the Inbox (a desktop that does not watch this run still sees its questions).
        let notify: Notify = Arc::new(move |event: &str, value: Value| {
            if event == "qanuvi-inbox" {
                inbox_watchers.send(&value);
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
                self.watch(&pid, args, stream);
                let me = self.me()?;
                let p = pid.clone();
                to(self.runs.start_run(&pid, self.watchers.clone(), move |sh| {
                    run_plan(&me.run_env(&p)?, sh, setup)
                }))
            }
            "plan_run_finish" => {
                let start = start_of(args)?;
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
            _ => Err(format!("This master does not know the command {cmd}.")),
        }
    }
}
