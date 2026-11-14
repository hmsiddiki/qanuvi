//! The plan runner end to end (ported from `src/lib/stores/runner.scenarios.test.ts`): the run loop in the core and
//! the real plan logic, against the world from memory (`fake.rs`) with a scripted agent. Each scenario is a run that
//! once went wrong or must keep working; every runner bug found in a real run gets one here first.
//!
//! `Run` below is the one place they meet the run loop (`plan::run`, R2c). The TypeScript runner goes (R2e) only
//! when all of them pass here.

use super::fake::{AgentCall, AgentReply, FakeEnv};
use super::{
    FullChecks, OnLimit, PauseMode, PlanRun, RunOptions, RunStatus, RunStepStatus, RunTeam, StepWhy,
};
use crate::plan::env::RunEnv;
use crate::plan::run::{Shared, run_plan, test_setup};
use crate::projects::ProjectSettings;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

const FILE: &str = "content/plugins/shop/PLAN.md";

/// A run of the scenarios' plan on a thread: the owner's Stop, the Inbox questions it waits on, its end.
struct Run {
    shared: Arc<Shared>,
    thread: std::thread::JoinHandle<Result<PlanRun, String>>,
}

/// The owner's Stop button, usable from inside an agent's turn.
#[derive(Clone)]
struct Stop(Arc<Shared>);

impl Stop {
    fn press(&self) {
        self.0.stop();
    }
}

impl Run {
    fn start(env: Arc<FakeEnv>, file: &str, opts: RunOptions, stop: Stop) -> Run {
        let shared = stop.0.clone();
        let sh = shared.clone();
        let setup = test_setup(file, opts);
        let thread = std::thread::spawn(move || run_plan(&*env, &sh, setup));
        // The run has its record before anyone looks at it.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while shared.run().is_none() && !thread.is_finished() && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        Run { shared, thread }
    }

    /// Questions the run waits on now (held steps in the Inbox).
    fn waiting_answers(&self) -> usize {
        self.shared.waiting_answers()
    }

    /// The run as it is now.
    fn now(&self) -> PlanRun {
        self.shared.run().expect("the run has started")
    }

    fn wait(self) -> PlanRun {
        self.thread
            .join()
            .expect("the run thread")
            .expect("the run starts")
    }
}

fn stop_button() -> Stop {
    Stop(Arc::new(Shared::new(Duration::from_millis(5))))
}

fn settings(changes_from: &str) -> ProjectSettings {
    ProjectSettings {
        run_command: String::new(),
        test_command: "php tests/run.php".into(),
        preview_url: String::new(),
        changes_from: changes_from.into(),
    }
}

fn options() -> RunOptions {
    RunOptions {
        from_line: 0,
        only_one: false,
        mode: "auto".into(),
        budget_usd: 0.0,
        attempts_per_model: 1,
        lean: true,
        index_tools: false,
        packets: false,
        replan: true,
        ranges: Some(String::new()),
        provider_id: Some("claude-code".into()),
        model: Some("claude-opus-5-5".into()),
        stronger_on_fail: Some(false),
        team: Some(RunTeam::Settings),
        carry_on: Some(false),
        full_checks: Some(FullChecks::Section),
        wait_before: Some(vec![]),
        extra_checks: Some(vec![]),
        on_limit: Some(OnLimit::Wait),
        step_cost_limit: Some(0.0),
        step_minutes: Some(0.0),
        pause: Some(PauseMode::Never),
        finish_pass: None,
        ..Default::default()
    }
}

fn extra(step: &str, why: &str) -> Option<Vec<StepWhy>> {
    Some(vec![StepWhy {
        step: step.into(),
        why: why.into(),
    }])
}

fn world(plan: &str) -> Arc<FakeEnv> {
    let env = Arc::new(FakeEnv::new());
    env.set_settings(settings(""));
    env.put(FILE, plan);
    env
}

fn run(env: &Arc<FakeEnv>, opts: RunOptions) -> PlanRun {
    Run::start(env.clone(), FILE, opts, stop_button()).wait()
}

/// The step number a prompt is about ("Step 97a-1: …" or the bold title in it); the longest match wins.
fn step_of<'a>(prompt: &str, numbers: &[&'a str]) -> &'a str {
    numbers
        .iter()
        .filter(|n| {
            prompt.contains(&format!("**{n}. "))
                || prompt.contains(&format!("step {n}"))
                || prompt.contains(&format!("Step {n}"))
        })
        .max_by_key(|n| n.len())
        .copied()
        .unwrap_or_else(|| {
            panic!(
                "no known step in prompt: {}",
                &prompt[..prompt.len().min(300)]
            )
        })
}

/// The planner's turns are read-only; the steps' are not.
fn planner(c: &AgentCall) -> bool {
    c.mode == "plan"
}

fn statuses(r: &PlanRun) -> Vec<(Option<String>, RunStepStatus)> {
    r.steps
        .iter()
        .map(|s| (s.number.clone(), s.status))
        .collect()
}

fn step<'a>(r: &'a PlanRun, n: &str) -> &'a super::StepRun {
    r.steps
        .iter()
        .find(|s| s.number.as_deref() == Some(n))
        .unwrap_or_else(|| panic!("no step {n} in the run"))
}

fn questions(env: &FakeEnv) -> usize {
    env.inbox().iter().filter(|i| i.kind_of == "answer").count()
}

fn ok_review(c: &AgentCall) -> AgentReply {
    assert!(planner(c));
    AgentReply {
        result: Some(r#"{"ok": true, "why": "fine"}"#.into()),
        cost: Some(0.001),
        ..Default::default()
    }
}

#[test]
fn runs_a_plain_plan_to_the_end() {
    let env =
        world("# Plan\n\n- [ ] **1. First**\n  - do it\n- [ ] **2. Second**\n  - then this\n");
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env
            .put(&format!("src/Step{n}.php"), &format!("<?php // {n}\n"));
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(
        statuses(&r),
        vec![
            (Some("1".into()), RunStepStatus::Done),
            (Some("2".into()), RunStepStatus::Done)
        ]
    );
    assert!(env.text(FILE).contains("- [x] **1. First**"));
    assert!(env.text(FILE).contains("- [x] **2. Second**"));
    assert_eq!(questions(&env), 0);
}

#[test]
fn plan_f_97a_a_review_that_sees_no_diff_of_real_work_does_not_turn_the_step_down() {
    let env = world("# Plan\n\n- [ ] **97a. Storage design**\n  - schema 26\n");
    *env.blind_diff.lock().unwrap() = true; // what git did in the folder it ignores
    env.set_agent(|c| {
        if planner(c) {
            panic!(
                "no review is asked for when no diff can be shown: {:?}",
                c.title
            );
        }
        c.env
            .put("content/plugins/shop/src/Schema.php", "<?php // v26\n");
        c.env.mark(FILE, "97a", 'x');
        AgentReply::default()
    });
    let r = run(
        &env,
        RunOptions {
            extra_checks: extra("97a", "schema migration"),
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(r.steps[0].status, RunStepStatus::Done);
    let review = r
        .decisions
        .iter()
        .flatten()
        .find(|d| d.kind == "review")
        .expect("a review decision");
    assert_eq!(review.decision, "accepted");
    assert!(review.why.contains("no diff could be shown"));
    assert!(env.text(FILE).contains("- [x] **97a."));
}

#[test]
fn a_review_sees_a_steps_change_in_an_ignored_plugin_folder_and_judges_it() {
    let env = world("# Plan\n\n- [ ] **3. Contact column**\n");
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        c.env.put(
            "content/plugins/shop/src/Cart.php",
            "<?php $contact_hash = 1;\n",
        );
        c.env.mark(FILE, "3", 'x');
        AgentReply::default()
    });
    let r = run(
        &env,
        RunOptions {
            extra_checks: extra("3", "schema"),
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Done);
    let asked = env
        .requests()
        .into_iter()
        .find(|q| {
            q.title
                .as_deref()
                .is_some_and(|t| t.starts_with("Decider: review"))
        })
        .expect("the decider was asked to review");
    assert!(asked.prompt.contains("+<?php $contact_hash = 1;"));
    assert!(!asked.prompt.contains("PLAN.md (before")); // the plan file itself is left out
}

#[test]
fn plan_f_97a_replanned_steps_get_free_numbers_the_replaced_step_is_not_held_and_it_is_ticked_after_them()
 {
    let env = world(
        &[
            "# Plan",
            "",
            "- [ ] **97a. Storage design**",
            "  - all of it",
            "- [ ] **97b. Phone mode**",
            "  - Needs: 97a",
            "- [ ] **97c. Texts**",
            "",
        ]
        .join("\n"),
    );
    // 97a never finishes on the one model; the planner splits it, writing its own labels before the titles.
    env.set_agent(|c| {
        if planner(c) {
            if c.title.is_some_and(|t| t.starts_with("Planner: rewrite")) {
                let steps = "```tasklist\n- [ ] **97a1. Make it visible**\n  - first\n- [ ] **97a2. Schema 26**\n  - second\n```";
                return AgentReply { result: Some(steps.into()), ..Default::default() };
            }
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["97a", "97a-1", "97a-2", "97b", "97c"]);
        if n != "97a" {
            c.env.put(&format!("src/{n}.php"), &format!("<?php // {n}\n"));
            c.env.mark(FILE, n, 'x');
        }
        AgentReply::default()
    });
    let r = run(&env, options());
    let plan = env.text(FILE);
    for done in [
        "97a-1. Make it visible",
        "97a-2. Schema 26",
        "97a. Storage design",
        "97b. Phone mode",
        "97c. Texts",
    ] {
        assert!(
            plan.contains(&format!("- [x] **{done}**")),
            "{done} is not ticked:\n{plan}"
        );
    }
    assert!(!plan.contains("**97b. 97a"));
    assert_eq!(r.status, RunStatus::Done);
    assert!(
        step(&r, "97a")
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("done through its replacement steps")
    );
    // Nobody was asked: the replaced step was never held and nothing waited on it.
    assert_eq!(questions(&env), 0);
    assert!(!r.decisions.iter().flatten().any(|d| d.kind == "set-aside"));
}

#[test]
fn git_chosen_but_the_plans_folder_is_out_of_gits_sight_one_question_and_the_run_goes_on() {
    let env = world("# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    env.set_settings(settings("git"));
    *env.git_blind.lock().unwrap() = vec!["content/plugins/shop".into()];
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.put(
            &format!("content/plugins/shop/src/S{n}.php"),
            &format!("<?php // {n}\n"),
        );
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    // No reviews in this run: the question comes from the check before the first step.
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    let asks: Vec<_> = env
        .inbox()
        .into_iter()
        .filter(|i| {
            i.source
                .as_ref()
                .and_then(|s| s.get("kind"))
                .and_then(|k| k.as_str())
                == Some("changes-from")
        })
        .collect();
    assert_eq!(asks.len(), 1);
    assert!(asks[0].text.contains("content/plugins/shop"));
    // The owner picks Qanuvi's snapshots: the project setting changes.
    env.answer(asks[0].key.as_deref().unwrap(), json!({"choice": "qanuvi"}));
    // The core's answer follower applies it (R2d calls it when an Inbox item is answered).
    let answered = env
        .inbox_by_key(asks[0].key.as_deref().unwrap())
        .unwrap()
        .unwrap();
    crate::plan::run::on_answer(&*env, &answered);
    assert_eq!(env.settings().changes_from, "qanuvi");
}

#[test]
fn a_blocked_step_goes_to_the_inbox_the_run_waits_and_an_answer_lets_it_go_on() {
    let env = world("# Plan\n\n- [ ] **1. Needs a key**\n- [ ] **2. Other**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = tries.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" && t.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            c.env.mark(FILE, "1", '!');
            let text = c.env.text(FILE).replace(
                "**1. Needs a key**",
                "**1. Needs a key**\n  - Blocked: the API key is missing",
            );
            c.env.put(FILE, &text);
            return AgentReply::default();
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop_button(),
    );
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while running.waiting_answers() == 0 {
        assert!(
            std::time::Instant::now() < until,
            "the run never waited for an answer"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let q = env
        .inbox()
        .into_iter()
        .find(|i| i.kind_of == "answer")
        .expect("a question");
    assert!(q.title.contains("step 1. Needs a key is blocked"));
    assert_eq!(step(&running.now(), "2").status, RunStepStatus::Done);
    env.answer(
        q.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "the key is in the secrets now"}),
    );
    let r = running.wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(env.text(FILE).contains("- [x] **1. Needs a key**"));
    assert!(
        env.prompts()
            .last()
            .unwrap()
            .1
            .contains("the key is in the secrets now")
    );
}

#[test]
fn failing_tests_go_back_to_the_same_session_with_only_the_failures_then_pass() {
    let env = world("# Plan\n\n- [ ] **5. Coupon rule**\n");
    let fixed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let f = fixed.clone();
    env.set_checks(move |_| {
        if f.load(std::sync::atomic::Ordering::SeqCst) {
            (true, "OK (12 tests)".into())
        } else {
            (
                false,
                "FAIL CouponTest::testRounding\nexpected 10, got 9".into(),
            )
        }
    });
    let f = fixed.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        if c.turn == 1 {
            c.env.put("src/Coupon.php", "<?php // round down\n");
            c.env.mark(FILE, "5", 'x');
        } else {
            c.env.put("src/Coupon.php", "<?php // round half up\n");
            f.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        AgentReply::default()
    });
    let r = run(
        &env,
        RunOptions {
            extra_checks: extra("5", "money"),
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Done);
    let steps: Vec<_> = env
        .requests()
        .into_iter()
        .filter(|q| q.mode != "plan")
        .collect();
    assert_eq!(steps.len(), 2); // one session, one fix turn
    assert!(steps[1].prompt.contains("CouponTest::testRounding"));
    assert!(steps[1].session_id.is_some());
    assert_eq!(r.steps[0].attempts.last().unwrap().tests_passed, Some(true));
}

#[test]
fn pause_after_each_step_stops_after_a_whole_step_and_the_next_run_goes_on_from_the_next_one() {
    let env = world("# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        c.env.mark(FILE, step_of(c.prompt, &["1", "2"]), 'x');
        AgentReply::default()
    });
    let first = run(
        &env,
        RunOptions {
            pause: Some(PauseMode::Step),
            ..options()
        },
    );
    assert_ne!(first.status, RunStatus::Running);
    let done: Vec<_> = first
        .steps
        .iter()
        .filter(|s| s.status == RunStepStatus::Done)
        .map(|s| s.number.clone())
        .collect();
    assert_eq!(done, vec![Some("1".to_string())]);
    assert!(env.text(FILE).contains("- [ ] **2. Two**"));
    let second = run(&env, options());
    assert_eq!(second.status, RunStatus::Done);
    assert!(env.text(FILE).contains("- [x] **2. Two**"));
}

#[test]
fn stopping_during_a_step_ends_the_run_as_stopped_and_the_step_stays_open() {
    let env = world("# Plan\n\n- [ ] **1. Long**\n- [ ] **2. Later**\n");
    // The owner presses Stop twice during the first turn (the second press stops at once). Only once: the gentle
    // stop sends one more message, and that late turn must not press Stop again.
    let stop = stop_button();
    let s = stop.clone();
    env.set_agent(move |c| {
        if !planner(c) && c.turn == 1 {
            s.press();
            s.press();
        }
        AgentReply::default()
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Stopped);
    assert!(env.text(FILE).contains("- [ ] **1. Long**"));
    assert!(env.text(FILE).contains("- [ ] **2. Later**"));
    assert_eq!(questions(&env), 0);
}

#[test]
fn a_step_whose_needs_line_names_an_unfinished_step_in_another_plan_is_skipped_with_the_reason() {
    // A Needs line names another plan by its path in the project.
    let env = world(
        "# Plan\n\n- [ ] **1. Uses the base**\n  - Needs: OTHER.md#4\n- [ ] **2. Free of it**\n",
    );
    env.put("OTHER.md", "# Other\n\n- [ ] **4. Base**\n");
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        c.env.mark(FILE, step_of(c.prompt, &["1", "2"]), 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Skipped);
    assert_eq!(
        one.reason.as_deref(),
        Some("needs step 4 of OTHER.md, which is not done")
    );
    // Skipped once, listed once (it came back as "sent back by the owner" and was listed twice before 2026-11-14).
    assert_eq!(
        r.steps
            .iter()
            .filter(|s| s.number.as_deref() == Some("1"))
            .count(),
        1
    );
    assert_eq!(
        r.message,
        "1 done · 1 skipped: step 1 (needs step 4 of OTHER.md, which is not done)"
    );
    assert_eq!(step(&r, "2").status, RunStepStatus::Done);
}

// ---------- Added with the run loop in the core (R2c) ----------

#[test]
fn pausing_in_the_middle_of_a_step_keeps_its_session_for_resume() {
    let env = world("# Plan\n\n- [ ] **1. Long**\n- [ ] **2. Later**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        if !planner(c) && c.turn == 1 {
            sh.pause();
        }
        AgentReply::default()
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Paused);
    assert!(r.message.starts_with("Paused in step 1."), "{}", r.message);
    let paused = r
        .paused_session
        .as_ref()
        .expect("the step's session is kept");
    assert_eq!(paused.number.as_deref(), Some("1"));
    // The agent was asked to write where it is, and its session was not stopped.
    assert!(
        env.sent()
            .iter()
            .any(|(_, t)| t.contains("the owner pauses the run now"))
    );
    assert!(!env.stopped().contains(&paused.session_id));
    assert_eq!(step(&r, "1").attempts[0].note, "paused by the owner");
    assert_eq!(step(&r, "2").status, RunStepStatus::Waiting);
}

#[test]
fn a_usage_limit_waits_for_the_reset_and_the_step_is_tried_again() {
    let env = world("# Plan\n\n- [ ] **1. One**\n");
    let started = env.now_ms();
    env.set_agent(|c| {
        if c.turn == 1 && c.env.prompts().len() == 1 {
            return AgentReply {
                result: Some("You've hit your usage limit · resets 11pm (UTC)".into()),
                is_error: true,
                ..Default::default()
            };
        }
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(env.prompts().len(), 2);
    // The fake clock moved on to the reset (it starts at 08:00 UTC).
    assert!(
        env.now_ms() - started >= 14 * 3_600_000,
        "waited {} ms",
        env.now_ms() - started
    );
    // The limit is no failed try.
    assert_eq!(step(&r, "1").attempts.len(), 1);
}

#[test]
fn the_owners_model_switch_hands_the_step_over_to_the_chosen_model() {
    let env = world("# Plan\n\n- [ ] **1. Rework**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        if c.model == "claude-opus-5-5" {
            sh.controls.switch_model("claude-sonnet-5-5");
            return AgentReply::default();
        }
        assert!(c.prompt.contains("The owner switched this step to you"));
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    let tries = &step(&r, "1").attempts;
    assert_eq!(tries.len(), 2);
    assert_eq!(
        tries[0].note,
        "handed over to Sonnet 5.5 (the owner's choice)"
    );
    assert_eq!(tries[1].model, "claude-sonnet-5-5");
    // The choice is kept in the step's Model line.
    assert!(env.text(FILE).contains("claude-sonnet-5-5"));
}

#[test]
fn private_details_reach_each_step_and_go_when_the_whole_plan_is_done() {
    let env = world("# Plan\n\n- [ ] **1. Sign in**\n");
    env.set_private(&["Test login: admin / test-pass-1"]);
    env.set_agent(|c| {
        assert!(c.prompt.contains("Test login: admin / test-pass-1"));
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    assert!(env.calls().iter().any(|c| c == "plan_private_clear"));
}

#[test]
fn an_answer_given_in_the_inbox_while_no_run_went_on_counts_at_the_next_start() {
    let env = world("# Plan\n\n- [ ] **1. Needs a key**\n- [ ] **2. Other**\n");
    let blocked = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let b = blocked.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" && b.load(std::sync::atomic::Ordering::SeqCst) {
            c.env.mark(FILE, "1", '!');
            return AgentReply::default();
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    // The first run holds step 1 and is ended without the answer.
    let stop = stop_button();
    let sh = stop.0.clone();
    let first = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop,
    );
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while first.waiting_answers() == 0 {
        assert!(std::time::Instant::now() < until, "the run never waited");
        std::thread::sleep(Duration::from_millis(2));
    }
    sh.finish_without();
    let r = first.wait();
    assert_eq!(r.status, RunStatus::NeedsYou);
    // Answered later, with no run going.
    let q = env
        .inbox()
        .into_iter()
        .find(|i| i.kind_of == "answer")
        .expect("a question");
    env.answer(
        q.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "the key is set now"}),
    );
    blocked.store(false, std::sync::atomic::Ordering::SeqCst);
    // The step is open again in the plan (the owner unblocked it), and the next run goes on with the answer.
    let text = env.text(FILE).replace("- [!] **1.", "- [ ] **1.");
    env.put(FILE, &text);
    let r = run(
        &env,
        RunOptions {
            replan: false,
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Done);
    assert!(
        env.prompts()
            .last()
            .unwrap()
            .1
            .contains("the key is set now")
    );
    assert_eq!(questions(&env), 0);
}
