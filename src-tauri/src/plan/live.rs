//! Runs as the commands reach them (24z R2d): one `Shared` per project, a run or a finishing pass on its own thread,
//! the run's view for the window (or a master's stream), and the start as the window sends it.
//!
//! The desktop and qanuvi-master both keep a `Runs`; each gives `start` the work to do on the thread (it builds the
//! project's `RunEnv` there), and a `Tell` that sends the view and the steps' agent events where they belong.

use super::decider::{TeamSettings, parse_team};
use super::env::{MemoryUse, PlannerAsk, RunEnv, ask_planner, assistant_text};
use super::finishing::{
    Checks, DoneBrief, FinishLevel, FinishPass, FinishStatus, ReviewFound, done_notes,
    finish_entry, finish_full_prompt, finish_review_prompt, is_plan_file, parse_finish_review,
    still_open_in,
};
use super::guidance as g;
use super::packet::failure_summary;
use super::routing::{AgentModel, JobType};
use super::run::{ResumeAt, RunSetup, Shared, status_word};
use super::runner::{Outcome, PlanRun, RunOptions, RunStepStatus};
use super::tasklist::parse_plan;
use super::turn::{self, AttemptAsk, CHECK_TIMEOUT, FAILURE_CHARS, StepWatch, Totals, TurnSetup};
use crate::agent::AgentEvent;
use crate::inbox::NewItem;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// What the window shows of a project's run ("qanuvi-run" event, `plan_run_state`).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub project_id: String,
    pub run: Option<PlanRun>,
    /// A run goes on (its steps, its waits).
    pub running: bool,
    /// A finishing pass goes on.
    pub finishing: bool,
    pub waiting_answers: usize,
    /// Milliseconds: the run waits for a usage limit to reset.
    pub waiting_until: Option<i64>,
    pub current_model: String,
    pub notices: Vec<String>,
    pub pausing: bool,
    pub stopping: bool,
    /// Qanuvi runs the project's checks now.
    pub checking: bool,
    /// The step's agent session at work.
    pub session_id: Option<String>,
    /// Owner messages typed into the step's chat that its turn has not taken yet.
    pub waiting_messages: usize,
    /// The last run or pass could not start, or ended with an error.
    pub error: Option<String>,
}

/// Where a running project's news goes: the view after every change, and each agent event of its steps with the
/// session it belongs to.
pub trait Tell: Send + Sync {
    fn view(&self, view: &RunView);
    fn agent(&self, project_id: &str, session_id: Option<&str>, event: &AgentEvent);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Busy {
    Run,
    Finish,
}

struct Entry {
    shared: Arc<Shared>,
    busy: Option<Busy>,
    error: Option<String>,
}

/// Every project's run handle. A project runs one thing at a time (a run, or the finishing pass after it).
#[derive(Default)]
pub struct Runs {
    map: Mutex<HashMap<String, Entry>>,
}

impl Runs {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The project's handle (made the first time), for Stop, Pause, answers and messages.
    pub fn shared(&self, project_id: &str) -> Arc<Shared> {
        self.map()
            .entry(project_id.to_string())
            .or_insert_with(|| Entry {
                shared: Arc::new(Shared::default()),
                busy: None,
                error: None,
            })
            .shared
            .clone()
    }

    /// A run or a pass goes on in the project.
    pub fn busy(&self, project_id: &str) -> bool {
        self.map().get(project_id).is_some_and(|e| e.busy.is_some())
    }

    /// Projects with a run or a pass going on (a master's "what runs here").
    pub fn active(&self) -> Vec<String> {
        self.map()
            .iter()
            .filter(|(_, e)| e.busy.is_some())
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn view(&self, project_id: &str) -> RunView {
        let (sh, busy, error) = {
            let map = self.map();
            match map.get(project_id) {
                Some(e) => (e.shared.clone(), e.busy, e.error.clone()),
                None => {
                    return RunView {
                        project_id: project_id.to_string(),
                        ..RunView::default()
                    };
                }
            }
        };
        let c = &sh.controls;
        RunView {
            project_id: project_id.to_string(),
            run: sh.run(),
            running: busy == Some(Busy::Run),
            finishing: busy == Some(Busy::Finish),
            waiting_answers: sh.waiting_answers(),
            waiting_until: sh.waiting_until(),
            current_model: sh.current_model(),
            notices: sh.notices(),
            pausing: c.pausing(),
            stopping: c.stop_asked().is_some(),
            checking: c.checking(),
            session_id: c.session_id(),
            waiting_messages: c.waiting_messages(),
            error,
        }
    }

    /// Starts a run of the project's plan on its own thread. `work` builds the project's `RunEnv` there and runs
    /// `run_plan` (or the scenario's stand-in) on the handle it is given.
    pub fn start_run(
        self: &Arc<Self>,
        project_id: &str,
        tell: Arc<dyn Tell>,
        work: impl FnOnce(&Shared) -> Result<PlanRun, String> + Send + 'static,
    ) -> Result<(), String> {
        self.start(project_id, Busy::Run, tell, work)
    }

    /// Starts the finishing pass on the project's last run (see `finish_pass`).
    pub fn start_finish(
        self: &Arc<Self>,
        project_id: &str,
        tell: Arc<dyn Tell>,
        work: impl FnOnce(&Shared) -> Result<PlanRun, String> + Send + 'static,
    ) -> Result<(), String> {
        self.start(project_id, Busy::Finish, tell, work)
    }

    fn start(
        self: &Arc<Self>,
        project_id: &str,
        kind: Busy,
        tell: Arc<dyn Tell>,
        work: impl FnOnce(&Shared) -> Result<PlanRun, String> + Send + 'static,
    ) -> Result<(), String> {
        let sh = self.shared(project_id);
        {
            let mut map = self.map();
            let e = map.get_mut(project_id).ok_or("no run handle")?;
            if e.busy.is_some() {
                return Err(match e.busy {
                    Some(Busy::Finish) => "The finishing pass is at work in this project.".into(),
                    _ => "A plan run is going on in this project.".into(),
                });
            }
            e.busy = Some(kind);
            e.error = None;
        }
        let runs = self.clone();
        let pid = project_id.to_string();
        let (t_view, t_agent) = (tell.clone(), tell.clone());
        let (r_view, p_agent) = (runs.clone(), pid.clone());
        let (r_agent, p_view) = (runs.clone(), pid.clone());
        // Each turn's events start with its session (steps, reviews and the planner alike).
        let session: Mutex<Option<String>> = Mutex::new(None);
        // The step session the view showed last: a new one is told at once (the window's chat follows it).
        let shown: Mutex<Option<String>> = Mutex::new(None);
        sh.listen(
            Some(Arc::new(move |_run: &PlanRun| {
                t_view.view(&r_view.view(&p_view));
            })),
            Some(Arc::new(move |e: &AgentEvent| {
                let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
                if let AgentEvent::Session { session } = e {
                    *s = Some(session.id.clone());
                }
                let sid = s.clone();
                drop(s);
                t_agent.agent(&p_agent, sid.as_deref(), e);
                if matches!(e, AgentEvent::Session { .. }) {
                    let view = r_agent.view(&p_agent);
                    let mut last = shown.lock().unwrap_or_else(|e| e.into_inner());
                    if view.session_id.is_some() && view.session_id != *last {
                        *last = view.session_id.clone();
                        drop(last);
                        t_agent.view(&view);
                    }
                }
            })),
        );
        tell.view(&self.view(project_id));
        let spawned = std::thread::Builder::new()
            .name(format!("qanuvi-run-{pid}"))
            .spawn(move || {
                let out = work(&sh);
                {
                    let mut map = runs.map();
                    if let Some(e) = map.get_mut(&pid) {
                        e.busy = None;
                        e.error = out.as_ref().err().cloned();
                    }
                }
                if let Ok(run) = &out {
                    sh.publish(run);
                }
                sh.listen(None, None);
                tell.view(&runs.view(&pid));
            });
        if let Err(e) = spawned {
            if let Some(en) = self.map().get_mut(project_id) {
                en.busy = None;
            }
            return Err(e.to_string());
        }
        Ok(())
    }

    /// Shows the project's last run (from the records) when nothing runs: the window's Run panel after a restart.
    pub fn show_last(&self, project_id: &str, run: PlanRun) {
        if !self.busy(project_id) {
            self.shared(project_id).publish(&run);
        }
    }
}

/// The newest run in the records (of one plan file, when given) that still reads.
pub fn last_run(records: &[crate::runs::RunRecord], file: Option<&str>) -> Option<PlanRun> {
    records
        .iter()
        .filter(|r| file.is_none_or(|f| r.file == f))
        .max_by_key(|r| r.updated_at)
        .and_then(|r| serde_json::from_str(&r.data).ok())
}

// ---------- The start as the window sends it ----------

/// The memory part of a start (Settings > Memory).
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemoryStart {
    pub on: bool,
    pub budget: usize,
}

/// What every turn is sent with.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TurnStart {
    pub effort: Option<String>,
    pub base_url: Option<String>,
    pub browser_tools: bool,
    pub lean: bool,
    pub index_tools: bool,
    /// Qanuvi's agent setup per agent, keys filled in.
    pub setups: BTreeMap<String, String>,
    pub memory: MemoryStart,
    pub labels: BTreeMap<String, String>,
}

/// A session's totals so far.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TotalsStart {
    pub cost: f64,
    pub input: i64,
    pub output: i64,
    pub ms: i64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ResumeStart {
    pub session_id: String,
    pub model: String,
    pub section: String,
    pub base: TotalsStart,
    pub text: String,
    pub is_note: bool,
}

/// A run's start from the window (`plan_run_start`), or the setup of a finishing pass (`plan_run_finish`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RunStart {
    pub project_name: String,
    pub file: String,
    pub options: RunOptions,
    pub provider: String,
    pub provider_name: String,
    pub models: Vec<AgentModel>,
    pub default_model: String,
    pub planner_model: String,
    /// Settings > Team as stored (anything unknown takes the default).
    pub team: Value,
    pub has_free: bool,
    pub monthly_budget_usd: f64,
    /// The project's model rules per kind of job ("code", "docs", ...).
    pub rules: HashMap<String, String>,
    pub turns: TurnStart,
    pub resume: Option<ResumeStart>,
}

impl RunStart {
    pub fn team(&self) -> TeamSettings {
        parse_team(&self.team)
    }

    pub fn setup(self) -> RunSetup {
        let team = self.team();
        let rules = self
            .rules
            .into_iter()
            .filter_map(|(k, v)| {
                let job: JobType = serde_json::from_value(Value::String(k)).ok()?;
                (!v.is_empty()).then_some((job, v))
            })
            .collect();
        let t = self.turns;
        RunSetup {
            project_name: self.project_name,
            file: self.file,
            options: self.options,
            provider: self.provider.clone(),
            provider_name: self.provider_name,
            models: self.models,
            default_model: self.default_model,
            planner_model: self.planner_model,
            team,
            has_free: self.has_free,
            monthly_budget_usd: self.monthly_budget_usd,
            rules,
            turns: TurnSetup {
                effort: t.effort,
                base_url: t.base_url,
                browser_tools: t.browser_tools,
                lean: t.lean,
                index_tools: t.index_tools,
                provider: self.provider,
                agent_override: None,
                setups: t.setups,
                memory: MemoryUse {
                    on: t.memory.on,
                    budget: t.memory.budget,
                },
                labels: t.labels,
                message_wait_ms: 0,
            },
            resume: self.resume.map(|r| ResumeAt {
                session_id: r.session_id,
                model: r.model,
                section: r.section,
                base: Totals {
                    cost: r.base.cost,
                    input: r.base.input,
                    output: r.base.output,
                    ms: r.base.ms,
                },
                text: r.text,
                is_note: r.is_note,
            }),
        }
    }
}

// ---------- The finishing pass (port of the window's `finishPass`) ----------

const STOPPED: &str = "Stopped by you.";

/// The finishing pass on a finished run: the project's full checks, the paid agent's review of the run's whole diff
/// (free models when it cannot answer) and, at the full level, a session that fixes what the review found. The pass
/// is kept in the run's record, its FINISH.md log and an Inbox item. Answers the run with its pass.
pub fn finish_pass(
    env: &dyn RunEnv,
    sh: &Shared,
    setup: RunSetup,
    mut run: PlanRun,
    level: FinishLevel,
) -> PlanRun {
    sh.begin();
    let model = setup.planner_model.clone();
    let started = env.now_ms() as f64;
    run.finish = Some(FinishPass {
        level,
        status: FinishStatus::Running,
        model: model.clone(),
        started_at: started,
        ended_at: None,
        cost_usd: 0.0,
        seconds: 0.0,
        checks: None,
        findings: vec![],
        still_open: vec![],
        session_id: None,
        note: None,
    });
    save(env, sh, &run);
    let stop = || sh.controls.stop_asked().is_some();
    let outcome = pass_work(env, sh, &setup, &mut run, level, &model, &stop);
    let pass = run.finish.as_mut().expect("the pass was set above");
    match outcome {
        Ok(()) => {
            if pass.status == FinishStatus::Running {
                pass.status = if stop() {
                    FinishStatus::Stopped
                } else {
                    FinishStatus::Done
                };
            }
        }
        Err(e) => {
            if stop() {
                pass.status = FinishStatus::Stopped;
                pass.note = Some(STOPPED.into());
            } else {
                pass.status = FinishStatus::Failed;
                pass.note = Some(e);
            }
        }
    }
    let ended = env.now_ms() as f64;
    pass.ended_at = Some(ended);
    pass.seconds = (ended - pass.started_at) / 1000.0;
    let pass = pass.clone();
    sh.set_current_model("");
    save(env, sh, &run);
    let _ = env.log_append(
        &format!("{}/FINISH.md", g::log_dir(&run.file)),
        &finish_entry(&pass),
    );
    let name = run.file.rsplit('/').next().unwrap_or(&run.file).to_string();
    let project = if setup.project_name.is_empty() {
        String::new()
    } else {
        format!("{}: ", setup.project_name)
    };
    let status = serde_json::to_value(pass.status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default();
    let text: Vec<String> = pass
        .findings
        .iter()
        .take(6)
        .cloned()
        .chain(pass.still_open.iter().map(|x| format!("Still open: {x}")))
        .collect();
    let _ = env.inbox_add(&NewItem {
        kind_of: "info".into(),
        kind: "run".into(),
        title: format!(
            "{project}{name} finishing pass {status} · {} still open",
            pass.still_open.len()
        ),
        text: text.join("\n"),
        key: None,
        open: Some(json!({"label": "Open the plan", "view": "plan", "projectId": run.project_id})),
        form: None,
        source: None,
    });
    run
}

fn save(env: &dyn RunEnv, sh: &Shared, run: &PlanRun) {
    if let Ok(data) = serde_json::to_string(run) {
        let _ = env.save_run(&run.id, &run.file, &status_word(run.status), &data);
    }
    sh.publish(run);
}

fn pass_work(
    env: &dyn RunEnv,
    sh: &Shared,
    setup: &RunSetup,
    run: &mut PlanRun,
    level: FinishLevel,
    model: &str,
    stop: &dyn Fn() -> bool,
) -> Result<(), String> {
    let command = env
        .project_settings()
        .map(|p| p.test_command.trim().to_string())
        .unwrap_or_default();
    if !command.is_empty() {
        let c = env.run_check(&command, CHECK_TIMEOUT);
        let checks = match c {
            Ok(c) => Checks {
                command: command.clone(),
                ok: c.ok,
                summary: if c.ok {
                    String::new()
                } else if c.timed_out {
                    "The checks did not finish within the time limit.".into()
                } else {
                    failure_summary(&c.output, FAILURE_CHARS)
                },
            },
            Err(e) => Checks {
                command: command.clone(),
                ok: false,
                summary: failure_summary(&e, FAILURE_CHARS),
            },
        };
        pass_of(run).checks = Some(checks);
        save(env, sh, run);
    }
    if stop() {
        return Err("stopped".into());
    }
    let (diff, files) = run_diff(env, run);
    let briefs = done_briefs(env, run);
    let checks = pass_of(run).checks.clone();
    let prompt = finish_review_prompt(&run.file, &briefs, &diff, checks.as_ref());
    let ask = PlannerAsk {
        prompt: prompt.clone(),
        model: model.to_string(),
        title: "Finishing pass: review the whole run".into(),
        agent: setup.provider.clone(),
        base_url: setup.turns.base_url.clone(),
        effort: setup.turns.effort.clone(),
        lean: true,
        index_tools: false,
        setup: setup.turns.setups.get(&setup.provider).cloned(),
    };
    let mut review = match ask_planner(env, setup.turns.memory, &ask, &mut |e| sh.forward(e)) {
        Ok(a) => {
            pass_of(run).cost_usd += a.cost_usd;
            parse_finish_review(&a.text)
        }
        Err(e) => {
            // The paid agent cannot answer now: the free models review instead, and the report says so.
            let free = setup
                .turns
                .setups
                .get("qanuvi")
                .ok_or_else(|| "no free models are set up".to_string())
                .and_then(|s| {
                    env.free_decide(
                        s,
                        "You review code changes. Answer with JSON only.",
                        &prompt,
                    )
                });
            match free {
                Ok((by, text)) => {
                    let mut r = parse_finish_review(&text);
                    r.findings.push(format!(
                        "(Reviewed by {by}: {} could not answer: {e})",
                        setup.provider_name
                    ));
                    r
                }
                Err(_) => super::finishing::FinishReview {
                    ok: false,
                    findings: vec![format!("The review did not answer ({e}).")],
                    still_open: vec![],
                },
            }
        }
    };
    if let Some(c) = checks.as_ref().filter(|c| !c.ok) {
        review.findings.insert(
            0,
            format!("The full checks fail: {}", super::js::cut(&c.summary, 300)),
        );
    }
    {
        let p = pass_of(run);
        p.findings = review.findings.clone();
        p.still_open = review.still_open.clone();
    }
    save(env, sh, run);
    if level != FinishLevel::Full || stop() {
        return Ok(());
    }
    // A session of its own on the run's agent: no handover, the run's own per-step limits.
    let mut turns = setup.turns.clone();
    turns.agent_override = Some(setup.provider.clone());
    let mut watch = StepWatch::for_step(
        env.now_ms(),
        0.0,
        super::costs::StepLimits {
            usd: run.options.step_cost_limit,
            minutes: run.options.step_minutes,
        },
        0,
        "",
    );
    sh.set_current_model(model);
    let found = ReviewFound {
        findings: review.findings.clone(),
        still_open: review.still_open.clone(),
    };
    let ask = AttemptAsk {
        prompt: finish_full_prompt(&run.file, &briefs, &found, checks.as_ref(), &files),
        model: model.to_string(),
        mode: run.options.mode.clone(),
        title: "Finishing pass".into(),
        test_command: command.clone(),
        resume: None,
    };
    let end = turn::attempt(env, &sh.controls, &mut watch, &turns, &ask, &mut |e| {
        sh.forward(e)
    });
    let a = end.attempt;
    let p = pass_of(run);
    if !a.session_id.is_empty() {
        p.session_id = Some(a.session_id.clone());
    }
    p.cost_usd += a.cost_usd;
    if !command.is_empty()
        && let Some(passed) = a.tests_passed
    {
        p.checks = Some(Checks {
            command: command.clone(),
            ok: passed,
            summary: if passed {
                String::new()
            } else {
                a.failure.clone().unwrap_or_default()
            },
        });
    }
    let mut answer = String::new();
    if !a.session_id.is_empty() {
        let texts: Vec<String> = env
            .session_lines(&a.session_id)
            .iter()
            .filter_map(|l| assistant_text(l))
            .collect();
        answer = texts[texts.len().saturating_sub(2)..].join("\n");
        if end.idle {
            env.agent_stop(&a.session_id);
        }
    }
    let mut findings: Vec<String> = review
        .findings
        .iter()
        .map(|f| format!("Review: {f}"))
        .collect();
    findings.push(if command.is_empty() {
        "No test command: the pass could not run the checks itself.".into()
    } else if a.tests_passed == Some(false) {
        "After the pass the full checks still fail.".into()
    } else {
        "After the pass the full checks pass.".into()
    });
    p.findings = findings;
    p.still_open = still_open_in(&answer).unwrap_or_else(|| {
        let mut v = review.still_open.clone();
        v.push("The pass's answer had no Still open list: read its session.".into());
        v
    });
    if a.outcome == Outcome::Stopped || watch.over_limit.is_some() {
        p.status = FinishStatus::Stopped;
        p.note = Some(watch.over_limit.clone().unwrap_or_else(|| STOPPED.into()));
    } else if a.outcome == Outcome::Error {
        p.status = FinishStatus::Failed;
        p.note = Some(if a.note.is_empty() {
            "The session ended with an error.".into()
        } else {
            a.note.clone()
        });
    }
    Ok(())
}

fn pass_of(run: &mut PlanRun) -> &mut FinishPass {
    run.finish.as_mut().expect("a finishing pass is set")
}

/// The run's own changes as one diff (the task list and its logs left out), and the files it changed.
fn run_diff(env: &dyn RunEnv, run: &PlanRun) -> (String, Vec<String>) {
    let dir = g::log_dir(&run.file);
    let skip = |p: &str| is_plan_file(p, &run.file, &dir);
    let mut files: Vec<String> = vec![];
    for f in run.steps.iter().flat_map(|s| s.changed.iter().flatten()) {
        if !skip(f) && !files.contains(f) {
            files.push(f.clone());
        }
    }
    let git = env
        .project_settings()
        .is_ok_and(|p| p.changes_from == "git");
    if let Some(snap) = &run.snapshot
        && let Ok(c) = env.snapshot_changes(&snap.id, &snap.also, git, None)
    {
        let parts: Vec<String> = c
            .changes
            .iter()
            .filter(|x| !skip(&x.path) && !x.diff.is_empty())
            .map(|x| x.diff.clone())
            .collect();
        return (parts.join("\n"), files);
    }
    // No snapshot (an older run, or it expired): no diff; the review goes by the files and the steps' Done notes.
    (String::new(), files)
}

/// The steps the run finished, with the Done notes they left in the task list.
fn done_briefs(env: &dyn RunEnv, run: &PlanRun) -> Vec<DoneBrief> {
    let steps = env
        .read(&run.file)
        .map(|f| parse_plan(&f.text).steps)
        .unwrap_or_default();
    run.steps
        .iter()
        .filter(|s| s.status == RunStepStatus::Done)
        .map(|s| {
            let found = steps
                .iter()
                .find(|x| x.number == s.number && x.title == s.title);
            DoneBrief {
                number: s.number.clone(),
                title: s.title.clone(),
                done: done_notes(found.map(|f| f.details.as_slice()).unwrap_or(&[])),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
