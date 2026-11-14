// Plan runs in the core, as the window reaches them (24z R2d): start, the finishing pass, the run's view, and the
// owner's asks (Stop, Pause, answers, messages, model, mode). The window hears each project's run as "qanuvi-run"
// (the view) and its steps' agent events as "qanuvi-run-agent" ({projectId, sessionId, event}).

use super::{AppState, Res};
use crate::plan::env::{CoreEnv, Notify};
use crate::plan::finishing::FinishLevel;
use crate::plan::holdask::AnswerChoice;
use crate::plan::live::{RunStart, RunView, Tell, finish_pass, last_run};
use crate::plan::run::run_plan;
use crate::{agent::AgentEvent, inbox, projects, runs};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// What the core tells the window: events, with a desktop notification for a new Inbox question.
pub(super) fn notify(app: &AppHandle) -> Notify {
    let app = app.clone();
    Arc::new(move |event: &str, value: serde_json::Value| {
        if event == "qanuvi-inbox"
            && value["change"] == "add"
            && value["new"] == true
            && let Ok(item) = serde_json::from_value::<inbox::InboxItem>(value["item"].clone())
        {
            crate::desktop_notify::new_item(&app, &item);
        }
        let _ = app.emit(event, value);
    })
}

struct WindowTell(AppHandle);

impl Tell for WindowTell {
    fn view(&self, view: &RunView) {
        let _ = self.0.emit("qanuvi-run", view);
    }

    fn agent(&self, project_id: &str, session_id: Option<&str>, event: &AgentEvent) {
        let _ = self.0.emit(
            "qanuvi-run-agent",
            serde_json::json!({"projectId": project_id, "sessionId": session_id, "event": event}),
        );
    }
}

/// The agent setups the window sends get the keys from this computer's credential store (the window never has them).
fn with_keys(mut start: RunStart) -> Res<RunStart> {
    for setup in start.turns.setups.values_mut() {
        *setup = crate::brain::with_keys(setup, super::model_key)?;
    }
    Ok(start)
}

/// Runs `f` with the project's `RunEnv` (on the run's own thread).
fn with_env<T>(app: &AppHandle, project_id: &str, f: impl FnOnce(&CoreEnv) -> T) -> Res<T> {
    let state = app.state::<AppState>();
    let project = projects::get(&state.db, project_id)?;
    let env = CoreEnv {
        db: state.db.clone(),
        agents: &state.agents,
        indexes: &state.indexes,
        project,
        notify: notify(app),
    };
    Ok(f(&env))
}

#[tauri::command]
pub(super) fn plan_run_start(
    app: AppHandle,
    state: State<AppState>,
    project_id: String,
    start: RunStart,
) -> Res<()> {
    projects::get(&state.db, &project_id)?;
    let start = with_keys(start)?;
    let tell: Arc<dyn Tell> = Arc::new(WindowTell(app.clone()));
    let pid = project_id.clone();
    let setup = start.setup();
    state.runs.start_run(&project_id, tell, move |sh| {
        with_env(&app, &pid, |env| run_plan(env, sh, setup))?
    })
}

/// The finishing pass on the project's last run (the one shown, else the newest record of the plan).
#[tauri::command]
pub(super) fn plan_run_finish(
    app: AppHandle,
    state: State<AppState>,
    project_id: String,
    start: RunStart,
    level: FinishLevel,
) -> Res<()> {
    let start = with_keys(start)?;
    let shown = state
        .runs
        .shared(&project_id)
        .run()
        .filter(|r| r.project_id == project_id);
    let run = match shown {
        Some(r) => r,
        None => {
            let file = (!start.file.is_empty()).then_some(start.file.as_str());
            last_run(&runs::list(&state.db, &project_id)?, file)
                .ok_or("This project has no run to finish.")?
        }
    };
    let tell: Arc<dyn Tell> = Arc::new(WindowTell(app.clone()));
    let pid = project_id.clone();
    let setup = start.setup();
    state.runs.start_finish(&project_id, tell, move |sh| {
        with_env(&app, &pid, |env| finish_pass(env, sh, setup, run, level))
    })
}

/// The project's run as it is now; with nothing shown yet, the newest one in the records.
#[tauri::command]
pub(super) fn plan_run_state(state: State<AppState>, project_id: String) -> Res<RunView> {
    let runs = &state.runs;
    if runs.shared(&project_id).run().is_none()
        && let Some(r) = last_run(&runs::list(&state.db, &project_id)?, None)
    {
        runs.show_last(&project_id, r);
    }
    Ok(runs.view(&project_id))
}

/// Projects with a run or a finishing pass going on.
#[tauri::command]
pub(super) fn plan_run_active(state: State<AppState>) -> Vec<String> {
    state.runs.active()
}

#[tauri::command]
pub(super) fn plan_run_stop(state: State<AppState>, project_id: String) {
    state.runs.shared(&project_id).stop();
}

#[tauri::command]
pub(super) fn plan_run_pause(state: State<AppState>, project_id: String) {
    state.runs.shared(&project_id).pause();
}

/// Ends a run that waits only for answers; what is set aside stays open.
#[tauri::command]
pub(super) fn plan_run_finish_without(state: State<AppState>, project_id: String) {
    state.runs.shared(&project_id).finish_without();
}

/// The owner answers a held step in the window (its number, else its title).
#[tauri::command]
pub(super) fn plan_run_answer(
    state: State<AppState>,
    project_id: String,
    step: String,
    choice: AnswerChoice,
    text: Option<String>,
) {
    state
        .runs
        .shared(&project_id)
        .answer(&step, choice, text.as_deref().unwrap_or(""));
}

/// The owner typed into the running step's chat: the runner sorts it before the agent hears it.
#[tauri::command]
pub(super) fn plan_run_message(state: State<AppState>, project_id: String, text: String) {
    state.runs.shared(&project_id).owner_message(&text);
}

/// Words for the running step's agent as they are, never sorted by a model (the plan's private details after the owner
/// gave one in the chat).
#[tauri::command]
pub(super) fn plan_run_tell(state: State<AppState>, project_id: String, text: String) {
    state.runs.shared(&project_id).controls.tell_agent(&text);
}

/// "Read my message now": the step's agent is interrupted so the waiting message goes in at once.
#[tauri::command]
pub(super) fn plan_run_interrupt(state: State<AppState>, project_id: String) {
    state.runs.shared(&project_id).controls.interrupt();
}

/// Another model for the step at work (from the next attempt; the agent finishes its edit first).
#[tauri::command]
pub(super) fn plan_run_model(state: State<AppState>, project_id: String, model: String) {
    state.runs.shared(&project_id).controls.switch_model(&model);
}

/// The run's permission mode from now on (kept for this plan).
#[tauri::command]
pub(super) fn plan_run_mode(state: State<AppState>, project_id: String, mode: String) {
    state.runs.shared(&project_id).controls.set_mode(&mode);
}

/// After an Inbox answer: what the run does with answers that change the project (Use Qanuvi's snapshots).
pub(super) fn answered(app: &AppHandle, item: &inbox::InboxItem) {
    let Some(pid) = item
        .source
        .as_ref()
        .and_then(|s| s.get("projectId"))
        .and_then(|v| v.as_str())
    else {
        return;
    };
    let _ = with_env(app, pid, |env| crate::plan::run::on_answer(env, item));
}
