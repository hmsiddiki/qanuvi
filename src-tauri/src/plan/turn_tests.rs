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
        checks: Vec::new(),
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
    env.set_agent(move |call| {
        if call.turn == 1 {
            cc.interrupt();
        }
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

fn quick_wait() -> TurnSetup {
    TurnSetup {
        message_wait_ms: 5000,
        ..setup()
    }
}

#[test]
fn a_change_asked_as_the_turn_stops_becomes_a_new_step_nothing_lost() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |_| {
        cc.owner_message("also add a CSV export");
        AgentReply {
            ticks: 1,
            status: Some("stopped"),
            ..Default::default()
        }
    });
    let mut w = watch(&env);
    let end = attempt(&env, &c, &mut w, &quick_wait(), &ask(""), &mut |_| {});
    assert_eq!(end.attempt.outcome, Outcome::Stopped);
    assert_eq!(w.new_steps.len(), 1);
    assert_eq!(w.new_steps[0].title, "Also add a CSV export");
    assert_eq!(w.messages.len(), 1);
    assert!(
        w.messages[0]
            .handled
            .starts_with("the turn ended before an answer")
    );
    // The question was asked and is settled again.
    assert!(env.inbox().is_empty());
}

#[test]
fn a_second_change_while_one_is_asked_becomes_its_own_step_and_the_first_waits_its_turn() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |call| {
        if call.turn == 1 {
            cc.owner_message("also add a CSV export");
            cc.owner_message("also remove the old XML export");
            return AgentReply {
                ticks: 3,
                ..Default::default()
            };
        }
        AgentReply::default()
    });
    let mut w = watch(&env);
    let mut said = Vec::new();
    let started = env.now_ms();
    attempt(&env, &c, &mut w, &quick_wait(), &ask(""), &mut |e| {
        if let AgentEvent::Line { line } = e
            && line.contains("qanuvi_runner")
        {
            said.push(line.clone());
        }
    });
    // The second at once; the first after the wait (5 s here, from the run's setting).
    let titles: Vec<&str> = w.new_steps.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Also remove the old XML export", "Also add a CSV export"]
    );
    assert!(env.now_ms() - started >= 5000);
    let sent: Vec<String> = env.sent().into_iter().map(|(_, t)| t).collect();
    assert_eq!(
        sent,
        [
            ownermsg::change_hold_message("also add a CSV export"),
            ownermsg::when_message(When::NewStep, "also add a CSV export"),
        ]
    );
    // The chat heard the question, the second change and the choice made.
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(said[2].contains("No answer in 1 minute"), "{}", said[2]);
    assert!(env.inbox().is_empty());
}

#[test]
fn words_for_the_agent_from_the_window_go_as_they_are_and_no_model_sorts_them() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(move |call| {
        if call.turn == 1 {
            cc.tell_agent("(the owner gave a private detail)\n\nPrivate details: db login");
            return AgentReply {
                ticks: 2,
                ..Default::default()
            };
        }
        AgentReply::default()
    });
    let mut w = watch(&env);
    attempt(&env, &c, &mut w, &quick_wait(), &ask(""), &mut |_| {});
    let sent: Vec<String> = env.sent().into_iter().map(|(_, t)| t).collect();
    assert_eq!(
        sent,
        ["(the owner gave a private detail)\n\nPrivate details: db login"]
    );
    assert!(!env.calls().iter().any(|c| c == "free_decide"));
    assert!(w.messages.is_empty(), "not an owner message to log");
}

#[test]
fn a_session_that_goes_on_into_the_next_step_takes_that_steps_title() {
    let env = FakeEnv::new();
    let c = Controls::default();
    let mut w = watch(&env);
    let mut a = ask("");
    a.title = "Step 2: Second".into();
    a.resume = Some(("s-1".into(), Totals::default()));
    run(&env, &c, &mut w, &a);
    let reqs = env.requests();
    assert_eq!(reqs[0].session_id.as_deref(), Some("s-1"));
    assert_eq!(reqs[0].title.as_deref(), Some("Step 2: Second"));
}

fn background(n: usize) -> String {
    let tasks: Vec<_> = (0..n)
        .map(|i| json!({"task_id": format!("b{i}")}))
        .collect();
    json!({"type": "system", "subtype": "background_tasks_changed", "tasks": tasks}).to_string()
}

#[test]
fn reads_the_number_of_background_commands() {
    assert_eq!(background_tasks(&background(2)), Some(2));
    assert_eq!(background_tasks(&background(0)), Some(0));
    assert_eq!(background_tasks(&usage(10)), None);
}

#[test]
fn a_turn_that_ends_while_its_background_command_runs_is_held_until_the_session_wakes() {
    let env = FakeEnv::new();
    env.set_agent(|call| {
        if call.turn == 1 {
            AgentReply {
                lines: vec![background(1)],
                wake_after: Some(45),
                ..Default::default()
            }
        } else {
            AgentReply {
                lines: vec![background(0)],
                result: Some("done".into()),
                ..Default::default()
            }
        }
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let mut said = Vec::new();
    let end = attempt(&env, &c, &mut w, &setup(), &ask(""), &mut |e| {
        if let AgentEvent::Line { line } = e {
            said.push(line.clone());
        }
    });
    // One attempt in one session: the finish woke it; nothing was sent and nothing stopped.
    let prompts = env.prompts();
    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[1].1, crate::plan::runner::fake::WOKE);
    assert!(env.sent().is_empty());
    assert!(env.stopped().is_empty());
    assert!(end.idle);
    assert_eq!(
        said.iter()
            .filter(|l| l.contains(BACKGROUND_WAIT_LINE))
            .count(),
        1
    );
}

#[test]
fn a_background_command_that_never_ends_is_given_up_after_the_wait() {
    let env = FakeEnv::new();
    env.set_agent(|call| AgentReply {
        lines: if call.turn == 1 {
            vec![background(1)]
        } else {
            vec![]
        },
        result: Some("stopped waiting".into()),
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    let started = env.now_ms();
    let end = run(&env, &c, &mut w, &ask(""));
    assert!(env.now_ms() - started >= BACKGROUND_WAIT_MS);
    assert_eq!(env.sent().len(), 1);
    assert_eq!(env.sent()[0].1, BACKGROUND_GIVE_UP);
    // The command still runs, but the turn is not held again.
    assert_eq!(env.prompts().len(), 2);
    assert!(end.idle);
}

#[test]
fn a_stop_while_held_for_a_background_command_stops_the_turn() {
    let env = FakeEnv::new();
    let c = Arc::new(Controls::default());
    let cc = c.clone();
    env.set_agent(|_| AgentReply {
        lines: vec![background(1)],
        ..Default::default()
    });
    // The owner presses Stop a minute into the wait.
    let from = env.now_ms();
    env.set_tick(move |e| {
        if e.now_ms() - from >= 60_000 {
            cc.stop_now();
        }
    });
    let mut w = watch(&env);
    let end = run(&env, &c, &mut w, &ask(""));
    assert!(env.now_ms() - from < BACKGROUND_WAIT_MS);
    assert_eq!(env.stopped().len(), 1);
    assert!(!end.idle);
}

#[test]
fn a_ticked_step_is_not_held_for_a_background_command_left_behind() {
    // bg1: Claude Code moved a slow `find /` to the background on its 120 s limit; the agent ticked the step and
    // ended its turn; the turn was held 30 minutes for a command nothing waited on.
    let env = FakeEnv::new();
    env.put("PLAN.md", "# Plan\n\n- [x] **1. Do it**\n");
    env.set_agent(|_| AgentReply {
        lines: vec![background(1)],
        result: Some("Step 1 is done and ticked.".into()),
        ..Default::default()
    });
    let c = Controls::default();
    let mut w = watch(&env);
    w.plan_step = Some(("PLAN.md".into(), "1".into()));
    let started = env.now_ms();
    let end = run(&env, &c, &mut w, &ask(""));
    assert!(
        env.now_ms() - started < 60_000,
        "held {} ms",
        env.now_ms() - started
    );
    assert!(env.sent().is_empty(), "{:?}", env.sent());
    assert_eq!(env.prompts().len(), 1);
    assert!(end.idle);
    // Not ticked: still held for it, as before (z4).
    env.put("PLAN.md", "# Plan\n\n- [ ] **1. Do it**\n");
    let mut w = watch(&env);
    w.plan_step = Some(("PLAN.md".into(), "1".into()));
    let started = env.now_ms();
    run(&env, &c, &mut w, &ask(""));
    assert!(env.now_ms() - started >= BACKGROUND_WAIT_MS);
}
