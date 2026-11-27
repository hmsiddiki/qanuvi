//! The plan run loop in the core (24z R2c), ported from the window's runner (`start` and the helpers it calls in
//! `src/lib/stores/runner.svelte.ts`): the steps in order, their Needs lines, holds before a step, attempts on the
//! step's model (moving up after failures), handovers, pauses, the step's limits, usage-limit waits, reviews, the
//! planner's rewrite of a step that failed on every model, held and failed steps set aside with a question in the
//! Inbox, sections' closing steps, steps added while the run goes, and what each step leaves for the next ones.
//!
//! The run works through `RunEnv` only, so the same loop runs on the desktop, in qanuvi-master and in the scenario
//! tests. The owner reaches a running loop through `Shared`: Stop, Pause, answers to held steps, the step's model and
//! the run's mode (the last two through `Controls`, read by the agent turn).

use super::costs::{StepLimits, tool_kinds};
use super::decider::{
    self, Assignment, DecisionRecord, Priority, StepBrief, TeamMode, TeamSettings, Worker,
};
use super::env::{MemoryUse, PlannerAsk, RunEnv, ask_planner, assistant_text};
use super::finishing::is_plan_file;
use super::guidance::{self as g, Carry, Recent};
use super::health::{self, Down};
use super::holdask::{
    self, HoldReply, InboxAnswer, hold_form, hold_key, hold_reply, sentence_start,
};
use super::housekeeping::{archive_path, done_numbers};
use super::js;
use super::limits::{limit_notice, limit_reset_at};
use super::ownerask;
use super::ownermsg;
use super::packet::{self, Outline, packet_prompt};
use super::planner;
use super::planstate::{self, PlanState};
use super::preflight::{self, waits_before};
use super::projectfacts;
use super::routing::{
    AgentModel, Budget, BudgetState, ChooseOptions, JobType, Picked, Stats, budget_state,
    choose_model, job_type, learn,
};
use super::runner::*;
use super::setaside::{self, AsideItem, DepStep, HeldKind, HeldStep, is_closing_step};
use super::tasklist::{
    Step, StepStatus, clear_stale_skips, parse_plan, set_step_model, set_step_skipped,
    set_step_status,
};
use super::turn::{self, AttemptAsk, Controls, StepWatch, Totals, TurnSetup};
use crate::agent::AgentEvent;
use crate::inbox::NewItem;
use crate::plan_data::Kind;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// What a run starts with: the plan, its options and what the window knows of the agent and the settings.
#[derive(Clone, Debug)]
pub struct RunSetup {
    /// The project's name, for the Inbox titles.
    pub project_name: String,
    pub file: String,
    pub options: RunOptions,
    /// The run's agent ("claude-code", "qanuvi", ...) and its name as the owner sees it.
    pub provider: String,
    pub provider_name: String,
    /// The agent's models, strongest first.
    pub models: Vec<AgentModel>,
    /// The agent's default model (Settings), and the planner's.
    pub default_model: String,
    pub planner_model: String,
    /// Settings > Team (the run's own Team mode, when it has one, wins).
    pub team: TeamSettings,
    /// Free models are set up (Settings, Models and providers).
    pub has_free: bool,
    pub monthly_budget_usd: f64,
    /// The project's model rules per kind of job.
    pub rules: HashMap<JobType, String>,
    /// What every turn is sent with (effort, base URL, setups, memory, model labels).
    pub turns: TurnSetup,
    /// Go on in a step's session (Resume after a pause, or the owner's go-ahead in a stopped step's chat).
    pub resume: Option<ResumeAt>,
}

/// A session the run's first step continues, and what it is told first.
#[derive(Clone, Debug, Default)]
pub struct ResumeAt {
    pub session_id: String,
    pub model: String,
    pub section: String,
    /// The session's totals so far (the step counts only its own share).
    pub base: Totals,
    /// The owner's words (said in the chat), or Qanuvi's resume note.
    pub text: String,
    /// `text` is Qanuvi's note (a pause's resume), not the owner's words.
    pub is_note: bool,
}

pub type OnRun = Arc<dyn Fn(&PlanRun) + Send + Sync>;
pub type OnEvent = Arc<dyn Fn(&AgentEvent) + Send + Sync>;

#[derive(Default)]
struct Live {
    run: Option<PlanRun>,
    waiting_answers: usize,
    waiting_until: Option<i64>,
    /// Y7: what is down while the run waits for it ("the proxy at 127.0.0.1:8787 is not answering").
    waiting_health: Option<String>,
    current_model: String,
    answers: Vec<(String, HoldReply)>,
    finish_without: bool,
    notices: Vec<String>,
}

/// A running loop as the owner reaches it (the commands in R2d): what it shows, and what it is told.
pub struct Shared {
    pub controls: Controls,
    live: Mutex<Live>,
    wake: Condvar,
    /// How often a run that waits for answers looks in the Inbox.
    pub poll: Duration,
    on_run: Mutex<Option<OnRun>>,
    on_event: Mutex<Option<OnEvent>>,
}

impl Default for Shared {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

impl Shared {
    pub fn new(poll: Duration) -> Self {
        Shared {
            controls: Controls::default(),
            live: Mutex::new(Live::default()),
            wake: Condvar::new(),
            poll,
            on_run: Mutex::new(None),
            on_event: Mutex::new(None),
        }
    }

    fn live(&self) -> std::sync::MutexGuard<'_, Live> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Who hears the run's changes and its steps' agent events (the window, a master's stream).
    pub fn listen(&self, on_run: Option<OnRun>, on_event: Option<OnEvent>) {
        *self.on_run.lock().unwrap_or_else(|e| e.into_inner()) = on_run;
        *self.on_event.lock().unwrap_or_else(|e| e.into_inner()) = on_event;
    }

    /// The run as it is now.
    pub fn run(&self) -> Option<PlanRun> {
        self.live().run.clone()
    }

    /// Only held steps are left and the run waits for this many answers.
    pub fn waiting_answers(&self) -> usize {
        self.live().waiting_answers
    }

    /// The run waits for a usage limit to reset at this moment (milliseconds).
    pub fn waiting_until(&self) -> Option<i64> {
        self.live().waiting_until
    }

    /// Y7: what is down while the run waits for it.
    pub fn waiting_health(&self) -> Option<String> {
        self.live().waiting_health.clone()
    }

    /// The model of the attempt at work.
    pub fn current_model(&self) -> String {
        self.live().current_model.clone()
    }

    /// What the run told the owner on the way (a handover, free models while the paid agent's limit resets).
    pub fn notices(&self) -> Vec<String> {
        self.live().notices.clone()
    }

    /// The owner answers a held step (its number, else its title), from the window; Inbox answers are read by the run.
    pub fn answer(&self, step: &str, choice: holdask::AnswerChoice, text: &str) {
        let reply = hold_reply(&InboxAnswer {
            choice,
            text: Some(text.to_string()),
        });
        self.live().answers.push((step.to_string(), reply));
        self.wake.notify_all();
    }

    /// Ends a run that waits for answers: what is set aside stays open, the summary lists it.
    pub fn finish_without(&self) {
        self.live().finish_without = true;
        self.wake.notify_all();
    }

    /// The owner typed a message into the running step's chat: the step's turn sorts it before the agent hears it
    /// (R2c part 2). One typed while no step works waits for the next step's turn.
    pub fn owner_message(&self, text: &str) {
        self.controls.owner_message(text);
        self.wake.notify_all();
    }

    /// Pause: a step at work ends the edit in hand and writes where it is; a waiting run pauses at once.
    pub fn pause(&self) {
        self.controls.pause();
        self.wake.notify_all();
    }

    /// Stop: gently while a step's agent works, at once otherwise or the second time.
    pub fn stop(&self) {
        self.controls.stop();
        self.wake.notify_all();
    }

    /// The run changed: kept as the one shown, and told to whoever listens.
    pub(crate) fn publish(&self, run: &PlanRun) {
        self.live().run = Some(run.clone());
        self.tell(run);
    }

    fn tell(&self, run: &PlanRun) {
        let hook = self
            .on_run
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(f) = hook {
            f(run);
        }
    }

    /// Something around the run changed (its waits, the model at work, a notice): listeners hear the run again.
    pub(crate) fn touch(&self) {
        let run = self.live().run.clone();
        if let Some(r) = run {
            self.tell(&r);
        }
    }

    pub(crate) fn set_current_model(&self, model: &str) {
        self.live().current_model = model.to_string();
        self.touch();
    }

    /// A run (or a finishing pass) starts on this handle: what the last one left is cleared.
    pub(crate) fn begin(&self) {
        self.controls.reset();
        let mut live = self.live();
        live.waiting_answers = 0;
        live.waiting_until = None;
        live.waiting_health = None;
        live.answers.clear();
        live.finish_without = false;
        live.notices.clear();
        live.current_model.clear();
    }

    pub(crate) fn forward(&self, e: &AgentEvent) {
        let hook = self
            .on_event
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(f) = hook {
            f(e);
        }
    }

    fn notice(&self, text: String) {
        self.live().notices.push(text);
        self.touch();
    }

    /// Waits up to `poll` for a wake-up (an answer, Pause, Stop, End the run here).
    fn nap(&self) {
        let live = self.live();
        let _ = self
            .wake
            .wait_timeout(live, self.poll)
            .unwrap_or_else(|e| e.into_inner());
    }
}

/// How long a waiting run gives a question left open in the check before Qanuvi decides it (0: no limit).
pub const ANSWER_WAIT_DEFAULT_MINUTES: f64 = 60.0;

fn answer_wait_ms(o: &RunOptions) -> i64 {
    let m = o.answer_wait_minutes.unwrap_or(ANSWER_WAIT_DEFAULT_MINUTES);
    if m > 0.0 { (m * 60_000.0) as i64 } else { 0 }
}

/// What the run tells a step that resumes after a pause.
pub const PAUSE_RESUME: &str = "You paused this step at the owner's request and the run now resumes: go on from where you stopped (your Progress note is in the step's log).";

const STOPPED: &str = "Stopped by you.";

/// The start of the list-to-look-at entry for a skipped real end-to-end check.
const SKIPPED_END_CHECK: &str =
    "You skipped the real end-to-end check, so the plan's work is not proven for real";

/// "You skipped the real end-to-end check, ...: <what it was to do>" from the hold's reason.
fn skipped_end_check_words(why: &str) -> String {
    let rest = why
        .strip_prefix(super::preflight::END_TO_END)
        .unwrap_or(why)
        .trim_start_matches([':', ' '])
        .trim();
    let rest: String = rest.chars().take(240).collect();
    if rest.is_empty() {
        SKIPPED_END_CHECK.to_string()
    } else {
        format!("{SKIPPED_END_CHECK}: {rest}")
    }
}

fn key(s: &StepRun) -> String {
    s.number.clone().unwrap_or_else(|| s.title.clone())
}

fn find(text: &str, number: Option<&str>, title: &str) -> Option<Step> {
    parse_plan(text)
        .steps
        .into_iter()
        .find(|s| s.number.as_deref() == number && s.title == title)
}

pub(crate) fn status_word(s: RunStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn step_word(s: RunStepStatus) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn outcome_word(o: Outcome) -> String {
    serde_json::to_value(o)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

fn folder_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) if i > 0 => path[..i].to_string(),
        _ => String::new(),
    }
}

/// The agent's text answers in a session's lines, in order.
fn texts_of(lines: &[String]) -> Vec<String> {
    lines.iter().filter_map(|l| assistant_text(l)).collect()
}

/// A step as the decider reads it.
fn brief_of(s: &Step) -> StepBrief {
    StepBrief {
        number: s.number.clone().unwrap_or_else(|| s.line.to_string()),
        title: s.title.clone(),
        details: s.details.clone(),
        kind: job_type(&[s.title.clone(), s.note.clone(), s.details.join("\n")].join("\n"))
            .id()
            .to_string(),
    }
}

/// Qanuvi's snapshot before some work, to see what it changed.
#[derive(Clone, Debug)]
struct Snap {
    id: String,
    also: Vec<String>,
}

/// What a review read.
struct Changed {
    parts: Vec<String>,
    unseen: Vec<String>,
    error: Option<String>,
}

/// The running step's model switch: the model, and the attempts from which it counts.
struct LiveModel {
    step: usize,
    model: String,
    from: usize,
}

struct Loop<'a> {
    env: &'a dyn RunEnv,
    sh: &'a Shared,
    s: RunSetup,
    run: PlanRun,
    /// The steps the run knows from the plan (as at the start, plus added and replacement steps).
    steps: Vec<Step>,
    cursor: usize,
    done: Vec<StepName>,
    done_numbers: HashSet<String>,
    known: HashSet<String>,
    reopened: HashSet<String>,
    plan_refs: Vec<String>,
    wait_list: Vec<preflight::StepWhy>,
    extra_checks: HashMap<String, String>,
    /// Y8: the plan file's own "Test build" section (it wins over the project's recipe for this plan).
    plan_test_build: Option<crate::test_build::TestBuild>,
    /// Y10 (a): work a done step left for later that no open step does: the run does not end "finished" with it.
    left_open: Vec<String>,
    assignment: HashMap<String, Assignment>,
    stats: Stats,
    free_first: bool,
    can_free: bool,
    one_model: String,
    spent_before: f64,
    live: Option<LiveModel>,
    /// The session the next step may continue, what it is, and its totals when the last attempt ended.
    carry: Option<(String, Carry, Totals)>,
    last_idle: bool,
    last_totals: Totals,
    resume_text: String,
    resume_is_handover: bool,
    /// Each reviewed step's snapshot from its first try: a review sees the whole step's work, also what a session
    /// before a handover, a model switch, a pause or a failed try did (not only the last session's).
    step_snaps: HashMap<String, Snap>,
    /// The plan's own git branch, while the run commits finished steps there (24y Y3).
    branch: Option<String>,
    keep_for_resume: bool,
    free_until: Option<i64>,
    /// Y7: the step meeting outages now, how many in a row, and since when (milliseconds).
    outage: Option<(String, u32, i64)>,
    /// Y7: the run said what is down in this outage already (it says so once).
    outage_said: bool,
    asked_changes_from: bool,
    answered: bool,
    watch: StepWatch,
}

/// Runs a plan to its end (done, stopped, paused or needing the owner) and answers the run. Errors only when it
/// cannot start (the plan cannot be read, a step range is not one).
pub fn run_plan(env: &dyn RunEnv, sh: &Shared, setup: RunSetup) -> Result<PlanRun, String> {
    let mut l = Loop::start(env, sh, setup)?;
    l.steps_loop();
    l.end();
    Ok(l.run)
}

impl<'a> Loop<'a> {
    fn start(env: &'a dyn RunEnv, sh: &'a Shared, mut s: RunSetup) -> Result<Loop<'a>, String> {
        // An agent that is not on this machine: no step tries its models one after another, the run says why.
        if let Some(why) = env.agent_missing(&s.provider) {
            return Err(why);
        }
        if let Some(m) = s.options.message_wait_minutes.filter(|m| *m > 0.0) {
            s.turns.message_wait_ms = (m * 60_000.0) as i64;
        }
        let file = s.file.clone();
        let options = s.options.clone();
        let mut text = env.read(&file)?.text;
        let ranges = parse_ranges(options.ranges.as_deref());
        if !ranges.error.is_empty() {
            return Err(ranges.error);
        }
        let steps = steps_to_run(
            &parse_plan(&text).steps,
            options.from_line,
            options.only_one,
            &ranges.ranges,
        );
        // What is done already, to check each step's Needs line; steps moved to the plan's archive count.
        let archive = env.read(&archive_path(&file)).ok().map(|f| f.text);
        let archived = done_numbers(&[archive.as_deref()]);
        // Skip lines from earlier runs whose needed steps are done now go first (they would say the wrong thing).
        let cleared = clear_stale_skips(&text, &archived);
        if cleared != text
            && let Ok(now) = env.read(&file)
            && now.text == text
            && env.write(&file, &cleared, Some(&now.hash)).is_ok()
        {
            text = cleared;
        }
        let mut done_set: HashSet<String> = archived.into_iter().collect();
        for st in parse_plan(&text).steps {
            if st.status == StepStatus::Done
                && let Some(n) = st.number
            {
                done_set.insert(n);
            }
        }
        sh.begin();
        let now = env.now_ms();
        let run = PlanRun {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: env.project_id().to_string(),
            file: file.clone(),
            provider: s.provider.clone(),
            options: options.clone(),
            steps: steps
                .iter()
                .map(|st| StepRun {
                    line: st.line,
                    number: st.number.clone(),
                    title: st.title.clone(),
                    attempts: vec![],
                    status: RunStepStatus::Waiting,
                    held: None,
                    waits_for: None,
                    answer: None,
                    talk: None,
                    instruction: None,
                    retry_from: None,
                    rerun_after: None,
                    job_type: None,
                    choice: None,
                    replaced_by: None,
                    replaces: None,
                    changed: None,
                    reason: None,
                })
                .collect(),
            status: if steps.is_empty() {
                RunStatus::Done
            } else {
                RunStatus::Running
            },
            message: if steps.is_empty() {
                // pm1: a step range that matches no open step says so, not just "nothing to run".
                match options.ranges.as_deref().map(str::trim) {
                    Some(r) if !r.is_empty() && !ranges.ranges.is_empty() => {
                        format!("No open steps to run: none of steps {r} is open in this plan.")
                    }
                    _ => "No open steps to run.".into(),
                }
            } else {
                String::new()
            },
            budget_reached: None,
            stopped_by: None,
            started_at: now as f64,
            ended_at: steps.is_empty().then_some(now as f64),
            paused_session: None,
            decisions: None,
            notes: None,
            decider_calls: None,
            snapshot: None,
            finish: None,
            branch: None,
            commits: None,
            report: None,
            review: None,
            test_hint: None,
        };
        let known = parse_plan(&text)
            .steps
            .iter()
            .map(|st| g::step_key(st.number.as_deref(), &st.title))
            .collect();
        let resume = s.resume.clone();
        let mut l = Loop {
            env,
            sh,
            run,
            steps,
            cursor: 0,
            done: vec![],
            done_numbers: done_set,
            known,
            reopened: HashSet::new(),
            plan_refs: vec![],
            wait_list: vec![],
            extra_checks: HashMap::new(),
            plan_test_build: None,
            left_open: Vec::new(),
            assignment: HashMap::new(),
            stats: Stats::new(),
            free_first: false,
            can_free: false,
            one_model: options.model.as_deref().unwrap_or("").trim().to_string(),
            spent_before: 0.0,
            live: None,
            carry: None,
            last_idle: false,
            last_totals: Totals::default(),
            resume_text: String::new(),
            resume_is_handover: false,
            step_snaps: HashMap::new(),
            branch: None,
            keep_for_resume: false,
            free_until: None,
            outage: None,
            outage_said: false,
            asked_changes_from: false,
            answered: false,
            watch: StepWatch::default(),
            s,
        };
        if let Some(r) = resume {
            l.carry = Some((
                r.session_id,
                Carry {
                    model: r.model,
                    section: r.section,
                    tokens: 0,
                    steps: 0,
                },
                r.base,
            ));
            l.resume_text = r.text;
            l.resume_is_handover = r.is_note;
        }
        if l.steps.is_empty() {
            l.save();
            return Ok(l);
        }
        l.prepare(&text);
        Ok(l)
    }

    /// Before the first step: the start snapshot, the plan's notes and kept state, the decider's assignment.
    fn prepare(&mut self, text: &str) {
        let file = self.s.file.clone();
        // 24y: the run's sessions are answered by the run rules (the project's, the plan file's section winning).
        self.s.turns.ruled = true;
        self.s.turns.plan_rules = crate::run_rules::plan_rules(text);
        self.plan_test_build = crate::test_build::plan_test_build(text);
        // Y3: the user said yes to the plan's own branch (on the card, or the plan's "Branch: yes"): the work goes
        // there, a commit per finished step. Before the start snapshot: making the branch changes no file.
        let wants_branch = self
            .env
            .project_settings()
            .is_ok_and(|p| p.run_rules.branch)
            || self.s.turns.plan_rules.as_ref().is_some_and(|r| r.branch);
        if wants_branch {
            let name = crate::git::plan_branch_name(&file);
            match self.env.plan_branch(&name) {
                Ok(done) => {
                    self.branch = Some(name.clone());
                    self.run.branch = Some(name);
                    self.decide(
                        "decided",
                        "Qanuvi",
                        "Git branch",
                        &done,
                        "you chose to keep the plan's work on its own branch: each finished step is committed there, your main branch is never changed, and merging stays yours",
                    );
                }
                Err(e) => self.decide(
                    "decided",
                    "Qanuvi",
                    "Git branch",
                    &format!("no branch: {e}"),
                    "Qanuvi's own copies undo a failed step instead; nothing goes into git",
                ),
            }
        }
        // The finishing pass compares with this, so changes from before the run are not taken for the run's (24o).
        let start = self.snapshot();
        if let Some(sn) = &start {
            self.run.snapshot = Some(RunSnapshot {
                id: sn.0.id.clone(),
                also: sn.0.also.clone(),
            });
        }
        self.check_changes_seen(start.as_ref());
        // The plan's notes from earlier runs: the owner's instructions hold for every run of this plan.
        self.run.notes = Some(vec![]);
        let kept_notes = self.load_notes();
        if !kept_notes.is_empty() {
            let lines: String = kept_notes.iter().map(|n| format!("- {n}\n")).collect();
            let _ = self.env.log_append(&g::facts_path(&file), &lines);
            let _ = self.env.plan_data_set(&file, Kind::Notes, &json!([]));
        }
        // What this plan kept from earlier runs (24u part 2).
        self.wait_list = self
            .s
            .options
            .wait_before
            .iter()
            .flatten()
            .map(|w| preflight::StepWhy {
                step: w.step.clone(),
                why: w.why.clone(),
            })
            .collect();
        self.extra_checks = self
            .s
            .options
            .extra_checks
            .iter()
            .flatten()
            .map(|c| (c.step.clone(), c.why.clone()))
            .collect();
        {
            let planned = parse_plan(text).steps;
            let open: HashSet<String> = planned
                .iter()
                .filter(|st| st.status != StepStatus::Done)
                .map(|st| st.number.clone().unwrap_or_else(|| st.title.clone()))
                .collect();
            let all: HashSet<String> = planned
                .iter()
                .map(|st| st.number.clone().unwrap_or_else(|| st.title.clone()))
                .collect();
            let mut state = self.load_state();
            // An answer given in the Inbox while no run went on counts as given (the window used to drop it).
            for k in &open {
                if let Some((reply, _)) = self
                    .inbox_answer(k)
                    .filter(|(r, _)| r.choice != holdask::AnswerChoice::End)
                {
                    state = planstate::keep_answer(
                        &state,
                        k,
                        plan_choice(reply.choice),
                        &reply.text,
                        self.env.now_ms() as f64,
                    );
                }
            }
            let mut kept = planstate::prune_state(&state, &open, &all);
            // lr2: a kept "it needs the program X" (address, secret) hold is tried again: gone once X is there.
            kept.holds
                .retain(|h| super::check_more::still_missing(self.env, &h.why));
            self.save_state(&kept);
            // Questions of this plan whose hold is gone (the step got done, or it is tried again now) close.
            let still: HashSet<&str> = kept.holds.iter().map(|h| h.step.as_str()).collect();
            for k in &all {
                if !still.contains(k.as_str()) {
                    let _ = self.env.inbox_resolve(&self.hold_key(k));
                }
            }
            for h in &kept.holds {
                if !self.wait_list.iter().any(|w| w.step == h.step) {
                    self.wait_list.push(preflight::StepWhy {
                        step: h.step.clone(),
                        why: planstate::hold_wording(&h.why, h.by),
                    });
                }
            }
            for c in &kept.checks {
                self.extra_checks
                    .entry(c.step.clone())
                    .or_insert_with(|| c.why.clone());
            }
            for d in &kept.decided {
                if !open.contains(&d.decided.step) {
                    continue;
                }
                let dd = &d.decided;
                let (decision, why) = match d.changed.as_deref().filter(|c| !c.is_empty()) {
                    Some(c) => (
                        format!("{} → {c} (you changed it from {})", dd.what, dd.answer),
                        "your answer".to_string(),
                    ),
                    None => (
                        format!("{} → {}", dd.what, dd.answer),
                        if dd.because.is_empty() {
                            "the check's recommended answer".to_string()
                        } else {
                            dd.because.clone()
                        },
                    ),
                };
                self.decide(
                    "decided",
                    "Qanuvi",
                    &format!("Step {}", dd.step),
                    &decision,
                    &why,
                );
            }
            for a in &kept.answers {
                let Some(st) = self.run.steps.iter_mut().find(|x| key(x) == a.step) else {
                    continue;
                };
                if a.choice == planstate::AnswerChoice::Skip {
                    st.status = RunStepStatus::Skipped;
                    st.reason = Some("you chose to skip it (in an earlier run)".into());
                } else {
                    st.answer = Some(a.text.clone());
                    st.held = Some(Held {
                        kind: HeldKind::Hold,
                        why: "it was held in an earlier run".into(),
                        card: None,
                    });
                }
            }
        }
        self.save();

        // What earlier runs (in every project) showed, and what this month has cost so far.
        let runs: Vec<PlanRun> = self
            .env
            .list_all_runs(300)
            .unwrap_or_default()
            .iter()
            .filter_map(|r| serde_json::from_str(&r.data).ok())
            .collect();
        self.stats = learn(runs.iter().map(|r| r.steps.as_slice()), job_type);
        let mut team = self.s.team;
        if let Some(m) = self.s.options.team_mode {
            team.mode = m;
        }
        self.can_free = self.s.provider != "qanuvi" && self.s.has_free;
        self.free_first = match self.s.options.team {
            Some(_) => free_first_for(
                self.s.options.team,
                decider::free_works(team, true, true),
                self.can_free,
            ),
            None => {
                self.s.options.free_first == Some(true)
                    && self.can_free
                    && decider::free_works(team, true, true)
            }
        };
        if free_wanted_but_missing(
            self.s.options.team,
            team.mode,
            self.s.has_free,
            self.s.provider == "qanuvi",
        ) {
            let name = self.s.provider_name.clone();
            self.decide(
                "assign",
                "Qanuvi",
                "Team",
                &format!("No free models set up: every step on {name}"),
                "The Team mode works with free models, and Settings, Models and providers has none yet.",
            );
        }
        // With free models first, the decider says once, for the whole plan, which steps they do (24b). Models only
        // come from the picks the owner asked for before the run ("Pick models").
        let briefs: Vec<StepBrief> = self.steps.iter().map(brief_of).collect();
        if self.free_first {
            let priority = if team.mode == TeamMode::Priority {
                team.priority
            } else {
                Priority::Save
            };
            let (list, by) = self.ask_assignment(&briefs, priority);
            for a in list {
                let worker = match a.worker {
                    Worker::Free => "free",
                    Worker::Paid => "paid",
                };
                self.decide("assign", &by, &format!("Step {}", a.number), worker, &a.why);
                self.assignment.insert(a.number.clone(), a);
            }
        }
        let saved: Option<super::picks::SavedPicks> = if self.one_model.is_empty() {
            self.env
                .plan_data_get(&file, Kind::Picks)
                .and_then(|v| serde_json::from_value(v).ok())
        } else {
            None
        };
        for b in &briefs {
            let Some(p) =
                super::picks::pick_for(saved.as_ref(), &b.number, &b.title, &self.s.provider)
            else {
                continue;
            };
            let Some(model) = p.model.clone() else {
                continue;
            };
            let worker = self
                .assignment
                .get(&b.number)
                .map_or(Worker::Paid, |a| a.worker);
            let why = p.why.clone();
            self.assignment.insert(
                b.number.clone(),
                Assignment {
                    number: b.number.clone(),
                    worker,
                    why: why.clone(),
                    model: Some(model.clone()),
                },
            );
            let label = self.label(&model);
            self.decide(
                "assign",
                "decider (Pick models, before the run)",
                &format!("Step {}", b.number),
                &label,
                &why,
            );
        }
        self.save();
        let now = jiff::Timestamp::from_millisecond(self.env.now_ms())
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned(jiff::tz::TimeZone::system());
        self.spent_before = self
            .env
            .usage_since(super::routing::month_start(&now))
            .map(|u| u.cost_usd)
            .unwrap_or(0.0);
        // Plans this one builds on (needed or named) whose knowledge exists: every step is told where it is.
        for p in g::referenced_plans(text, &file) {
            let dir = g::log_dir(&p);
            if self.env.read(&format!("{dir}/MAP.md")).is_ok()
                || self.env.read(&format!("{dir}/FACTS.md")).is_ok()
            {
                self.plan_refs.push(p);
            }
        }
    }

    // ---------- Small helpers ----------

    /// A model's name as the owner sees it: the window's label, else the agent's model list, else its id.
    fn label(&self, model: &str) -> String {
        self.s
            .turns
            .labels
            .get(model)
            .cloned()
            .or_else(|| {
                self.s
                    .models
                    .iter()
                    .find(|m| m.id == model && !m.label.is_empty())
                    .map(|m| m.label.clone())
            })
            .unwrap_or_else(|| model.to_string())
    }

    fn plan(&self) -> Option<super::env::FileText> {
        self.env.read(&self.s.file).ok()
    }

    fn hold_key(&self, step: &str) -> String {
        hold_key(self.env.project_id(), &self.s.file, step)
    }

    fn memory(&self) -> MemoryUse {
        self.s.turns.memory
    }

    fn load_state(&self) -> PlanState {
        self.env
            .plan_data_get(&self.s.file, Kind::State)
            .map(|v| PlanState::from_json(&v))
            .unwrap_or_default()
    }

    fn save_state(&self, state: &PlanState) {
        if let Ok(v) = serde_json::to_value(state) {
            let _ = self.env.plan_data_set(&self.s.file, Kind::State, &v);
        }
    }

    fn load_notes(&self) -> Vec<String> {
        match self.env.plan_data_get(&self.s.file, Kind::Notes) {
            Some(Value::Array(a)) => a
                .into_iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
            _ => vec![],
        }
    }

    fn save(&self) {
        let r = &self.run;
        if let Ok(data) = serde_json::to_string(r) {
            // Keeping the record is helpful, not essential; the run goes on.
            let _ = self
                .env
                .save_run(&r.id, &r.file, &status_word(r.status), &data);
        }
        self.sh.publish(r);
    }

    fn decide(&mut self, kind: &str, by: &str, subject: &str, decision: &str, why: &str) {
        let at = self.env.now_ms() as f64;
        self.run
            .decisions
            .get_or_insert_with(Vec::new)
            .push(DecisionRecord {
                at,
                kind: kind.into(),
                by: by.into(),
                subject: subject.into(),
                decision: decision.into(),
                why: why.into(),
            });
    }

    /// wd1: the owner allowed a folder outside the project: it goes into the project's run rules before the step
    /// runs again, so the write guard lets it through this time and in later runs (listed in the decisions).
    fn allow_write_dir(&mut self, dir: &str) {
        let Ok(mut ps) = self.env.project_settings() else {
            return;
        };
        if ps
            .run_rules
            .write_dirs
            .iter()
            .any(|d| d.eq_ignore_ascii_case(dir))
        {
            return;
        }
        ps.run_rules.write_dirs.push(dir.to_string());
        if self.env.set_project_settings(&ps).is_ok() {
            self.decide(
                "rules",
                "Qanuvi",
                "Run rules: may write in",
                dir,
                "your answer allowed this folder outside the project, so the step's next try may write there",
            );
        }
    }

    /// PM a: what the owner said about the project goes into This project memory at once (it is theirs); a site
    /// address also fills Preview address when that is empty.
    fn keep_owner_fact(&mut self, title: &str, text: &str) {
        let kept = self.env.memory_keep(title, text).is_ok();
        if let Some(url) = projectfacts::first_url(text)
            && let Ok(mut ps) = self.env.project_settings()
            && ps.preview_url.trim().is_empty()
        {
            ps.preview_url = url.clone();
            if self.env.set_project_settings(&ps).is_ok() {
                self.decide(
                    "memory",
                    "Qanuvi",
                    "Preview address",
                    &url,
                    "your answer named the site to test on, and the project had no preview address",
                );
            }
        }
        if kept {
            self.decide(
                "memory",
                "Qanuvi",
                "This project memory",
                &format!("{title}: {text}"),
                "your answer is about the project, not only this step, so every later plan and check gets it",
            );
        }
    }

    /// PM b + e: at the run's end, the project facts its notes hold become memory suggestions, kept with one click
    /// from the Inbox; the first time in a project, every plan's notes are read the same way. The test command the
    /// run used is kept for the finishing card when the project has none.
    fn learn_project_facts(&mut self) {
        let Ok(mut ps) = self.env.project_settings() else {
            return;
        };
        let mut files = vec![g::facts_path(&self.s.file)];
        let first = !ps.memory_read;
        if first {
            for f in self.env.plan_fact_files() {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        let notes: String = files
            .iter()
            .filter_map(|f| self.env.read(f).ok().map(|t| (f, t.text)))
            .filter(|(_, t)| !js::trim(t).is_empty())
            .map(|(f, t)| format!("From {f}:\n{t}\n"))
            .collect();
        if js::trim(&notes).is_empty() {
            return;
        }
        let name = if self.s.project_name.is_empty() {
            "this project".to_string()
        } else {
            self.s.project_name.clone()
        };
        let prompt =
            projectfacts::prompt(&name, &notes, &self.env.project_memory(), &ps.test_command);
        // Free models first (a small reading job); the run's own model when there are none.
        let answer = match self.free_decide(projectfacts::SYSTEM, &prompt) {
            Ok((_, t)) => Some(t),
            Err(_) => {
                let model = if self.s.default_model.is_empty() {
                    self.s.planner_model.clone()
                } else {
                    self.s.default_model.clone()
                };
                match self.ask(
                    &prompt,
                    &model,
                    "What the run learned about the project",
                    true,
                ) {
                    Ok((t, cost)) => {
                        self.decider_call(&model, cost);
                        Some(t)
                    }
                    Err(_) => None,
                }
            }
        };
        let Some(learned) = answer.as_deref().and_then(projectfacts::parse) else {
            return;
        };
        if first {
            ps.memory_read = true;
            let _ = self.env.set_project_settings(&ps);
        }
        if ps.test_command.trim().is_empty() && !learned.test_command.is_empty() {
            self.run.test_hint = Some(learned.test_command.clone());
        }
        let mut kept: Vec<(String, String)> = vec![];
        let mut ids: Vec<String> = vec![];
        for (t, b) in &learned.facts {
            if let Some(id) = self.env.memory_suggest(t, b) {
                ids.push(id);
                kept.push((t.clone(), b.clone()));
            }
        }
        if ids.is_empty() {
            return;
        }
        let pid = self.env.project_id().to_string();
        let project = if self.s.project_name.is_empty() {
            String::new()
        } else {
            format!("{}: ", self.s.project_name)
        };
        let _ = self.env.inbox_add(&NewItem {
            kind_of: "answer".into(),
            kind: "knowledge".into(),
            title: format!(
                "{project}{} {} learned about the project",
                ids.len(),
                if ids.len() == 1 { "thing" } else { "things" }
            ),
            text: projectfacts::inbox_text(&kept),
            key: Some(format!("memory:{pid}:{}", self.run.id)),
            open: Some(json!({"label": "Look first", "view": "memory", "projectId": pid})),
            form: Some(json!({"choices": [
                {"id": "keep", "label": "Keep all"},
                {"id": "later", "label": "Not now"}
            ]})),
            source: Some(json!({"kind": "memory", "projectId": pid, "memoryIds": ids})),
        });
        self.decide(
            "memory",
            "Qanuvi",
            "This project memory",
            &format!("{} suggested", ids.len()),
            if first {
                "facts about the project from this run's notes and the earlier plans' notes; they wait for your yes in the Inbox"
            } else {
                "facts about the project from this run's notes; they wait for your yes in the Inbox"
            },
        );
    }

    fn open_plan(&self) -> Value {
        json!({"label": "Open the plan", "view": "plan", "projectId": self.env.project_id()})
    }

    fn plan_name(&self) -> String {
        self.s
            .file
            .rsplit('/')
            .next()
            .unwrap_or(&self.s.file)
            .to_string()
    }

    fn finish(&mut self, status: RunStatus, message: &str) {
        self.run.status = status;
        self.run.message = message.to_string();
        self.run.ended_at = Some(self.env.now_ms() as f64);
        // Y5: the end report (done; set aside and why; waiting for you, with what to do), in the Run panel and below.
        let report = run_report(&self.run);
        self.run.report = Some(report.clone());
        if message != STOPPED {
            let done = self
                .run
                .steps
                .iter()
                .filter(|st| st.status == RunStepStatus::Done)
                .count();
            let what = match status {
                RunStatus::Done => "finished",
                RunStatus::NeedsYou => "needs you",
                RunStatus::Paused => "paused",
                _ => "stopped",
            };
            let project = if self.s.project_name.is_empty() {
                String::new()
            } else {
                format!("{}: ", self.s.project_name)
            };
            let text = vec![message.to_string(), report_text(&report)];
            let _ = self.env.inbox_add(&NewItem {
                kind_of: "info".into(),
                kind: "run".into(),
                title: format!(
                    "{project}{} {what} · {done}/{} steps done",
                    self.plan_name(),
                    self.run.steps.len()
                ),
                text: text
                    .into_iter()
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                key: None,
                open: Some(self.open_plan()),
                form: None,
                source: None,
            });
        }
    }

    fn stop_asked(&self) -> bool {
        self.sh.controls.stop_asked().is_some()
    }

    fn pausing(&self) -> bool {
        self.sh.controls.pausing()
    }

    // ---------- Snapshots and reviews ----------

    /// Qanuvi's snapshot of the project's files now (git or not; the plan's folder is followed even where ignore
    /// rules leave it out), with the folders git cannot see.
    fn snapshot(&self) -> Option<(Snap, crate::snapshots::Taken)> {
        let also: Vec<String> = vec![folder_of(&self.s.file)]
            .into_iter()
            .filter(|f| !f.is_empty())
            .collect();
        let t = self.env.snapshot_take(&also).ok()?;
        Some((
            Snap {
                id: t.id.clone(),
                also,
            },
            t,
        ))
    }

    fn wants_git(&self) -> bool {
        self.env
            .project_settings()
            .is_ok_and(|p| p.changes_from == "git")
    }

    fn changes_since(&self, before: Option<&Snap>) -> Changed {
        let Some(b) = before else {
            return Changed {
                parts: vec![],
                unseen: vec![],
                error: Some("no snapshot was taken before this work".into()),
            };
        };
        let file = &self.s.file;
        let dir = g::log_dir(file);
        let skip = |p: &str| is_plan_file(p, file, &dir);
        match self
            .env
            .snapshot_changes(&b.id, &b.also, self.wants_git(), None)
        {
            Ok(c) => Changed {
                parts: c
                    .changes
                    .iter()
                    .filter(|x| !skip(&x.path) && !x.diff.is_empty())
                    .map(|x| x.diff.clone())
                    .collect(),
                unseen: c.unseen.into_iter().filter(|p| !skip(p)).collect(),
                error: None,
            },
            Err(e) => Changed {
                parts: vec![],
                unseen: vec![],
                error: Some(e),
            },
        }
    }

    /// Before the first step: can the reviews see what the steps change? Said in the run's decisions, and with git a
    /// question in the Inbox; the run goes on with Qanuvi's snapshots meanwhile.
    fn check_changes_seen(&mut self, snap: Option<&(Snap, crate::snapshots::Taken)>) {
        let Some((_, t)) = snap else {
            self.decide(
                "decided",
                "Qanuvi",
                "Changes for reviews",
                "Qanuvi could not record the project's files before the run",
                "Reviews cannot show what a step changed, so a step is never turned down for an empty diff alone; tests decide.",
            );
            return;
        };
        if t.truncated {
            let files = t.files;
            self.decide(
                "decided",
                "Qanuvi",
                "Changes for reviews",
                &format!("only the first {files} files are followed"),
                "The project has more files than a snapshot keeps; the plan's folder is followed first.",
            );
        }
        if self.wants_git() && !t.git_blind.is_empty() {
            let blind = t.git_blind.clone();
            self.ask_changes_from(&blind, "folders");
        }
    }

    fn ask_changes_from(&mut self, paths: &[String], what: &str) {
        if self.asked_changes_from {
            return;
        }
        self.asked_changes_from = true;
        let list = paths
            .iter()
            .take(5)
            .map(|p| {
                if p.is_empty() {
                    "(the project's top folder)".to_string()
                } else {
                    p.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let more = if paths.len() > 5 {
            format!(" and {} more", paths.len() - 5)
        } else {
            String::new()
        };
        self.decide(
            "decided",
            "Qanuvi",
            "Changes for reviews",
            &format!("git cannot see {list}{more}: Qanuvi's own snapshots show their changes"),
            "This project uses git for reviews, but no repository sees these (none, or one that ignores them).",
        );
        let pid = self.env.project_id().to_string();
        let _ = self.env.inbox_add(&NewItem {
            kind_of: "answer".into(),
            kind: "run".into(),
            title: format!("{}: git cannot see some of the plan's {what}", self.s.project_name),
            text: format!(
                "{list}{more}. The run goes on and shows their changes from Qanuvi's own snapshots. Keep git for the \
                 rest, or use Qanuvi's snapshots for this whole project?"
            ),
            key: Some(format!("changes-from:{pid}")),
            form: Some(json!({"choices": [
                {"id": "keep", "label": "Keep git for the rest"},
                {"id": "qanuvi", "label": "Use Qanuvi's snapshots", "hint": "For every file of this project, git or not"}
            ]})),
            source: Some(json!({"kind": "changes-from", "projectId": pid})),
            open: Some(self.open_plan()),
        });
    }

    /// The decider's look at what a step changed (the diff since it started, the tests).
    fn review(
        &mut self,
        i: usize,
        step: &Step,
        before: Option<&Snap>,
        tests_passed: Option<bool>,
    ) -> Verdict {
        let file = self.s.file.clone();
        let dir = g::log_dir(&file);
        let changed = self.changes_since(before);
        if !changed.unseen.is_empty() {
            let unseen = changed.unseen.clone();
            self.ask_changes_from(&unseen, "files");
        }
        let touched: Vec<String> = self.run.steps[i]
            .changed
            .iter()
            .flatten()
            .filter(|p| !is_plan_file(p, &file, &dir))
            .cloned()
            .collect();
        let read = ReadChanges {
            parts: changed.parts.clone(),
            error: changed.error.clone(),
        };
        if let Some(v) = review_without_diff(&read, &touched, tests_passed) {
            return v;
        }
        let tests = match tests_passed {
            Some(true) => "passed",
            Some(false) => "failed",
            None => "",
        };
        // Where the work is, and what the agent says it did (the step as the plan has it now, after the attempt).
        let home = match file.rfind('/') {
            Some(i) => file[..=i].to_string(),
            None => String::new(),
        };
        let done_line = self
            .plan()
            .and_then(|now| find(&now.text, step.number.as_deref(), &step.title))
            .and_then(|after| {
                after
                    .details
                    .iter()
                    .find(|d| d.trim().to_lowercase().starts_with("done:"))
                    .cloned()
            })
            .unwrap_or_default();
        let step_log = self
            .env
            .read(&g::log_path(&file, step.number.as_deref(), &step.title))
            .map(|f| f.text)
            .unwrap_or_default();
        let prompt = decider::review_prompt_with_log(
            &step.title,
            &step.details,
            &changed.parts.join("\n"),
            tests,
            &home,
            &done_line,
            &step_log,
        );
        let name = self.run.steps[i]
            .number
            .clone()
            .unwrap_or_else(|| self.run.steps[i].title.clone());
        // A short look at one step's diff: the agent's own default model, not the planner's bigger one.
        let model = if self.s.default_model.is_empty() {
            self.s.planner_model.clone()
        } else {
            self.s.default_model.clone()
        };
        match self.ask(
            &prompt,
            &model,
            &format!("Decider: review step {name}"),
            true,
        ) {
            Ok((text, cost)) => {
                self.decider_call(&model, cost);
                let r = decider::parse_review(&text);
                Verdict {
                    ok: r.ok,
                    why: r.why,
                }
            }
            Err(e) => {
                // The paid agent cannot answer now: the free models review instead (24e).
                if let Ok((by, text)) =
                    self.free_decide("You review code changes. Answer with JSON only.", &prompt)
                {
                    let r = decider::parse_review(&text);
                    return Verdict {
                        ok: r.ok,
                        why: format!(
                            "{} (reviewed by {by}: {} could not answer)",
                            r.why, self.s.provider_name
                        ),
                    };
                }
                // Tests decide when there are any, else the step stays accepted with a note.
                if tests_passed == Some(false) {
                    Verdict {
                        ok: false,
                        why: "tests failed and the review did not answer".into(),
                    }
                } else {
                    Verdict {
                        ok: true,
                        why: format!("not reviewed ({e})"),
                    }
                }
            }
        }
    }

    /// One question to the planner on the run's agent: its text and cost.
    fn ask(
        &self,
        prompt: &str,
        model: &str,
        title: &str,
        lean: bool,
    ) -> Result<(String, f64), String> {
        let ask = PlannerAsk {
            prompt: prompt.to_string(),
            model: model.to_string(),
            title: title.to_string(),
            agent: self.s.provider.clone(),
            base_url: self.s.turns.base_url.clone(),
            effort: self.s.turns.effort.clone(),
            lean,
            index_tools: false,
            setup: self.s.turns.setups.get(&self.s.provider).cloned(),
        };
        let a = ask_planner(self.env, self.memory(), &ask, &mut |e| self.sh.forward(e))?;
        Ok((a.text, a.cost_usd))
    }

    fn free_decide(&self, system: &str, prompt: &str) -> Result<(String, String), String> {
        let setup = self
            .s
            .turns
            .setups
            .get("qanuvi")
            .ok_or("no free models are set up")?;
        self.env.free_decide(setup, system, prompt)
    }

    fn decider_call(&mut self, model: &str, cost: f64) {
        self.run
            .decider_calls
            .get_or_insert_with(Vec::new)
            .push(DeciderCall {
                model: model.to_string(),
                cost_usd: cost,
            });
    }

    /// One decider call for the run's steps: who does each (free or paid).
    fn ask_assignment(
        &mut self,
        briefs: &[StepBrief],
        priority: Priority,
    ) -> (Vec<Assignment>, String) {
        let prompt = decider::assign_prompt(briefs, priority);
        let mut by = self.s.provider_name.clone();
        let model = self.s.planner_model.clone();
        let answer = match self.ask(&prompt, &model, "Decider: who does each step", true) {
            Ok((text, cost)) => {
                self.decider_call(&model, cost);
                text
            }
            // The paid agent cannot answer now (limit, sign-in, offline): the free models take the seat (24e).
            Err(_) => match self.free_decide(
                "You plan work for coding agents. Answer with JSON only.",
                &prompt,
            ) {
                Ok((who, text)) => {
                    by = format!("{who} (free, {} could not answer)", self.s.provider_name);
                    text
                }
                Err(_) => String::new(),
            },
        };
        let list = decider::parse_assignment(&answer, briefs, priority);
        (
            list,
            if answer.is_empty() {
                "Qanuvi core".into()
            } else {
                by
            },
        )
    }

    // ---------- The steps ----------

    fn steps_loop(&mut self) {
        // Held and failed steps are set aside, not the end of the run (24u): the run goes on with the rest, then waits
        // for the owner's answers and goes back to the steps they release.
        let mut ending = false;
        if self.run.status != RunStatus::Running {
            return;
        }
        loop {
            let mut again = false;
            self.cursor = 0;
            while self.cursor < self.run.steps.len() {
                if self.run.status != RunStatus::Running {
                    break;
                }
                if self.run.steps[self.cursor].status != RunStepStatus::Waiting {
                    self.cursor += 1;
                    continue;
                }
                if self.ready(self.cursor) {
                    self.work(self.cursor);
                    self.after_step(self.cursor);
                }
                // Answered while the run went on: back to the released steps first (they come earlier in the plan).
                if self.take_answers() {
                    self.answered = true;
                }
                if self.answered && self.run.status == RunStatus::Running {
                    self.answered = false;
                    again = true;
                    break;
                }
                self.cursor += 1;
            }
            if again {
                continue;
            }
            if self.run.status != RunStatus::Running {
                break;
            }
            // A step opened again behind the pass (a closing step's catch-up) gets its turn first.
            if self
                .run
                .steps
                .iter()
                .any(|x| x.status == RunStepStatus::Waiting)
            {
                continue;
            }
            let asks = self
                .run
                .steps
                .iter()
                .filter(|x| x.status == RunStepStatus::Held)
                .count();
            if asks == 0 || ending {
                // About to end: closing steps that missed late steps run once now, then the run ends.
                if self.run_closings(true) {
                    continue;
                }
                break;
            }
            // Only set-aside steps are left: wait for the owner, spending nothing, and go on by itself once answered.
            self.sh.live().waiting_answers = asks;
            self.run.message = setaside::waiting_line(asks);
            self.save();
            let limit_ms = answer_wait_ms(&self.s.options);
            let since = self.env.now_ms();
            let mut napped = 0i64;
            loop {
                if self.take_answers() {
                    break;
                }
                if self.stop_asked() || self.pausing() || self.sh.live().finish_without {
                    break;
                }
                // Nobody answered a question the check left open: Qanuvi decides it the safer way and goes on.
                if limit_ms > 0
                    && (self.env.now_ms() - since).max(napped) >= limit_ms
                    && self.decide_unanswered(limit_ms)
                {
                    break;
                }
                self.sh.nap();
                napped += self.sh.poll.as_millis() as i64;
            }
            self.sh.live().waiting_answers = 0;
            // bg2: the wait is over: "Waiting for you (1 answer)" no longer holds (it stayed while the run went on).
            self.run.message.clear();
            self.save();
            self.sh.touch();
            self.answered = false;
            if self.stop_asked() {
                self.finish(RunStatus::Stopped, STOPPED);
                break;
            }
            if self.pausing() {
                self.finish(
                    RunStatus::Paused,
                    "Paused while waiting for your answers. Resume to go on.",
                );
                break;
            }
            if std::mem::take(&mut self.sh.live().finish_without) {
                ending = true;
            }
        }
    }

    /// Whether a waiting step runs now: its needs are done, and nobody wanted to see it first. Else it is skipped,
    /// set aside or held, with the reason.
    fn ready(&mut self, i: usize) -> bool {
        let (number, title) = (
            self.run.steps[i].number.clone(),
            self.run.steps[i].title.clone(),
        );
        let now = self.plan();
        let now_step = now
            .as_ref()
            .and_then(|n| find(&n.text, number.as_deref(), &title));
        if let Some(n) = &now {
            for x in parse_plan(&n.text).steps {
                if x.status == StepStatus::Done
                    && let Some(num) = x.number
                {
                    self.done_numbers.insert(num);
                }
            }
        }
        let needs = now_step
            .as_ref()
            .map(|st| st.needs.clone())
            .unwrap_or_default();
        let unmet = unmet_need(&needs, &self.done_numbers, &self.other_plans(&needs));
        let aside_need = self
            .run
            .steps
            .iter()
            .find(|x| {
                matches!(x.status, RunStepStatus::Held | RunStepStatus::SetAside)
                    && x.number.as_ref().is_some_and(|n| needs.contains(n))
            })
            .cloned();
        if unmet.is_some()
            && let Some(a) = aside_need
        {
            let st = &mut self.run.steps[i];
            st.status = RunStepStatus::SetAside;
            st.waits_for = Some(StepWhy {
                step: match (&a.status, &a.waits_for) {
                    (RunStepStatus::SetAside, Some(w)) => w.step.clone(),
                    _ => key(&a),
                },
                why: format!(
                    "its Needs line names step {}",
                    a.number.clone().unwrap_or_default()
                ),
            });
            self.save();
            return false;
        }
        // What it needs comes later in this run and has not had its turn yet: it waits (still open) and the pass goes
        // on; the run comes back to it after them. Only later steps count, so steps that need each other end skipped.
        if unmet.is_some() {
            let mut soon = self.done_numbers.clone();
            soon.extend(self.run.steps[i + 1..].iter().filter_map(|x| {
                (x.status == RunStepStatus::Waiting)
                    .then(|| x.number.clone())
                    .flatten()
            }));
            if unmet_need(&needs, &soon, &self.other_plans(&needs)).is_none() {
                return false;
            }
        }
        if let Some(why) = unmet {
            let st = &mut self.run.steps[i];
            st.status = RunStepStatus::Skipped;
            st.reason = Some(why.clone());
            self.note_skipped(i, Some(&why));
            self.log_result(i);
            self.save();
            return false;
        }
        // A step the owner chose to see first (risky, or a question left open in the check before the run).
        if self.run.steps[i].answer.is_none()
            && let Some(why) = waits_before(number.as_deref(), Some(&self.wait_list))
        {
            let why = why.to_string();
            self.set_aside(i, HeldKind::Hold, &why);
            return false;
        }
        true
    }

    /// The step's attempts until it is done, blocked, failed, replaced, or the run stops or pauses.
    fn work(&mut self, i: usize) {
        let file = self.s.file.clone();
        let fresh_model: Option<Option<String>> = self
            .plan()
            .and_then(|n| {
                find(
                    &n.text,
                    self.run.steps[i].number.as_deref(),
                    &self.run.steps[i].title,
                )
            })
            .map(|st| st.model);
        // No "- Model:" line in the file (Qanuvi no longer writes them, R2e i): the model the owner chose for the
        // step in Qanuvi, kept in the plan's own data.
        let fresh_model = match fresh_model {
            Some(None) | None => {
                let kept = self.load_state().kept_model(
                    self.run.steps[i].number.as_deref(),
                    &self.run.steps[i].title,
                );
                match (fresh_model, kept) {
                    (_, Some(m)) => Some(Some(m)),
                    (f, None) => f,
                }
            }
            some => some,
        };
        self.run.steps[i].status = RunStepStatus::Running;
        // Released by the owner's answer: what it was held for and what they said go into its next try (24u).
        let mut answer_note = String::new();
        if let Some(released) = self.run.steps[i].held.take() {
            let said = match self.run.steps[i].answer.as_deref() {
                Some(a) if !a.is_empty() => format!(" The owner answered: \"{a}\"."),
                _ => String::new(),
            };
            let talk = self.run.steps[i].talk.clone().unwrap_or_default();
            // n2: a held step talked with the owner too: it gets the talk and what their last reply means.
            let means = self.run.steps[i]
                .instruction
                .as_deref()
                .filter(|m| !m.trim().is_empty())
                .map(|m| format!(" What their answer means for this step: {}", m.trim()))
                .unwrap_or_default();
            answer_note = if !talk.is_empty() {
                let last = talk.last().cloned().unwrap_or_default();
                format!(
                    "\n\n{}",
                    ownerask::next_try_note(
                        &released.why,
                        &talk,
                        self.run.steps[i].answer.as_deref().unwrap_or(&last.answer),
                        self.run.steps[i].instruction.as_deref().unwrap_or(""),
                    )
                )
            } else if released.kind == HeldKind::Hold {
                format!(
                    "\n\nThis step was held for the owner ({}); they said go ahead.{said}{means}",
                    released.why
                )
            } else {
                format!(
                    "\n\nEarlier tries did not finish this step ({}).{said} Try it again with that in mind.",
                    released.why
                )
            };
            // A blocked step is open again in the plan, so its next try can mark it done.
            if released.kind == HeldKind::Blocked
                && let Some(now) = self.plan()
                && let Some(st) = parse_plan(&now.text).steps.into_iter().find(|x| {
                    x.line == self.run.steps[i].line && x.title == self.run.steps[i].title
                })
                && st.status == StepStatus::Blocked
                && let Ok(next) =
                    set_step_status(&now.text, st.line, StepStatus::Open, Some(&st.title))
            {
                let _ = self.env.write(&file, &next, Some(&now.hash));
            }
        }
        // A section's closing step hears what is set aside, does what it can and writes what is still open.
        let rerun = self.run.steps[i].rerun_after.take();
        let aside_note = if is_closing_step(self.run.steps[i].number.as_deref()) {
            let mut note = rerun
                .filter(|r| !r.is_empty())
                .map(|r| setaside::closing_rerun_note(&r))
                .unwrap_or_default();
            let items: Vec<AsideItem> = self
                .run
                .steps
                .iter()
                .filter(|x| matches!(x.status, RunStepStatus::Held | RunStepStatus::SetAside))
                .map(|x| AsideItem {
                    label: key(x),
                    why: if x.status == RunStepStatus::Held {
                        x.held
                            .as_ref()
                            .map_or("it needs the owner".into(), |h| h.why.clone())
                    } else {
                        format!(
                            "waits for {}",
                            x.waits_for
                                .as_ref()
                                .map(|w| w.step.clone())
                                .unwrap_or_default()
                        )
                    },
                })
                .collect();
            note.push_str(&setaside::set_aside_block(&items));
            note
        } else {
            String::new()
        };
        let planned = self
            .steps
            .iter()
            .find(|st| st.line == self.run.steps[i].line)
            .cloned();
        let job = job_type(
            &[
                self.run.steps[i].title.clone(),
                planned.as_ref().map(|p| p.note.clone()).unwrap_or_default(),
                planned
                    .as_ref()
                    .map(|p| p.details.join("\n"))
                    .unwrap_or_default(),
            ]
            .join("\n"),
        );
        let assigned = self
            .assignment
            .get(
                &self.run.steps[i]
                    .number
                    .clone()
                    .unwrap_or_else(|| self.run.steps[i].line.to_string()),
            )
            .cloned();
        let step_model: Option<String> = if !self.one_model.is_empty() {
            Some(self.one_model.clone())
        } else {
            match &fresh_model {
                Some(m) => m.clone(),
                None => planned.as_ref().and_then(|p| p.model.clone()),
            }
        };
        let picked = assigned.as_ref().and_then(|a| {
            a.model.clone().map(|m| Picked {
                model: m,
                why: a.why.clone(),
            })
        });
        let budget = self.budget();
        let mut choice = choose_model(&ChooseOptions {
            step_model: step_model.as_deref(),
            job,
            rules: &self.s.rules,
            stats: &self.stats,
            models: &self.s.models,
            default_model: &self.s.default_model,
            budget,
            picked: picked.as_ref(),
        });
        if !self.one_model.is_empty() {
            choice.reason = "one model for the whole run (your choice)".into();
        }
        self.run.steps[i].job_type = Some(job);
        let first = choice.model.clone();
        // Free first, except money and security work and steps that name their model.
        let names_model = match &fresh_model {
            Some(m) => m.is_some(),
            None => planned.as_ref().is_some_and(|p| p.model.is_some()),
        };
        let free = self.free_first
            && job != JobType::Security
            && !names_model
            && assigned.as_ref().is_none_or(|a| a.worker == Worker::Free);
        self.run.steps[i].choice = Some(if free {
            let why = assigned
                .as_ref()
                .map(|a| a.why.clone())
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "free models can do it".into());
            format!(
                "free first (decider: {why}); then {first}: {}",
                choice.reason
            )
        } else if self.free_first && assigned.as_ref().is_some_and(|a| a.worker == Worker::Paid) {
            if choice.reason.starts_with("decider:") {
                format!("paid; {}", choice.reason)
            } else {
                format!(
                    "paid (decider: {}); {}",
                    assigned.as_ref().map(|a| a.why.clone()).unwrap_or_default(),
                    choice.reason
                )
            }
        } else {
            choice.reason.clone()
        });
        let models: Vec<AgentModel> =
            ladder(&self.s.models, self.s.options.stronger_on_fail).to_vec();
        let per_model = self.s.options.attempts_per_model;
        loop {
            if self.stop_asked() {
                self.finish(RunStatus::Stopped, STOPPED);
                self.run.steps[i].status = RunStepStatus::Waiting;
                break;
            }
            if budget_state(self.budget()) == BudgetState::Over {
                let m = format!(
                    "The monthly budget of ${:.2} is used up.",
                    self.s.monthly_budget_usd
                );
                self.finish(RunStatus::NeedsYou, &m);
                self.run.steps[i].status = RunStepStatus::Waiting;
                break;
            }
            let spent = self.run_cost();
            if self.s.options.budget_usd > 0.0 && spent >= self.s.options.budget_usd {
                let m = format!(
                    "The run reached its budget of ${:.2}.",
                    self.s.options.budget_usd
                );
                self.run.budget_reached = Some(self.s.options.budget_usd);
                self.finish(RunStatus::Stopped, &m);
                self.run.steps[i].status = RunStepStatus::Waiting;
                break;
            }
            let st = self.run.steps[i].clone();
            let tries = &st.attempts[st.retry_from.unwrap_or(0).min(st.attempts.len())..];
            // The paid agent's limit is not reset yet and the owner chose free models meanwhile.
            let free_now = self.free_until.is_some_and(|t| self.env.now_ms() < t)
                && self.can_free
                && !is_free(&first);
            let chosen = self.live.as_ref().filter(|l| l.step == i);
            let model = if free_now {
                Some(FREE_MODEL.to_string())
            } else if let Some(c) = chosen {
                next_model(
                    &models,
                    &st.attempts[c.from.min(st.attempts.len())..],
                    &c.model,
                    per_model,
                )
            } else if free {
                next_model_free_first(&models, tries, FREE_MODEL, &first, per_model)
            } else {
                next_model(&models, tries, &first, per_model)
            };
            if model.is_none()
                && self.s.options.replan
                && st.replaced_by.is_none()
                && st.retry_from.is_none()
            {
                // Back to the planner, once: rewrite the step into smaller ones and carry on with those.
                if self.replan(i) {
                    break;
                }
            }
            let Some(model) = model else {
                self.run.steps[i].status = RunStepStatus::Failed;
                // Y4: its own checks still failed after the fix rounds of every try: said so, with the commands.
                let last = st
                    .attempts
                    .last()
                    .map(|a| a.note.clone())
                    .unwrap_or_default();
                self.run.steps[i].reason = Some(match last.strip_prefix(turn::CHECKS_FAILED) {
                    Some(what) => format!(
                        "its checks still fail after every try: {}",
                        what.trim_end_matches('.')
                    ),
                    None => "not finished even by the strongest model".into(),
                });
                // Set aside after its result is logged; the run goes on with the steps that do not need it (24u).
                break;
            };
            // Read the list again: earlier steps or the owner may have changed it.
            let Some(current) = self.plan() else {
                self.run.steps[i].status = RunStepStatus::Skipped;
                break;
            };
            let Some(fresh) = find(&current.text, st.number.as_deref(), &st.title) else {
                self.run.steps[i].status = RunStepStatus::Skipped;
                break;
            };
            // Done by hand (or by an agent in the chat) since the run started: it counts as done.
            if fresh.status == StepStatus::Done && st.attempts.is_empty() {
                self.run.steps[i].status = RunStepStatus::Done;
                self.run.steps[i].reason = Some("marked done outside the run".into());
                self.mark_done(i);
                break;
            }
            // The owner picked another mode for the rest of the run (kept for this plan).
            if let Some(m) = self.sh.controls.mode()
                && m != self.run.options.mode
            {
                self.run.options.mode = m.clone();
                self.s.options.mode = m.clone();
                self.patch_run_settings(json!({"mode": m}));
            }
            self.sh.set_current_model(&model);
            let title = format!(
                "Step {}{}{}",
                st.number
                    .as_ref()
                    .map(|n| format!("{n}: "))
                    .unwrap_or_default(),
                st.title,
                if st.attempts.is_empty() {
                    String::new()
                } else {
                    format!(" (try {})", st.attempts.len() + 1)
                }
            );
            // The full suite after every step, or only at the last step of a section.
            let last_of_section = !self.run.steps[i + 1..].iter().any(|x| {
                x.status == RunStepStatus::Waiting
                    && self
                        .steps
                        .iter()
                        .find(|s| s.line == x.line)
                        .is_some_and(|s| s.section == fresh.section)
            });
            let full_now =
                self.s.options.full_checks != Some(FullChecks::Section) || last_of_section;
            // A step with extra checks always runs the full tests (24u part 3).
            let extra = self.extra_checks.get(&key(&st)).cloned();
            let test_command = if (self.s.options.packets && full_now) || extra.is_some() {
                self.env
                    .project_settings()
                    .map(|p| p.test_command.trim().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            // Y4: the checks the step names, run by Qanuvi after it; a check the run rules refuse sets the step aside
            // before any work (no try could pass it).
            let (checks, refused_checks) = self.step_checks(&fresh, &test_command);
            // Y8: the step asks for the app's test build; one that cannot run sets the step aside before any work.
            let test_build = match self.step_test_build(&fresh) {
                Ok(t) => t,
                Err(why) => {
                    self.run.steps[i].status = RunStepStatus::Blocked;
                    self.run.steps[i].reason = Some(why);
                    self.mark_blocked_in_plan(i);
                    break;
                }
            };
            if !refused_checks.is_empty() {
                self.run.steps[i].status = RunStepStatus::Blocked;
                self.run.steps[i].reason = Some(format!(
                    "its check {}",
                    refused_reason(&refused_checks).trim_start_matches("it needed ")
                ));
                self.mark_blocked_in_plan(i);
                break;
            }
            let private = self.env.plan_private(&file);
            let own_log = g::log_path(&file, fresh.number.as_deref(), &fresh.title);
            let own_log_exists = self.env.read(&own_log).is_ok();
            let resumed_with = std::mem::take(&mut self.resume_text);
            let handed_over = std::mem::take(&mut self.resume_is_handover);
            let read = |p: &str| self.env.read(p).map(|f| f.text).unwrap_or_default();
            let recent: Vec<Recent> = self
                .run
                .steps
                .iter()
                .enumerate()
                .filter(|(j, x)| *j != i && x.changed.as_ref().is_some_and(|c| !c.is_empty()))
                .map(|(_, x)| Recent {
                    number: x.number.clone(),
                    files: x.changed.clone().unwrap_or_default(),
                })
                .collect();
            let mut prompt = String::new();
            // The owner's go-ahead from the chat comes first, so it is the first thing read.
            if !resumed_with.is_empty() && !handed_over {
                prompt.push_str(&format!(
                    "The owner said in this session's chat: \"{resumed_with}\". Go on as agreed in the chat above (do \
                     first what was decided there), then finish the step.\n\n"
                ));
            }
            prompt.push_str(&g::knowledge_block(
                &file,
                &read(&g::map_path(&file)),
                &read(&g::facts_path(&file)),
                &recent,
                self.s.options.index_tools,
            ));
            prompt.push_str(&g::referenced_block(&self.plan_refs));
            prompt.push('\n');
            if handed_over {
                prompt.push_str(&format!("{resumed_with}\n\n"));
            }
            prompt.push_str(&step_prompt(&file, &fresh, &self.done));
            prompt.push_str(&answer_note);
            prompt.push_str(&aside_note);
            prompt.push_str(&g::private_block(&private));
            prompt.push_str(&g::logs_block(&file, &own_log, own_log_exists));
            if !full_now {
                prompt.push_str(
                    "\n\nChecks: run only the tests for what this step changes (and lint the files you touch). The full \
                     suite and the plan's other whole-project checks run once, at the last step of this section; skip \
                     them here, even where the plan's rules ask for them after every step.",
                );
            }
            // The reviewer turned down an earlier try's work: the next model hears why.
            if let Some(rejected) = st
                .attempts
                .iter()
                .rev()
                .find(|a| a.note.starts_with("Review: "))
            {
                let who = if is_free(&rejected.model) {
                    "A free model"
                } else {
                    "An earlier try"
                };
                prompt.push_str(&format!(
                    "\n\n{who} already worked on this step, but the review turned it down: {}\nIts work is kept: do \
                     not redo what is already done and recorded (the step's Progress note and Done line say what was \
                     checked). Do only what the review names as missing or wrong, record it in the step's log, then \
                     finish the step.",
                    &rejected.note["Review: ".len()..]
                ));
            }
            if let Some(why) = &extra {
                prompt.push_str(&format!(
                    "\n\nThis step goes ahead with extra checks ({why}): the full tests must pass and the decider \
                     reviews your diff before it counts as done. Keep the change small and easy to check."
                ));
            }
            if !checks.is_empty() {
                let list: Vec<String> = checks.iter().map(|c| format!("`{c}`")).collect();
                prompt.push_str(&format!(
                    "\n\nAfter this step Qanuvi runs its checks ({}); it counts as done only when they pass.",
                    list.join(", ")
                ));
            }
            // Y6: checks that need a person's eyes go to the review list after the step; the step never waits for them.
            let eyes = self.eyes_checks(&fresh);
            if !eyes.is_empty() {
                prompt.push_str(&format!(
                    "\n\nThese checks need a person's eyes ({}): Qanuvi puts them on the owner's review list once this \
                     step is done. Do not wait for them and do not mark the step blocked for them.",
                    eyes.join("; ")
                ));
            }
            prompt.push_str(
                "\n\nIf something is left that only a person can judge (how a page looks, a wording choice), do not \
                 stop for it: finish the step and add a line `- To look at: <what, and where>` under it in the plan.",
            );
            // Y10 (a) + (c): work left for later is carried to its step; checks decided "the run does it" are the
            // step's own.
            let check_rules = super::check_more::CheckRules::load(self.env, &self.s.file);
            prompt.push_str(&super::check_more::step_prompt_part(
                &fresh.details,
                &check_rules.eyes_of(&key(&st)),
            ));
            // Y10 d1: a step given leave before the run to delete files inside the project gets it for its turns.
            let granted_turns = check_rules
                .grants_of(&key(&st))
                .iter()
                .any(|g| g == "delete")
                .then(|| {
                    let mut t = self.s.turns.clone();
                    let mut r = t.plan_rules.clone().unwrap_or_default();
                    r.delete_inside = true;
                    t.plan_rules = Some(r);
                    t
                });
            if test_build.is_some() {
                prompt.push_str(
                    "\n\nAfter this step Qanuvi test-builds the app itself (builds it into its own folder, starts it on \
                     fresh data, checks it, closes it); it counts as done only when that passes. Do not build or start \
                     the app for this yourself, and never over the installed copy.",
                );
            }
            // Every step's snapshot from its first try: a review sees the whole step's work, and a failed step is
            // undone back to it (Y3).
            let step_snap = match self.step_snaps.get(&key(&st)) {
                Some(s) => Some(s.clone()),
                None => {
                    let s = self.snapshot().map(|s| s.0);
                    if let Some(s) = &s {
                        self.step_snaps.insert(key(&st), s.clone());
                    }
                    s
                }
            };
            let reviewed = is_free(&model) || extra.is_some();
            let before = if reviewed { step_snap } else { None };
            // p1: the list as the step's agent got it, to put back a step line it breaks.
            let plan_before = self.plan().map(|p| p.text);
            if self.s.options.packets {
                let ctx = self.context(&fresh);
                prompt = packet_prompt(&prompt, &ctx, &test_command);
            }
            // Same model, same section, still small: the step continues the last step's session. A reviewed step
            // (extra checks, a free model) starts fresh when the session is already past half the handover limit, so
            // it does not hand over half-way through its checks.
            let section = fresh.section.clone();
            let handover_at = self
                .s
                .options
                .handover_limit
                .map_or(g::HANDOVER_DEFAULT, |h| h as u64);
            let too_big_for_checks = reviewed
                && self
                    .carry
                    .as_ref()
                    .is_some_and(|c| c.1.tokens >= handover_at / 2);
            let carry_on = (!resumed_with.is_empty() && !handed_over && self.carry.is_some())
                || !too_big_for_checks
                    && g::can_carry(
                        self.carry.as_ref().map(|c| &c.1),
                        &model,
                        if self.s.options.carry_across_sections == Some(false) {
                            &section
                        } else {
                            self.carry
                                .as_ref()
                                .map_or(section.as_str(), |c| c.1.section.as_str())
                        },
                        self.s.options.carry_limit.unwrap_or(150_000.0) as u64,
                    );
            if !carry_on {
                self.drop_carry();
            }
            let resume = if carry_on {
                self.carry.as_ref().map(|c| (c.0.clone(), c.2))
            } else {
                None
            };
            // PM c: a new session gets what the project memory knows (a continued one has it already), so a step
            // never asks what the owner answered once.
            if resume.is_none() {
                let memory = projectfacts::memory_block(&self.env.project_memory());
                if !memory.is_empty() {
                    prompt = format!("{prompt}\n\n{memory}");
                }
            }
            let step_started = self.env.now_ms() - 2000;
            // The step's limits: what earlier attempts of it spent counts too.
            if st.attempts.is_empty() {
                self.watch =
                    StepWatch::for_step(self.env.now_ms(), 0.0, StepLimits::default(), 0, "");
            }
            self.watch.handover = 0;
            self.watch.step_log = own_log.clone();
            self.watch.plan_step = Some((self.s.file.clone(), key(&st)));
            self.watch.handover_limit = self
                .s
                .options
                .handover_limit
                .map_or(g::HANDOVER_DEFAULT, |h| h as u64);
            self.watch.spent_before = st.attempts.iter().map(|a| a.cost_usd).sum();
            self.watch.attempt_base = resume.as_ref().map_or(0.0, |r| r.1.cost);
            self.watch.limits = StepLimits {
                usd: self.s.options.step_cost_limit,
                minutes: self.s.options.step_minutes,
            };
            self.watch.over_limit = None;
            self.watch.switched = None;
            // Y7: the paid agent goes through a proxy that does not answer: wait for it before the try (a try now
            // could only fail).
            if !is_free(&model)
                && let Some(url) = self.proxy_url()
                && !self.env.reachable(&url)
                && !self.wait_healthy(&Down::Proxy(url), &key(&st), 0)
            {
                self.end_health_wait();
                self.run.steps[i].status = RunStepStatus::Waiting;
                break;
            }
            let ask = AttemptAsk {
                prompt,
                model: model.clone(),
                mode: self.run.options.mode.clone(),
                title,
                test_command: test_command.clone(),
                checks: checks.clone(),
                test_build: test_build.clone(),
                resume,
            };
            // The window learns the step is at work before its agent starts: a model picked for it and words typed
            // into its chat go to the runner only while the window sees it running (R2e).
            self.save();
            let sh = self.sh;
            let end = turn::attempt(
                self.env,
                &sh.controls,
                &mut self.watch,
                granted_turns.as_ref().unwrap_or(&self.s.turns),
                &ask,
                &mut |e| sh.forward(e),
            );
            self.last_idle = end.idle;
            self.last_totals = end.totals;
            let mut attempt = end.attempt;
            // The owner switched the step's model while it ran: written into its Model line, counted from the next try.
            if let Some(m) = self.watch.switched.clone() {
                self.write_step_model(i, &m);
                self.live = Some(LiveModel {
                    step: i,
                    model: m,
                    from: self.run.steps[i].attempts.len() + 1,
                });
            }
            self.settle_carry(&attempt, &section, carry_on);
            // What the step changed, for the next steps (and its log).
            let changed = self.env.changed_since(step_started).unwrap_or_default();
            let mut all = self.run.steps[i].changed.clone().unwrap_or_default();
            for c in changed {
                if !all.contains(&c) {
                    all.push(c);
                }
            }
            self.run.steps[i].changed = Some(all);
            self.log_attempt(i, &mut attempt);
            self.take_messages(i);
            // Handed over (context too big, or the owner's model switch): no failure; unless the step got done, a new
            // session goes on from the note.
            if self.watch.handover != 0
                && !self.watch.pause_now
                && self.watch.over_limit.is_none()
                && !self.stop_asked()
            {
                let size = std::mem::take(&mut self.watch.handover);
                let switch_to = self.watch.switched.take();
                let open = self.plan().is_some_and(|now| {
                    step_outcome(&now.text, st.number.as_deref(), &st.title) == StepOutcome::Open
                });
                if open {
                    attempt.outcome = Outcome::Stopped;
                    if let Some(m) = &switch_to {
                        // The owner's switch: no failed try, and not counted against the handover limit.
                        attempt.note =
                            format!("handed over to {} (the owner's choice)", self.label(m));
                        self.resume_text = g::SWITCH_RESUME.into();
                    } else {
                        self.watch.handovers += 1;
                        attempt.note = format!(
                            "handed over to a new session at about {}k tokens of context",
                            (size as f64 / 1000.0).round()
                        );
                        self.resume_text = g::HANDOVER_RESUME.into();
                        self.sh.notice(format!(
                            "Step {}: the context reached about {}k tokens, so a new session takes over from the progress note.",
                            key(&st),
                            (size as f64 / 1000.0).round()
                        ));
                    }
                    let sid = attempt.session_id.clone();
                    self.run.steps[i].attempts.push(attempt);
                    self.drop_carry();
                    if !sid.is_empty() {
                        self.env.agent_stop(&sid);
                    }
                    self.resume_is_handover = true;
                    continue;
                }
            }
            // f9: the pause came while the agent was finishing: the step got done (or set aside) all the same. It
            // counts as a whole step, and the run pauses after it (it said "0 of 1 done" with the step ticked).
            if self.watch.pause_now
                && self.plan().is_some_and(|now| {
                    step_outcome(&now.text, st.number.as_deref(), &st.title) != StepOutcome::Open
                })
            {
                self.watch.pause_now = false;
                self.settle_carry(&attempt, &section, carry_on);
            }
            // Paused now by the owner: the attempt is no failure, the session is kept, Resume goes on in it.
            if self.watch.pause_now {
                self.watch.pause_now = false;
                attempt.outcome = Outcome::Stopped;
                attempt.note = "paused by the owner".into();
                let sid = attempt.session_id.clone();
                let am = attempt.model.clone();
                self.run.steps[i].attempts.push(attempt);
                self.run.steps[i].status = RunStepStatus::Waiting;
                if !sid.is_empty() && self.carry.as_ref().is_none_or(|c| c.0 != sid) {
                    self.carry = Some((
                        sid.clone(),
                        Carry {
                            model: am.clone(),
                            section: section.clone(),
                            tokens: 0,
                            steps: 1,
                        },
                        self.last_totals,
                    ));
                }
                self.keep_for_resume = true;
                if !sid.is_empty() {
                    self.run.paused_session = Some(PausedSession {
                        session_id: sid,
                        model: am,
                        section,
                        number: st.number.clone(),
                    });
                }
                self.sh.controls.clear_pause();
                self.finish(
                    RunStatus::Paused,
                    &format!(
                        "Paused in step {}. Resume goes on in the same session (you can change the run's settings first).",
                        key(&st)
                    ),
                );
                break;
            }
            // Over the step's limits: the run pauses and says so (Resume to go on, maybe with other limits or a model).
            if let Some(why) = self.watch.over_limit.take() {
                self.drop_carry();
                self.run.steps[i].attempts.push(attempt);
                self.run.steps[i].status = RunStepStatus::Waiting;
                self.finish(
                    RunStatus::Paused,
                    &format!(
                        "Step {} was paused: {why}. Resume to go on (raise the limit or choose another model).",
                        key(&st)
                    ),
                );
                break;
            }
            // A usage limit is no failure: wait for the reset (or let free models work), then try the step again.
            if matches!(attempt.outcome, Outcome::Error | Outcome::Open)
                && let Some(reset) = self.limit_reset(&attempt)
            {
                self.drop_carry();
                if self.s.options.on_limit == Some(OnLimit::Free) && self.can_free {
                    self.free_until = Some(reset);
                    self.sh.notice(
                        "The paid agent hit its limit: free models work until it resets.".into(),
                    );
                    continue;
                }
                if !self.wait_until(reset) {
                    if self.pausing() {
                        self.sh.controls.clear_pause();
                        self.finish(
                            RunStatus::Paused,
                            "Paused while waiting for the usage limit. Resume when you want to go on.",
                        );
                    } else {
                        self.finish(
                            RunStatus::Stopped,
                            "Stopped by you while waiting for the usage limit to reset.",
                        );
                    }
                    self.run.steps[i].status = RunStepStatus::Waiting;
                    break;
                }
                continue;
            }
            // Y7: the proxy or the model provider was down: no failure. The run waits (says so once), then tries
            // the step again; a step that met outages for hours is set aside instead.
            if matches!(attempt.outcome, Outcome::Error | Outcome::Open)
                && let Some(down) = self.outage_of(&attempt)
            {
                let k = key(&st);
                let now = self.env.now_ms();
                let (n, since) = match &self.outage {
                    Some((sk, n, since)) if *sk == k => (n + 1, *since),
                    _ => (1, now),
                };
                self.outage = Some((k.clone(), n, since));
                self.drop_carry();
                if now - since >= health::GIVE_UP_AFTER.as_millis() as i64 {
                    let words = match &down {
                        Down::Provider(notice) => notice.clone(),
                        Down::Proxy(_) => down.says(),
                    };
                    attempt.outcome = Outcome::Blocked;
                    attempt.note = format!(
                        "the model provider kept failing for {} hours ({words})",
                        health::GIVE_UP_AFTER.as_secs() / 3600
                    );
                    self.outage = None;
                } else {
                    if !self.wait_healthy(&down, &k, n) {
                        self.end_health_wait();
                        self.run.steps[i].status = RunStepStatus::Waiting;
                        break;
                    }
                    continue;
                }
            } else if self.outage.is_some() {
                // Back: the next outage is a new one, said again.
                self.outage = None;
                self.outage_said = false;
            }
            // p1: its own line broken by its agent: put back before it is judged (it was called skipped).
            if let Some(was) = plan_before.as_deref()
                && let Some(now) = self.plan()
                && let Some(fixed) =
                    restore_step_line(was, &now.text, st.number.as_deref(), &st.title)
                && self.env.write(&file, &fixed, Some(&now.hash)).is_ok()
            {
                self.decide(
                    "repaired",
                    "Qanuvi",
                    &format!("Step {}", key(&st)),
                    "its line in the plan put back",
                    "the step's agent broke the step's own line in the plan file",
                );
            }
            if attempt.outcome == Outcome::Open
                && let Some(after) = self.plan()
            {
                attempt.outcome = match step_outcome(&after.text, st.number.as_deref(), &st.title) {
                    StepOutcome::Done => Outcome::Done,
                    StepOutcome::Blocked => Outcome::Blocked,
                    StepOutcome::Open | StepOutcome::Missing => Outcome::Open,
                };
                // With Qanuvi running the tests, "done" also needs them to pass.
                if attempt.outcome == Outcome::Done
                    && (!test_command.is_empty() || !checks.is_empty() || test_build.is_some())
                    && attempt.tests_passed == Some(false)
                {
                    attempt.outcome = Outcome::Open;
                    if !attempt.note.starts_with(turn::CHECKS_FAILED)
                        && !attempt.note.starts_with(crate::test_build::NO_MEMORY)
                    {
                        attempt.note = "Marked done, but the tests still fail.".into();
                    }
                }
            }
            // Quality first: a free model's "done" is checked by the decider before it counts. A step with extra checks
            // goes through the same review, also on the paid agent (24u part 3).
            if attempt.outcome == Outcome::Done && reviewed && (self.free_first || extra.is_some())
            {
                let review = self.review(i, &fresh, before.as_ref(), attempt.tests_passed);
                let provider = self.s.provider.clone();
                self.decide(
                    "review",
                    &provider,
                    &format!("Step {}", key(&st)),
                    if review.ok { "accepted" } else { "rejected" },
                    &review.why,
                );
                if !review.ok {
                    attempt.outcome = Outcome::Open;
                    attempt.note = format!("Review: {}", review.why);
                    // The list changed meanwhile: the next attempt reads it again.
                    if let Some(now) = self.plan()
                        && let Some(s2) = find(&now.text, fresh.number.as_deref(), &fresh.title)
                        && let Ok(next) =
                            set_step_status(&now.text, s2.line, StepStatus::Open, Some(&s2.title))
                    {
                        let _ = self.env.write(&file, &next, Some(&now.hash));
                    }
                }
            }
            // 24y: the run rules refused a command the step needed and it is not done: set aside with the command
            // named (no retry would get further), the run goes on. Done anyway: the refusal stays in its note.
            let refused = if attempt.session_id.is_empty() {
                Vec::new()
            } else {
                self.env.agent_refused(&attempt.session_id)
            };
            if !refused.is_empty() {
                let why = refused_reason(&refused);
                if matches!(attempt.outcome, Outcome::Open | Outcome::Blocked) {
                    attempt.outcome = Outcome::Blocked;
                    attempt.note = why;
                } else if attempt.outcome == Outcome::Done {
                    attempt.note = if attempt.note.is_empty() {
                        format!("Done without: {why}")
                    } else {
                        format!("{} Done without: {why}", attempt.note)
                    };
                }
            }
            // Y8: its test build waited too long for free memory: set aside with the reason (another try would wait
            // the same way); the run goes on with the other steps.
            if attempt.outcome != Outcome::Done
                && let Some(why) = attempt.note.strip_prefix(crate::test_build::NO_MEMORY)
            {
                attempt.outcome = Outcome::Blocked;
                attempt.note = format!("its test build found too little free memory ({why})");
            }
            let outcome = attempt.outcome;
            let note = attempt.note.clone();
            self.run.steps[i].attempts.push(attempt);
            self.save();
            match outcome {
                Outcome::Done => {
                    self.run.steps[i].status = RunStepStatus::Done;
                    self.mark_done(i);
                    self.add_review(i);
                    // An earlier run's "Skipped:" line no longer applies.
                    self.note_skipped(i, None);
                    let pause = self.s.options.pause.unwrap_or(PauseMode::Decisions);
                    if pause == PauseMode::Step
                        && self
                            .run
                            .steps
                            .iter()
                            .any(|x| x.status == RunStepStatus::Waiting)
                    {
                        self.finish(
                            RunStatus::Stopped,
                            &format!(
                                "Paused after step {}, as chosen. Run again to go on.",
                                key(&st)
                            ),
                        );
                    }
                    break;
                }
                Outcome::Blocked => {
                    let reason = if note.is_empty() {
                        Some(self.blocked_reason(i)).filter(|r| !r.is_empty())
                    } else {
                        Some(note)
                    };
                    self.run.steps[i].status = RunStepStatus::Blocked;
                    self.run.steps[i].reason =
                        Some(reason.unwrap_or_else(|| "it needs you".into()));
                    self.mark_blocked_in_plan(i);
                    // Set aside after its result is logged; the run carries on (24u).
                    break;
                }
                Outcome::Stopped => {
                    self.finish(RunStatus::Stopped, STOPPED);
                    self.run.steps[i].status = RunStepStatus::Waiting;
                    break;
                }
                Outcome::Open | Outcome::Error => {}
            }
        }
    }

    /// Y4: the checks a step names, without the test command already run; and the ones the run rules refuse. A check
    /// written without backticks counts only when its first word is a program here.
    fn step_checks(
        &self,
        step: &Step,
        test_command: &str,
    ) -> (Vec<String>, Vec<crate::run_rules::Refusal>) {
        use crate::run_rules::{Refusal, Verdict};
        let mut checks = Vec::new();
        let mut refused = Vec::new();
        for c in super::tasklist::step_checks(step) {
            // Y8: "Check: test build" is the app's test build, run by its recipe (step_test_build).
            if crate::test_build::names_test_build(&c.command) {
                continue;
            }
            let program = c.command.split_whitespace().next().unwrap_or("");
            if c.command == test_command || !c.quoted && !self.env.has_program(program) {
                continue;
            }
            match self.env.rules_verdict(
                &c.command,
                self.s.turns.plan_rules.as_ref(),
                &self.run.options.mode,
            ) {
                Verdict::Allow => checks.push(c.command),
                Verdict::Never(why) => refused.push(Refusal {
                    what: c.command,
                    never: why,
                }),
                Verdict::Unlisted => refused.push(Refusal {
                    what: c.command,
                    never: String::new(),
                }),
            }
        }
        (checks, refused)
    }

    /// Y6: the checks a step names in words that are no command here ("Check: the page looks right"): a person
    /// looks at them, after the step.
    fn eyes_checks(&self, step: &Step) -> Vec<String> {
        super::tasklist::step_checks(step)
            .into_iter()
            .filter(|c| {
                let program = c.command.split_whitespace().next().unwrap_or("");
                !c.quoted
                    && c.at_start
                    && !crate::test_build::names_test_build(&c.command)
                    && !self.env.has_program(program)
            })
            .map(|c| c.command)
            .collect()
    }

    /// Y6: a done step's checks that need eyes and its "To look at:" lines go on the plan's review list in the
    /// Inbox (one item per plan, kept up to date); the run does not wait for them.
    fn add_review(&mut self, i: usize) {
        let st = self.run.steps[i].clone();
        let Some(now) = self.plan() else { return };
        let Some(step) = find(&now.text, st.number.as_deref(), &st.title) else {
            return;
        };
        let lines: Vec<&str> = std::iter::once(step.note.as_str())
            .chain(step.details.iter().map(String::as_str))
            .collect();
        // Y10 (c): checks decided before the run as the run's own (or skipped) are not the owner's to look at.
        let decided = super::check_more::CheckRules::load(self.env, &self.s.file);
        let eyes = decided.eyes_of(&key(&st));
        let mut whats = if !eyes.is_empty() && eyes.iter().all(|e| e.act != "end") {
            Vec::new()
        } else {
            self.eyes_checks(&step)
        };
        for w in super::review::looks_in(&lines) {
            if !whats.contains(&w) {
                whats.push(w);
            }
        }
        // Y10 (a): work left for later goes to the open step that does it; with none, the owner hears of it and the
        // run does not end "finished".
        let mut text = now.text.clone();
        for (what, to) in super::check_more::left_for_later(&step.details) {
            let target = to.as_deref().and_then(|n| {
                parse_plan(&text).steps.into_iter().find(|s| {
                    s.number.as_deref() == Some(n)
                        && !matches!(s.status, StepStatus::Done | StepStatus::Blocked)
                })
            });
            let k = key(&st);
            match target {
                Some(s) => {
                    if let Ok(next) = super::tasklist::add_step_line(
                        &text,
                        s.line,
                        &format!("- Promised by step {k}: {what}"),
                    ) {
                        if next != text {
                            let to = s.number.clone().unwrap_or_else(|| s.title.clone());
                            self.decide(
                                "promise",
                                "Qanuvi",
                                &format!("Step {k}"),
                                &format!("carried to step {to}"),
                                &format!("left for later: {what}"),
                            );
                        }
                        text = next;
                    }
                }
                None => {
                    let w = format!("Left for later by step {k}, and no open step does it: {what}");
                    if !self.left_open.contains(&w) {
                        self.left_open.push(w.clone());
                    }
                    if !whats.contains(&w) {
                        whats.push(w);
                    }
                }
            }
        }
        if text != now.text {
            let _ = self.env.write(&self.s.file, &text, Some(&now.hash));
        }
        self.review_list_add(st.number.clone(), &st.title, &whats);
    }

    /// Y6: things a step leaves for a person go on the plan's one review list in the Inbox (kept up to date).
    fn review_list_add(&mut self, number: Option<String>, title: &str, whats: &[String]) {
        if whats.is_empty() {
            return;
        }
        let list = self.run.review.get_or_insert_with(Vec::new);
        for what in whats {
            let item = LookItem {
                number: number.clone(),
                title: title.to_string(),
                what: what.clone(),
            };
            if !list.contains(&item) {
                list.push(item);
            }
        }
        let pid = self.env.project_id().to_string();
        let list_key = super::review::key(&pid, &self.s.file);
        // What an earlier run put there and nobody answered yet stays on the list.
        let mut entries =
            super::review::open_entries(self.env.inbox_by_key(&list_key).ok().flatten().as_ref());
        for e in self.run.review.iter().flatten() {
            if !entries.contains(e) {
                entries.push(e.clone());
            }
        }
        let preview = self
            .env
            .project_settings()
            .map(|p| p.preview_url)
            .unwrap_or_default();
        let item = super::review::inbox_item(
            &pid,
            &self.s.project_name,
            &self.s.file,
            &self.run.id,
            &preview,
            &entries,
        );
        let _ = self.env.inbox_add(&item);
        self.decide(
            "review",
            "Qanuvi",
            &format!("Step {}", number.as_deref().unwrap_or(title)),
            "on the review list",
            &format!("needs a person's eyes: {}", whats.join("; ")),
        );
    }

    /// Y8: the test build the step asks for ("Check: test build"): the plan's own recipe, else the project's. Err
    /// says why it cannot run (no recipe, it would reach the installed app, or the run rules forbid one of its
    /// commands), for setting the step aside.
    fn step_test_build(&self, step: &Step) -> Result<Option<crate::test_build::TestBuild>, String> {
        use crate::run_rules::Verdict;
        if !super::tasklist::step_checks(step)
            .iter()
            .any(|c| crate::test_build::names_test_build(&c.command))
        {
            return Ok(None);
        }
        let recipe = self
            .plan_test_build
            .clone()
            .filter(crate::test_build::TestBuild::is_set)
            .or_else(|| self.env.project_settings().ok().map(|p| p.test_build))
            .unwrap_or_default();
        if let Some(why) = recipe.problem() {
            return Err(why);
        }
        for c in recipe.commands() {
            if let Verdict::Never(why) =
                self.env
                    .rules_verdict(c, self.s.turns.plan_rules.as_ref(), &self.run.options.mode)
            {
                return Err(format!(
                    "its test build runs `{c}`, which the run rules never allow ({why})"
                ));
            }
        }
        Ok(Some(recipe))
    }

    /// Y3: puts back what a failed step changed (from its first try's snapshot; the plan's own files stay). Answers
    /// what was done, in plain words, or None when it changed nothing or could not be undone (said in the decisions).
    fn undo_step(&mut self, i: usize) -> Option<String> {
        let k = key(&self.run.steps[i]);
        let snap = self.step_snaps.get(&k)?.clone();
        match self.env.snapshot_undo(&snap.id, &snap.also, &self.s.file) {
            Ok(u) => {
                self.step_snaps.remove(&k);
                let n = u.restored.len() + u.removed.len();
                if n == 0 && u.kept.is_empty() {
                    return None;
                }
                let mut said = format!(
                    "its changes were undone ({n} file{})",
                    if n == 1 { "" } else { "s" }
                );
                if !u.moved_to.is_empty() {
                    said.push_str(&format!("; the files it added are kept in {}", u.moved_to));
                }
                if !u.kept.is_empty() {
                    said.push_str(&format!(
                        "; not undone (binary or too large): {}",
                        u.kept.join(", ")
                    ));
                }
                self.decide(
                    "undo",
                    "Qanuvi",
                    &format!("Step {k}"),
                    &said,
                    "a failed step is undone, so the steps after it start from clean files",
                );
                Some(said)
            }
            Err(e) => {
                self.decide(
                    "undo",
                    "Qanuvi",
                    &format!("Step {k}"),
                    "its changes stay",
                    &format!("they could not be undone: {e}"),
                );
                None
            }
        }
    }

    /// Y3: on the plan's own branch, commits what a finished step changed (its files only, the plan's tick too).
    /// A commit that fails stops the commits for this run, said once; the work stays in the files.
    fn commit_done(&mut self, i: usize) {
        let Some(branch) = self.branch.clone() else {
            return;
        };
        let k = key(&self.run.steps[i]);
        let Some(snap) = self.step_snaps.get(&k).cloned() else {
            return;
        };
        let paths: Vec<String> = match self.env.snapshot_changes(&snap.id, &snap.also, false, None)
        {
            Ok(c) => c.changes.into_iter().map(|c| c.path).collect(),
            Err(_) => self.run.steps[i].changed.clone().unwrap_or_default(),
        };
        let st = &self.run.steps[i];
        let message = format!(
            "{}{}\n\nPlan {}, step done in a Qanuvi run.",
            st.number
                .as_ref()
                .map(|n| format!("Step {n}: "))
                .unwrap_or_default(),
            st.title,
            self.plan_name()
        );
        match self.env.commit_step(&branch, &paths, &message) {
            Ok(Some(id)) => self
                .run
                .commits
                .get_or_insert_with(Vec::new)
                .push(StepCommit { step: k, id }),
            Ok(None) => {}
            Err(e) => {
                self.branch = None;
                self.decide(
                    "decided",
                    "Qanuvi",
                    "Git branch",
                    "no more commits in this run",
                    &format!(
                        "committing step {k} failed: {e}. The work stays in the files; Qanuvi's own copies still undo failed steps"
                    ),
                );
            }
        }
    }

    /// A step the run blocked itself (the run rules refused what it needs) is marked [!] in the plan, as an agent
    /// marks one: an open step the run counts as blocked would read as one the owner sent back.
    fn mark_blocked_in_plan(&self, i: usize) {
        let st = &self.run.steps[i];
        if let Some(now) = self.plan()
            && let Some(p) = find(&now.text, st.number.as_deref(), &st.title)
            && p.status == StepStatus::Open
            && let Ok(next) =
                set_step_status(&now.text, p.line, StepStatus::Blocked, Some(&p.title))
        {
            let _ = self.env.write(&self.s.file, &next, Some(&now.hash));
        }
    }

    fn mark_done(&mut self, i: usize) {
        let st = &self.run.steps[i];
        self.done.push(StepName {
            number: st.number.clone(),
            title: st.title.clone(),
        });
        if let Some(n) = st.number.clone() {
            self.done_numbers.insert(n);
        }
    }

    /// After a step's attempts: its result in the log, steps added meanwhile, notes, API.md, replacements, setting
    /// aside a step that needs the owner, and closing steps to run again.
    fn after_step(&mut self, i: usize) {
        self.log_result(i);
        // Paused by the owner: the run stops here, after a whole step; Resume goes on from the next open one.
        if self.pausing() && self.run.status == RunStatus::Running {
            self.sh.controls.clear_pause();
            let k = key(&self.run.steps[i]);
            self.finish(
                RunStatus::Paused,
                &format!("Paused after step {k}. Resume to go on (you can change the run's settings first)."),
            );
        }
        // Steps added while this one ran (the owner asked for them mid-step) come next.
        self.take_added(i);
        self.collect_notes(i);
        self.update_api(i);
        // Y3: on the plan's own branch, a finished step is committed there.
        if self.run.steps[i].status == RunStepStatus::Done {
            self.commit_done(i);
        }
        // A step the planner just rewrote is not the owner's problem: its replacement steps do its work, nothing
        // waits for it, and it is ticked once they are all done.
        let k = key(&self.run.steps[i]);
        let st = &self.run.steps[i];
        let replaced = st.status == RunStepStatus::Blocked
            && st.replaced_by.is_some()
            && self
                .run
                .steps
                .iter()
                .any(|s| s.replaces.as_deref() == Some(k.as_str()));
        if replaced {
            let by = st.replaced_by.clone().unwrap_or_default();
            self.run.steps[i].reason = Some(format!("replaced by steps {by}"));
        }
        if self.run.steps[i].status == RunStepStatus::Done && self.run.steps[i].replaces.is_some() {
            self.close_replaced(i);
        }
        let status = self.run.steps[i].status;
        if !replaced
            && self.run.status == RunStatus::Running
            && matches!(status, RunStepStatus::Blocked | RunStepStatus::Failed)
        {
            let kind = if status == RunStepStatus::Blocked {
                HeldKind::Blocked
            } else {
                HeldKind::Failed
            };
            let mut why = self.run.steps[i]
                .reason
                .clone()
                .unwrap_or_else(|| "it needs you".into());
            // Y3: a failed step is undone (only its own changes), so the steps after it start from clean files.
            if kind == HeldKind::Failed
                && let Some(undone) = self.undo_step(i)
            {
                why = format!("{why}; {undone}");
                self.run.steps[i].reason = Some(why.clone());
            }
            self.set_aside(i, kind, &why);
        }
        // Finished after its section's closing step already ran (it was set aside): the closing step runs once more,
        // once the rest of the section is through.
        if self.run.steps[i].status == RunStepStatus::Done && self.run.status == RunStatus::Running
        {
            self.after_done(i);
        }
    }

    fn budget(&self) -> Budget {
        Budget {
            monthly_usd: self.s.monthly_budget_usd,
            spent_usd: self.spent_before + self.run_cost(),
        }
    }

    fn run_cost(&self) -> f64 {
        run_totals(
            &self.run.steps,
            self.run.decider_calls.as_deref().unwrap_or(&[]),
            self.run.finish.as_ref(),
        )
        .cost_usd
    }

    fn patch_run_settings(&self, patch: Value) {
        let mut now = match self.env.plan_data_get(&self.s.file, Kind::Run) {
            Some(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        if let Value::Object(p) = patch {
            now.extend(p);
        }
        let _ = self
            .env
            .plan_data_set(&self.s.file, Kind::Run, &Value::Object(now));
    }

    /// Keeps the owner's model choice for the step in the plan's own data (R2e i), not in the plan file. A "- Model:"
    /// line the step already has is taken out, as it would win over the new choice.
    fn write_step_model(&self, i: usize, model: &str) {
        let st = &self.run.steps[i];
        let mut state = self.load_state();
        state.set_kept_model(st.number.as_deref(), &st.title, Some(model));
        self.save_state(&state);
        if let Some(now) = self.plan()
            && let Some(found) = find(&now.text, st.number.as_deref(), &st.title)
            && found.model.is_some()
            && let Ok(next) = set_step_model(&now.text, found.line, None, Some(&found.title))
        {
            // The choice still holds (plan data); only the file keeps its old line when this fails.
            let _ = self.env.write(&self.s.file, &next, Some(&now.hash));
        }
    }

    /// After an attempt: keep its session for the next step, or close it.
    fn settle_carry(&mut self, attempt: &Attempt, section: &str, continued: bool) {
        // A step paused in the middle keeps its session for Resume, also with carry-on off (the window closed it
        // here before it got to the pause).
        if self.watch.pause_now {
            return;
        }
        let id = attempt.session_id.clone();
        if id.is_empty()
            || !self.last_idle
            || self.s.options.carry_on == Some(false)
            || attempt.outcome != Outcome::Open
        {
            if !id.is_empty() && self.last_idle {
                self.env.agent_stop(&id);
            }
            self.carry = None;
            return;
        }
        let lines = self.env.session_lines(&id);
        let steps = if continued {
            self.carry.as_ref().map_or(0, |c| c.1.steps)
        } else {
            0
        } + 1;
        self.carry = Some((
            id,
            Carry {
                model: attempt.model.clone(),
                section: section.to_string(),
                tokens: g::context_size(&lines),
                steps,
            },
            self.last_totals,
        ));
    }

    /// Closes the carried session (the run ends, or the next step may not continue it).
    fn drop_carry(&mut self) {
        if let Some((id, _, _)) = self.carry.take() {
            self.env.agent_stop(&id);
        }
    }

    /// When the usage limit this attempt met resets (milliseconds), or None when it met none.
    fn limit_reset(&self, a: &Attempt) -> Option<i64> {
        if a.session_id.is_empty() {
            return None;
        }
        let lines = self.env.session_lines(&a.session_id);
        let now = jiff::Timestamp::from_millisecond(self.env.now_ms()).ok()?;
        // Only the agent's own error end can be a limit (limit_notice), never what the step's work says or reads.
        for l in lines.iter().rev().take(8) {
            if let Some(notice) = limit_notice(l)
                && let Some(at) = limit_reset_at(&notice, now)
            {
                return Some(at);
            }
        }
        None
    }

    /// Y7: the proxy the paid agent's turns go through: the run's own address, else the one Claude Code's settings
    /// on this machine name (for Claude Code only).
    fn proxy_url(&self) -> Option<String> {
        let own = self
            .s
            .turns
            .base_url
            .clone()
            .filter(|u| !u.trim().is_empty());
        own.or_else(|| {
            (self.s.turns.provider == "claude-code" && self.s.turns.agent_override.is_none())
                .then(|| self.env.agent_proxy())
                .flatten()
                .filter(|u| !u.trim().is_empty())
        })
    }

    /// Y7: what was down when this attempt ended, from its agent's own error end (never what its work says), or
    /// None. A proxy that does not answer then is named; otherwise it was the provider.
    fn outage_of(&self, a: &Attempt) -> Option<Down> {
        if a.session_id.is_empty() {
            return None;
        }
        let lines = self.env.session_lines(&a.session_id);
        let notice = lines
            .iter()
            .rev()
            .take(8)
            .find_map(|l| health::outage_notice(l))?;
        Some(match self.proxy_url() {
            Some(url) if !self.env.reachable(&url) => Down::Proxy(url),
            _ => Down::Provider(notice),
        })
    }

    /// Y7: waits while it is down (Stop or Pause ends it): a proxy until its port answers again, the provider for a
    /// while that grows with the outages in a row (then the step is tried again). Said once per outage, in the
    /// run's notices; the wait goes into the decisions. Answers false when the owner stopped or paused the run.
    fn wait_healthy(&mut self, down: &Down, step: &str, in_a_row: u32) -> bool {
        let says = down.says();
        self.sh.live().waiting_health = Some(says.clone());
        if !self.outage_said {
            self.outage_said = true;
            let mut first = says.clone();
            if let Some(c) = first.get(..1) {
                first = c.to_uppercase() + &first[1..];
            }
            self.sh.notice(format!(
                "{first}: the run waits and goes on by itself when it is back."
            ));
        } else {
            self.sh.touch();
        }
        let started = self.env.now_ms();
        let wait_ms = health::provider_wait(in_a_row).as_millis() as i64;
        let poll_ms = health::PROXY_POLL.as_millis() as i64;
        let proxy = match down {
            Down::Proxy(url) => Some(url.clone()),
            Down::Provider(_) => self.proxy_url(),
        };
        let mut looked = started;
        let back = loop {
            if self.stop_asked() || self.pausing() {
                break false;
            }
            let now = self.env.now_ms();
            let waited_enough = matches!(down, Down::Proxy(_)) || now - started >= wait_ms;
            if waited_enough && now - looked >= poll_ms.min(wait_ms) {
                looked = now;
                if proxy.as_deref().is_none_or(|u| self.env.reachable(u)) {
                    break true;
                }
            }
            self.env.sleep(Duration::from_secs(5));
        };
        let minutes = ((self.env.now_ms() - started) as f64 / 60_000.0).round();
        let detail = match down {
            Down::Provider(notice) => format!("{says} ({notice})"),
            Down::Proxy(_) => says.clone(),
        };
        self.decide(
            "health",
            "Qanuvi",
            &format!("Step {step}"),
            if back {
                "waited, then went on"
            } else {
                "waited, then stopped"
            },
            &format!("{detail}; waited {minutes} min"),
        );
        self.sh.live().waiting_health = None;
        self.sh.touch();
        back && !self.stop_asked()
    }

    /// Y7: the owner paused or stopped the run while it waited for the proxy or provider.
    fn end_health_wait(&mut self) {
        if self.pausing() {
            self.sh.controls.clear_pause();
            self.finish(
                RunStatus::Paused,
                "Paused while waiting for the proxy or the model provider. Resume when you want to go on.",
            );
        } else {
            self.finish(
                RunStatus::Stopped,
                "Stopped by you while waiting for the proxy or the model provider to come back.",
            );
        }
    }

    /// Waits until the moment (Stop or Pause ends it); answers false when the owner stopped or paused the run.
    fn wait_until(&self, at: i64) -> bool {
        self.sh.live().waiting_until = Some(at);
        self.sh.touch();
        let reached = loop {
            if self.env.now_ms() >= at {
                break true;
            }
            if self.stop_asked() || self.pausing() {
                break false;
            }
            self.env.sleep(Duration::from_secs(5));
        };
        self.sh.live().waiting_until = None;
        self.sh.touch();
        reached && !self.stop_asked()
    }

    // ---------- The step's log and the plan's knowledge ----------

    /// Adds an attempt to the step's log: model, outcome, cost, tests, the failure and the agent's last answer.
    fn log_attempt(&self, i: usize, a: &mut Attempt) {
        let st = &self.run.steps[i];
        let path = g::log_path(&self.s.file, st.number.as_deref(), &st.title);
        if st.attempts.is_empty() && self.env.read(&path).is_err() {
            let n = st
                .number
                .as_ref()
                .map(|n| format!("{n}. "))
                .unwrap_or_default();
            let _ = self.env.log_append(
                &path,
                &format!("# Step {n}{}\n\nPlan: {}\n", st.title, self.s.file),
            );
        }
        let lines = if a.session_id.is_empty() {
            vec![]
        } else {
            self.env.session_lines(&a.session_id)
        };
        a.tools = Some(tool_kinds(&lines));
        let texts = texts_of(&lines);
        let entry = g::attempt_entry(&g::AttemptLog {
            when: self.env.now_ms(),
            model: a.model.clone(),
            outcome: outcome_word(a.outcome),
            cost_usd: a.cost_usd,
            seconds: a.seconds,
            tests_passed: a.tests_passed,
            note: Some(a.note.clone()).filter(|n| !n.is_empty()),
            failure: a.failure.clone(),
            answer: texts.last().cloned(),
        });
        let _ = self.env.log_append(&path, &entry);
    }

    /// The owner's messages in the step's chat (R2c part 2): each goes into the step's log and the run's decisions
    /// with how it was handled; a change wanted as its own step is written into the plan after this step (the run
    /// takes it next).
    fn take_messages(&mut self, i: usize) {
        let messages = std::mem::take(&mut self.watch.messages);
        let new_steps = std::mem::take(&mut self.watch.new_steps);
        if messages.is_empty() && new_steps.is_empty() {
            return;
        }
        let st = self.run.steps[i].clone();
        let path = g::log_path(&self.s.file, st.number.as_deref(), &st.title);
        let subject = format!("Step {}", key(&st));
        for m in &messages {
            let time = jiff::Timestamp::from_millisecond(m.at)
                .map(|t| t.strftime("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            let _ = self.env.log_append(&path, &ownermsg::log_entry(m, &time));
            self.decide(
                "owner message",
                &m.by,
                &subject,
                m.kind.word(),
                &format!("\"{}\": {}", m.text, m.handled),
            );
            // A standing rule (24z2): the plan's memory keeps it for every later step and run, on any model.
            let rule = js::cut(
                &m.text.split_whitespace().collect::<Vec<_>>().join(" "),
                400,
            )
            .to_string();
            if m.kind == ownermsg::Kind::Rule && !rule.is_empty() && !g::looks_secret(&rule) {
                let notes = self.run.notes.get_or_insert_with(Vec::new);
                if !notes.contains(&rule) {
                    notes.push(rule.clone());
                    let _ = self
                        .env
                        .log_append(&g::facts_path(&self.s.file), &ownermsg::rule_fact(&rule));
                }
            }
        }
        let archive = self
            .env
            .read(&archive_path(&self.s.file))
            .map(|f| f.text)
            .unwrap_or_default();
        for n in &new_steps {
            let Some(now) = self.plan() else { break };
            match ownermsg::add_step(&now.text, &archive, st.number.as_deref(), &st.title, n) {
                Ok((next, label)) => {
                    if self.env.write(&self.s.file, &next, Some(&now.hash)).is_ok() {
                        let _ = self.env.log_append(
                            &path,
                            &format!("\n- The owner's change became step {label}: {}.\n", n.title),
                        );
                    }
                }
                Err(e) => {
                    let _ = self.env.log_append(
                        &path,
                        &format!(
                            "\n- The owner's change \"{}\" could not be added as a step: {e}\n",
                            n.text
                        ),
                    );
                }
            }
        }
        self.save();
    }

    /// Adds the step's result (done, blocked, skipped with why) to its log.
    fn log_result(&self, i: usize) {
        let st = &self.run.steps[i];
        if matches!(st.status, RunStepStatus::Waiting | RunStepStatus::Running) {
            return;
        }
        let files = match &st.changed {
            Some(c) if !c.is_empty() => format!("Files changed: {}\n", c.join(", ")),
            _ => String::new(),
        };
        let _ = self.env.log_append(
            &g::log_path(&self.s.file, st.number.as_deref(), &st.title),
            &(g::result_entry(&step_word(st.status), st.reason.as_deref()) + &files),
        );
    }

    /// Writes (or with None removes) the step's "- Skipped:" line with the reason (24f).
    fn note_skipped(&self, i: usize, reason: Option<&str>) {
        let st = &self.run.steps[i];
        let Some(now) = self.plan() else { return };
        let Some(found) = find(&now.text, st.number.as_deref(), &st.title) else {
            return;
        };
        if let Ok(next) = set_step_skipped(&now.text, found.line, reason, Some(&found.title))
            && next != now.text
        {
            // The list changed meanwhile: the reason stays in the run's record.
            let _ = self.env.write(&self.s.file, &next, Some(&now.hash));
        }
    }

    /// The "Blocked: ..." line the agent wrote under a step it marked blocked, without the label.
    fn blocked_reason(&self, i: usize) -> String {
        let st = &self.run.steps[i];
        let Some(now) = self.plan() else {
            return String::new();
        };
        let Some(found) = find(&now.text, st.number.as_deref(), &st.title) else {
            return String::new();
        };
        found
            .details
            .iter()
            .map(|d| d.trim())
            .find(|d| d.to_lowercase().starts_with("- blocked:"))
            .map(|d| {
                let rest = d["- blocked:".len()..].trim_start();
                rest.chars().take(300).collect()
            })
            .unwrap_or_default()
    }

    /// The finished step numbers of other plans that a step's needs name ("file#number"), by file.
    fn other_plans(&self, needs: &[String]) -> HashMap<String, HashSet<String>> {
        let mut out = HashMap::new();
        for need in needs {
            let Some(h) = need.rfind('#').filter(|&h| h > 0) else {
                continue;
            };
            let other = &need[..h];
            if out.contains_key(other) {
                continue;
            }
            if let Ok(f) = self.env.read(other) {
                let a = self.env.read(&archive_path(other)).ok().map(|a| a.text);
                out.insert(
                    other.to_string(),
                    done_numbers(&[Some(&f.text), a.as_deref()])
                        .into_iter()
                        .collect(),
                );
            }
        }
        out
    }

    /// Notes the step's agent left for later steps ("Note for the next steps: ...").
    fn collect_notes(&mut self, i: usize) {
        let Some(last) = self.run.steps[i].attempts.last() else {
            return;
        };
        if last.session_id.is_empty() {
            return;
        }
        let texts = texts_of(&self.env.session_lines(&last.session_id));
        let tail = texts[texts.len().saturating_sub(3)..].join("\n");
        let have = self.run.notes.clone().unwrap_or_default();
        let found: Vec<String> = g::notes_in(&tail)
            .into_iter()
            .filter(|n| !have.contains(n))
            .collect();
        if found.is_empty() {
            return;
        }
        self.run
            .notes
            .get_or_insert_with(Vec::new)
            .extend(found.iter().cloned());
        self.save();
        let lines: String = found.iter().map(|n| format!("- {n}\n")).collect();
        let _ = self.env.log_append(&g::facts_path(&self.s.file), &lines);
    }

    /// API.md: what the plan added, from Qanuvi's code index for the files the step changed (no model tokens).
    fn update_api(&self, i: usize) {
        let code: Vec<String> = self.run.steps[i]
            .changed
            .iter()
            .flatten()
            .filter(|p| g::is_code(p))
            .take(30)
            .cloned()
            .collect();
        if code.is_empty() {
            return;
        }
        let updates: Vec<(String, String)> = code
            .iter()
            .map(|p| {
                let section = if self.env.read(p).is_ok() {
                    let items: Vec<g::ApiItem> = self
                        .env
                        .outline(p)
                        .into_iter()
                        .map(|o| g::ApiItem {
                            name: o.name,
                            kind: o.kind,
                            line: o.line,
                            container: o.container,
                        })
                        .collect();
                    g::api_section(p, &items)
                } else {
                    String::new()
                };
                (p.clone(), section)
            })
            .collect();
        let path = g::api_path(&self.s.file);
        let now = self.env.read(&path).ok();
        let text = g::merge_api(now.as_ref().map_or("", |n| n.text.as_str()), &updates);
        let _ = self
            .env
            .write(&path, &text, now.as_ref().map(|n| n.hash.as_str()));
    }

    /// Where the code the step mentions is (from the index): one line per definition, outlines of its files.
    fn context(&self, step: &Step) -> String {
        static VENDOR: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(
                r"(?i)(^|/)(vendor|node_modules|tinymce|dist|build|third[-_]?party)/|\.min\.js$",
            )
            .unwrap()
        });
        let m = packet::mentions(
            &[
                step.title.clone(),
                step.note.clone(),
                step.details.join("\n"),
            ]
            .join("\n"),
        );
        let file = &self.s.file;
        // The plan's own folder only (a plugin's plan means the plugin's code), and never vendor code.
        let home = match file.rfind('/') {
            Some(i) => file[..=i].to_string(),
            None => String::new(),
        };
        let mut defs = vec![];
        for name in m.names.iter().take(8) {
            let found: Vec<_> = self
                .env
                .find_symbols(name)
                .into_iter()
                .filter(|h| !VENDOR.is_match(&h.path))
                .collect();
            let hits: Vec<_> = if home.is_empty() {
                found
            } else {
                found
                    .into_iter()
                    .filter(|h| h.path.starts_with(&home))
                    .collect()
            };
            let exact: Vec<_> = hits.iter().filter(|h| &h.name == name).cloned().collect();
            let pick = if exact.is_empty() {
                hits.into_iter().take(1).collect()
            } else {
                exact
            };
            defs.extend(pick.into_iter().take(2));
        }
        let mut outlines = vec![];
        let mut new_files = vec![];
        for path in m.paths.iter().take(6) {
            if self.env.read(path).is_ok() {
                outlines.push(Outline {
                    path: path.clone(),
                    items: self.env.outline(path),
                });
            } else {
                new_files.push(path.clone());
            }
        }
        packet::location_block(&defs, &outlines, &new_files, 6000)
    }

    /// Puts steps added to the plan while a step ran right after it in the run, in the plan's order.
    fn take_added(&mut self, i: usize) {
        let Some(now) = self.plan() else { return };
        let all = parse_plan(&now.text).steps;
        let in_run: HashSet<String> = self
            .run
            .steps
            .iter()
            .map(|s| g::step_key(s.number.as_deref(), &s.title))
            .collect();
        // Earlier steps the owner sent the run back to come first, then steps added while this one ran.
        let back: Vec<Step> = g::reopened_steps(&self.run.steps, &all, &self.reopened)
            .into_iter()
            .cloned()
            .collect();
        for s in &back {
            self.reopened
                .insert(g::step_key(s.number.as_deref(), &s.title));
        }
        let mut added = back;
        added.extend(
            g::added_steps(&all, &self.known)
                .into_iter()
                .filter(|s| !in_run.contains(&g::step_key(s.number.as_deref(), &s.title)))
                .cloned(),
        );
        for s in &all {
            self.known
                .insert(g::step_key(s.number.as_deref(), &s.title));
        }
        if added.is_empty() {
            return;
        }
        let rows: Vec<StepRun> = added.iter().map(|s| new_step_run(s, None)).collect();
        self.steps.extend(added);
        self.run.steps.splice(i + 1..i + 1, rows);
        self.save();
    }

    // ---------- Held steps ----------

    /// The newest answer in the Inbox to a held step's question, if it was answered.
    fn inbox_answer(&self, step: &str) -> Option<(HoldReply, String)> {
        let k = self.hold_key(step);
        let item = self.env.inbox_by_key(&k).ok().flatten()?;
        if item.status != "answered" {
            return None;
        }
        let a: InboxAnswer = serde_json::from_value(item.answer?).ok()?;
        Some((hold_reply(&a), k))
    }

    /// Answers the owner gave since the last look (in the window, or in the Inbox from any window): the held steps
    /// they settle go on or are skipped. Says whether any came.
    fn take_answers(&mut self) -> bool {
        let mut got = std::mem::take(&mut self.sh.live().answers);
        for st in self
            .run
            .steps
            .iter()
            .filter(|s| s.status == RunStepStatus::Held)
        {
            let k = key(st);
            if got.iter().any(|(s, _)| *s == k) {
                continue;
            }
            if let Some((reply, _)) = self.inbox_answer(&k) {
                got.push((k, reply));
            }
        }
        let mut any = false;
        for (step, reply) in got {
            any |= self.answer(&step, reply);
        }
        any
    }

    /// Questions the check left open that nobody answered in `limit_ms`: each is decided as "Let Qanuvi decide"
    /// (the safer way, said in the step's summary), and the Inbox says so. Risky steps the owner chose to see first
    /// and blocked steps keep waiting. Says whether any was decided.
    fn decide_unanswered(&mut self, limit_ms: i64) -> bool {
        let open: Vec<(String, String, String)> = self
            .run
            .steps
            .iter()
            .filter(|s| s.status == RunStepStatus::Held)
            .filter_map(|s| {
                let h = s.held.as_ref()?;
                (h.kind == HeldKind::Hold && h.why.starts_with(preflight::OPEN_QUESTION))
                    .then(|| (key(s), s.title.clone(), h.why.clone()))
            })
            .collect();
        let mins = (limit_ms + 59_999) / 60_000;
        let mut any = false;
        for (k, title, why) in open {
            let reply = hold_reply(&InboxAnswer {
                choice: holdask::AnswerChoice::Decide,
                text: None,
            });
            if !self.answer(&k, reply) {
                continue;
            }
            any = true;
            let project = if self.s.project_name.is_empty() {
                String::new()
            } else {
                format!("{}: ", self.s.project_name)
            };
            let _ = self.env.inbox_add(&NewItem {
                kind_of: "info".into(),
                kind: "run".into(),
                title: format!(
                    "{project}{} step {k}. {title}: decided with no answer",
                    self.plan_name()
                ),
                text: format!(
                    "No answer came in {mins} minutes, and nothing else in the run could go on. Qanuvi decided it the \
                     safer way and went on; the step's summary says what it chose.\n\n{}",
                    sentence_start(&why)
                ),
                key: None,
                open: Some(self.open_plan()),
                form: None,
                source: None,
            });
            self.sh.notice(format!(
                "Step {k}: no answer in {mins} minutes, so Qanuvi decided it the safer way."
            ));
        }
        any
    }

    /// The owner answers a held step: "go" lets it run (with the answer, if any, told to it), "skip" leaves it and the
    /// steps waiting for it.
    fn answer(&mut self, step: &str, reply: HoldReply) -> bool {
        let Some(i) = self
            .run
            .steps
            .iter()
            .position(|x| key(x) == step && x.status == RunStepStatus::Held)
        else {
            return false;
        };
        // n1: "End the run": what can still run does, then the run ends; the step stays open for the next run.
        if reply.choice == holdask::AnswerChoice::End {
            self.sh.live().finish_without = true;
            return false;
        }
        // n1: a typed answer is read before anything runs again (n2: a held step's too, every reply by meaning).
        // "Let Qanuvi decide" with no words of their own has nothing to read.
        let typed = reply.choice == holdask::AnswerChoice::Go
            && !reply.text.is_empty()
            && reply.text != holdask::DECIDE_TEXT;
        let mut instruction = None;
        if typed {
            match self.read_answer(i, &reply.text) {
                ownerask::Reading::Act {
                    instruction: how,
                    fact,
                    write_dir,
                } => {
                    instruction = Some(how).filter(|h| !h.is_empty());
                    if let Some(f) = fact {
                        self.keep_owner_fact(&f.title, &f.text);
                    }
                    if let Some(dir) = write_dir {
                        self.allow_write_dir(&dir);
                    }
                }
                ownerask::Reading::Clarify(card) | ownerask::Reading::FollowUp(card) => {
                    self.ask_again(i, &reply.text, card);
                    return false;
                }
            }
        }
        if typed {
            let asked = self.run.steps[i]
                .held
                .as_ref()
                .and_then(|h| h.card.as_ref())
                .map(|c| c.headline.clone());
            if let Some(asked) = asked {
                self.run.steps[i]
                    .talk
                    .get_or_insert_with(Vec::new)
                    .push(ownerask::Exchange {
                        asked,
                        answer: reply.text.clone(),
                    });
            }
        }
        self.run.steps[i].instruction = instruction;
        // Kept for this plan: a run started again later does not ask again (24u part 2).
        let state = planstate::keep_answer(
            &self.load_state(),
            step,
            plan_choice(reply.choice),
            &reply.text,
            self.env.now_ms() as f64,
        );
        self.save_state(&state);
        // Its question is settled everywhere (asked again later, it is a new question).
        let _ = self.env.inbox_resolve(&self.hold_key(step));
        let skip = reply.choice == holdask::AnswerChoice::Skip;
        for x in self.run.steps.iter_mut() {
            if x.status != RunStepStatus::SetAside
                || x.waits_for.as_ref().is_none_or(|w| w.step != step)
            {
                continue;
            }
            if skip {
                x.status = RunStepStatus::Skipped;
                x.reason = Some(format!("needs {step}, which you skipped"));
            } else {
                x.status = RunStepStatus::Waiting;
                x.waits_for = None;
            }
        }
        // The real end-to-end check skipped: the plan's work was not proven for real. It goes on the list to look at,
        // and the run ends "needs you", not "finished".
        let skipped_end_check = skip
            && self.run.steps[i]
                .held
                .as_ref()
                .is_some_and(|h| h.why.starts_with(super::preflight::END_TO_END));
        if skipped_end_check {
            let w = skipped_end_check_words(&self.run.steps[i].held.as_ref().unwrap().why);
            if !self.left_open.contains(&w) {
                self.left_open.push(w.clone());
            }
            let (number, title) = (
                self.run.steps[i].number.clone(),
                self.run.steps[i].title.clone(),
            );
            self.review_list_add(number, &title, &[w]);
        }
        let st = &mut self.run.steps[i];
        if skip {
            st.status = RunStepStatus::Skipped;
            st.reason = Some(if skipped_end_check {
                "you chose to skip the real end-to-end check (on the list to look at)".into()
            } else {
                "you chose to skip it".into()
            });
        } else {
            st.status = RunStepStatus::Waiting;
            st.answer = Some(reply.text.clone());
            st.retry_from = Some(st.attempts.len());
        }
        self.save();
        true
    }

    /// A held step becomes a question in the Inbox with its answer form (24u part 4), seen in every window.
    fn ask_owner(&self, i: usize, kind: HeldKind, why: &str) {
        let st = &self.run.steps[i];
        let k = key(st);
        let what = match kind {
            HeldKind::Hold => "needs your answer",
            HeldKind::Blocked => "is blocked",
            _ => "did not finish",
        };
        let n = st
            .number
            .as_ref()
            .map(|n| format!("{n}. "))
            .unwrap_or_default();
        let pid = self.env.project_id().to_string();
        // Blocked or not finished after a try is a new question, even in the same words: an earlier answered one goes
        // first, so the Inbox never takes it for the old question asked again (no notification, old answer shown).
        // A hold asked again at a run's start keeps the owner's first answer.
        if kind != HeldKind::Hold {
            let _ = self.env.inbox_resolve(&self.hold_key(&k));
        }
        let card = st.held.as_ref().and_then(|h| h.card.clone());
        let text = card
            .as_ref()
            .map(ownerask::card_text)
            .unwrap_or_else(|| sentence_start(why));
        let mut form = hold_form(kind, why);
        form.card = card;
        let _ = self.env.inbox_add(&NewItem {
            kind_of: "answer".into(),
            kind: "run".into(),
            title: format!(
                "{}: {} step {n}{} {what}",
                self.s.project_name,
                self.plan_name(),
                st.title
            ),
            text,
            key: Some(self.hold_key(&k)),
            form: serde_json::to_value(form).ok(),
            source: Some(
                json!({"kind": "hold", "projectId": pid, "file": self.s.file, "runId": self.run.id,
                "step": k, "held": kind}),
            ),
            open: Some(self.open_plan()),
        });
    }

    /// n1: the agent's last message in the step's latest try (its final answer, else its last text).
    fn last_message(&self, i: usize) -> String {
        let Some(a) = self.run.steps[i]
            .attempts
            .iter()
            .rev()
            .find(|a| !a.session_id.is_empty())
        else {
            return String::new();
        };
        let lines = self.env.session_lines(&a.session_id);
        let last_text = lines
            .iter()
            .rev()
            .find_map(|l| super::env::assistant_text(l))
            .unwrap_or_default();
        lines
            .iter()
            .rev()
            .find_map(|l| super::env::result_of(l).filter(|(t, err)| !err && !t.trim().is_empty()))
            .map(|(t, _)| super::env::final_text(t, &last_text))
            .unwrap_or(last_text)
    }

    /// n1: one short call for the owner's question or the reading of their answer (the decider's way: the run's
    /// model, else a free one); None when nobody could answer.
    fn owner_call(&mut self, prompt: &str, title: &str) -> Option<String> {
        let model = if self.s.default_model.is_empty() {
            self.s.planner_model.clone()
        } else {
            self.s.default_model.clone()
        };
        match self.ask(prompt, &model, title, true) {
            Ok((text, cost)) => {
                self.decider_call(&model, cost);
                Some(text)
            }
            Err(_) => self
                .free_decide(
                    "You write short, plain questions for a person who may not be a programmer. Answer with JSON only.",
                    prompt,
                )
                .ok()
                .map(|(_, t)| t),
        }
    }

    /// n1: what the calls read about step `i`: (step, details, last message, files changed, talk).
    fn facts_of(
        &self,
        i: usize,
    ) -> (
        String,
        Vec<String>,
        String,
        Vec<String>,
        Vec<ownerask::Exchange>,
    ) {
        let st = &self.run.steps[i];
        let n = st
            .number
            .as_ref()
            .map(|n| format!("{n}. "))
            .unwrap_or_default();
        let details = self
            .plan()
            .and_then(|now| find(&now.text, st.number.as_deref(), &st.title))
            .map(|s| s.details)
            .unwrap_or_default();
        (
            format!("{n}{}", st.title),
            details,
            self.last_message(i),
            st.changed.clone().unwrap_or_default(),
            st.talk.clone().unwrap_or_default(),
        )
    }

    /// n1: the question for a blocked or unfinished step, standing on its own.
    fn make_card(&mut self, i: usize, why: &str) -> ownerask::Card {
        let (step, details, last, changed, talk) = self.facts_of(i);
        let f = ownerask::StepFacts {
            step: &step,
            details: &details,
            why,
            last_message: &last,
            changed: &changed,
            talk: &talk,
        };
        let k = key(&self.run.steps[i]);
        let memory = projectfacts::memory_block(&self.env.project_memory());
        let prompt = if memory.is_empty() {
            ownerask::explain_prompt(&f)
        } else {
            format!(
                "{}\n\n{memory}\nWhen the memory answers what the step needs, the card asks only for a yes to using it.",
                ownerask::explain_prompt(&f)
            )
        };
        self.owner_call(&prompt, &format!("Question for you: step {k}"))
            .and_then(|t| ownerask::parse_card(&t))
            .unwrap_or_else(|| ownerask::fallback_card(&f))
    }

    /// n1/n2: what the owner's typed answer means, read by meaning. Without any model it is acted on: the step's own
    /// agent reads the words (and is told to answer a question back rather than do the work).
    fn read_answer(&mut self, i: usize, answer: &str) -> ownerask::Reading {
        let (step, details, last, changed, talk) = self.facts_of(i);
        let held = self.run.steps[i].held.clone();
        let why = held.as_ref().map(|h| h.why.clone()).unwrap_or_default();
        let f = ownerask::StepFacts {
            step: &step,
            details: &details,
            why: &why,
            last_message: &last,
            changed: &changed,
            talk: &talk,
        };
        let card = held
            .and_then(|h| h.card)
            .unwrap_or_else(|| ownerask::fallback_card(&f));
        let k = key(&self.run.steps[i]);
        let asked_now = card.headline.clone();
        match self
            .owner_call(
                &ownerask::reading_prompt(&f, &card, answer),
                &format!("Your answer: step {k}"),
            )
            .and_then(|t| ownerask::parse_reading(&t))
        {
            // n2: a new question is never the same words as one already asked.
            Some(ownerask::Reading::Clarify(c)) => {
                ownerask::Reading::Clarify(ownerask::not_repeated(c, &asked_now, &talk))
            }
            Some(ownerask::Reading::FollowUp(c)) => {
                ownerask::Reading::FollowUp(ownerask::not_repeated(c, &asked_now, &talk))
            }
            Some(r) => r,
            // n2: no model could read it, and no phrase list guesses: the step's own agent reads the owner's words.
            None => ownerask::Reading::Act {
                instruction: String::new(),
                fact: None,
                write_dir: None,
            },
        }
    }

    /// n1: the answer did not settle the need: the talk is kept, the step stays held and asks `card` instead.
    fn ask_again(&mut self, i: usize, answer: &str, card: ownerask::Card) {
        let k = key(&self.run.steps[i]);
        let st = &mut self.run.steps[i];
        let asked = st
            .held
            .as_ref()
            .and_then(|h| h.card.as_ref())
            .map(|c| c.headline.clone())
            .unwrap_or_else(|| st.held.as_ref().map(|h| h.why.clone()).unwrap_or_default());
        st.talk
            .get_or_insert_with(Vec::new)
            .push(ownerask::Exchange {
                asked,
                answer: answer.to_string(),
            });
        let Some(h) = st.held.as_mut() else {
            return;
        };
        h.card = Some(card);
        let (kind, why) = (h.kind, h.why.clone());
        // The answered question is closed and the new one asked (a new Inbox item, so it notifies).
        let _ = self.env.inbox_resolve(&self.hold_key(&k));
        self.ask_owner(i, kind, &why);
        self.save();
    }

    /// Sets a step aside for the owner's answer (24u), and the open steps that need it: the plan says so first
    /// (Needs, same step number, shared files), then one short decider call for the rest. A section's closing step is
    /// never set aside. The run then goes on with what is left.
    fn set_aside(&mut self, i: usize, kind: HeldKind, why: &str) {
        {
            let st = &mut self.run.steps[i];
            st.status = RunStepStatus::Held;
            st.held = Some(Held {
                kind,
                why: why.to_string(),
                card: None,
            });
            st.answer = None;
        }
        let k = key(&self.run.steps[i]);
        // n1: a blocked or unfinished step asks a question that stands on its own.
        if kind != HeldKind::Hold {
            let card = self.make_card(i, why);
            if let Some(h) = self.run.steps[i].held.as_mut() {
                h.card = Some(card);
            }
        }
        self.ask_owner(i, kind, why);
        let plan_steps = self
            .plan()
            .map(|n| parse_plan(&n.text).steps)
            .unwrap_or_else(|| self.steps.clone());
        let run_steps = self.steps.clone();
        let info = |s: &StepRun| -> DepStep {
            let p = plan_steps
                .iter()
                .find(|x| x.number == s.number && x.title == s.title)
                .or_else(|| run_steps.iter().find(|x| x.line == s.line));
            DepStep {
                number: s.number.clone(),
                title: s.title.clone(),
                details: p.map(|p| p.details.clone()).unwrap_or_default(),
                needs: p.map(|p| p.needs.clone()).unwrap_or_default(),
                changed: s.changed.clone().unwrap_or_default(),
            }
        };
        let open_of = |steps: &[StepRun]| -> Vec<usize> {
            steps
                .iter()
                .enumerate()
                .filter(|(_, x)| {
                    x.status == RunStepStatus::Waiting && !is_closing_step(x.number.as_deref())
                })
                .map(|(j, _)| j)
                .collect()
        };
        // The plan first, also for steps that need a step set aside just now (72b needs 72a, which needs 95b).
        let mut outs = vec![i];
        let mut asides: Vec<(usize, String, String)> = vec![];
        let mut grew = true;
        while grew {
            grew = false;
            for x in open_of(&self.run.steps) {
                if asides.iter().any(|a| a.0 == x) {
                    continue;
                }
                let later = info(&self.run.steps[x]);
                for &h in &outs {
                    let Some(reason) = setaside::plan_dependence(&info(&self.run.steps[h]), &later)
                    else {
                        continue;
                    };
                    let why = if h == i {
                        reason
                    } else {
                        format!(
                            "{reason} (step {}, which waits for {k})",
                            key(&self.run.steps[h])
                        )
                    };
                    asides.push((x, "the plan".into(), why));
                    outs.push(x);
                    grew = true;
                    break;
                }
            }
            for (x, by, why) in asides.drain(..) {
                self.aside(x, &k, &by, &why);
            }
        }
        // Then the decider, once, for the open steps the plan says nothing about.
        let rest: Vec<usize> = open_of(&self.run.steps).into_iter().take(30).collect();
        if !rest.is_empty() {
            let held: Vec<HeldStep> = outs
                .iter()
                .map(|&h| HeldStep {
                    step: info(&self.run.steps[h]),
                    why: if h == i {
                        why.to_string()
                    } else {
                        format!("waits for {k}")
                    },
                })
                .collect();
            let open: Vec<DepStep> = rest.iter().map(|&x| info(&self.run.steps[x])).collect();
            let prompt = setaside::dependence_prompt(&held, &open);
            let keys: Vec<String> = rest.iter().map(|&x| key(&self.run.steps[x])).collect();
            let held_keys: Vec<String> = outs.iter().map(|&h| key(&self.run.steps[h])).collect();
            let model = if self.s.default_model.is_empty() {
                self.s.planner_model.clone()
            } else {
                self.s.default_model.clone()
            };
            let mut by = self.s.provider.clone();
            let text = match self.ask(
                &prompt,
                &model,
                &format!("Decider: what waits for step {k}"),
                true,
            ) {
                Ok((text, cost)) => {
                    self.decider_call(&model, cost);
                    text
                }
                Err(_) => {
                    match self.free_decide("You read plan steps. Answer with JSON only.", &prompt) {
                        Ok((who, text)) => {
                            by = who;
                            text
                        }
                        Err(_) => {
                            // Nobody could answer: the plan's own word stands, the other steps go on.
                            self.decide(
                                "set-aside",
                                "Qanuvi",
                                &format!("Step {k}"),
                                "other steps go on",
                                "the decider could not answer; only what the plan says waits",
                            );
                            String::new()
                        }
                    }
                }
            };
            for (step, d) in setaside::parse_dependence(&text, &keys, &held_keys) {
                if let Some(&x) = rest.iter().find(|&&x| key(&self.run.steps[x]) == step) {
                    let reason = if d.why.is_empty() {
                        format!("needs step {}", d.on)
                    } else {
                        d.why
                    };
                    self.aside(x, &k, &format!("decider ({by})"), &reason);
                }
            }
        }
        self.save();
    }

    fn aside(&mut self, x: usize, held: &str, by: &str, why: &str) {
        let st = &mut self.run.steps[x];
        st.status = RunStepStatus::SetAside;
        st.waits_for = Some(StepWhy {
            step: held.to_string(),
            why: why.to_string(),
        });
        let subject = format!("Step {}", key(st));
        self.decide("set-aside", by, &subject, &format!("waits for {held}"), why);
    }

    // ---------- Closing steps and replacements ----------

    /// After a step is done (24u): a closing step records what of its section is still set aside; any other step is
    /// noted as late for the closing steps that ran without it. Kept per plan, so a later run catches up too.
    fn after_done(&mut self, i: usize) {
        let k = key(&self.run.steps[i]);
        let mut state = self.load_state();
        if is_closing_step(self.run.steps[i].number.as_deref()) {
            let parsed = self
                .plan()
                .map(|p| parse_plan(&p.text).steps)
                .unwrap_or_default();
            let st = &self.run.steps[i];
            let section = parsed
                .iter()
                .find(|x| x.number == st.number && x.title == st.title)
                .map(|x| x.section.clone());
            let in_section: HashSet<String> = parsed
                .iter()
                .filter(|x| Some(&x.section) == section.as_ref())
                .map(|x| x.number.clone().unwrap_or_else(|| x.title.clone()))
                .collect();
            let pending: Vec<String> = self
                .run
                .steps
                .iter()
                .enumerate()
                .filter(|(j, x)| {
                    *j != i
                        && matches!(x.status, RunStepStatus::Held | RunStepStatus::SetAside)
                        && in_section.contains(&key(x))
                })
                .map(|(_, x)| key(x))
                .collect();
            state = planstate::closing_ran(&state, &k, &pending);
        } else {
            state = planstate::mark_late(&state, &k);
        }
        self.save_state(&state);
        self.run_closings(false);
    }

    /// Opens closing steps again that missed steps done since they ran: once per section, when nothing they wait for
    /// is still pending in this run, or when the run is about to end. A closing step done in an earlier run is added
    /// to this one. Says whether one was opened.
    fn run_closings(&mut self, ending: bool) -> bool {
        let state = self.load_state();
        let pending = |step: &str| {
            self.run.steps.iter().any(|x| {
                key(x) == step
                    && matches!(
                        x.status,
                        RunStepStatus::Held
                            | RunStepStatus::SetAside
                            | RunStepStatus::Waiting
                            | RunStepStatus::Running
                    )
            })
        };
        let due = planstate::closings_due(&state, pending, ending);
        let mut opened = false;
        for d in due {
            let at = self.run.steps.iter().position(|x| key(x) == d.closing);
            if let Some(j) = at
                && self.run.steps[j].status != RunStepStatus::Done
            {
                continue; // Waiting, running, or failed in this run: not again.
            }
            let Some(now) = self.plan() else { continue };
            let Some(s) = parse_plan(&now.text)
                .steps
                .into_iter()
                .find(|x| x.number.clone().unwrap_or_else(|| x.title.clone()) == d.closing)
            else {
                continue;
            };
            if s.status == StepStatus::Done {
                let Ok(next) = set_step_status(&now.text, s.line, StepStatus::Open, Some(&s.title))
                else {
                    continue;
                };
                if self
                    .env
                    .write(&self.s.file, &next, Some(&now.hash))
                    .is_err()
                {
                    continue; // The plan changed meanwhile: the next look tries again.
                }
            }
            let j = match at {
                Some(j) => j,
                None => {
                    let pos = self
                        .run
                        .steps
                        .iter()
                        .position(|x| x.line > s.line)
                        .unwrap_or(self.run.steps.len());
                    self.run.steps.insert(pos, new_step_run(&s, None));
                    if pos <= self.cursor && self.cursor < self.run.steps.len() - 1 {
                        self.cursor += 1;
                    }
                    pos
                }
            };
            let c = &mut self.run.steps[j];
            c.status = RunStepStatus::Waiting;
            c.retry_from = Some(c.attempts.len());
            c.rerun_after = Some(d.late.clone());
            opened = true;
        }
        if opened {
            self.save();
        }
        opened
    }

    /// A replacement step is done: once all of them are, the step they replace is ticked too.
    fn close_replaced(&mut self, i: usize) {
        let Some(k) = self.run.steps[i].replaces.clone() else {
            return;
        };
        if !replacements_done(&self.run.steps, &k) {
            return;
        }
        let Some(j) = self
            .run
            .steps
            .iter()
            .position(|s| key(s) == k && s.replaced_by.is_some())
        else {
            return;
        };
        let (number, title) = (
            self.run.steps[j].number.clone(),
            self.run.steps[j].title.clone(),
        );
        if let Some(now) = self.plan()
            && let Some(st) = find(&now.text, number.as_deref(), &title)
            && let Ok(next) = set_step_status(&now.text, st.line, StepStatus::Done, Some(&st.title))
        {
            // The list changed meanwhile: the step stays as it is in the file; the run still counts it done.
            let _ = self.env.write(&self.s.file, &next, Some(&now.hash));
        }
        let old = &mut self.run.steps[j];
        old.status = RunStepStatus::Done;
        old.reason = Some(format!(
            "done through its replacement steps {}",
            old.replaced_by.clone().unwrap_or_default()
        ));
        if let Some(n) = number {
            self.done_numbers.insert(n);
        }
        self.save();
    }

    /// Asks the planner to rewrite a step that failed on every model, writes the new steps in its place (the step is
    /// marked blocked with a note), and adds them to the run. Says whether it did.
    fn replan(&mut self, i: usize) -> bool {
        let Some(current) = self.plan() else {
            return false;
        };
        let st = self.run.steps[i].clone();
        let Some(step) = find(&current.text, st.number.as_deref(), &st.title) else {
            return false;
        };
        let mut what: Vec<String> = st
            .attempts
            .iter()
            .map(|a| {
                let note = if a.note.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", a.note)
                };
                format!("Try on {}: {}{note}", a.model, outcome_word(a.outcome))
            })
            .collect();
        what.push(
            st.attempts
                .last()
                .and_then(|a| a.failure.clone())
                .map(|f| format!("Last test failures:\n{f}"))
                .unwrap_or_default(),
        );
        self.run.steps[i].replaced_by = Some("(asking the planner)".into());
        let prompt = planner::replan_prompt(&self.s.file, &step, &what.join("\n"), &self.s.models);
        let model = self.s.planner_model.clone();
        let name = st.number.clone().unwrap_or_else(|| st.title.clone());
        let done = (|| -> Option<Vec<Step>> {
            let (text, cost) = self
                .ask(
                    &prompt,
                    &model,
                    &format!("Planner: rewrite step {name}"),
                    false,
                )
                .ok()?;
            self.decider_call(&model, cost);
            let block = planner::extract_steps(&text).block;
            if block.is_empty() {
                return None;
            }
            // Numbers no other step has, in the plan or its archive (97a → 97a-1, 97a-2, …).
            let archive = self
                .env
                .read(&archive_path(&self.s.file))
                .map(|a| a.text)
                .unwrap_or_default();
            let count = parse_plan(&block).steps.len();
            let numbered = planner::renumber_with(
                &block,
                &planner::free_labels(&current.text, &archive, step.number.as_deref(), count),
            );
            let labels = planner::label_range(&numbered);
            // The planner's model per step goes into the plan's own data, not the file (R2e i).
            let taken = planner::take_models(&numbered);
            let numbered = taken.block;
            let next = planner::replace_step(
                &current.text,
                step.number.as_deref(),
                &step.title,
                &numbered,
                &labels,
            )
            .ok()?;
            self.env
                .write(&self.s.file, &next, Some(&current.hash))
                .ok()?;
            if !taken.models.is_empty() {
                let mut state = self.load_state();
                for m in &taken.models {
                    state.set_kept_model(m.number.as_deref(), &m.title, Some(&m.model));
                }
                self.save_state(&state);
            }
            let new_numbers: Vec<Option<String>> = parse_plan(&numbered)
                .steps
                .into_iter()
                .map(|n| n.number)
                .collect();
            let added: Vec<Step> = parse_plan(&next)
                .steps
                .into_iter()
                .filter(|s| new_numbers.contains(&s.number))
                .collect();
            self.run.steps[i].replaced_by = Some(labels);
            self.run.steps[i].status = RunStepStatus::Blocked;
            Some(added)
        })();
        let Some(added) = done.filter(|a| !a.is_empty()) else {
            self.run.steps[i].replaced_by = None;
            return false;
        };
        let k = key(&st);
        let rows: Vec<StepRun> = added.iter().map(|s| new_step_run(s, Some(&k))).collect();
        for s in &added {
            self.known
                .insert(g::step_key(s.number.as_deref(), &s.title));
        }
        self.steps.extend(added);
        self.run.steps.splice(i + 1..i + 1, rows);
        self.save();
        true
    }

    // ---------- The end ----------

    /// Y10 d7: the steps this run held or set aside are looked at once: could the check before the run have found
    /// the reason? What it could have is kept as a lesson the next check is told.
    fn learn_check_lessons(&mut self) {
        let held: Vec<(String, String)> = self
            .run
            .steps
            .iter()
            .filter(|s| s.status == RunStepStatus::Blocked)
            .filter_map(|s| s.reason.clone().map(|r| (key(s), r)))
            .take(10)
            .collect();
        if held.is_empty() {
            return;
        }
        let model = self.s.planner_model.clone();
        if let Ok((text, _)) = self.ask(
            &super::check_more::lessons_prompt(&held),
            &model,
            "After the run: what the check could have found",
            true,
        ) {
            let lessons = super::check_more::parse_lessons(&text);
            if !lessons.is_empty() {
                super::check_more::keep_lessons(self.env, &lessons);
                self.decide(
                    "lessons",
                    "Qanuvi",
                    "The check before the run",
                    "learned",
                    &lessons.join("; "),
                );
            }
        }
    }

    fn end(&mut self) {
        if self.run.status == RunStatus::Running {
            // Steps still held or set aside (the owner ended the run without answering): the run is not done, it
            // needs them.
            if end_status(&self.run.steps) != RunStatus::NeedsYou && !self.left_open.is_empty() {
                // Y10 (a): promised work no step did, or the real end-to-end check skipped: not "finished".
                // The skipped end check is named in a few words (its command is on the list); work left over in full.
                let later: Vec<&str> = self
                    .left_open
                    .iter()
                    .filter(|w| !w.starts_with(SKIPPED_END_CHECK))
                    .map(String::as_str)
                    .collect();
                let skipped = later.len() < self.left_open.len();
                let mut why = Vec::new();
                if skipped {
                    why.push(
                        "you skipped the real end-to-end check, so the work is not proven for real"
                            .to_string(),
                    );
                }
                if !later.is_empty() {
                    why.push(format!(
                        "work was left for later that no step did ({})",
                        later.join("; ")
                    ));
                }
                let m = format!(
                    "Ended here: {}. Not finished: {}. It is on the list to look at in the Inbox: say there what to \
                     do, and your words become a new step.",
                    run_summary(&self.run.steps),
                    why.join("; and ")
                );
                self.finish(RunStatus::NeedsYou, &m);
            } else if end_status(&self.run.steps) == RunStatus::NeedsYou {
                let m = format!(
                    "Ended here: {}. Held steps stay open: answer them in their chat or with Run. The end-of-run review \
                     comes once they are done.",
                    run_summary(&self.run.steps)
                );
                self.finish(RunStatus::NeedsYou, &m);
            } else {
                let m = run_summary(&self.run.steps);
                self.finish(RunStatus::Done, &m);
            }
        }
        // PM b + e: what the run learned about the project is offered to the memory (not after a Stop: the owner
        // wanted it to end).
        if matches!(self.run.status, RunStatus::Done | RunStatus::NeedsYou) {
            self.learn_project_facts();
            self.learn_check_lessons();
        }
        // A run paused in a step keeps that step's session for Resume.
        if !(self.run.status == RunStatus::Paused && self.keep_for_resume) {
            self.drop_carry();
        }
        self.keep_for_resume = false;
        // The whole plan is done: its private details are deleted, unless the owner chose to keep them.
        if self.s.options.forget_private_when_done != Some(false)
            && let Some(end) = self.plan()
            && parse_plan(&end.text)
                .steps
                .iter()
                .all(|x| x.status == StepStatus::Done)
        {
            self.env.plan_private_clear(&self.s.file);
        }
        self.sh.live().current_model.clear();
        self.save();
    }
}

/// Why a step was set aside for commands the run rules refused (24y), in plain words with what to do about it.
pub fn refused_reason(refused: &[crate::run_rules::Refusal]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    let mut never = false;
    for r in refused {
        if seen.contains(&r.what.as_str()) || seen.len() >= 3 {
            continue;
        }
        seen.push(&r.what);
        never |= !r.never.is_empty();
        parts.push(if r.never.is_empty() {
            format!("`{}`", r.what)
        } else {
            format!("`{}` ({})", r.what, r.never)
        });
    }
    let what = parts.join(", ");
    if never {
        format!(
            "it needed {what}, which a run never does by itself. Do it yourself, or answer how the step can go on without it"
        )
    } else {
        format!(
            "it needed {what}, which this project's run rules do not allow. Allow it under Run rules (Details) for next time, or answer how to go on"
        )
    }
}

fn plan_choice(c: holdask::AnswerChoice) -> planstate::AnswerChoice {
    if c == holdask::AnswerChoice::Skip {
        planstate::AnswerChoice::Skip
    } else {
        planstate::AnswerChoice::Go
    }
}

fn new_step_run(s: &Step, replaces: Option<&str>) -> StepRun {
    StepRun {
        line: s.line,
        number: s.number.clone(),
        title: s.title.clone(),
        attempts: vec![],
        status: RunStepStatus::Waiting,
        held: None,
        waits_for: None,
        answer: None,
        talk: None,
        instruction: None,
        retry_from: None,
        rerun_after: None,
        job_type: None,
        choice: None,
        replaced_by: None,
        replaces: replaces.map(String::from),
        changed: None,
        reason: None,
    }
}

/// The owner's answer to a question the run asked (an Inbox item answered in any window). Today: "git cannot see",
/// answered with Qanuvi's snapshots, changes the project's setting. Called by the core when an item is answered.
pub fn on_answer(env: &dyn RunEnv, item: &crate::inbox::InboxItem) {
    let kind = item
        .source
        .as_ref()
        .and_then(|s| s.get("kind"))
        .and_then(Value::as_str);
    let choice = item
        .answer
        .as_ref()
        .and_then(|a| a.get("choice"))
        .and_then(Value::as_str);
    // PM b: "Keep all" makes the run's memory suggestions active.
    if kind == Some("memory") && choice == Some("keep") {
        let ids: Vec<String> = item
            .source
            .as_ref()
            .and_then(|s| s.get("memoryIds"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        env.memory_approve(&ids);
    }
    if kind == Some("changes-from")
        && choice == Some("qanuvi")
        && let Ok(mut s) = env.project_settings()
    {
        s.changes_from = "qanuvi".into();
        let _ = env.set_project_settings(&s);
    }
    if kind == Some("hold") {
        reopen_answered_hold(env, item);
    }
    // Y6: words typed on the review list become a step in the plan.
    if kind == Some("review") {
        let _ = super::review::on_answer(env, item);
    }
}

/// r5: a held step answered (Try again, Go ahead, Decide, or own words) after its run has ended is opened again in
/// the plan, so the next Run picks it up and goes on with the answer (kept from the Inbox at start). A run still
/// going, or paused, takes the answer itself; Skip and End leave the step as it is.
fn reopen_answered_hold(env: &dyn RunEnv, item: &crate::inbox::InboxItem) {
    let Some(src) = item.source.as_ref() else {
        return;
    };
    let field = |name: &str| src.get(name).and_then(Value::as_str).unwrap_or("");
    let (file, step, run_id) = (field("file"), field("step"), field("runId"));
    if file.is_empty() || step.is_empty() {
        return;
    }
    let goes_on = item
        .answer
        .as_ref()
        .and_then(|a| serde_json::from_value::<InboxAnswer>(a.clone()).ok())
        .is_some_and(|a| hold_reply(&a).choice == holdask::AnswerChoice::Go);
    if !goes_on {
        return;
    }
    let live = [
        status_word(RunStatus::Running),
        status_word(RunStatus::Paused),
    ];
    let still_going = env.list_runs().ok().is_some_and(|runs| {
        runs.iter().any(|r| {
            r.id == run_id
                && serde_json::from_str::<Value>(&r.data)
                    .ok()
                    .and_then(|d| d.get("status").and_then(Value::as_str).map(String::from))
                    .is_some_and(|s| live.contains(&s))
        })
    });
    if still_going {
        return;
    }
    let Ok(now) = env.read(file) else { return };
    let Some(s) = parse_plan(&now.text)
        .steps
        .into_iter()
        .find(|x| x.number.clone().unwrap_or_else(|| x.title.clone()) == step)
    else {
        return;
    };
    if s.status != StepStatus::Blocked {
        return; // Opened, done or changed by hand meanwhile: left as the owner has it.
    }
    if let Ok(next) = set_step_status(&now.text, s.line, StepStatus::Open, Some(&s.title)) {
        let _ = env.write(file, &next, Some(&now.hash));
    }
}

#[cfg(test)]
pub(crate) fn test_setup(file: &str, options: RunOptions) -> RunSetup {
    RunSetup {
        project_name: "Shop".into(),
        file: file.into(),
        provider: options
            .provider_id
            .clone()
            .unwrap_or_else(|| "claude-code".into()),
        provider_name: "Claude Code".into(),
        models: vec![
            AgentModel {
                id: "claude-opus-5-5".into(),
                label: "Opus 5.5".into(),
                price: Some([5.0, 25.0]),
            },
            AgentModel {
                id: "claude-sonnet-5-5".into(),
                label: "Sonnet 5.5".into(),
                price: Some([3.0, 15.0]),
            },
        ],
        default_model: "claude-sonnet-5-5".into(),
        planner_model: "claude-opus-5-5".into(),
        team: decider::DEFAULT_TEAM,
        has_free: false,
        monthly_budget_usd: 0.0,
        rules: HashMap::new(),
        turns: TurnSetup {
            provider: options
                .provider_id
                .clone()
                .unwrap_or_else(|| "claude-code".into()),
            lean: options.lean,
            ..Default::default()
        },
        resume: None,
        options,
    }
}
