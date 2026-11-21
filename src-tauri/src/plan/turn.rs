//! A step's agent work in the core (24z R2b), ported from the window's runner (`#turn` and `#attempt` in
//! `src/lib/stores/runner.svelte.ts`): one turn of an agent session that waits for its end, and one attempt at a step,
//! that is a turn and, when Qanuvi runs the tests, up to two fix rounds in the same session with only the failures.
//!
//! While a turn runs it keeps the step's limits (cost and time), hands over when the context grows too big, and
//! hears the owner: pause, stop (gently, then at once), another model for the step, and "read my message now". The
//! owner's asks come through `Controls`, shared with the commands; what happened to the step stays in `StepWatch`,
//! which the run loop (R2c) owns and reads after the attempt.

use crate::agent::AgentEvent;
use crate::checks::CheckResult;
use crate::plan::costs::{StepLimits, over_limit};
use crate::plan::env::{Flow, MemoryUse, RunEnv, TurnEnd, TurnRequest, memory_context, turn_over};
use crate::plan::guidance::{context_size, handover_due, handover_request, model_switch_request};
use crate::plan::ownermsg::{self, Kind, When};
use crate::plan::packet::{failure_summary, fix_prompt};
use crate::plan::runner::{Attempt, Outcome, is_free};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

/// Fix rounds after the first test run of an attempt.
pub const MAX_FIX_ROUNDS: u32 = 2;
/// How long the test command may run.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(600);
/// After "read my message now", the agent has this long to go on before the step counts as paused by the owner.
pub const INTERRUPT_GRACE_MS: i64 = 30_000;
/// Sent when the agent stays quiet after the owner's message: it goes on with the step.
pub const INTERRUPT_GO_ON: &str = "(Qanuvi: the owner's message is taken into account. Go on with the step now from where you are; if the message changes what the step should do, follow it and say so in the step's log.)";
/// How often the step's limits are checked while a turn runs.
pub const LIMIT_CHECK_MS: i64 = 15_000;
/// An agent asked to write its progress note and stop (handover, pause, another model) is stopped after this long.
pub const LATE_STOP_MS: i64 = 180_000;
/// An agent asked to stop gently (finish the edit in hand) is stopped at once after this long.
pub const GENTLE_STOP_MS: i64 = 60_000;
/// The longest test failure summary sent back to the agent.
pub const FAILURE_CHARS: usize = 3000;
/// A turn that ends while its own background commands run is held this long at most for them (z4); the agent's
/// session wakes by itself when they finish.
pub const BACKGROUND_WAIT_MS: i64 = 30 * 60_000;
/// Shown in the step's chat when the turn is held for the agent's background commands.
pub const BACKGROUND_WAIT_LINE: &str =
    "Waiting for the step's background command to finish; the agent goes on when it does.";
/// Sent when the background commands still run after BACKGROUND_WAIT_MS: the agent stops waiting for them.
pub const BACKGROUND_GIVE_UP: &str = "(Qanuvi: your background command is still running after 30 minutes. Do not wait for it any longer: stop it if it is stuck, finish what you can of the step, and say in the step's log where it stands.)";

/// How many background commands the agent's session has running, from Claude Code's
/// `background_tasks_changed` line (None: another line).
/// bg1: the step is already ticked in the plan, so nothing waits on the agent's background commands (a command
/// Claude Code moved to the background on its 120 s limit and the agent left behind held a done step 30 minutes).
pub fn step_ticked(env: &dyn RunEnv, watch: &StepWatch) -> bool {
    let Some((file, step)) = &watch.plan_step else {
        return false;
    };
    let Ok(now) = env.read(file) else {
        return false;
    };
    crate::plan::tasklist::parse_plan(&now.text)
        .steps
        .iter()
        .any(|s| {
            s.number.clone().unwrap_or_else(|| s.title.clone()) == *step
                && s.status == crate::plan::tasklist::StepStatus::Done
        })
}

pub fn background_tasks(line: &str) -> Option<usize> {
    if !line.contains("background_tasks_changed") {
        return None;
    }
    let msg: serde_json::Value = serde_json::from_str(line).ok()?;
    if msg["type"] != "system" || msg["subtype"] != "background_tasks_changed" {
        return None;
    }
    Some(msg["tasks"].as_array().map_or(0, Vec::len))
}

/// What the owner says to a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopAsk {
    /// The agent finishes the edit in hand first.
    Gentle,
    /// At once.
    Now,
}

#[derive(Clone, Debug, Default)]
struct Asks {
    stop: Option<StopAsk>,
    pause: bool,
    interrupted: bool,
    switch_to: Option<String>,
    mode: Option<String>,
    session_id: Option<String>,
    checking: bool,
    /// The owner's messages in the step's chat, not taken by a turn yet.
    said: std::collections::VecDeque<String>,
    /// Words for the agent that go as they are, never sorted by a model (a private detail the owner gave).
    direct: std::collections::VecDeque<String>,
}

/// The owner's asks to the running step, set by the commands and read by the turn (and later the run loop).
#[derive(Debug, Default)]
pub struct Controls(Mutex<Asks>);

impl Controls {
    fn asks(&self) -> std::sync::MutexGuard<'_, Asks> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stop: gently while a step's agent works (it finishes the edit in hand), at once otherwise or the second time.
    pub fn stop(&self) {
        let mut a = self.asks();
        a.stop = Some(if a.stop.is_none() && a.session_id.is_some() {
            StopAsk::Gentle
        } else {
            StopAsk::Now
        });
    }

    pub fn stop_now(&self) {
        self.asks().stop = Some(StopAsk::Now);
    }

    pub fn stop_asked(&self) -> Option<StopAsk> {
        self.asks().stop
    }

    /// Pause: a step at work ends the edit in hand, writes where it is and stops; its session is kept.
    pub fn pause(&self) {
        self.asks().pause = true;
    }

    pub fn pausing(&self) -> bool {
        self.asks().pause
    }

    /// The run paused or goes on again: the pause is done with.
    pub fn clear_pause(&self) {
        self.asks().pause = false;
    }

    /// "Stop this command, read my message now" on the running step (24r).
    pub fn interrupt(&self) {
        self.asks().interrupted = true;
    }

    /// The owner typed a message into the step's chat: the turn sorts it before the agent hears it (R2c part 2).
    pub fn owner_message(&self, text: &str) {
        let t = text.trim();
        if !t.is_empty() {
            self.asks().said.push_back(t.to_string());
        }
    }

    /// Words the window built for the step's agent (the plan's private details after the owner gave one): they go to
    /// the agent as they are, never to the model that sorts messages.
    pub fn tell_agent(&self, text: &str) {
        if !text.trim().is_empty() {
            self.asks().direct.push_back(text.to_string());
        }
    }

    fn take_direct(&self) -> Option<String> {
        self.asks().direct.pop_front()
    }

    /// Messages not taken yet.
    pub fn waiting_messages(&self) -> usize {
        self.asks().said.len()
    }

    fn take_said(&self) -> Option<String> {
        self.asks().said.pop_front()
    }

    /// The owner picked another model for the running step.
    pub fn switch_model(&self, model: &str) {
        self.asks().switch_to = Some(model.to_string());
    }

    /// The owner picked another permission mode for the rest of the run.
    pub fn set_mode(&self, mode: &str) {
        self.asks().mode = Some(mode.to_string());
    }

    pub fn mode(&self) -> Option<String> {
        self.asks().mode.clone()
    }

    /// The step session at work now (for the window and for Stop).
    pub fn session_id(&self) -> Option<String> {
        self.asks().session_id.clone()
    }

    /// Qanuvi runs the step's tests now.
    pub fn checking(&self) -> bool {
        self.asks().checking
    }

    /// A new run: nothing asked yet.
    pub fn reset(&self) {
        *self.asks() = Asks::default();
    }
}

/// What happened to the step while its agent worked; the run loop owns it, sets the limits per step and reads it after
/// each attempt.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepWatch {
    /// When the step started (milliseconds), for its time limit.
    pub started_ms: i64,
    /// What the step's earlier attempts cost.
    pub spent_before: f64,
    /// The session's cost when this attempt began (a step that continues a session counts only its own share).
    pub attempt_base: f64,
    pub limits: StepLimits,
    /// The run's handover limit (context tokens; 0 = never).
    pub handover_limit: u64,
    /// Handovers in this step so far.
    pub handovers: u32,
    /// The step's log, where its agent writes the progress note.
    pub step_log: String,
    /// A handover was asked: the context size that set it off (1 for a model switch); 0 = none.
    pub handover: u64,
    /// The step went over a limit: why.
    pub over_limit: Option<String>,
    /// The step's agent pauses now (not at the end of the step).
    pub pause_now: bool,
    /// The model the owner switched the step to (used by the fix rounds of this attempt).
    pub live_model: Option<String>,
    /// The switch the agent was told about, until the new session starts (the chat footer shows it).
    pub switching_to: Option<String>,
    /// A model switch the run loop has not taken yet: the step goes on with this model from the next attempt.
    pub switched: Option<String>,
    /// The owner's messages and how each was handled (the run loop logs them).
    pub messages: Vec<ownermsg::Record>,
    /// Changes the owner wants as their own steps (the run loop writes them into the plan).
    pub new_steps: Vec<ownermsg::NewStep>,
    /// bg1: the plan file and the step (number, else title) this watch is for, so a turn can see the step is ticked.
    pub plan_step: Option<(String, String)>,
}

impl StepWatch {
    /// A step starts: its limits and nothing seen yet.
    pub fn for_step(
        now_ms: i64,
        spent_before: f64,
        limits: StepLimits,
        handover_limit: u64,
        step_log: &str,
    ) -> Self {
        StepWatch {
            started_ms: now_ms,
            spent_before,
            limits,
            handover_limit,
            step_log: step_log.to_string(),
            ..Default::default()
        }
    }
}

/// What every turn of the run is sent with (the run's settings at its start, changed when the owner changes them).
#[derive(Clone, Debug, Default)]
pub struct TurnSetup {
    pub effort: Option<String>,
    pub base_url: Option<String>,
    pub browser_tools: bool,
    pub lean: bool,
    pub index_tools: bool,
    /// The run's agent ("claude-code", ...).
    pub provider: String,
    /// Another agent for the turns now (a finishing pass on another provider).
    pub agent_override: Option<String>,
    /// The Qanuvi agent's setup per agent, keys filled in.
    pub setups: BTreeMap<String, String>,
    pub memory: MemoryUse,
    /// Model names as the owner sees them ("Opus 5.5"), for the messages to the agent.
    pub labels: BTreeMap<String, String>,
    /// How long a change question waits for the owner (milliseconds; 0 = the default, 10 minutes).
    pub message_wait_ms: i64,
    /// 24y: the turns are a plan run's, answered by the project's run rules instead of waiting for a click.
    pub ruled: bool,
    /// The plan file's own "Run rules" section (it wins for that plan).
    pub plan_rules: Option<crate::run_rules::RunRules>,
}

impl TurnSetup {
    /// The agent and model a turn goes to: a free model ("qanuvi:...") goes to Qanuvi's own agent, which lets
    /// Qanuvi core pick the model.
    fn route(&self, model: &str) -> (String, String) {
        if is_free(model) {
            return ("qanuvi".into(), model["qanuvi:".len()..].to_string());
        }
        let agent = self
            .agent_override
            .clone()
            .unwrap_or_else(|| self.provider.clone());
        (agent, model.to_string())
    }

    fn label(&self, model: &str) -> String {
        self.labels
            .get(model)
            .cloned()
            .unwrap_or_else(|| model.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnOutcome {
    /// The agent answered and waits for the next message.
    Idle,
    Stopped,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub session_id: Option<String>,
    /// How the session stood at the end (totals of all its turns).
    pub last: Option<TurnEnd>,
    pub outcome: TurnOutcome,
    pub note: String,
}

/// The texts the agent gets when the owner pauses or stops the run (as the window sent them).
pub fn pause_request(log: &str) -> String {
    format!(
        "(Qanuvi: the owner pauses the run now. Finish only the edit you are making; do not start anything new. Then \
         add a short \"Progress\" note to {log}: what is done, what is left, which files you touched and anything \
         half-way. Then stop and answer \"Paused\". When the run resumes, this same session goes on from here.)"
    )
}

pub const STOP_REQUEST: &str = "(Qanuvi: the owner stops the run. Finish only the edit you are making, start nothing new, then stop and answer \"Stopped\".)";

/// A change the owner asked for in the step's chat, waiting for their answer (now, after this step, new step).
struct Asking {
    at: i64,
    text: String,
    title: String,
    by: String,
    key: String,
    until: i64,
}

/// Sorts the owner's message by its meaning on the free decider (any language; `waiting`: the change Qanuvi asked
/// "when?" about, which the message may answer). With no model: a question by its mark, else a change (asked).
fn sort_message(
    env: &dyn RunEnv,
    setup: &TurnSetup,
    step: &str,
    text: &str,
    waiting: Option<&str>,
) -> ownermsg::Sorted {
    if let Some(free) = setup.setups.get("qanuvi")
        && let Ok((who, answer)) = env.free_decide(
            free,
            ownermsg::SORT_SYSTEM,
            &ownermsg::sort_prompt(step, text, waiting),
        )
        && let Some((kind, title, when)) = ownermsg::parse_sort(&answer)
    {
        return ownermsg::Sorted {
            kind,
            by: who,
            title,
            when: when.filter(|_| waiting.is_some()),
        };
    }
    ownermsg::Sorted {
        kind: ownermsg::guess(text),
        by: ownermsg::WORD_RULES.into(),
        title: String::new(),
        when: None,
    }
}

/// The owner's change question is settled (their answer, or the safe choice): the Inbox question goes, the step's
/// record keeps what was chosen. Answers what the agent is told and what the chat shows.
fn settle(
    env: &dyn RunEnv,
    watch: &mut StepWatch,
    a: Asking,
    when: When,
    by_default: bool,
    wait: i64,
) -> (Flow, String) {
    let _ = env.inbox_resolve(&a.key);
    if when == When::NewStep {
        watch.new_steps.push(ownermsg::NewStep {
            title: a.title.clone(),
            text: a.text.clone(),
        });
    }
    watch.messages.push(ownermsg::Record {
        at: a.at,
        text: a.text.clone(),
        kind: Kind::Change,
        by: a.by,
        handled: ownermsg::chosen_text(when, by_default, wait),
    });
    let said = if by_default {
        ownermsg::default_text(wait)
    } else {
        ownermsg::when_text(when).to_string()
    };
    (Flow::Send(ownermsg::when_message(when, &a.text)), said)
}

/// One turn of an agent session (a new session, or a follow-up in one), waited for until it ends. Every event goes
/// to `forward` (the window shows the step's chat from them).
#[allow(clippy::too_many_arguments)]
pub fn turn(
    env: &dyn RunEnv,
    controls: &Controls,
    watch: &mut StepWatch,
    setup: &TurnSetup,
    session_id: Option<&str>,
    prompt: &str,
    model: &str,
    mode: &str,
    title: &str,
    outer: &mut dyn FnMut(&AgentEvent),
) -> Turn {
    // What the runner says in the step's chat is kept in the session too (opened later, the chat still shows it).
    let mut forward = |e: &AgentEvent| {
        if let AgentEvent::Line { line } = e
            && line.contains("\"qanuvi_runner\"")
            && let Some(sid) = controls.session_id()
        {
            env.keep_line(&sid, line);
        }
        outer(e);
    };
    // Stop asked before this turn began: nothing is half-way, so no agent starts.
    if controls.stop_asked().is_some() {
        return Turn {
            session_id: session_id.map(String::from),
            last: None,
            outcome: TurnOutcome::Stopped,
            note: String::new(),
        };
    }
    let (agent, agent_model) = setup.route(model);
    let req = TurnRequest {
        session_id: session_id.map(String::from),
        prompt: prompt.to_string(),
        model: agent_model,
        mode: mode.to_string(),
        setup: setup.setups.get(&agent).cloned(),
        agent,
        base_url: setup.base_url.clone(),
        effort: setup.effort.clone(),
        lean: setup.lean,
        index_tools: setup.index_tools,
        browser_tools: setup.browser_tools,
        // A step's first turn gets the relevant part of the user's memory.
        context: if session_id.is_none() {
            memory_context(env, setup.memory, prompt)
        } else {
            None
        },
        // Also on a session that goes on into the next step: it is renamed to the step at work (the window title
        // shows it), not left on the first step's title.
        title: (!title.is_empty()).then(|| title.to_string()),
        ruled: setup.ruled,
        plan_rules: setup.plan_rules.clone(),
    };
    let current = watch
        .live_model
        .clone()
        .unwrap_or_else(|| model.to_string());
    let mut id: Option<String> = session_id.map(String::from);
    let mut gentle_sent = false;
    let mut late: Vec<i64> = Vec::new();
    let mut interrupt_wait: Option<i64> = None;
    // The agent's own background commands (z4): how many run, since when the turn waits for them, and whether it
    // stopped waiting.
    let mut background = 0usize;
    let mut background_since: Option<i64> = None;
    let mut background_given_up = false;
    let mut last_limit_check = env.now_ms();
    let mut cost_seen = 0.0f64;
    let mut exited = false;
    let mut asking: Option<Asking> = None;
    let wait = if setup.message_wait_ms > 0 {
        setup.message_wait_ms
    } else {
        ownermsg::WAIT_DEFAULT_MS
    };

    let end = env.agent_turn(&req, &mut |event| {
        let now = env.now_ms();
        // The owner's asks (the session is known first, so listeners hear each event with its session).
        let (stop, pause, switch) = {
            let mut a = controls.asks();
            if let Some(AgentEvent::Session { session }) = event {
                a.session_id = Some(session.id.clone());
            }
            (a.stop, a.pause, a.switch_to.take())
        };
        if let Some(e) = event {
            forward(e);
        }
        if let Some(AgentEvent::Session { session }) = event {
            id = Some(session.id.clone());
            cost_seen = session.cost_usd;
        }
        if let Some(AgentEvent::Line { line }) = event
            && let Some(n) = background_tasks(line)
        {
            background = n;
        }
        match stop {
            Some(StopAsk::Now) => return Flow::Stop,
            Some(StopAsk::Gentle) if !gentle_sent => {
                gentle_sent = true;
                late.push(now + GENTLE_STOP_MS);
                return Flow::Send(STOP_REQUEST.into());
            }
            _ => {}
        }
        // Another model for the step: counted from the next attempt; the agent at work finishes its edit and writes a
        // progress note, then the chosen model goes on in a new session. The model it is already on changes nothing.
        if let Some(m) = switch
            && m != watch.live_model.clone().unwrap_or_else(|| current.clone())
        {
            watch.live_model = Some(m.clone());
            watch.switched = Some(m.clone());
            if !watch.step_log.is_empty()
                && watch.handover == 0
                && !watch.pause_now
                && stop.is_none()
            {
                watch.switching_to = Some(m.clone());
                watch.handover = 1;
                late.push(now + LATE_STOP_MS);
                return Flow::Send(model_switch_request(&watch.step_log, &setup.label(&m)));
            }
        }
        if pause && !watch.pause_now && !watch.step_log.is_empty() {
            // Now, not after the whole step: the agent ends the edit it is making and writes where it is.
            watch.pause_now = true;
            late.push(now + LATE_STOP_MS);
            return Flow::Send(pause_request(&watch.step_log));
        }
        // An agent that was asked to stop and did not is stopped (its session is kept for a pause all the same).
        if late.iter().any(|t| now >= *t) {
            return Flow::Stop;
        }
        if let Some(text) = controls.take_direct() {
            return Flow::Send(text);
        }
        // The owner's words in the step's chat (R2c part 2): sorted first, then passed on as fits.
        if let Some(text) = controls.take_said() {
            // A change question waiting: the model reads whether this answers it ("plus tard", "after", "now"...).
            let waiting = asking.as_ref().map(|a| a.text.clone());
            let sorted = sort_message(env, setup, title, &text, waiting.as_deref());
            if let Some(when) = sorted.when
                && let Some(a) = asking.take()
            {
                let (flow, said) = settle(env, watch, a, when, false, wait);
                forward(&AgentEvent::Line {
                    line: ownermsg::chat_line(&said),
                });
                return flow;
            }
            let record = |handled: &str| ownermsg::Record {
                at: now,
                text: text.clone(),
                kind: sorted.kind,
                by: sorted.by.clone(),
                handled: handled.to_string(),
            };
            let told = |handled: &str| {
                ownermsg::chat_line(&format!("Taken as a {}: {handled}.", sorted.kind.word()))
            };
            let step_title = if sorted.title.is_empty() {
                ownermsg::title_of(&text)
            } else {
                sorted.title.clone()
            };
            match sorted.kind {
                Kind::Note | Kind::Rule | Kind::Question => {
                    let handled = ownermsg::handled_text(sorted.kind);
                    watch.messages.push(record(handled));
                    forward(&AgentEvent::Line {
                        line: told(handled),
                    });
                    return Flow::Send(match sorted.kind {
                        Kind::Note => ownermsg::note_message(&text),
                        Kind::Rule => ownermsg::rule_message(&text),
                        _ => ownermsg::question_message(&text),
                    });
                }
                Kind::Stop | Kind::Pause => {
                    // As the buttons: the next look at the owner's asks does the rest.
                    let handled = ownermsg::handled_text(sorted.kind);
                    watch.messages.push(record(handled));
                    forward(&AgentEvent::Line {
                        line: told(handled),
                    });
                    if sorted.kind == Kind::Stop {
                        controls.stop();
                        return Flow::Hold;
                    }
                    controls.pause();
                    return if watch.step_log.is_empty() {
                        Flow::Go
                    } else {
                        Flow::Hold
                    };
                }
                Kind::Change if asking.is_some() => {
                    // One question at a time: a second change while one is asked becomes its own step (nothing lost).
                    watch.messages.push(record(
                        "another change was being asked about, so it was added as a new step after this one",
                    ));
                    watch.new_steps.push(ownermsg::NewStep {
                        title: step_title,
                        text: text.clone(),
                    });
                    forward(&AgentEvent::Line {
                        line: ownermsg::chat_line(&format!(
                            "Your change \"{text}\" becomes a new step after this one (another change is being asked about)."
                        )),
                    });
                    return Flow::Go;
                }
                Kind::Change => {
                    let key = format!(
                        "msg:{}:{}:{now}",
                        env.project_id(),
                        id.clone().unwrap_or_else(|| title.to_string())
                    );
                    forward(&AgentEvent::Line {
                        line: ownermsg::chat_line(&ownermsg::ask_text(&text, wait)),
                    });
                    let _ = env.inbox_add(&crate::inbox::NewItem {
                        kind_of: "answer".into(),
                        kind: "run".into(),
                        title: format!("{title}: when should your change be done?"),
                        text: text.clone(),
                        key: Some(key.clone()),
                        open: None,
                        form: Some(ownermsg::form()),
                        source: Some(serde_json::json!({"kind": "step-message",
                            "projectId": env.project_id(), "session": id.clone(), "step": title})),
                    });
                    asking = Some(Asking {
                        at: now,
                        title: step_title,
                        text: text.clone(),
                        by: sorted.by.clone(),
                        key,
                        until: now + wait,
                    });
                    return Flow::Send(ownermsg::change_hold_message(&text));
                }
            }
        }
        // An open change question: answered in the Inbox, or its wait is over (the safe choice: a new step).
        let answered = asking.as_ref().and_then(|a| {
            let item = env.inbox_by_key(&a.key).ok().flatten()?;
            if item.status != "answered" {
                return None;
            }
            When::from_id(item.answer?.get("choice")?.as_str()?)
        });
        let over = asking.as_ref().is_some_and(|a| now >= a.until);
        if (answered.is_some() || over)
            && let Some(a) = asking.take()
        {
            let (flow, said) = settle(
                env,
                watch,
                a,
                answered.unwrap_or(When::NewStep),
                answered.is_none(),
                wait,
            );
            forward(&AgentEvent::Line {
                line: ownermsg::chat_line(&said),
            });
            return flow;
        }
        // The step's limits (cost and time): over them, the turn stops and the run asks (not while the owner is asked).
        if asking.is_none() && now - last_limit_check >= LIMIT_CHECK_MS {
            last_limit_check = now;
            let spent = watch.spent_before + (cost_seen - watch.attempt_base).max(0.0);
            let minutes = (now - watch.started_ms) as f64 / 60_000.0;
            if let Some(why) = over_limit(spent, minutes, watch.limits) {
                watch.over_limit = Some(why);
                return Flow::Stop;
            }
        }
        match event {
            None => {
                if let Some(since) = background_since
                    && now - since >= BACKGROUND_WAIT_MS
                {
                    background_since = None;
                    background_given_up = true;
                    return Flow::Send(BACKGROUND_GIVE_UP.into());
                }
                if let Some(at) = interrupt_wait
                    && now >= at
                {
                    // Nothing went on after the owner's message: the step goes on (it used to count as paused by
                    // the owner, which left the run waiting). If the agent stops again, that is the turn's end.
                    interrupt_wait = None;
                    return Flow::Send(INTERRUPT_GO_ON.into());
                }
                Flow::Go
            }
            Some(AgentEvent::Line { line }) => {
                // The context of the call just made: too big, and the step hands over (once per attempt).
                if watch.handover == 0
                    && !watch.pause_now
                    && !watch.step_log.is_empty()
                    && line.contains("\"usage\"")
                    && line.contains("\"assistant\"")
                {
                    let size = context_size(std::slice::from_ref(line));
                    if size > 0 && handover_due(size, watch.handover_limit, watch.handovers) {
                        watch.handover = size;
                        late.push(now + LATE_STOP_MS);
                        return Flow::Send(handover_request(&watch.step_log, size));
                    }
                }
                Flow::Go
            }
            Some(AgentEvent::Session { session }) => {
                let st = session.status.as_str();
                if st == "running" {
                    interrupt_wait = None;
                    background_since = None;
                }
                // The agent waits for the owner's answer to a change question.
                if asking.is_some() && st == "idle" {
                    return Flow::Hold;
                }
                if st == "idle" && std::mem::take(&mut controls.asks().interrupted) {
                    // "Read my message now": this end is not the step's end; the agent reads the message and goes on.
                    interrupt_wait = Some(now + INTERRUPT_GRACE_MS);
                    return Flow::Hold;
                }
                if interrupt_wait.is_some() && st == "idle" {
                    return Flow::Hold;
                }
                // Its own background commands still run (z4): not the step's end, and no failed try. The session
                // wakes by itself when they finish; a pause, a stop or a handover asked meanwhile is not held.
                if st == "idle"
                    && background > 0
                    && !background_given_up
                    && !step_ticked(env, watch)
                    && !watch.pause_now
                    && watch.handover == 0
                    && stop.is_none()
                {
                    if background_since.is_none() {
                        background_since = Some(now);
                        forward(&AgentEvent::Line {
                            line: ownermsg::chat_line(BACKGROUND_WAIT_LINE),
                        });
                    }
                    return Flow::Hold;
                }
                Flow::Go
            }
            Some(AgentEvent::Exit { .. }) => {
                exited = true;
                Flow::Go
            }
            Some(AgentEvent::Error { .. }) => Flow::Go,
        }
    });

    // The turn ended while a change question was open (stopped, an error): the safe choice, nothing lost.
    if let Some(a) = asking.take() {
        let _ = settle(env, watch, a, When::NewStep, true, wait);
        if let Some(m) = watch.messages.last_mut() {
            m.handled =
                "the turn ended before an answer, so it was added as a new step after this one"
                    .into();
        }
    }
    let end = match end {
        Ok(e) => e,
        Err(e) => {
            return Turn {
                session_id: id,
                last: None,
                outcome: TurnOutcome::Error,
                note: e,
            };
        }
    };
    let stop_asked = controls.stop_asked().is_some();
    let outcome = if end.cut_short {
        TurnOutcome::Stopped
    } else if exited && !turn_over(&end.status) || end.status == "ended" {
        if stop_asked {
            TurnOutcome::Stopped
        } else {
            TurnOutcome::Error
        }
    } else {
        match end.status.as_str() {
            "idle" => TurnOutcome::Idle,
            "stopped" => TurnOutcome::Stopped,
            _ => TurnOutcome::Error,
        }
    };
    Turn {
        session_id: Some(end.session_id.clone()),
        last: Some(end),
        outcome,
        note: String::new(),
    }
}

/// A session's running totals, to count each step's own share when steps continue one session.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub cost: f64,
    pub input: i64,
    pub output: i64,
    pub ms: i64,
}

impl Totals {
    pub fn of(end: Option<&TurnEnd>) -> Self {
        end.map(|e| Totals {
            cost: e.cost_usd,
            input: e.input_tokens,
            output: e.output_tokens,
            ms: e.duration_ms,
        })
        .unwrap_or_default()
    }
}

/// How an attempt's note starts when the step's own checks still failed after its fix rounds (24y Y4).
pub const CHECKS_FAILED: &str = "Checks failed: ";

/// One attempt's request: the prompt, the model and mode it starts with, and the tests Qanuvi runs after it.
#[derive(Clone, Debug, Default)]
pub struct AttemptAsk {
    pub prompt: String,
    pub model: String,
    pub mode: String,
    /// The session's title in the agent list.
    pub title: String,
    /// The project's test command ("" = Qanuvi runs no tests).
    pub test_command: String,
    /// The step's own checks (24y Y4), run after the test command; they must pass too.
    pub checks: Vec<String>,
    /// Go on in an open session (the step before left it idle): its id and its totals before this attempt.
    pub resume: Option<(String, Totals)>,
}

/// How the attempt left its session, for the run loop to decide whether the next step continues it.
#[derive(Clone, Debug, PartialEq)]
pub struct AttemptEnd {
    pub attempt: Attempt,
    /// The session is open and waits for a message (the next step may continue it).
    pub idle: bool,
    pub totals: Totals,
}

/// One attempt at a step: a turn and, when Qanuvi runs the tests, up to two fix rounds in the same session with only
/// the failures. The session stays open; the run loop decides whether the next step continues it.
pub fn attempt(
    env: &dyn RunEnv,
    controls: &Controls,
    watch: &mut StepWatch,
    setup: &TurnSetup,
    ask: &AttemptAsk,
    forward: &mut dyn FnMut(&AgentEvent),
) -> AttemptEnd {
    let mut t = turn(
        env,
        controls,
        watch,
        setup,
        ask.resume.as_ref().map(|(id, _)| id.as_str()),
        &ask.prompt,
        &ask.model,
        &ask.mode,
        &ask.title,
        forward,
    );
    let mut test_runs = 0u32;
    let mut tests_passed: Option<bool> = None;
    let mut failure = String::new();
    // The project's test command, then the step's own checks (24y Y4): all must pass.
    let commands: Vec<&str> = std::iter::once(ask.test_command.as_str())
        .chain(ask.checks.iter().map(String::as_str))
        .filter(|c| !c.trim().is_empty())
        .collect();
    let mut failing: Vec<String> = Vec::new();
    if !commands.is_empty() {
        let mut round = 0u32;
        // A paused step does not go on to the tests: it stops where the owner paused it.
        while t.outcome == TurnOutcome::Idle
            && controls.stop_asked().is_none()
            && watch.handover == 0
            && !watch.pause_now
        {
            controls.asks().checking = true;
            failing.clear();
            let mut outputs: Vec<(String, CheckResult)> = Vec::new();
            for command in &commands {
                let check = env
                    .run_check(command, CHECK_TIMEOUT)
                    .unwrap_or_else(|e| CheckResult {
                        ok: false,
                        exit_code: None,
                        timed_out: false,
                        duration_ms: 0,
                        output: e,
                    });
                if !check.ok {
                    failing.push(command.to_string());
                    outputs.push((command.to_string(), check));
                }
            }
            controls.asks().checking = false;
            test_runs += 1;
            tests_passed = Some(failing.is_empty());
            if failing.is_empty() || round >= MAX_FIX_ROUNDS {
                break;
            }
            let share = FAILURE_CHARS / outputs.len().max(1);
            let summary = outputs
                .iter()
                .map(|(command, check)| {
                    let what = if check.timed_out {
                        "It did not finish within the time limit.".to_string()
                    } else {
                        failure_summary(&check.output, share)
                    };
                    if outputs.len() > 1 {
                        format!("{command}:\n{what}")
                    } else {
                        what
                    }
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            failure = summary.clone();
            round += 1;
            let follow = fix_prompt(&failing.join("`, `"), &summary, round, MAX_FIX_ROUNDS);
            // A model or mode the owner picked meanwhile stays (it is not switched back for the fix round).
            let model = watch
                .live_model
                .clone()
                .unwrap_or_else(|| ask.model.clone());
            let mode = controls.mode().unwrap_or_else(|| ask.mode.clone());
            let Some(sid) = t.session_id.clone() else {
                break;
            };
            t = turn(
                env,
                controls,
                watch,
                setup,
                Some(&sid),
                &follow,
                &model,
                &mode,
                // A fix round in the same step: the session keeps the title its first turn gave it.
                "",
                forward,
            );
        }
    }
    controls.asks().session_id = None;
    watch.live_model = None;
    watch.switching_to = None;
    let totals = Totals::of(t.last.as_ref());
    let base = ask.resume.as_ref().map(|(_, b)| *b).unwrap_or_default();
    let idle = t.session_id.is_some() && t.last.as_ref().is_some_and(|l| l.status == "idle");
    let note = if !t.note.is_empty() {
        t.note.clone()
    } else if tests_passed == Some(false) {
        let own: Vec<String> = failing
            .iter()
            .filter(|f| **f != ask.test_command)
            .map(|f| format!("`{f}`"))
            .collect();
        if own.is_empty() {
            "Tests failed.".into()
        } else {
            format!("{CHECKS_FAILED}{}.", own.join(", "))
        }
    } else {
        String::new()
    };
    AttemptEnd {
        attempt: Attempt {
            model: ask.model.clone(),
            session_id: t.session_id.clone().unwrap_or_default(),
            outcome: match t.outcome {
                TurnOutcome::Stopped => Outcome::Stopped,
                TurnOutcome::Error => Outcome::Error,
                TurnOutcome::Idle => Outcome::Open,
            },
            cost_usd: (totals.cost - base.cost).max(0.0),
            input_tokens: (totals.input - base.input).max(0) as f64,
            output_tokens: (totals.output - base.output).max(0) as f64,
            seconds: (totals.ms - base.ms).max(0) as f64 / 1000.0,
            note,
            test_runs: Some(test_runs),
            tests_passed,
            tools: None,
            failure: if tests_passed == Some(false) {
                Some(if failure.is_empty() {
                    failure_summary("", FAILURE_CHARS)
                } else {
                    failure
                })
            } else {
                None
            },
        },
        idle,
        totals,
    }
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;
