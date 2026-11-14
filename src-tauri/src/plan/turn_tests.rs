//! The step's agent work (turn and attempt) against the stand-in world: tests and fix rounds, limits, handovers and
//! the owner's asks while a turn runs.

use super::*;
use crate::plan::runner::fake::{AgentReply, FakeEnv};
use serde_json::json;
use std::sync::Arc;

fn setup() -> TurnSetup {
    TurnSetup {
        provider: "claude-code".into(),
        memory: MemoryUse {
            on: true,
            budget: 2000,
        },
        ..Default::default()
    }
}

fn watch(env: &FakeEnv) -> StepWatch {
    StepWatch::for_step(
        env.now_ms(),
        0.0,
        StepLimits::default(),
        200_000,
        "plans/logs/step-1.md",
    )
}

fn ask(test_command: &str) -> AttemptAsk {
    AttemptAsk {
        prompt: "Do step 1.".into(),
        model: "opus".into(),
        mode: "auto".into(),
        title: "Step 1".into(),
        test_command: test_command.into(),
        resume: None,
    }
}

fn run(env: &FakeEnv, controls: &Controls, w: &mut StepWatch, a: &AttemptAsk) -> AttemptEnd {
    attempt(env, controls, w, &setup(), a, &mut |_| {})
}

fn usage(context: u64) -> String {
    json!({"type": "assistant", "message": {"usage": {"input_tokens": 10, "cache_read_input_tokens": context}}})
        .to_string()
}

#[test]
fn passing_tests_end_the_attempt_after_one_turn() {
    let env = FakeEnv::new();
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.outcome, Outcome::Open);
    assert_eq!(end.attempt.test_runs, Some(1));
    assert_eq!(end.attempt.tests_passed, Some(true));
    assert_eq!(end.attempt.failure, None);
    assert_eq!(env.prompts().len(), 1);
    assert!(end.idle);
    assert_eq!(end.attempt.input_tokens, 1000.0);
    assert_eq!(end.attempt.seconds, 2.0);
    // The session is left open (the run loop decides), and the window no longer counts it as at work.
    assert!(env.stopped().is_empty());
    assert_eq!(c.session_id(), None);
    assert!(!c.checking());
}

#[test]
fn failing_tests_get_two_fix_rounds_in_the_same_session() {
    let env = FakeEnv::new();
    let runs = Arc::new(Mutex::new(0));
    let r = runs.clone();
    env.set_checks(move |_| {
        let mut n = r.lock().unwrap();
        *n += 1;
        (*n >= 3, format!("FAIL test_{n}\nexpected 1, got 2"))
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.test_runs, Some(3));
    assert_eq!(end.attempt.tests_passed, Some(true));
    let reqs = env.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(
        reqs[1].session_id.as_deref(),
        Some(end.attempt.session_id.as_str())
    );
    assert!(reqs[1].prompt.contains("fix round 1 of 2"));
    assert!(reqs[2].prompt.contains("fix round 2 of 2"));
    // Only the first turn gets the memory slice and a title.
    assert!(reqs[0].title.is_some() && reqs[1].title.is_none());
    assert_eq!(
        env.calls().iter().filter(|c| *c == "memory_slice").count(),
        1
    );
}

#[test]
fn tests_that_keep_failing_end_with_the_failure() {
    let env = FakeEnv::new();
    env.set_checks(|_| (false, "FAIL the parser\nexpected 1, got 2".into()));
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.test_runs, Some(3));
    assert_eq!(end.attempt.tests_passed, Some(false));
    assert_eq!(end.attempt.note, "Tests failed.");
    assert!(end.attempt.failure.unwrap().contains("the parser"));
    assert_eq!(env.prompts().len(), 3);
}

#[test]
fn no_test_command_runs_no_checks() {
    let env = FakeEnv::new();
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask(""));
    assert_eq!(end.attempt.test_runs, Some(0));
    assert_eq!(end.attempt.tests_passed, None);
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn a_resumed_session_counts_only_its_own_share() {
    let env = FakeEnv::new();
    let c = Controls::default();
    let mut w = watch(&env);
    let first = run(&env, &c, &mut w, &ask(""));
    let mut next = ask("");
    next.resume = Some((first.attempt.session_id.clone(), first.totals));
    let second = run(&env, &c, &mut w, &next);
    assert_eq!(second.attempt.session_id, first.attempt.session_id);
    assert!((second.attempt.cost_usd - 0.01).abs() < 1e-9);
    assert_eq!(second.attempt.input_tokens, 1000.0);
    assert_eq!(second.totals.input, 2000);
}

#[test]
fn a_free_model_goes_to_qanuvis_own_agent() {
    let env = FakeEnv::new();
    let c = Controls::default();
    let mut w = watch(&env);
    let mut s = setup();
    s.setups.insert("qanuvi".into(), "{\"keys\":1}".into());
    let mut a = ask("");
    a.model = "qanuvi:auto".into();
    attempt(&env, &c, &mut w, &s, &a, &mut |_| {});
    let req = &env.requests()[0];
    assert_eq!((req.agent.as_str(), req.model.as_str()), ("qanuvi", "auto"));
    assert_eq!(req.setup.as_deref(), Some("{\"keys\":1}"));
    // The attempt keeps the model as the plan names it.
    assert_eq!(env.requests().len(), 1);
}

#[test]
fn a_step_over_its_cost_limit_is_stopped_without_tests() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        cost: Some(0.5),
        ticks: 40,
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    w.limits = StepLimits {
        usd: Some(0.1),
        minutes: None,
    };
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert!(w.over_limit.as_deref().unwrap().contains("over the limit"));
    assert_eq!(env.stopped().len(), 1);
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn a_step_over_its_time_limit_is_stopped() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        ticks: 200,
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    w.limits = StepLimits {
        usd: None,
        minutes: Some(1.0),
    };
    let end = run(&env, &c, &mut w, &ask(""));
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert!(w.over_limit.is_some());
}

#[test]
fn a_big_context_asks_for_a_handover_and_skips_the_tests() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        lines: vec![usage(50_000), usage(210_000)],
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(w.handover, 210_010);
    let sent = env.sent();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].1.contains("plans/logs/step-1.md"));
    assert_eq!(end.attempt.outcome, Outcome::Open);
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn an_agent_that_does_not_hand_over_is_stopped_later() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        lines: vec![usage(210_000)],
        ticks: 400,
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask(""));
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert_eq!(env.stopped().len(), 1);
}

#[test]
fn no_handover_past_the_most_handovers() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        lines: vec![usage(210_000)],
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    w.handovers = crate::plan::guidance::HANDOVER_MAX;
    run(&env, &c, &mut w, &ask(""));
    assert_eq!(w.handover, 0);
    assert!(env.sent().is_empty());
}

#[test]
fn stop_before_the_turn_starts_no_agent() {
    let env = FakeEnv::new();
    let c = Controls::default();
    c.stop();
    assert_eq!(c.stop_asked(), Some(StopAsk::Now));
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert!(env.prompts().is_empty());
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn stop_now_during_a_turn_stops_the_session() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.stop_now();
        AgentReply {
            ticks: 10,
            ..Default::default()
        }
    });
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert!(env.sent().is_empty());
    assert_eq!(env.stopped().len(), 1);
}

#[test]
fn stop_during_a_turn_is_gentle_then_at_once() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        // The owner clicks Stop while the agent works.
        cc.asks().session_id = Some("s1".into());
        cc.stop();
        AgentReply {
            ticks: 120,
            ..Default::default()
        }
    });
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(c.stop_asked(), Some(StopAsk::Gentle));
    assert_eq!(env.sent().len(), 1);
    assert!(env.sent()[0].1.contains("owner stops the run"));
    // The agent did not stop within the minute: it is stopped.
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert_eq!(env.stopped().len(), 1);
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn a_gently_stopped_agent_that_answers_runs_no_tests() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.asks().session_id = Some("s1".into());
        cc.stop();
        AgentReply {
            ticks: 5,
            result: Some("Stopped".into()),
            ..Default::default()
        }
    });
    let mut w = watch(&env);
    run(&env, &c, &mut w, &ask("npm test"));
    assert!(env.stopped().is_empty());
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}

#[test]
fn pause_during_a_turn_asks_for_a_note_and_skips_the_fix_rounds() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.pause();
        AgentReply {
            result: Some("Paused".into()),
            ..Default::default()
        }
    });
    env.set_checks(|_| (false, "FAIL".into()));
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert!(w.pause_now);
    assert!(env.sent()[0].1.contains("owner pauses the run"));
    // The window's runner ran the tests and sent a fix round after a pause; this one stops where it was paused.
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
    assert_eq!(env.prompts().len(), 1);
    // The session is kept for the resume.
    assert!(end.idle);
    assert!(env.stopped().is_empty());
}

#[test]
fn another_model_takes_over_after_the_edit_in_hand() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.switch_model("sonnet");
        AgentReply::default()
    });
    let mut w = watch(&env);
    let mut s = setup();
    s.labels.insert("sonnet".into(), "Sonnet 5.5".into());
    let end = attempt(&env, &c, &mut w, &s, &ask("npm test"), &mut |_| {});
    assert_eq!(w.switched.as_deref(), Some("sonnet"));
    assert_eq!(w.handover, 1);
    assert!(env.sent()[0].1.contains("Sonnet 5.5"));
    // Cleared at the attempt's end; the run loop takes `switched`.
    assert_eq!((w.live_model.clone(), w.switching_to.clone()), (None, None));
    assert_eq!(end.attempt.model, "opus");
}

#[test]
fn switching_to_the_same_model_changes_nothing() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.switch_model("opus");
        AgentReply::default()
    });
    let mut w = watch(&env);
    run(&env, &c, &mut w, &ask(""));
    assert_eq!(w.switched, None);
    assert!(env.sent().is_empty());
}

#[test]
fn read_my_message_now_holds_the_turn_then_tells_the_agent_to_go_on() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.interrupt();
        AgentReply::default()
    });
    let mut w = watch(&env);
    let started = env.now_ms();
    let end = run(&env, &c, &mut w, &ask("npm test"));
    // The idle end was held for the grace time; nothing went on, so the agent is told to go on (no pause).
    assert!(env.now_ms() - started >= INTERRUPT_GRACE_MS);
    assert_eq!(env.sent().len(), 1);
    assert_eq!(env.sent()[0].1, INTERRUPT_GO_ON);
    assert!(!w.pause_now);
    assert_eq!(end.attempt.outcome, Outcome::Open);
    assert!(env.stopped().is_empty());
}

#[test]
fn events_reach_the_window_and_the_session_is_known_at_once() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        result: Some("done".into()),
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let mut kinds = Vec::new();
    let mut ids = Vec::new();
    attempt(&env, &c, &mut w, &setup(), &ask(""), &mut |e| {
        kinds.push(match e {
            AgentEvent::Session { .. } => "session",
            AgentEvent::Line { .. } => "line",
            _ => "other",
        });
        // The commands' Stop needs the session at work while the turn runs.
        ids.push(c.session_id());
    });
    // The started session first, then the answer and the end.
    assert_eq!(kinds, ["session", "line", "session"]);
    assert_eq!(ids[1].as_deref(), Some("s1"));
    assert_eq!(c.session_id(), None);
}

#[test]
fn an_agent_error_is_an_error_with_its_text() {
    let env = FakeEnv::new();
    env.set_agent(|_| AgentReply {
        status: Some("failed"),
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask("npm test"));
    assert_eq!(end.attempt.outcome, Outcome::Error);
    assert!(!end.idle);
    assert!(!env.calls().iter().any(|c| c == "checks_run"));
}
