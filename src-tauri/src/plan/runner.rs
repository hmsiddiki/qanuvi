//! Running a task list step by step, each step on its own model (ported from `src/lib/plan/runner.ts`): the run
//! records and the plain decisions. The run itself moves into the core in R2.

use super::costs::ToolKinds;
use super::decider::{DecisionRecord, TeamMode};
use super::finishing::{FINISHER, FinishAuto, FinishPass};
use super::routing::{AgentModel, JobType};
use super::setaside::HeldKind;
use super::tasklist::{Step, StepStatus, parse_plan};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// When a run stops for the user (24f).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PauseMode {
    /// Runs to the end; a blocked step, or one no model could finish, is skipped with its reason.
    Never,
    /// Stops at a blocked step or one no model could finish.
    Decisions,
    /// Stops after every step too.
    Step,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunTeam {
    Settings,
    Paid,
    Free,
    FreeOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnLimit {
    Wait,
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FullChecks {
    Step,
    Section,
}

/// A step named with why (a pause before it, or extra checks on it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepWhy {
    pub step: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOptions {
    /// Line of the first step to run; earlier steps are skipped.
    pub from_line: usize,
    /// Run only that one step.
    pub only_one: bool,
    /// Permission mode for the run's sessions.
    pub mode: String,
    /// Stop when the run has cost this much (US dollars); 0 means no limit.
    pub budget_usd: f64,
    /// Failed attempts on one model before the step moves to a stronger model.
    pub attempts_per_model: u32,
    /// Sessions without MCP servers and with only the file and command tools: cheaper, and cached between steps.
    pub lean: bool,
    /// Give each step's session Qanuvi's code-index tools.
    pub index_tools: bool,
    /// Work packets: hand each step the code it mentions, and let Qanuvi run the tests and send back only failures.
    pub packets: bool,
    /// When a step fails even on the strongest model, ask the planner once to rewrite it into smaller steps.
    pub replan: bool,
    /// Free models first (money and security work excepted); the paid agent takes over when they do not finish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_first: Option<bool>,
    /// Which steps by number, for example "2-6, 10-15"; empty: all open steps from `from_line` (24f).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranges: Option<String>,
    /// The agent the run uses; missing: the one Qanuvi picks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// One model for every step; empty or missing: as the plan says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A step that keeps failing moves to a stronger model; missing: yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stronger_on_fail: Option<bool>,
    /// The Team mode for this run; missing: the Settings choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_mode: Option<TeamMode>,
    /// A step may cost at most this much (US dollars) and run this many minutes; 0 or missing: no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_cost_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_minutes: Option<f64>,
    /// When the paid agent hits its usage limit: wait for the reset, or let free models work until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_limit: Option<OnLimit>,
    /// Steps the run pauses before, with why (24i).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_before: Option<Vec<StepWhy>>,
    /// Steps that go ahead with extra checks (24u part 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_checks: Option<Vec<StepWhy>>,
    /// When every step of the plan is done, its private details (logins) are deleted; missing: yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget_private_when_done: Option<bool>,
    /// Steps in a row on the same model and in the same section continue one session; missing: yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carry_on: Option<bool>,
    /// A session also goes on into the next section; missing: yes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carry_across_sections: Option<bool>,
    /// The full test suite after every step, or once at the end of each section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_checks: Option<FullChecks>,
    /// Past this many tokens of context the next step starts a new session (with the run's notes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carry_limit: Option<f64>,
    /// Past this many tokens of context a step in the middle of its work hands over to a new session; 0 = never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handover_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<RunTeam>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<PauseMode>,
    /// At the end of a run that got done: offer the finishing pass, or run it by itself (24o); missing: offer it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_pass: Option<FinishAuto>,
}

/// The "model" of an attempt on Qanuvi's own agent (Qanuvi core picks among the free and local models).
pub const FREE_MODEL: &str = "qanuvi:auto";

pub fn is_free(model: &str) -> bool {
    model.starts_with("qanuvi:")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Done,
    Open,
    Blocked,
    Error,
    Stopped,
}

fn failed(o: Outcome) -> bool {
    matches!(o, Outcome::Open | Outcome::Error)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub model: String,
    #[serde(default)]
    pub session_id: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub input_tokens: f64,
    #[serde(default)]
    pub output_tokens: f64,
    #[serde(default)]
    pub seconds: f64,
    #[serde(default)]
    pub note: String,
    /// How often Qanuvi ran the tests in this attempt (work packets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_runs: Option<u32>,
    /// Whether the last test run passed; None when Qanuvi did not run them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_passed: Option<bool>,
    /// What its tool calls were for: reading, changing files, running commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolKinds>,
    /// The last test failures sent to the agent (for the planner when the step is rewritten).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStepStatus {
    Waiting,
    Running,
    Done,
    Blocked,
    Failed,
    Skipped,
    /// Set aside, waiting for the owner's answer (24u).
    Held,
    /// Waits for a held step (`waits_for`).
    SetAside,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Held {
    pub kind: HeldKind,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRun {
    pub line: usize,
    pub number: Option<String>,
    pub title: String,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    pub status: RunStepStatus,
    /// Why a held step waits for the owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
    /// The held step (number, else title) a set-aside step needs, and why it needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits_for: Option<StepWhy>,
    /// The owner's answer to a held step: the next try is told it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Tries before this index were before the owner's answer: the model ladder starts again after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_from: Option<usize>,
    /// A closing step opened again: the set-aside steps that finished after it first ran (24u).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerun_after: Option<Vec<String>>,
    /// Kind of work, for learned routing (Milestone 13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_type: Option<JobType>,
    /// Why the step started on its model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// The planner rewrote this step into the steps named here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by: Option<String>,
    /// This step is one of the planner's replacements for the step with this number (or title).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// Files the step changed (modified while it ran).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<Vec<String>>,
    /// Why the run skipped, or could not finish, the step (24f).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Running,
    Done,
    Stopped,
    Failed,
    NeedsYou,
    Paused,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PausedSession {
    pub session_id: String,
    pub model: String,
    pub section: String,
    pub number: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeciderCall {
    pub model: String,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRun {
    pub id: String,
    pub project_id: String,
    pub file: String,
    pub provider: String,
    pub options: RunOptions,
    pub steps: Vec<StepRun>,
    pub status: RunStatus,
    #[serde(default)]
    pub message: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    /// Paused in the middle of a step: its session, so Resume goes on in it (also after a restart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_session: Option<PausedSession>,
    /// What the decider decided in this run, with its reasons (24b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decisions: Option<Vec<DecisionRecord>>,
    /// What the owner said during the run and what steps left for later ones: every later step gets them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
    /// The decider's own calls (assignment, reviews): they count in the run's cost (24e).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decider_calls: Option<Vec<DeciderCall>>,
    /// Qanuvi's snapshot of the project when the run started (kept so the finishing pass still sees only the run's
    /// own changes after a restart).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<RunSnapshot>,
    /// The finishing pass over the whole run (24o), once it ran or runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<FinishPass>,
}

/// The models a step may move up to after failures: the agent's list, or none when the run stays on one model.
pub fn ladder(models: &[AgentModel], stronger_on_fail: Option<bool>) -> &[AgentModel] {
    if stronger_on_fail == Some(false) {
        &[]
    } else {
        models
    }
}

/// Whether free models go first in a run, by the run's own Team choice.
pub fn free_first_for(team: Option<RunTeam>, team_allows: bool, can_free: bool) -> bool {
    if !can_free || team == Some(RunTeam::Paid) {
        return false;
    }
    team == Some(RunTeam::Free) || team_allows
}

/// Whether the run's Team choice wanted free models but none are set up, so every step goes to the paid agent.
/// Not for runs on Qanuvi's own agent, or ones set to paid only.
pub fn free_wanted_but_missing(
    team: Option<RunTeam>,
    mode: TeamMode,
    has_free: bool,
    own_agent: bool,
) -> bool {
    if has_free || own_agent || team == Some(RunTeam::Paid) {
        return false;
    }
    matches!(team, Some(RunTeam::Free | RunTeam::FreeOnly)) || mode != TeamMode::Direct
}

/// Roughly how much one model on every step costs against the plan's own models (by output price; tokens per step
/// taken as alike). None when a price is unknown.
pub fn cost_ratio(models: &[AgentModel], step_models: &[String], one: &str) -> Option<f64> {
    let out = |id: &str| {
        models
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.price)
            .map(|p| p[1])
    };
    let target = out(one)?;
    if step_models.is_empty() {
        return None;
    }
    let mut own = 0.0;
    for id in step_models {
        own += out(id)?;
    }
    (own > 0.0).then(|| target * step_models.len() as f64 / own)
}

/// The run lines' key for the decider's calls on a model.
pub const DECIDER: &str = "decider:";

/// The steps a run covers: open steps (not done, not blocked) from the chosen one on.
pub fn steps_to_run(
    steps: &[Step],
    from_line: usize,
    only_one: bool,
    ranges: &[(u64, u64)],
) -> Vec<Step> {
    let open = steps
        .iter()
        .filter(|s| s.status != StepStatus::Done && s.status != StepStatus::Blocked);
    if !ranges.is_empty() && !only_one {
        return open
            .filter(|s| in_ranges(s.number.as_deref(), ranges))
            .cloned()
            .collect();
    }
    let from = open.filter(|s| s.line >= from_line);
    if only_one {
        from.filter(|s| s.line == from_line)
            .take(1)
            .cloned()
            .collect()
    } else {
        from.cloned().collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ranges {
    pub ranges: Vec<(u64, u64)>,
    pub error: String,
}

/// Reads step ranges such as "2-6, 10-15, 20". Unreadable parts are reported, not guessed.
pub fn parse_ranges(text: Option<&str>) -> Ranges {
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[,;]+").unwrap());
    static SPAN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)^([0-9]+)\s*(?:-|–|to)\s*([0-9]+)$").unwrap());
    static ONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]+)$").unwrap());
    let mut out = Ranges::default();
    let mut bad: Vec<&str> = vec![];
    for part in SPLIT
        .split(text.unwrap_or(""))
        .map(super::js::trim)
        .filter(|p| !p.is_empty())
    {
        let Some(m) = SPAN.captures(part).or_else(|| ONE.captures(part)) else {
            bad.push(part);
            continue;
        };
        let num = |s: &str| s.parse::<u64>().unwrap_or(u64::MAX);
        let a = num(&m[1]);
        let b = m.get(2).map_or(a, |x| num(x.as_str()));
        out.ranges.push((a.min(b), a.max(b)));
    }
    if !bad.is_empty() {
        out.error = format!("Not a step range: {}", bad.join(", "));
    }
    out
}

/// `parseInt(s, 10)`: the leading whole number, if any.
fn parse_int(s: &str) -> Option<i64> {
    let t = super::js::trim_start(s);
    let (sign, rest) = match t.as_bytes().first() {
        Some(b'-') => (-1, &t[1..]),
        Some(b'+') => (1, &t[1..]),
        _ => (1, t),
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok().map(|n| sign * n)
}

/// Whether a step number (such as "12" or "12b") falls in the ranges. Steps without a number never do.
pub fn in_ranges(number: Option<&str>, ranges: &[(u64, u64)]) -> bool {
    let Some(n) = number.and_then(parse_int) else {
        return false;
    };
    ranges.iter().any(|&(a, b)| n >= a as i64 && n <= b as i64)
}

/// The first thing a step needs that is not done, as a reason to skip it; None when all is done (24f). `done` holds
/// this plan's finished step numbers; `others` the finished numbers of other plans by file (missing: not read).
pub fn unmet_need(
    needs: &[String],
    done: &HashSet<String>,
    others: &HashMap<String, HashSet<String>>,
) -> Option<String> {
    for need in needs {
        match need.rfind('#').filter(|&h| h > 0) {
            Some(h) => {
                let (file, num) = (&need[..h], &need[h + 1..]);
                let Some(there) = others.get(file) else {
                    return Some(format!(
                        "needs step {num} of {file}, which could not be read"
                    ));
                };
                if !there.contains(num) {
                    return Some(format!("needs step {num} of {file}, which is not done"));
                }
            }
            None if !done.contains(need) => {
                return Some(format!("needs step {need}, which is not done"));
            }
            None => {}
        }
    }
    None
}

/// How a run that went through its steps ends: "needs-you" while steps are still held or set aside, else "done".
pub fn end_status(steps: &[StepRun]) -> RunStatus {
    if steps
        .iter()
        .any(|s| matches!(s.status, RunStepStatus::Held | RunStepStatus::SetAside))
    {
        RunStatus::NeedsYou
    } else {
        RunStatus::Done
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub id: String,
    pub also: Vec<String>,
}

/// What a review read: the diffs, or why they could not be read.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ReadChanges {
    #[serde(default)]
    pub parts: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Verdict {
    pub ok: bool,
    pub why: String,
}

/// A review that cannot see the change must never turn a step down for that alone (PLAN-F 97a, 2026-11-14). The
/// changes could not be read, or the step changed files but no diff came back: the tests decide, and the reason is
/// said. None: the review goes ahead.
pub fn review_without_diff(
    changed: &ReadChanges,
    touched: &[String],
    tests_passed: Option<bool>,
) -> Option<Verdict> {
    let cause = if let Some(e) = &changed.error {
        format!("the changes could not be read ({e})")
    } else if changed.parts.is_empty() && !touched.is_empty() {
        let mut names = touched
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if touched.len() > 3 {
            names.push_str(&format!(" and {} more", touched.len() - 3));
        }
        let what = if touched.len() == 1 {
            "a file".to_string()
        } else {
            format!("{} files", touched.len())
        };
        format!("the step changed {what} ({names}) but no diff could be shown")
    } else {
        return None;
    };
    if tests_passed == Some(false) {
        return Some(Verdict {
            ok: false,
            why: format!("the tests failed, and {cause}"),
        });
    }
    let tests = if tests_passed == Some(true) {
        "the tests passed"
    } else {
        "no tests ran"
    };
    Some(Verdict {
        ok: true,
        why: format!("not reviewed: {cause}; {tests}, so it counts as done"),
    })
}

/// Whether every replacement step of the step `key` is done (false when it has none).
pub fn replacements_done(steps: &[StepRun], key: &str) -> bool {
    let mut mine = steps
        .iter()
        .filter(|s| s.replaces.as_deref() == Some(key))
        .peekable();
    mine.peek().is_some() && mine.all(|s| s.status == RunStepStatus::Done)
}

/// The end-of-run report (24f): what was done, and what was skipped or left, with why.
pub fn run_summary(steps: &[StepRun]) -> String {
    let done = steps
        .iter()
        .filter(|s| s.status == RunStepStatus::Done)
        .count();
    let name = |s: &StepRun| {
        s.number
            .as_ref()
            .map_or(s.title.clone(), |n| format!("step {n}"))
    };
    let why = |s: &StepRun| -> Option<String> {
        let w = match s.status {
            RunStepStatus::Held => s.held.as_ref().map(|h| h.why.clone()),
            RunStepStatus::SetAside if s.waits_for.is_some() => s
                .waits_for
                .as_ref()
                .map(|w| format!("waits for {}", w.step)),
            _ => s.reason.clone(),
        };
        w.filter(|w| !w.is_empty())
    };
    let mut parts = vec![format!("{done} done")];
    let kinds = [
        (RunStepStatus::Skipped, "skipped"),
        (RunStepStatus::Blocked, "blocked"),
        (RunStepStatus::Failed, "failed"),
        (RunStepStatus::Held, "waiting for your answer"),
        (RunStepStatus::SetAside, "set aside"),
    ];
    for (kind, word) in kinds {
        let these: Vec<&StepRun> = steps.iter().filter(|s| s.status == kind).collect();
        if !these.is_empty() {
            let list: Vec<String> = these
                .iter()
                .map(|s| match why(s) {
                    Some(w) => format!("{} ({w})", name(s)),
                    None => name(s),
                })
                .collect();
            parts.push(format!("{} {word}: {}", these.len(), list.join("; ")));
        }
    }
    parts.join(" · ")
}

/// The model a step starts on: its own "- Model:" line, else the agent's default.
pub fn start_model(model: Option<&str>, default_model: &str) -> String {
    model
        .map(super::js::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(default_model)
        .to_string()
}

/// A model and how its attempt ended: what the next-model decisions read.
pub trait Tried {
    fn model(&self) -> &str;
    fn outcome(&self) -> Outcome;
}

impl Tried for Attempt {
    fn model(&self) -> &str {
        &self.model
    }
    fn outcome(&self) -> Outcome {
        self.outcome
    }
}

/// The model for the next attempt. Models are listed strongest first. After `per_model` failed attempts on a model
/// the step moves one model up; None means the strongest model has failed too, so the run needs the user.
pub fn next_model<T: Tried>(
    models: &[AgentModel],
    attempts: &[T],
    first: &str,
    per_model: u32,
) -> Option<String> {
    let failed: Vec<&T> = attempts.iter().filter(|a| failed(a.outcome())).collect();
    let Some(last) = failed.last() else {
        return Some(first.into());
    };
    let current = last.model();
    let on_current = failed.iter().filter(|a| a.model() == current).count() as u32;
    if on_current < per_model.max(1) {
        return Some(current.into());
    }
    match models.iter().position(|m| m.id == current) {
        // Already the strongest (or a model outside the list): ask the user.
        None | Some(0) => None,
        Some(i) => Some(models[i - 1].id.clone()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepOutcome {
    Done,
    Blocked,
    Open,
    Missing,
}

/// What the task list says about a step after an attempt. The step is found by its number and title, not its line.
pub fn step_outcome(text: &str, number: Option<&str>, title: &str) -> StepOutcome {
    let plan = parse_plan(text);
    match plan
        .steps
        .iter()
        .find(|s| s.number.as_deref() == number && s.title == title)
    {
        None => StepOutcome::Missing,
        Some(s) if s.status == StepStatus::Done => StepOutcome::Done,
        Some(s) if s.status == StepStatus::Blocked => StepOutcome::Blocked,
        Some(_) => StepOutcome::Open,
    }
}

/// A step's number and title.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepName {
    pub number: Option<String>,
    pub title: String,
}

/// The request for one step: only what the step needs, not the whole history.
pub fn step_prompt(file: &str, step: &Step, done: &[StepName]) -> String {
    let label = match &step.number {
        Some(n) => format!("{n}. {}", step.title),
        None => step.title.clone(),
    };
    let note = if step.note.is_empty() {
        String::new()
    } else {
        format!(" {}", step.note)
    };
    let mut lines = vec![
        format!(
            "You are working through the task list {file} in this project, one step at a time. Your step:"
        ),
        String::new(),
        format!("**{label}**{note}"),
    ];
    lines.extend(step.details.iter().map(|d| format!("  {d}")));
    if !done.is_empty() {
        lines.push(String::new());
        lines.push("Already done in this run (for context only):".into());
        for d in &done[done.len().saturating_sub(10)..] {
            let n = d
                .number
                .as_ref()
                .map(|n| format!("{n}. "))
                .unwrap_or_default();
            lines.push(format!("- {n}{}", d.title));
        }
    }
    lines.push(String::new());
    lines.push("Rules:".into());
    lines.push("- Work only on this step.".into());
    lines.push(format!(
        "- When the step is complete and checked, change its line in {file} to \"- [x]\" and add a short \"  - Done: \
         ...\" line under it saying what you did and how you checked it. Make these changes with your file edit tool, \
         never a shell or regex replace: those can change text such as \"$45\" or \"$0\" in the note."
    ));
    lines.push(
        "- If you cannot finish it, leave it open and say why in your last message. If only the user can unblock it, \
         mark it \"- [!]\" and add the reason under it."
            .into(),
    );
    lines.join("\n")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTotal {
    pub attempts: u32,
    pub cost_usd: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTotals {
    pub cost_usd: f64,
    pub input_tokens: f64,
    pub output_tokens: f64,
    pub seconds: f64,
    /// Per model, in the order first seen (decider and finisher calls under their own keys).
    pub by_model: Vec<(String, ModelTotal)>,
}

fn total_for<'a>(by: &'a mut Vec<(String, ModelTotal)>, key: &str) -> &'a mut ModelTotal {
    let i = match by.iter().position(|(k, _)| k == key) {
        Some(i) => i,
        None => {
            by.push((key.to_string(), ModelTotal::default()));
            by.len() - 1
        }
    };
    &mut by[i].1
}

/// Totals for a run, per model and overall.
pub fn run_totals(
    steps: &[StepRun],
    decider_calls: &[DeciderCall],
    finish: Option<&FinishPass>,
) -> RunTotals {
    let mut t = RunTotals::default();
    for a in steps.iter().flat_map(|s| &s.attempts) {
        t.cost_usd += a.cost_usd;
        t.input_tokens += a.input_tokens;
        t.output_tokens += a.output_tokens;
        t.seconds += a.seconds;
        let m = total_for(&mut t.by_model, &a.model);
        m.attempts += 1;
        m.cost_usd += a.cost_usd;
        m.input_tokens += a.input_tokens;
        m.output_tokens += a.output_tokens;
    }
    for c in decider_calls {
        t.cost_usd += c.cost_usd;
        let m = total_for(&mut t.by_model, &format!("{DECIDER}{}", c.model));
        m.attempts += 1;
        m.cost_usd += c.cost_usd;
    }
    // The finishing pass (24o) counts in the run's cost, on its own line.
    if let Some(f) = finish {
        t.cost_usd += f.cost_usd;
        t.seconds += f.seconds;
        let m = total_for(&mut t.by_model, &format!("{FINISHER}{}", f.model));
        m.attempts += 1;
        m.cost_usd += f.cost_usd;
    }
    t
}

/// The model for a step's next attempt when free models go first: the free agent (as often as one model gets tries),
/// then the paid agent from `paid_first` up, as without free models. None: even the strongest paid model failed.
pub fn next_model_free_first<T: Tried + Clone>(
    models: &[AgentModel],
    attempts: &[T],
    start: &str,
    paid_first: &str,
    per_model: u32,
) -> Option<String> {
    let failed_count = attempts.iter().filter(|a| failed(a.outcome())).count();
    if failed_count == 0 {
        return Some(start.into());
    }
    let paid: Vec<T> = attempts
        .iter()
        .filter(|a| !is_free(a.model()))
        .cloned()
        .collect();
    if paid.is_empty() {
        let on_free = attempts
            .iter()
            .filter(|a| failed(a.outcome()) && is_free(a.model()))
            .count() as u32;
        return Some(
            if on_free < per_model.max(1) {
                start
            } else {
                paid_first
            }
            .into(),
        );
    }
    next_model(models, &paid, paid_first, per_model)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeSavings {
    pub steps: u32,
    pub estimate_usd: Option<f64>,
}

/// What free models saved in a run: the steps they finished, and what those would have cost on the paid agent going
/// by the user's own earlier runs (average cost of a finished step of the same kind; this run's paid steps when there
/// is no history). `estimate_usd` is None when nothing paid has been measured yet.
pub fn free_savings(steps: &[StepRun], paid_per_step: &HashMap<String, f64>) -> FreeSavings {
    let mut count = 0;
    let mut estimate = 0.0;
    let mut known = true;
    // This run's own paid finished steps, as a fallback measure.
    let paid_done: Vec<f64> = steps
        .iter()
        .flat_map(|s| &s.attempts)
        .filter(|a| a.outcome == Outcome::Done && !is_free(&a.model))
        .map(|a| a.cost_usd)
        .collect();
    let run_average = (!paid_done.is_empty())
        .then(|| paid_done.iter().fold(0.0, |t, c| t + c) / paid_done.len() as f64);
    for s in steps {
        let Some(done) = s.attempts.iter().find(|a| a.outcome == Outcome::Done) else {
            continue;
        };
        if !is_free(&done.model) {
            continue;
        }
        count += 1;
        let t = s.job_type.unwrap_or(JobType::Feature).id();
        match paid_per_step.get(t).copied().or(run_average) {
            Some(per) => estimate += per,
            None => known = false,
        }
    }
    FreeSavings {
        steps: count,
        estimate_usd: if count > 0 && known {
            Some(estimate)
        } else if count > 0 {
            None
        } else {
            Some(0.0)
        },
    }
}

/// Average cost of a finished step per kind of job on paid models, from what earlier runs learned.
pub fn paid_per_step(stats: &super::routing::Stats) -> HashMap<String, f64> {
    let mut sums: HashMap<String, (f64, u32)> = HashMap::new();
    for (key, s) in stats {
        let mut parts = key.split('|');
        let (t, model) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        if is_free(model) || s.successes == 0 {
            continue;
        }
        let e = sums.entry(t.to_string()).or_default();
        e.0 += s.cost_usd;
        e.1 += s.successes;
    }
    sums.into_iter()
        .map(|(k, (cost, done))| (k, cost / done as f64))
        .collect()
}

#[cfg(test)]
mod tests;
