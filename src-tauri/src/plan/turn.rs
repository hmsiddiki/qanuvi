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
    forward: &mut dyn FnMut(&AgentEvent),
) -> Turn {
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
        title: if session_id.is_none() && !title.is_empty() {
            Some(title.to_string())
        } else {
            None
        },
    };
    let current = watch
        .live_model
        .clone()
        .unwrap_or_else(|| model.to_string());
    let mut id: Option<String> = session_id.map(String::from);
    let mut gentle_sent = false;
    let mut late: Vec<i64> = Vec::new();
    let mut interrupt_wait: Option<i64> = None;
    let mut last_limit_check = env.now_ms();
    let mut cost_seen = 0.0f64;
    let mut exited = false;

    let end = env.agent_turn(&req, &mut |event| {
        let now = env.now_ms();
        if let Some(e) = event {
            forward(e);
        }
        // The owner's asks.
        let (stop, pause, switch) = {
            let mut a = controls.asks();
            if let Some(AgentEvent::Session { session }) = event {
                a.session_id = Some(session.id.clone());
            }
            (a.stop, a.pause, a.switch_to.take())
        };
        if let Some(AgentEvent::Session { session }) = event {
            id = Some(session.id.clone());
            cost_seen = session.cost_usd;
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
        // The step's limits (cost and time): over them, the turn stops and the run asks.
        if now - last_limit_check >= LIMIT_CHECK_MS {
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
                }
                if st == "idle" && std::mem::take(&mut controls.asks().interrupted) {
                    // "Read my message now": this end is not the step's end; the agent reads the message and goes on.
                    interrupt_wait = Some(now + INTERRUPT_GRACE_MS);
                    return Flow::Hold;
                }
                if interrupt_wait.is_some() && st == "idle" {
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
    if !ask.test_command.is_empty() {
        let mut round = 0u32;
        // A paused step does not go on to the tests: it stops where the owner paused it.
        while t.outcome == TurnOutcome::Idle
            && controls.stop_asked().is_none()
            && watch.handover == 0
            && !watch.pause_now
        {
            controls.asks().checking = true;
            let check = env
                .run_check(&ask.test_command, CHECK_TIMEOUT)
                .unwrap_or_else(|e| CheckResult {
                    ok: false,
                    exit_code: None,
                    timed_out: false,
                    duration_ms: 0,
                    output: e,
                });
            controls.asks().checking = false;
            test_runs += 1;
            tests_passed = Some(check.ok);
            if check.ok || round >= MAX_FIX_ROUNDS {
                break;
            }
            let summary = if check.timed_out {
                "The tests did not finish within the time limit.".to_string()
            } else {
                failure_summary(&check.output, FAILURE_CHARS)
            };
            failure = summary.clone();
            round += 1;
            let follow = fix_prompt(&ask.test_command, &summary, round, MAX_FIX_ROUNDS);
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
                &ask.title,
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
        "Tests failed.".into()
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
