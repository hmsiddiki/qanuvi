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
use crate::plan::ownermsg;
use crate::plan::run::{RunSetup, Shared, run_plan, test_setup};
use crate::plan_data::Kind;
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
        Run::start_setup(env, test_setup(file, opts), stop)
    }

    fn start_setup(env: Arc<FakeEnv>, setup: RunSetup, stop: Stop) -> Run {
        let shared = stop.0.clone();
        let sh = shared.clone();
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
        ..Default::default()
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
    assert_eq!(r.budget_reached, None);
}

#[test]
fn y2_a_step_that_failed_on_every_model_runs_once_and_asks_once() {
    let env = world("# Plan\n\n- [ ] **1. Hard**\n  - do it\n- [ ] **2. Easy**\n  - then this\n");
    let hard = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let h = hard.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        if step_of(c.prompt, &["1", "2"]) == "1" {
            // Never finishes: step 1 stays open on every model.
            h.lock().unwrap().push(c.model.to_string());
        } else {
            c.env.mark(FILE, "2", 'x');
        }
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            stronger_on_fail: Some(true),
            ..options()
        },
        stop.clone(),
    );
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while running.waiting_answers() == 0 || step(&running.now(), "2").status != RunStepStatus::Done
    {
        assert!(std::time::Instant::now() < until, "the run never asked");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Time for a wrong second copy of step 1 to start.
    std::thread::sleep(Duration::from_millis(300));
    let now = running.now();
    let copies = now
        .steps
        .iter()
        .filter(|s| s.number.as_deref() == Some("1"))
        .count();
    let tries = hard.lock().unwrap().clone();
    stop.press();
    stop.press();
    let _ = running.wait();
    assert_eq!(copies, 1, "step 1 is in the run once");
    assert_eq!(running_questions(&env), 1, "one question about step 1");
    let mut seen = tries.clone();
    seen.dedup();
    assert_eq!(
        seen.len(),
        tries.len(),
        "each model tried once, no second ladder: {tries:?}"
    );
}

fn running_questions(env: &FakeEnv) -> usize {
    env.inbox().iter().filter(|i| i.kind_of == "answer").count()
}

#[test]
fn y2_an_agent_that_is_not_on_this_machine_stops_the_run_before_any_step_tries_its_models() {
    let env = world("# Plan\n\n- [ ] **1. First**\n  - do it\n");
    *env.missing_agent.lock().unwrap() = Some("The agent is not on this machine.".into());
    let tried = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t = tried.clone();
    env.set_agent(move |_| {
        t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        AgentReply::default()
    });
    let stop = stop_button();
    let got = run_plan(&*env, &stop.0, test_setup(FILE, options()));
    assert_eq!(got.unwrap_err(), "The agent is not on this machine.");
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(questions(&env), 0);
    assert!(env.text(FILE).contains("- [ ] **1. First**"));
}

#[test]
fn b1_a_run_stopped_by_its_budget_says_so_on_the_record() {
    let env =
        world("# Plan\n\n- [ ] **1. First**\n  - do it\n- [ ] **2. Second**\n  - then this\n");
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.mark(FILE, n, 'x');
        AgentReply {
            cost: Some(1.16),
            ..Default::default()
        }
    });
    let r = run(
        &env,
        RunOptions {
            budget_usd: 1.0,
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Stopped);
    assert_eq!(r.budget_reached, Some(1.0));
    assert!(r.message.contains("budget of $1.00"), "{}", r.message);
    assert_eq!(step(&r, "1").status, RunStepStatus::Done);
    assert_eq!(step(&r, "2").status, RunStepStatus::Waiting);
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
                let steps = "```tasklist\n- [ ] **97a1. Make it visible**\n  - first\n  - Model: claude-sonnet-5-5\n- [ ] **97a2. Schema 26**\n  - second\n```";
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
    // The planner's model for a new step is kept in the plan's own data, not written into the file (R2e i).
    assert!(!plan.contains("Model:"), "{plan}");
    let state = crate::plan::planstate::PlanState::from_json(
        &env.plan_data_get(FILE, crate::plan_data::Kind::State)
            .unwrap(),
    );
    assert_eq!(
        state
            .kept_model(Some("97a-1"), "Make it visible")
            .as_deref(),
        Some("claude-sonnet-5-5")
    );
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
fn a_step_blocked_again_after_the_answer_asks_again_as_a_new_question() {
    let env = world("# Plan\n\n- [ ] **1. Needs a login**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = tries.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        // The first two tries block with the very same words; the third (after the second answer) is done.
        if t.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 2 {
            c.env.mark(FILE, "1", '!');
            let text = c.env.text(FILE).replace(
                "**1. Needs a login**",
                "**1. Needs a login**\n  - Blocked: the admin login is missing",
            );
            c.env.put(FILE, &text);
            return AgentReply::default();
        }
        c.env.mark(FILE, "1", 'x');
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
    let open_question = |env: &FakeEnv| {
        env.inbox()
            .into_iter()
            .find(|i| i.kind_of == "answer" && i.status == "open")
    };
    let wait_for = |what: &str, f: &dyn Fn() -> bool| {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !f() {
            assert!(std::time::Instant::now() < until, "{what}");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    wait_for("the first question never came", &|| {
        open_question(&env).is_some()
    });
    let first = open_question(&env).unwrap();
    env.answer(
        first.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "try again"}),
    );
    // Blocked again with the same words: a new, open question (so the owner is told again), never the answered one.
    wait_for("the second question never came", &|| {
        open_question(&env).is_some_and(|q| q.id != first.id)
    });
    let second = open_question(&env).unwrap();
    assert_eq!(second.key, first.key);
    assert!(second.answer.is_none());
    assert_eq!(
        env.inbox().iter().filter(|i| i.kind_of == "answer").count(),
        1,
        "the answered question went; only the new one is there"
    );
    env.answer(
        second.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "the login is admin now"}),
    );
    let r = running.wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(
        env.prompts()
            .last()
            .unwrap()
            .1
            .contains("the login is admin now")
    );
}

/// n1: a step blocked on a server change (the agent's last message holds the nginx rule).
fn nginx_blocker(c: &AgentCall) -> AgentReply {
    c.env.mark(FILE, "20", '!');
    let text = c.env.text(FILE).replace(
        "**20. Serve uploads**",
        "**20. Serve uploads**\n  - Blocked: needs an nginx rule on the server",
    );
    c.env.put(FILE, &text);
    AgentReply {
        result: Some(
            "Uploads return 404: nginx does not serve /files/. Add this to the server's nginx.conf, inside the \
             server block:\n```nginx\nlocation /files/ {\n    alias /srv/app/files/;\n}\n```\nThen reload nginx."
                .into(),
        ),
        ..Default::default()
    }
}

fn wait_until(what: &str, f: &dyn Fn() -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !f() {
        assert!(std::time::Instant::now() < until, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn open_question(env: &FakeEnv) -> Option<crate::inbox::InboxItem> {
    env.inbox()
        .into_iter()
        .find(|i| i.kind_of == "answer" && i.status == "open")
}

#[test]
fn a_blocked_step_asks_a_question_that_stands_on_its_own_and_talks_until_the_need_is_met() {
    let env = world("# Plan\n\n- [ ] **20. Serve uploads**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = tries.clone();
    env.set_agent(move |c| {
        let title = c.title.unwrap_or("");
        if title.starts_with("Question for you") {
            assert!(c.prompt.contains("alias /srv/app/files/;"), "the card call reads the agent's message");
            return AgentReply {
                result: Some(r#"{"headline": "Add one rule to the web server", "happened": "Uploaded files give a 404 error.", "whyYou": "nginx.conf is on the server, outside the project.", "wanted": "Add the rule below to nginx.conf and reload nginx.", "snippet": {"label": "Add to nginx.conf, inside the server block", "text": "location /files/ {\n    alias /srv/app/files/;\n}"}, "canDo": "Say \"do it yourself\" and tell me where nginx.conf is."}"#.into()),
                ..Default::default()
            };
        }
        if title.starts_with("Your answer") {
            let reply = if c.prompt.contains("<<<\nI don't understand\n>>>") {
                r#"{"intent": "clarify", "card": {"headline": "The web server needs to be told where uploaded files are", "wanted": "Open nginx.conf on the server, paste the rule inside the server block, save, then run: sudo nginx -s reload", "snippet": {"label": "The rule", "text": "location /files/ {\n    alias /srv/app/files/;\n}"}}}"#
            } else if c.prompt.contains("<<<\nlater maybe\n>>>") {
                r#"{"intent": "followup", "card": {"headline": "Should I wait, or add the rule myself now?"}}"#
            } else {
                r#"{"intent": "act", "instruction": "Add the rule to /etc/nginx/nginx.conf yourself and reload nginx."}"#
            };
            return AgentReply { result: Some(reply.into()), ..Default::default() };
        }
        if planner(c) {
            return ok_review(c);
        }
        if t.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return nginx_blocker(c);
        }
        c.env.mark(FILE, "20", 'x');
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
    wait_until("the question never came", &|| open_question(&env).is_some());
    let first = open_question(&env).unwrap();
    // (a) The whole matter on its own: what failed, why the owner, the exact change to copy, what Qanuvi can do.
    assert!(
        first.text.starts_with("Add one rule to the web server"),
        "{}",
        first.text
    );
    assert!(
        first
            .text
            .contains("Why it needs you: nginx.conf is on the server")
    );
    assert!(
        first
            .text
            .contains("Add to nginx.conf, inside the server block:\nlocation /files/ {")
    );
    let form = first.form.clone().unwrap();
    assert_eq!(
        form["card"]["snippet"]["text"],
        "location /files/ {\n    alias /srv/app/files/;\n}"
    );
    let ids: Vec<&str> = form["choices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["go", "skip", "end"]);
    assert!(form["text"].as_str().unwrap().starts_with("Your answer"));

    // (b) "I don't understand": a new, simpler question; the step does not run again.
    env.answer(
        first.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "I don't understand"}),
    );
    wait_until("no simpler question", &|| {
        open_question(&env).is_some_and(|q| q.id != first.id)
    });
    let second = open_question(&env).unwrap();
    assert!(
        second
            .text
            .starts_with("The web server needs to be told where uploaded files are")
    );
    assert!(second.text.contains("sudo nginx -s reload"));
    assert_eq!(
        tries.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no blind retry"
    );

    // An answer that does not settle the need: a follow-up question, still no retry.
    env.answer(
        second.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "later maybe"}),
    );
    wait_until("no follow-up", &|| {
        open_question(&env).is_some_and(|q| q.id != second.id)
    });
    let third = open_question(&env).unwrap();
    assert!(
        third
            .text
            .starts_with("Should I wait, or add the rule myself now?")
    );
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);

    // A real answer: the agent is told the talk and what to do, and acts on it.
    env.answer(
        third.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "insert the block yourself"}),
    );
    let r = running.wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(env.text(FILE).contains("- [x] **20. Serve uploads**"));
    let last = env.prompts().last().unwrap().1.clone();
    assert!(last.contains("Owner: \"I don't understand\""), "{last}");
    assert!(last.contains("Owner: \"later maybe\""));
    assert!(last.contains("insert the block yourself"));
    assert!(last.contains(
        "What their answer means for this try: Add the rule to /etc/nginx/nginx.conf yourself"
    ));
    assert!(last.contains("make the change yourself, make it now"));
    let st = step(&r, "20");
    assert_eq!(st.talk.as_ref().map(Vec::len), Some(3));
}

#[test]
fn without_any_model_the_question_still_says_it_all_and_end_the_run_leaves_the_step_open() {
    let env = world("# Plan\n\n- [ ] **20. Serve uploads**\n- [ ] **21. Other**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = tries.clone();
    env.set_agent(move |c| {
        if planner(c) {
            // No card, no reading: the review's answer is all any call gets.
            return ok_review(c);
        }
        if step_of(c.prompt, &["20", "21"]) == "21" {
            c.env.mark(FILE, "21", 'x');
            return AgentReply::default();
        }
        t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        nginx_blocker(c)
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
    wait_until("the question never came", &|| running.waiting_answers() > 0);
    let first = open_question(&env).unwrap();
    // The agent's own words and its snippet, never only "it needs you".
    assert!(
        first.text.contains("needs an nginx rule on the server"),
        "{}",
        first.text
    );
    assert!(first.text.contains("Uploads return 404"));
    assert!(first.text.contains("alias /srv/app/files/;"));
    // n2: no phrase list. With no model to read the reply, the owner's words go to the step's own agent, told to
    // answer a question back and ask again rather than guess or redo the work.
    env.answer(
        first.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "I don't understand"}),
    );
    wait_until("the step never heard the reply", &|| {
        tries.load(std::sync::atomic::Ordering::SeqCst) == 2
    });
    let told = env
        .prompts()
        .iter()
        .rev()
        .find(|(_, p)| p.contains("Owner: \"I don't understand\""))
        .map(|(_, p)| p.clone())
        .expect("the reply reached the step");
    assert!(told.contains("ask something back"), "{told}");
    wait_until("no new question", &|| {
        open_question(&env).is_some_and(|q| q.id != first.id)
    });
    let second = open_question(&env).unwrap();
    // End the run: it ends, the step stays open for the next run.
    env.answer(second.key.as_deref().unwrap(), json!({"choice": "end"}));
    let r = running.wait();
    assert_ne!(r.status, RunStatus::Running);
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert_eq!(step(&r, "21").status, RunStepStatus::Done);
    assert!(!env.text(FILE).contains("- [x] **20."));
}

/// n2: a step blocked on a value only the owner's computer knows (the MySQL port).
fn port_blocker(c: &AgentCall) -> AgentReply {
    c.env.mark(FILE, "30", '!');
    let text = c.env.text(FILE).replace(
        "**30. Database tests**",
        "**30. Database tests**\n  - Blocked: needs the MySQL port of the local server",
    );
    c.env.put(FILE, &text);
    AgentReply {
        result: Some(
            "The tests cannot reach MySQL on 3306. Which port does the local MySQL listen on?"
                .into(),
        ),
        ..Default::default()
    }
}

fn says(text: &str) -> AgentReply {
    AgentReply {
        result: Some(text.into()),
        ..Default::default()
    }
}

#[test]
fn the_inbox_reads_every_reply_by_meaning_answers_a_question_back_guides_and_never_repeats_itself()
{
    let env = world("# Plan\n\n- [ ] **30. Database tests**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let readings = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let (t, seen) = (tries.clone(), readings.clone());
    env.set_agent(move |c| {
        let title = c.title.unwrap_or("");
        if title.starts_with("Question for you") {
            return says(
                r#"{"headline": "Which port does your MySQL use?", "happened": "The tests could not reach the database.", "wanted": "The port number of MySQL on this computer."}"#,
            );
        }
        if title.starts_with("Your answer") {
            seen.lock().unwrap().push(c.prompt.to_string());
            let p = c.prompt;
            return says(if p.contains("<<<\nwhat is a port??\n>>>") {
                r#"{"intent": "question", "reply": "A port is the number a program listens on, like a door number; MySQL usually uses 3306.", "card": {"headline": "Which door number does MySQL use on your computer?"}}"#
            } else if p.contains("<<<\nhuh\n>>>") {
                // The very words of the first question: Qanuvi never asks the same way twice.
                r#"{"intent": "unclear", "card": {"headline": "Which port does your MySQL use?", "wanted": "Open Wnmp and look at the number next to MySQL, for example 3306."}}"#
            } else if p.contains("<<<\nfind it yourself\n>>>") {
                r#"{"intent": "tried", "card": {"headline": "I found port 3307 in my.ini. Shall I use it?"}}"#
            } else {
                r#"{"intent": "act", "instruction": "Use MySQL port 3307 (found in my.ini) for the tests."}"#
            });
        }
        if planner(c) {
            return ok_review(c);
        }
        if t.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            return port_blocker(c);
        }
        c.env.mark(FILE, "30", 'x');
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
    wait_until("the question never came", &|| open_question(&env).is_some());
    let ask = |prev: &crate::inbox::InboxItem, text: &str| -> crate::inbox::InboxItem {
        env.answer(
            prev.key.as_deref().unwrap(),
            json!({"choice": "go", "text": text}),
        );
        wait_until(&format!("no question after \"{text}\""), &|| {
            open_question(&env).is_some_and(|q| q.id != prev.id)
        });
        open_question(&env).unwrap()
    };
    let first = open_question(&env).unwrap();
    assert!(first.text.starts_with("Which port does your MySQL use?"));

    // A question back: answered plainly first, then the need asked again; the step does not run.
    let second = ask(&first, "what is a port??");
    assert!(
        second
            .text
            .starts_with("A port is the number a program listens on"),
        "{}",
        second.text
    );
    assert!(
        second
            .text
            .contains("Which door number does MySQL use on your computer?")
    );
    assert_eq!(
        second.form.as_ref().unwrap()["card"]["reply"],
        "A port is the number a program listens on, like a door number; MySQL usually uses 3306."
    );
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);

    // Still stuck: the reading is told to guide, one small question with where to look and an example. Its card
    // repeats the first question word for word, so Qanuvi words it differently.
    let third = ask(&second, "huh");
    let huh = readings.lock().unwrap()[1].clone();
    assert!(huh.contains("still stuck"), "{huh}");
    assert!(huh.contains("one small question"));
    assert!(
        huh.contains("Which door number does MySQL use on your computer?"),
        "the talk so far"
    );
    assert!(
        !third.text.starts_with("Which port does your MySQL use?"),
        "asked the same way twice: {}",
        third.text
    );
    assert!(
        third
            .text
            .starts_with("Open Wnmp and look at the number next to MySQL")
    );
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);

    // An instruction it can carry out by looking: done read-only, the result shown for a yes.
    let fourth = ask(&third, "find it yourself");
    assert!(readings.lock().unwrap()[2].contains("read-only"));
    assert!(
        fourth
            .text
            .starts_with("I found port 3307 in my.ini. Shall I use it?")
    );
    assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);

    // The yes: the step runs with the whole talk and what the answer means.
    env.answer(
        fourth.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "yes"}),
    );
    let r = running.wait();
    assert_eq!(r.status, RunStatus::Done);
    let last = env.prompts().last().unwrap().1.clone();
    assert!(last.contains("Owner: \"what is a port??\""), "{last}");
    assert!(last.contains("Owner: \"find it yourself\""));
    assert!(last.contains("What their answer means for this try: Use MySQL port 3307"));
    assert_eq!(step(&r, "30").talk.as_ref().map(Vec::len), Some(4));
}

#[test]
fn a_held_steps_typed_answer_is_read_too() {
    let env = world("# Plan\n\n- [ ] **2a. Connect the test database**\n");
    let tries = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let t = tries.clone();
    env.set_agent(move |c| {
        let title = c.title.unwrap_or("");
        if title.starts_with("Your answer") {
            return says(if c.prompt.contains("<<<\nwhat do you mean??\n>>>") {
                r#"{"intent": "question", "reply": "It is the address of the database server, like localhost:3306.", "card": {"headline": "Where is the test database?"}}"#
            } else {
                r#"{"intent": "act", "instruction": "Use localhost:3306 as the test database address."}"#
            });
        }
        if planner(c) {
            return ok_review(c);
        }
        t.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        c.env.mark(FILE, "2a", 'x');
        AgentReply::default()
    });
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            wait_before: extra(
                "2a",
                "it needs Where the test database is (DB_HOST). Before the run you wrote \"the usual one\". Give the value, or say where Qanuvi can find it",
            ),
            ..options()
        },
        stop_button(),
    );
    wait_until("the question never came", &|| open_question(&env).is_some());
    let first = open_question(&env).unwrap();
    assert!(
        first.form.as_ref().unwrap()["text"].is_string(),
        "an answer box"
    );
    env.answer(
        first.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "what do you mean??"}),
    );
    wait_until("no answer to the question back", &|| {
        open_question(&env).is_some_and(|q| q.id != first.id)
    });
    let second = open_question(&env).unwrap();
    assert!(
        second
            .text
            .starts_with("It is the address of the database server"),
        "{}",
        second.text
    );
    assert!(second.text.contains("Where is the test database?"));
    assert_eq!(
        tries.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "not run on a question back"
    );
    env.answer(
        second.key.as_deref().unwrap(),
        json!({"choice": "go", "text": "localhost:3306"}),
    );
    let r = running.wait();
    assert_eq!(r.status, RunStatus::Done);
    let last = env.prompts().last().unwrap().1.clone();
    assert!(
        last.contains("Use localhost:3306 as the test database address."),
        "{last}"
    );
    assert!(last.contains("Owner: \"what do you mean??\""));
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

#[test]
fn a_step_that_needs_a_later_step_of_the_run_waits_for_it_and_runs_after_it() {
    // R2e live drive 2026-11-15: 6b needed 6c (the pre-run check wrote the line), 6c came later in the file and was
    // done in the same run, yet 6b was skipped as "needs step 6c, which is not done".
    let env = world(
        "# Plan\n\n- [ ] **6b. Sorted unique**\n  - Needs: 6c\n- [ ] **6c. Unique**\n- [ ] **6d. Last**\n",
    );
    let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = order.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["6b", "6c", "6d"]);
        seen.lock().unwrap().push(n.to_string());
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(step(&r, "6b").status, RunStepStatus::Done);
    assert_eq!(step(&r, "6b").reason, None);
    assert_eq!(*order.lock().unwrap(), vec!["6c", "6d", "6b"]);
    assert!(env.text(FILE).contains("- [x] **6b. Sorted unique**"));
}

#[test]
fn two_steps_that_need_each_other_are_skipped_and_the_run_ends() {
    let env = world(
        "# Plan\n\n- [ ] **1. One**\n  - Needs: 2\n- [ ] **2. Two**\n  - Needs: 1\n- [ ] **3. Three**\n",
    );
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        c.env.mark(FILE, step_of(c.prompt, &["1", "2", "3"]), 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(step(&r, "1").status, RunStepStatus::Skipped);
    assert_eq!(step(&r, "2").status, RunStepStatus::Skipped);
    assert_eq!(step(&r, "3").status, RunStepStatus::Done);
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
    // The Inbox says paused, not stopped (R2e live drive 2026-11-15).
    let note = env
        .inbox()
        .into_iter()
        .find(|i| i.kind == "run")
        .expect("a run note in the Inbox");
    assert!(
        note.title.contains(" paused · 0/2 steps done"),
        "{}",
        note.title
    );
}

/// A step whose work is about a rate limit (a tool result quoting the plan, the agent's reply) is no usage limit: the
/// run goes on at once (once it waited an hour for nothing and looked stuck).
#[test]
fn a_step_about_a_rate_limit_is_not_the_agents_usage_limit() {
    let env = world("# Plan\n\n- [ ] **1. Rate limit**\n- [ ] **2. Next**\n");
    let started = env.now_ms();
    env.set_agent(|c| {
        let n = if c.env.prompts().len() == 1 { "1" } else { "2" };
        c.env.mark(FILE, n, 'x');
        let work =
            "the REST rate limit (`rest.php:434`) is skipped; the limit is reached by any visitor";
        AgentReply {
            result: Some(format!("Step done. {work}")),
            lines: vec![
                json!({"type": "user", "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": work}]}})
                .to_string(),
            ],
            ..Default::default()
        }
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(env.prompts().len(), 2, "each step once, no try again");
    assert!(
        env.now_ms() - started < 3_600_000,
        "waited {} ms",
        env.now_ms() - started
    );
    assert_eq!(step(&r, "1").attempts.len(), 1);
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
fn the_window_sees_the_step_at_work_as_running_while_its_agent_works() {
    // R2e: the run was told to the window only after a step's attempt, so the window found no running step during
    // it: a model picked for the step and words typed into its chat never reached the runner, and the panel said
    // "try 2" on a first try.
    let env = world("# Plan\n\n- [ ] **1. First**\n- [ ] **2. Second**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        let view = sh.run().expect("the run is shown");
        let st = step(&view, n);
        s.lock()
            .unwrap()
            .push((n.to_string(), st.status, st.attempts.len()));
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            ("1".to_string(), RunStepStatus::Running, 0),
            ("2".to_string(), RunStepStatus::Running, 0)
        ]
    );
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
    // The choice is kept in the plan's own data (R2e i), not written into the plan file.
    assert!(!env.text(FILE).contains("Model:"));
    let state = crate::plan::planstate::PlanState::from_json(
        &env.plan_data_get(FILE, crate::plan_data::Kind::State)
            .unwrap(),
    );
    assert_eq!(
        state.kept_model(Some("1"), "Rework").as_deref(),
        Some("claude-sonnet-5-5")
    );
}

#[test]
fn a_step_runs_on_the_model_the_owner_chose_in_qanuvi() {
    let env =
        world("# Plan\n\n- [ ] **1. Rework**\n- [ ] **2. Docs**\n  - Model: claude-haiku-5-5\n");
    let mut state = crate::plan::planstate::PlanState::default();
    state.set_kept_model(Some("1"), "Rework", Some("claude-sonnet-5-5"));
    // A "- Model:" line the owner wrote in the file wins over a kept choice.
    state.set_kept_model(Some("2"), "Docs", Some("claude-opus-5-5"));
    env.plan_data_set(
        FILE,
        crate::plan_data::Kind::State,
        &serde_json::to_value(&state).unwrap(),
    )
    .unwrap();
    env.set_agent(|c| {
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    // No one model for the whole run: each step's own choice counts.
    let r = run(
        &env,
        RunOptions {
            model: None,
            ..options()
        },
    );
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(step(&r, "1").attempts[0].model, "claude-sonnet-5-5");
    assert_eq!(step(&r, "2").attempts[0].model, "claude-haiku-5-5");
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

/// r5: a run held on step 1 and ended without the answer; then the owner answers the question in the Inbox (as the
/// master does it: the answer, then its after-answer work).
fn end_held_then_answer(choice: &str) -> (Arc<FakeEnv>, Arc<std::sync::atomic::AtomicBool>) {
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
    wait_for_question(&first);
    sh.finish_without();
    assert_eq!(first.wait().status, RunStatus::NeedsYou);
    let key = open_question(&env).expect("a question").key.unwrap();
    env.answer(
        &key,
        json!({"choice": choice, "text": "the key is set now"}),
    );
    let item = env.inbox_by_key(&key).unwrap().expect("answered");
    crate::plan::run::on_answer(env.as_ref(), &item);
    (env, blocked)
}

fn wait_for_question(r: &Run) {
    let until = std::time::Instant::now() + Duration::from_secs(3);
    while r.waiting_answers() == 0 {
        assert!(std::time::Instant::now() < until, "the run never waited");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn try_again_on_an_ended_runs_held_step_opens_it_and_the_next_run_goes_on() {
    let (env, blocked) = end_held_then_answer("go");
    assert!(
        env.text(FILE).contains("- [ ] **1. Needs a key**"),
        "{}",
        env.text(FILE)
    );
    blocked.store(false, std::sync::atomic::Ordering::SeqCst);
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

#[test]
fn ending_or_skipping_an_ended_runs_held_step_leaves_it_held() {
    for choice in ["end", "skip"] {
        let (env, _) = end_held_then_answer(choice);
        assert!(
            env.text(FILE).contains("- [!] **1. Needs a key**"),
            "{choice}: {}",
            env.text(FILE)
        );
    }
}

#[test]
fn a_go_ahead_while_the_run_still_waits_leaves_the_plan_to_the_run() {
    let env = world("# Plan\n\n- [ ] **1. Needs a key**\n- [ ] **2. Other**\n");
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.mark(FILE, n, if n == "1" { '!' } else { 'x' });
        AgentReply::default()
    });
    let stop = stop_button();
    let sh = stop.0.clone();
    let live = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop,
    );
    wait_for_question(&live);
    let mut go = open_question(&env).expect("a question");
    go.status = "answered".into();
    go.answer = Some(json!({"choice": "go"}));
    crate::plan::run::on_answer(env.as_ref(), &go);
    assert!(
        env.text(FILE).contains("- [!] **1. Needs a key**"),
        "{}",
        env.text(FILE)
    );
    sh.finish_without();
    live.wait();
}

// ---------- The owner's messages in a step's chat (R2c part 2) ----------

/// A run whose turns have Qanuvi's free models set up (the decider that sorts the owner's messages).
fn with_free(env: &Arc<FakeEnv>, opts: RunOptions, stop: Stop) -> Run {
    let mut setup = test_setup(FILE, opts);
    setup.turns.setups.insert("qanuvi".into(), "{}".into());
    Run::start_setup(env.clone(), setup, stop)
}

/// The free decider sorts every message as `kind` (with a step title for a change).
fn decider_says(env: &FakeEnv, kind: &'static str, title: &'static str) {
    env.set_decide(move |prompt| {
        assert!(prompt.contains("Sort it."), "{prompt}");
        Ok((
            "Free model".into(),
            json!({"kind": kind, "title": title}).to_string(),
        ))
    });
}

fn log_of(env: &FakeEnv, number: &str, title: &str) -> String {
    env.text(&crate::plan::guidance::log_path(FILE, Some(number), title))
}

fn sent_texts(env: &FakeEnv) -> Vec<String> {
    env.sent().into_iter().map(|(_, t)| t).collect()
}

fn messages_decided(r: &PlanRun) -> Vec<(String, String, String)> {
    r.decisions
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.kind == "owner message")
        .map(|d| (d.by, d.decision, d.why))
        .collect()
}

#[test]
fn a_note_typed_in_the_steps_chat_is_sorted_first_then_passed_on_and_the_step_goes_on() {
    let env = world("# Plan\n\n- [ ] **1. Prices**\n");
    decider_says(&env, "note", "");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        sh.owner_message("the price helper is in lib/money.php");
        c.env.mark(FILE, "1", 'x');
        AgentReply {
            ticks: 2,
            ..Default::default()
        }
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    // The agent never got the owner's words raw: it got them as a note, and nothing paused or held the step.
    assert_eq!(
        sent_texts(&env),
        [ownermsg::note_message(
            "the price helper is in lib/money.php"
        )]
    );
    assert_eq!(step(&r, "1").attempts.len(), 1);
    assert!(env.calls().iter().any(|c| c == "free_decide"));
    let log = log_of(&env, "1", "Prices");
    assert!(
        log.contains("Owner message")
            && log.contains(
                "\"the price helper is in lib/money.php\". Taken as a note (by Free model)"
            ),
        "{log}"
    );
    assert_eq!(messages_decided(&r).len(), 1);
    assert_eq!(messages_decided(&r)[0].1, "note");
}

#[test]
fn a_question_is_answered_briefly_in_the_chat_and_without_free_models_the_word_rules_sort_it() {
    let env = world("# Plan\n\n- [ ] **1. Store**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        sh.owner_message("why sqlite and not a JSON file?");
        c.env.mark(FILE, "1", 'x');
        AgentReply {
            ticks: 2,
            ..Default::default()
        }
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(
        sent_texts(&env),
        [ownermsg::question_message(
            "why sqlite and not a JSON file?"
        )]
    );
    assert!(!env.calls().iter().any(|c| c == "free_decide"));
    assert_eq!(messages_decided(&r)[0].0, ownermsg::WORD_RULES);
}

#[test]
fn a_change_with_no_answer_waits_then_becomes_a_new_step_after_this_one_and_runs_next() {
    let env = world("# Plan\n\n- [ ] **1. Export**\n- [ ] **2. Later**\n");
    decider_says(&env, "change", "CSV export");
    let stop = stop_button();
    let sh = stop.0.clone();
    let started = env.now_ms();
    env.set_agent(move |c| {
        if c.prompt
            .starts_with("(Qanuvi: go on with the step as planned")
        {
            // Told to go on once the wait was over: it finishes the step as planned.
            c.env.mark(FILE, "1", 'x');
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "1a", "2"]);
        if n == "1" {
            sh.owner_message("also add a CSV export");
            return AgentReply {
                ticks: 2,
                ..Default::default()
            };
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    // The agent held after the edit in hand; the owner was asked in the Inbox; no answer in 10 minutes.
    let sent = sent_texts(&env);
    assert_eq!(
        sent[0],
        ownermsg::change_hold_message("also add a CSV export")
    );
    assert_eq!(
        sent[1],
        ownermsg::when_message(ownermsg::When::NewStep, "also add a CSV export")
    );
    assert!(env.now_ms() - started >= ownermsg::WAIT_DEFAULT_MS);
    // The change is its own step right after step 1, done next, and the question is gone from the Inbox.
    let plan = env.text(FILE);
    assert!(
        plan.contains("- [x] **1. Export**\n- [x] **1a. CSV export**\n  - Asked by the owner during step 1: \"also add a CSV export\"\n  - Needs: 1\n- [x] **2. Later**"),
        "{plan}"
    );
    let order: Vec<Option<String>> = r.steps.iter().map(|s| s.number.clone()).collect();
    assert_eq!(
        order,
        [Some("1".into()), Some("1a".into()), Some("2".into())]
    );
    assert_eq!(questions(&env), 0);
    let log = log_of(&env, "1", "Export");
    assert!(
        log.contains("no answer in 10 minutes, so added as a new step after this one"),
        "{log}"
    );
    assert!(
        log.contains("The owner's change became step 1a: CSV export."),
        "{log}"
    );
}

/// The decider for a change and then the owner's answer to "when?": the answer is read by the model (any language),
/// which gets the waiting change in its prompt; no words are matched by Qanuvi itself.
fn decider_reads_when(env: &FakeEnv, answer: &'static str, when: &'static str) {
    env.set_decide(move |prompt| {
        assert!(prompt.contains("Sort it."), "{prompt}");
        let reply = if prompt.contains(&format!("Message: \"{answer}\"")) {
            assert!(prompt.contains("Qanuvi is waiting"), "{prompt}");
            json!({"kind": "note", "when": when})
        } else {
            json!({"kind": "change", "title": "JSON export", "when": null})
        };
        Ok(("Free model".into(), reply.to_string()))
    });
}

#[test]
fn a_change_answered_now_in_words_in_the_chat_goes_into_this_step() {
    let env = world("# Plan\n\n- [ ] **1. Export**\n");
    // The owner answers in French: read by meaning, not by English words.
    decider_reads_when(&env, "fais-le maintenant", "now");
    let stop = stop_button();
    let sh = stop.0.clone();
    let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sh2, once) = (sh.clone(), asked.clone());
    // The owner reads the question and answers "now" in the chat while the agent waits.
    env.set_tick(move |e| {
        let open = e
            .inbox()
            .iter()
            .any(|i| i.key.as_deref().is_some_and(|k| k.starts_with("msg:")));
        if open && !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            sh2.owner_message("fais-le maintenant");
        }
    });
    env.set_agent(move |c| {
        if c.turn == 1 {
            sh.owner_message("write the export as JSON instead");
            return AgentReply::default();
        }
        assert!(
            c.prompt.contains("the owner wants the change now"),
            "{}",
            c.prompt
        );
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(asked.load(std::sync::atomic::Ordering::SeqCst));
    // No new step, the question is settled, and the step's log says what the owner chose.
    assert_eq!(r.steps.len(), 1);
    assert!(!env.text(FILE).contains("1a."));
    assert_eq!(questions(&env), 0);
    let decided = messages_decided(&r);
    assert_eq!(decided.len(), 1);
    assert!(
        decided[0].2.ends_with("the owner chose: done in this step"),
        "{:?}",
        decided
    );
}

#[test]
fn a_change_answered_plus_tard_in_the_chat_becomes_a_new_step_without_waiting() {
    let env = world("# Plan\n\n- [ ] **1. Export**\n");
    decider_reads_when(&env, "plus tard", "new");
    let stop = stop_button();
    let sh = stop.0.clone();
    let asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (sh2, once) = (sh.clone(), asked.clone());
    let started = env.now_ms();
    env.set_tick(move |e| {
        let open = e
            .inbox()
            .iter()
            .any(|i| i.key.as_deref().is_some_and(|k| k.starts_with("msg:")));
        if open && !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            sh2.owner_message("plus tard");
        }
    });
    env.set_agent(move |c| {
        if c.prompt
            .starts_with("(Qanuvi: go on with the step as planned")
        {
            c.env.mark(FILE, "1", 'x');
            return AgentReply::default();
        }
        if c.turn == 1 && step_of(c.prompt, &["1", "1a"]) == "1" {
            sh.owner_message("write the export as JSON instead");
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "1a"]);
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(asked.load(std::sync::atomic::Ordering::SeqCst));
    // Taken as "a new step" at once: no 10-minute wait, the change is step 1a, the question is gone.
    assert!(env.now_ms() - started < ownermsg::WAIT_DEFAULT_MS);
    assert!(
        env.text(FILE).contains("**1a. JSON export**"),
        "{}",
        env.text(FILE)
    );
    assert_eq!(questions(&env), 0);
}

#[test]
fn with_no_model_a_change_answer_in_words_is_not_guessed_and_the_safe_choice_is_taken() {
    // No free model: "do it now" is not read by English words; it is sorted as a message of its own, and the waiting
    // question gets the safe choice (a new step) once its wait is over. The owner can answer in the Inbox instead.
    let env = world("# Plan\n\n- [ ] **1. Export**\n- [ ] **2. Later**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        if c.prompt
            .starts_with("(Qanuvi: go on with the step as planned")
        {
            c.env.mark(FILE, "1", 'x');
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "1a", "2"]);
        if n == "1" && c.turn == 1 {
            sh.owner_message("write the export as JSON instead");
            return AgentReply {
                ticks: 2,
                ..Default::default()
            };
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = Run::start(env.clone(), FILE, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert!(!env.calls().iter().any(|c| c == "free_decide"));
    let decided = messages_decided(&r);
    assert_eq!(decided[0].0, ownermsg::WORD_RULES);
    assert_eq!(decided[0].1, "change");
}

#[test]
fn a_change_answered_after_this_step_in_the_inbox_goes_on_in_the_same_session() {
    let env = world("# Plan\n\n- [ ] **1. Export**\n");
    decider_says(&env, "change", "Header row");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_tick(|e| {
        if let Some(q) = e
            .inbox()
            .into_iter()
            .find(|i| i.status == "open" && i.key.as_deref().is_some_and(|k| k.starts_with("msg:")))
        {
            assert!(
                q.title.ends_with("when should your change be done?"),
                "{}",
                q.title
            );
            e.answer(q.key.as_deref().unwrap(), json!({"choice": "after"}));
        }
    });
    env.set_agent(move |c| {
        if c.turn == 1 {
            sh.owner_message("the export also needs a header row");
            return AgentReply::default();
        }
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    let sent = sent_texts(&env);
    assert_eq!(
        sent.last().unwrap(),
        &ownermsg::when_message(ownermsg::When::After, "the export also needs a header row")
    );
    // One session for all of it.
    assert_eq!(env.requests().len(), 1);
    assert_eq!(questions(&env), 0);
    assert!(
        messages_decided(&r)[0]
            .2
            .ends_with("the owner chose: done after this step, in the same session")
    );
}

#[test]
fn stop_typed_in_the_chat_in_any_language_stops_the_run_like_the_button() {
    // Read by meaning on the model ("arrête tout"), never by a list of English stop words.
    let env = world("# Plan\n\n- [ ] **1. Long**\n- [ ] **2. Later**\n");
    decider_says(&env, "stop", "");
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |_| {
        sh.owner_message("arrête tout !");
        AgentReply {
            ticks: 2,
            ..Default::default()
        }
    });
    let r = with_free(&env, options(), stop).wait();
    assert_eq!(r.status, RunStatus::Stopped);
    assert!(env.calls().iter().any(|c| c == "free_decide"));
    assert!(
        sent_texts(&env)
            .iter()
            .any(|t| t == crate::plan::turn::STOP_REQUEST)
    );
    assert_eq!(step(&r, "2").status, RunStepStatus::Waiting);
    assert_eq!(messages_decided(&r)[0].1, "stop");
}

// ---------- Standing owner rules (R2d, 24z2) ----------

#[test]
fn a_rule_said_in_step_1_reaches_step_3_on_another_model_and_a_one_off_note_stays_with_its_step() {
    let env = world(
        "# Plan\n\n- [ ] **1. Prices**\n- [ ] **2. Cart**\n- [ ] **3. Export**\n  - Model: claude-sonnet-5-5\n",
    );
    env.set_decide(|prompt| {
        let kind = if prompt.contains("always follow WordPress core style") {
            "rule"
        } else {
            "note"
        };
        Ok(("Free model".into(), json!({"kind": kind}).to_string()))
    });
    let stop = stop_button();
    let sh = stop.0.clone();
    env.set_agent(move |c| {
        let n = step_of(c.prompt, &["1", "2", "3"]);
        if n == "1" {
            sh.owner_message("always follow WordPress core style");
            sh.owner_message("the price helper is in lib/money.php");
        }
        c.env.mark(FILE, n, 'x');
        AgentReply {
            ticks: if n == "1" { 3 } else { 0 },
            ..Default::default()
        }
    });
    // No one model for the run: each step goes as the plan says (step 3 names its own).
    let opts = RunOptions {
        model: None,
        ..options()
    };
    let r = with_free(&env, opts, stop).wait();
    assert_eq!(r.status, RunStatus::Done);
    // Step 1's agent heard the rule at once, as a rule for the whole plan.
    assert!(
        sent_texts(&env)
            .iter()
            .any(|t| t == &ownermsg::rule_message("always follow WordPress core style"))
    );
    // Kept in the plan's memory and the run's notes.
    let facts = env.text(&crate::plan::guidance::facts_path(FILE));
    assert!(
        facts.contains("Owner rule: always follow WordPress core style"),
        "{facts}"
    );
    assert!(
        !facts.contains("lib/money.php"),
        "a one-off note is no rule: {facts}"
    );
    assert!(
        r.notes
            .clone()
            .unwrap_or_default()
            .iter()
            .any(|n| n.contains("always follow WordPress core style"))
    );
    // Step 3 runs on another model in its own session and still has the rule in its request.
    let (model, prompt) = env
        .prompts()
        .into_iter()
        .rfind(|(_, p)| step_of(p, &["1", "2", "3"]) == "3")
        .expect("step 3 ran");
    assert_eq!(model, "claude-sonnet-5-5");
    assert!(
        prompt.contains("always follow WordPress core style"),
        "{prompt}"
    );
    assert!(!prompt.contains("lib/money.php"), "{prompt}");
    let kinds: Vec<String> = messages_decided(&r).into_iter().map(|m| m.1).collect();
    assert_eq!(kinds, ["rule", "note"]);
    // What the runner said in step 1's chat is kept in its session (the chat shows it when opened again).
    let sid = step(&r, "1").attempts[0].session_id.clone();
    let kept = crate::plan::env::RunEnv::session_lines(&*env, &sid);
    assert!(
        kept.iter()
            .any(|l| l.contains("qanuvi_runner") && l.contains("Taken as a rule")),
        "{kept:?}"
    );
}

/// Step 10 of EMAIL-ROUTER-PLAN (2026-11-17): the session before a handover did the live check and changed files; the
/// next one only wrote the Done line. The review must see the whole step's diff and its log, not only the last
/// session's work, so the step is not sent back to redo everything.
#[test]
fn a_review_after_a_handover_sees_the_whole_steps_work_and_its_log() {
    let env = world("# Plan\n\n- [ ] **10. Live check**\n");
    let stop = stop_button();
    let sh = stop.0.clone();
    let log = crate::plan::guidance::log_path(FILE, Some("10"), "Live check");
    let log2 = log.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        if c.model == "claude-opus-5-5" {
            c.env.put("tinycode.md", "## Email router\nnotes\n");
            c.env.put(
                &log2,
                "Progress: live check done, 7 mails arrived once; full suite passed 133/0.\n",
            );
            sh.controls.switch_model("claude-sonnet-5-5");
            return AgentReply::default();
        }
        c.env.mark(FILE, "10", 'x');
        AgentReply::default()
    });
    let r = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            extra_checks: extra("10", "changes local site state"),
            ..options()
        },
        stop,
    )
    .wait();
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
    assert!(
        asked.prompt.contains("+## Email router"),
        "the first session's change is in the diff"
    );
    assert!(
        asked.prompt.contains("full suite passed 133/0"),
        "the log is read"
    );
    assert!(!asked.prompt.contains("Tests: none were run."));
    assert_eq!(step(&r, "10").attempts.len(), 2, "no fix round");
}

#[test]
fn y24_a_command_the_run_rules_refuse_sets_its_step_aside_with_the_command_named_and_the_run_goes_on()
 {
    let env = world(
        "# Plan\n\n## Run rules\n- Allow: make docs\n\n- [ ] **1. Start the app**\n  - check it opens\n- [ ] **2. Second**\n  - then this\n- [ ] **3. Third**\n  - and this\n",
    );
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        if c.title.unwrap_or("").starts_with("Question for you") {
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "2", "3"]);
        match n {
            // Needed a command nobody allowed, and could not finish without it.
            "1" => c.env.refuse(c.session_id, "cargo run", ""),
            // Refused a never-list command but got the step done another way.
            "3" => {
                c.env.refuse(
                    c.session_id,
                    "git commit -m x",
                    "nothing goes into git during a run",
                );
                c.env.mark(FILE, "3", 'x');
            }
            _ => c.env.mark(FILE, n, 'x'),
        }
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop.clone(),
    );
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while running.waiting_answers() == 0 || step(&running.now(), "3").status != RunStepStatus::Done
    {
        assert!(
            std::time::Instant::now() < until,
            "the run never set step 1 aside: {:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let r = running.now();
    stop.press();
    stop.press();
    let _ = running.wait();
    // Every step turn is answered by the run rules, with the plan's own section.
    let steps: Vec<_> = env
        .requests()
        .into_iter()
        .filter(|q| q.mode != "plan" && q.prompt.contains("Start the app"))
        .collect();
    assert!(!steps.is_empty() && steps.iter().all(|q| q.ruled));
    assert_eq!(
        steps[0].plan_rules.as_ref().unwrap().allow,
        vec!["make docs"]
    );
    // Step 1 set aside once (no retries that cannot get further), named; the others went on. It is marked [!] in the
    // plan, so it is not read as one the owner sent back (no second row for it).
    assert_eq!(r.steps.len(), 3, "{:?}", statuses(&r));
    assert!(env.text(FILE).contains("- [!] **1. Start the app**"));
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Held, "{:?}", statuses(&r));
    assert_eq!(one.attempts.len(), 1);
    let why = &one.held.as_ref().unwrap().why;
    assert!(
        why.contains("`cargo run`") && why.contains("Run rules"),
        "{why}"
    );
    assert_eq!(step(&r, "2").status, RunStepStatus::Done);
    assert_eq!(step(&r, "3").status, RunStepStatus::Done);
    assert!(
        step(&r, "3").attempts[0]
            .note
            .contains("Done without: it needed `git commit -m x`")
    );
    assert!(
        open_question(&env)
            .is_some_and(|q| format!("{} {} {:?}", q.title, q.text, q.form).contains("cargo run"))
    );
}

#[test]
fn y24_part_2_checks_get_fix_rounds_a_failed_step_is_undone_done_steps_are_committed_and_the_report_says_it_all()
 {
    use std::sync::atomic::{AtomicBool, Ordering};
    let env = world(
        "# Plan\n\n- [ ] **1. Add mul**\n  - Add mul to src/math.js.\n  - Check: `npm run lint`\n- [ ] **2. Rework keep**\n  - Change src/keep.php.\n  - Check: `npm run fail`\n- [ ] **3. Third**\n  - and this\n- [ ] **4. Deploy docs**\n  - Check: `make deploy`\n",
    );
    env.put("src/keep.php", "<?php // original\n");
    let mut s = settings("");
    s.test_command = String::new();
    s.run_rules.branch = true;
    env.set_settings(s);
    *env.git.lock().unwrap() = true;
    env.never.lock().unwrap().push("make deploy".into());
    let fixed = Arc::new(AtomicBool::new(false));
    let lint_ok = fixed.clone();
    env.set_checks(move |cmd| match cmd {
        "npm run lint" if lint_ok.load(Ordering::SeqCst) => (true, String::new()),
        "npm run lint" => (false, "src/math.js:1 missing semicolon".into()),
        "npm run fail" => (false, "keep.php: 3 failures".into()),
        _ => (true, String::new()),
    });
    let fixer = fixed.clone();
    env.set_agent(move |c| {
        if planner(c) || c.title.unwrap_or("").starts_with("Question for you") {
            return if planner(c) {
                ok_review(c)
            } else {
                AgentReply::default()
            };
        }
        // A fix round names the failing check, not the step.
        if c.prompt.contains("`npm run lint` failed (fix round") {
            assert!(c.prompt.contains("missing semicolon"), "{}", c.prompt);
            c.env
                .put("src/math.js", "export const mul = (a, b) => a * b;\n");
            fixer.store(true, Ordering::SeqCst);
            return AgentReply::default();
        }
        if c.prompt.contains("`npm run fail` failed (fix round") {
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "2", "3", "4"]);
        match n {
            "1" => {
                assert!(c.prompt.contains("Qanuvi runs its checks (`npm run lint`)"));
                c.env
                    .put("src/math.js", "export const mul = (a, b) => a * b\n");
            }
            "2" => {
                c.env.put("src/keep.php", "<?php // reworked\n");
                c.env.put("src/bad.php", "<?php // half done\n");
            }
            "4" => panic!("step 4 must never start: its check is refused"),
            _ => c.env.put("src/three.txt", "3\n"),
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop.clone(),
    );
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while running.waiting_answers() < 2 || step(&running.now(), "3").status != RunStepStatus::Done {
        assert!(
            std::time::Instant::now() < until,
            "the run never got through: {:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.press();
    stop.press();
    let r = running.wait();

    assert_eq!(r.steps.len(), 4, "{:?}", statuses(&r));
    // Y4: step 1's check failed, one fix round made it pass, done.
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Done, "{:?}", statuses(&r));
    assert_eq!(one.attempts[0].test_runs, Some(2));
    // Y3: step 2's check never passed: undone (its change back, its new file moved out), set aside, the run went on.
    let two = step(&r, "2");
    assert_eq!(two.status, RunStepStatus::Held);
    let why = &two.held.as_ref().unwrap().why;
    assert!(
        why.contains("its checks still fail after every try: `npm run fail`")
            && why.contains("its changes were undone (2 files)"),
        "{why}"
    );
    assert_eq!(env.text("src/keep.php"), "<?php // original\n");
    assert_eq!(env.text("src/bad.php"), "");
    assert_eq!(env.undos.lock().unwrap().len(), 1);
    // The plan file keeps what the run wrote (it is never undone).
    assert!(env.text(FILE).contains("**3. Third**"));
    // Y4: step 4's check is on the never-list: set aside before any work, the command named.
    let four = step(&r, "4");
    assert_eq!(four.status, RunStepStatus::Held);
    assert!(four.attempts.is_empty());
    assert!(
        four.held.as_ref().unwrap().why.contains("`make deploy`"),
        "{:?}",
        four.held
    );
    // Y3: the plan's own branch, a commit per finished step with only its files.
    assert_eq!(r.branch.as_deref(), Some("qanuvi/plan"));
    let commits = env.commits.lock().unwrap().clone();
    assert_eq!(commits.len(), 2, "{commits:?}");
    assert!(commits[0].0.starts_with("Step 1: Add mul"));
    assert!(commits[0].1.contains(&"src/math.js".to_string()));
    assert!(!commits[0].1.contains(&"src/keep.php".to_string()));
    assert!(commits[1].0.starts_with("Step 3: Third"));
    assert_eq!(r.commits.as_ref().map(Vec::len), Some(2));
    // Y5: the report.
    let rep = r.report.as_ref().expect("the run has its report");
    assert_eq!(rep.done, ["Step 1: Add mul", "Step 3: Third"]);
    let waiting: Vec<&str> = rep.waiting.iter().map(|w| w.step.as_str()).collect();
    assert_eq!(
        waiting,
        ["Step 2: Rework keep", "Step 4: Deploy docs"],
        "{:?} {:?}",
        statuses(&r),
        r.steps
            .iter()
            .map(|s| (&s.line, &s.replaces, &s.reason))
            .collect::<Vec<_>>()
    );
    assert!(rep.waiting[0].action.starts_with("Read its log"));
    assert!(rep.waiting[1].action.starts_with("Do that part yourself"));
    assert!(rep.branch.contains("qanuvi/plan (2 commits)"));
}

#[test]
fn y24_part_2_without_git_nothing_is_committed_and_a_failed_step_is_still_undone() {
    let env = world("# Plan\n\n- [ ] **1. Rework**\n  - Check: `npm run fail`\n");
    env.put("src/a.php", "<?php // a\n");
    let mut s = settings("");
    s.test_command = String::new();
    s.run_rules.branch = true;
    env.set_settings(s);
    env.set_checks(|cmd| (cmd != "npm run fail", "fails".into()));
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        if c.title.unwrap_or("").starts_with("Question for you") || c.prompt.contains("fix round") {
            return AgentReply::default();
        }
        c.env.put("src/a.php", "<?php // b\n");
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(
        env.clone(),
        FILE,
        RunOptions {
            replan: false,
            ..options()
        },
        stop.clone(),
    );
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while running.waiting_answers() < 1 {
        assert!(
            std::time::Instant::now() < until,
            "{:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.press();
    stop.press();
    let r = running.wait();
    assert_eq!(r.branch, None);
    assert!(env.commits.lock().unwrap().is_empty());
    assert_eq!(env.text("src/a.php"), "<?php // a\n");
    assert!(
        r.decisions
            .iter()
            .flatten()
            .any(|d| d.subject == "Git branch" && d.decision.starts_with("no branch")),
        "{:?}",
        r.decisions
    );
    // Owner (b): the report says the yes to a branch was not kept, and why.
    let rep = super::run_report(&r);
    assert!(
        rep.branch.starts_with("Not on the plan's own branch: ")
            && rep.branch.contains("Nothing went into git"),
        "{}",
        rep.branch
    );
    assert!(super::report_text(&rep).contains("Not on the plan's own branch"));
}

#[test]
fn pm_a_run_offers_what_it_learned_about_the_project_once_and_keep_all_makes_it_active() {
    let env = world("# Plan\n\n- [ ] **1. Prices**\n");
    env.set_settings(ProjectSettings {
        test_command: String::new(),
        ..settings("")
    });
    let facts = crate::plan::guidance::facts_path(FILE);
    env.put(
        &facts,
        "- Tests: php tests/run.php --strict\n- Step 1 left the cart for step 2\n",
    );
    env.put(
        ".qanuvi/plan-logs/OLD-PLAN/FACTS.md",
        "- Test site: https://shop.example.test/ (start it with site.sh)\n",
    );
    env.set_decide(|prompt| {
        assert!(prompt.contains("Pick out the facts"), "{prompt}");
        assert!(prompt.contains("Step 1 left the cart"), "this plan's notes");
        assert!(
            prompt.contains("shop.example.test"),
            "the first time, every plan's notes"
        );
        Ok((
            "Free model".into(),
            json!({"facts": [
                {"title": "How tests run", "text": "php tests/run.php --strict"},
                {"title": "Test site", "text": "https://shop.example.test/, started with site.sh"}
            ], "testCommand": "php tests/run.php --strict"})
            .to_string(),
        ))
    });
    env.set_agent(|c| {
        let n = if c.env.text(FILE).contains("**1. ") {
            "1"
        } else {
            "2"
        };
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = with_free(&env, options(), stop_button()).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(r.test_hint.as_deref(), Some("php tests/run.php --strict"));
    let kept = env.memory_entries();
    assert_eq!(kept.len(), 2);
    assert!(
        kept.iter()
            .all(|e| e.status == "suggested" && e.part == "project")
    );
    assert!(env.settings().memory_read, "the plans' notes are read once");
    let item = env
        .inbox()
        .into_iter()
        .find(|i| i.kind == "knowledge")
        .expect("an Inbox question");
    assert_eq!(item.kind_of, "answer");
    assert!(
        item.title.contains("2 things learned about the project"),
        "{}",
        item.title
    );
    assert!(
        item.text
            .contains("- Test site: https://shop.example.test/")
    );
    // Keep all, answered anywhere: the core makes them active.
    let answered = crate::inbox::InboxItem {
        answer: Some(json!({"choice": "keep"})),
        ..item
    };
    crate::plan::run::on_answer(env.as_ref(), &answered);
    assert!(env.memory_entries().iter().all(|e| e.status == "active"));
    assert_eq!(env.project_memory().len(), 2);

    // A later run: only its own notes, nothing the memory has is offered again.
    env.set_decide(|prompt| {
        assert!(
            !prompt.contains("site.sh)\n"),
            "the old plans are not read again"
        );
        assert!(
            prompt.contains("[fact] How tests run"),
            "the memory is shown"
        );
        Ok((
            "Free model".into(),
            json!({"facts": [{"title": "How tests run", "text": "php tests/run.php --strict"}]})
                .to_string(),
        ))
    });
    env.put(FILE, "# Plan\n\n- [ ] **2. Cart**\n");
    let before = env.inbox().len();
    let r = with_free(&env, options(), stop_button()).wait();
    assert_eq!(r.status, RunStatus::Done);
    assert_eq!(env.memory_entries().len(), 2);
    assert_eq!(env.inbox().len(), before + 1, "only the run's report");
}

#[test]
fn once_the_answer_comes_the_run_no_longer_says_it_waits_for_you() {
    // bg2: "Waiting for you (1 answer)" stayed on the run while it went on with the next steps.
    let env = world("# Plan\n\n- [ ] **1. Needs a key**\n- [ ] **2. Other**\n  - Needs: 1\n");
    let blocked = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let b = blocked.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" && b.swap(false, std::sync::atomic::Ordering::SeqCst) {
            c.env.mark(FILE, "1", '!');
            return AgentReply::default();
        }
        if n == "2" {
            std::thread::sleep(Duration::from_millis(400));
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
    wait_until("the question never came", &|| running.waiting_answers() > 0);
    assert!(
        running
            .now()
            .message
            .starts_with("Waiting for you (1 answer)")
    );
    let key = open_question(&env).expect("a question").key.unwrap();
    env.answer(&key, json!({"choice": "go", "text": "the key is set now"}));
    wait_until("step 2 never started", &|| {
        step(&running.now(), "2").status == RunStepStatus::Running
    });
    let message = running.now().message;
    assert!(!message.starts_with("Waiting for you"), "{message}");
    assert_eq!(running.wait().status, RunStatus::Done);
}

fn qanuvi_recipe() -> crate::test_build::TestBuild {
    crate::test_build::TestBuild {
        build: "cargo build --release --target-dir {out}".into(),
        start: "{out}/release/app --data {data}".into(),
        check: "{out}/release/app status --data {data}".into(),
        min_free_gb: 0,
    }
}

#[test]
fn y8_a_step_that_asks_for_the_test_build_waits_for_memory_gets_a_fix_round_and_is_done_when_it_passes()
 {
    use std::sync::atomic::{AtomicBool, Ordering};
    let env = world(
        "# Plan\n\n- [ ] **1. Add status**\n  - Add the status answer.\n  - Check: test build\n- [ ] **2. Docs**\n  - Write it down.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    s.test_build = qanuvi_recipe();
    env.set_settings(s);
    // 1 GB free at first, then enough: the build waits once.
    *env.free_mb.lock().unwrap() = vec![1024, 8192];
    let fixed = Arc::new(AtomicBool::new(false));
    let built = fixed.clone();
    env.set_checks(move |cmd| match cmd {
        "test build" if built.load(Ordering::SeqCst) => (true, "Check passed".into()),
        "test build" => (
            false,
            "Test build: the build failed\nerror[E0425]: no status".into(),
        ),
        other => panic!("only the test build runs here, not `{other}`"),
    });
    let fixer = fixed.clone();
    env.set_agent(move |c| {
        if planner(c) {
            return ok_review(c);
        }
        if c.prompt.contains("`test build` failed (fix round") {
            assert!(c.prompt.contains("E0425"), "{}", c.prompt);
            fixer.store(true, Ordering::SeqCst);
            return AgentReply::default();
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" {
            assert!(
                c.prompt.contains("Qanuvi test-builds the app itself"),
                "{}",
                c.prompt
            );
        } else {
            assert!(!c.prompt.contains("test-builds the app"));
        }
        c.env.put(&format!("src/{n}.rs"), "// done\n");
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(r.status, RunStatus::Done, "{:?}", statuses(&r));
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Done);
    assert_eq!(one.attempts[0].test_runs, Some(2));
    let builds = env.test_builds.lock().unwrap().clone();
    assert_eq!(builds, vec![qanuvi_recipe(), qanuvi_recipe()]);
    // Step 2 names no test build: none ran for it.
    assert_eq!(step(&r, "2").status, RunStepStatus::Done);
}

#[test]
fn y8_a_test_build_without_a_recipe_sets_the_step_aside_before_any_work_and_the_run_goes_on() {
    let env = world(
        "# Plan\n\n- [ ] **1. Add status**\n  - Check: the test build.\n- [ ] **2. Docs**\n  - Write it down.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    env.set_settings(s);
    env.set_checks(|cmd| panic!("nothing is checked here, not `{cmd}`"));
    env.set_agent(|c| {
        if planner(c) || c.title.unwrap_or("").starts_with("Question for you") {
            return if planner(c) {
                ok_review(c)
            } else {
                AgentReply::default()
            };
        }
        let n = step_of(c.prompt, &["1", "2"]);
        assert_eq!(
            n, "2",
            "step 1 must never start: it has no test build recipe"
        );
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(env.clone(), FILE, options(), stop.clone());
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while running.waiting_answers() < 1 || step(&running.now(), "2").status != RunStepStatus::Done {
        assert!(
            std::time::Instant::now() < until,
            "the run never got through: {:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.press();
    stop.press();
    let r = running.wait();
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Held, "{:?}", statuses(&r));
    assert!(one.attempts.is_empty());
    assert!(
        one.held
            .as_ref()
            .unwrap()
            .why
            .contains("no test build recipe"),
        "{:?}",
        one.held
    );
    assert!(env.test_builds.lock().unwrap().is_empty());
}

#[test]
fn y8_the_plans_own_recipe_wins_and_too_little_memory_sets_the_step_aside() {
    let env = world(
        "# Plan\n\n## Test build\n- Build: `make app OUT={out}`\n- Check: `{out}/app --version`\n- Memory: 6 GB\n\n## Steps\n\n- [ ] **1. Add status**\n  - Check: test build\n- [ ] **2. Docs**\n  - Write it down.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    s.test_build = qanuvi_recipe();
    env.set_settings(s);
    *env.free_mb.lock().unwrap() = vec![5 * 1024];
    env.set_checks(|cmd| panic!("no build may run without the memory, not `{cmd}`"));
    env.set_agent(|c| {
        if planner(c) || c.title.unwrap_or("").starts_with("Question for you") {
            return if planner(c) {
                ok_review(c)
            } else {
                AgentReply::default()
            };
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(env.clone(), FILE, options(), stop.clone());
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while running.waiting_answers() < 1 || step(&running.now(), "2").status != RunStepStatus::Done {
        assert!(
            std::time::Instant::now() < until,
            "the run never got through: {:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.press();
    stop.press();
    let r = running.wait();
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Held, "{:?}", statuses(&r));
    let why = &one.held.as_ref().unwrap().why;
    assert!(
        why.contains("too little free memory")
            && why.contains("5 GB free, the test build needs 6 GB"),
        "{why}"
    );
    // Tried once: another try would wait the same way.
    assert_eq!(one.attempts.len(), 1);
    assert!(env.test_builds.lock().unwrap().is_empty());
}

fn overloaded() -> AgentReply {
    AgentReply {
        result: Some(
            r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#.into(),
        ),
        is_error: true,
        ..Default::default()
    }
}

fn health_waits(r: &PlanRun) -> Vec<String> {
    r.decisions
        .iter()
        .flatten()
        .filter(|d| d.kind == "health")
        .map(|d| format!("{}: {}", d.decision, d.why))
        .collect()
}

#[test]
fn y7_a_provider_outage_is_no_failure_the_run_waits_says_so_once_and_goes_on() {
    let env = world("# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    let started = env.now_ms();
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        // Step 1's first two tries meet the provider down.
        if n == "1" && c.env.prompts().len() <= 2 {
            return overloaded();
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let r = Run::start(env.clone(), FILE, options(), stop.clone()).wait();
    assert_eq!(
        r.status,
        RunStatus::Done,
        "{:?} {}",
        statuses(&r),
        r.message
    );
    // The outages are not tries: step 1 was done by its first counted try, on the same model.
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Done);
    assert_eq!(one.attempts.len(), 1, "{:?}", one.attempts);
    let said: Vec<String> = stop
        .0
        .notices()
        .into_iter()
        .filter(|n| n.contains("not answering"))
        .collect();
    assert_eq!(
        said,
        [
            "The model provider is not answering: the run waits and goes on by itself when it is back."
        ]
    );
    let waits = health_waits(&r);
    assert_eq!(waits.len(), 2, "{waits:?}");
    assert!(
        waits[0].starts_with(
            "waited, then went on: the model provider is not answering (API Error: 529"
        )
    );
    // 1 minute, then 2 (it waits longer the more outages in a row).
    assert!(env.now_ms() - started >= 3 * 60_000);
    assert_eq!(stop.0.waiting_health(), None);
}

#[test]
fn y7_a_proxy_that_does_not_answer_holds_the_try_until_it_is_back() {
    let env = world("# Plan\n\n- [ ] **1. One**\n");
    *env.proxy.lock().unwrap() = Some("http://127.0.0.1:8787".into());
    // Down at the first two looks, then back.
    *env.up.lock().unwrap() = vec![false, false, true];
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        assert!(
            c.env
                .calls()
                .iter()
                .filter(|c| *c == "health_probe")
                .count()
                >= 3,
            "no try before the proxy answers"
        );
        c.env.mark(FILE, "1", 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let r = Run::start(env.clone(), FILE, options(), stop.clone()).wait();
    assert_eq!(
        r.status,
        RunStatus::Done,
        "{:?} {}",
        statuses(&r),
        r.message
    );
    assert_eq!(step(&r, "1").attempts.len(), 1);
    assert!(
        stop.0
            .notices()
            .iter()
            .any(|n| n == "The proxy at 127.0.0.1:8787 is not answering: the run waits and goes on by itself when it is back."),
        "{:?}",
        stop.0.notices()
    );
    assert_eq!(health_waits(&r).len(), 1);
}

#[test]
fn y7_a_provider_that_keeps_failing_for_hours_sets_the_step_aside_and_the_run_goes_on() {
    let env = world("# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    env.set_agent(|c| {
        if planner(c) || c.title.unwrap_or("").starts_with("Question for you") {
            return if planner(c) {
                ok_review(c)
            } else {
                AgentReply::default()
            };
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" {
            return overloaded();
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let stop = stop_button();
    let running = Run::start(env.clone(), FILE, options(), stop.clone());
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while running.waiting_answers() < 1 || step(&running.now(), "2").status != RunStepStatus::Done {
        assert!(
            std::time::Instant::now() < until,
            "the run never got through: {:?}",
            statuses(&running.now())
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    stop.press();
    stop.press();
    let r = running.wait();
    let one = step(&r, "1");
    assert_eq!(one.status, RunStepStatus::Held, "{:?}", statuses(&r));
    let why = &one.held.as_ref().unwrap().why;
    assert!(
        why.contains("the model provider kept failing for 3 hours (API Error: 529"),
        "{why}"
    );
    // Said once for the whole outage.
    assert_eq!(
        stop.0
            .notices()
            .iter()
            .filter(|n| n.contains("not answering"))
            .count(),
        1
    );
}

#[test]
fn y6_checks_that_need_eyes_go_on_one_review_list_and_the_run_never_waits_for_them() {
    let env = world(
        "# Plan\n\n- [ ] **1. Cart page**\n  - Add the cart page.\n  - Check: the page looks right on a phone\n- [ ] **2. Welcome text**\n  - Write it.\n- [ ] **3. Docs**\n  - Write it down.\n  - Add a comparison check: the same inputs give the same results.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    s.preview_url = "http://localhost/shop/".into();
    env.set_settings(s);
    env.set_checks(|cmd| panic!("a check in words is never run, not `{cmd}`"));
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2", "3"]);
        if n == "1" {
            assert!(
                c.prompt.contains("These checks need a person's eyes (the page looks right on a phone)"),
                "{}",
                c.prompt
            );
        }
        assert!(c.prompt.contains("- To look at: <what, and where>"));
        if n == "2" {
            // The agent leaves the wording for the owner instead of stopping.
            let t = c.env.text(FILE).replace(
                "**2. Welcome text**\n  - Write it.",
                "**2. Welcome text**\n  - Write it.\n  - To look at: the wording of the welcome text",
            );
            c.env.put(FILE, &t);
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(
        r.status,
        RunStatus::Done,
        "{:?} {}",
        statuses(&r),
        r.message
    );
    assert!(r.steps.iter().all(|s| s.status == RunStepStatus::Done));
    let rep = r.report.as_ref().unwrap();
    let look: Vec<String> = rep
        .look
        .iter()
        .map(|l| format!("{}: {}", l.step, l.why))
        .collect();
    assert_eq!(
        look,
        [
            "Step 1: Cart page: the page looks right on a phone",
            "Step 2: Welcome text: the wording of the welcome text"
        ]
    );
    // One list in the Inbox for the plan, with both and the Preview.
    let key = crate::plan::review::key("p1", FILE);
    let lists: Vec<_> = env
        .inbox()
        .into_iter()
        .filter(|i| i.key.as_deref() == Some(&key))
        .collect();
    assert_eq!(lists.len(), 1);
    let list = &lists[0];
    assert_eq!(list.kind_of, "answer");
    assert!(
        list.title.ends_with("2 things to look at from PLAN.md"),
        "{}",
        list.title
    );
    assert!(
        list.text
            .contains("• Step 1: Cart page: the page looks right on a phone")
    );
    assert!(list.text.contains("Preview: http://localhost/shop/"));

    // The owner says what needs changing: it becomes a step after the last one looked at.
    let mut answered = list.clone();
    answered.status = "answered".into();
    answered.answer = Some(json!({"choice": "ok", "text": "Make the welcome text shorter"}));
    crate::plan::run::on_answer(&*env, &answered);
    let plan = env.text(FILE);
    let two = plan.find("**2. Welcome text**").unwrap();
    let fix = plan
        .find("Changes after your look")
        .expect("the change is a step now");
    let three = plan.find("**3. Docs**").unwrap();
    assert!(two < fix && fix < three, "{plan}");
    assert!(plan.contains("\"Make the welcome text shorter\""), "{plan}");
    // "All fine" with nothing typed adds nothing.
    let before = env.text(FILE);
    answered.answer = Some(json!({"choice": "ok", "text": ""}));
    crate::plan::run::on_answer(&*env, &answered);
    assert_eq!(env.text(FILE), before);
}

#[test]
fn y6_an_unanswered_list_keeps_what_an_earlier_run_put_there() {
    let env = world("# Plan\n\n- [ ] **1. One**\n  - Check: the logo is sharp\n");
    let mut s = settings("");
    s.test_command = String::new();
    env.set_settings(s);
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    // The first run has step 1 only; step 2 is written after it.
    let one = run(&env, options());
    assert_eq!(one.review.as_ref().map(Vec::len), Some(1));
    let more = env.text(FILE) + "- [ ] **2. Two**\n  - Check: the menu reads well\n";
    env.put(FILE, &more);
    let second = run(&env, options());
    assert_eq!(second.review.as_ref().map(Vec::len), Some(1));
    let key = crate::plan::review::key("p1", FILE);
    let list = env
        .inbox()
        .into_iter()
        .find(|i| i.key.as_deref() == Some(&key))
        .unwrap();
    assert!(list.title.contains("2 things to look at"), "{}", list.title);
    assert!(list.text.contains("the logo is sharp") && list.text.contains("the menu reads well"));
}

#[test]
fn y9_1_a_run_started_without_a_window_gets_the_check_first_and_keeps_what_it_settled() {
    let env = world(
        "# Plan\n\n- [ ] **1. Deploy docs**\n  - Push the docs to the site.\n- [ ] **2. Pick a theme**\n- [ ] **3. Load the test data**\n- [ ] **4. Slugs**\n  - Check: test build\n- [x] **5. Done before**\n",
    );
    let answer = json!({
        "secrets": [{"name": "DB_PASSWORD", "why": "the test database", "steps": ["3"]}],
        "questions": [
            {"step": "2", "question": "Which theme?", "sort": "you", "recommended": "", "because": "only the owner knows"},
            {"step": "4", "question": "Slug separator?", "sort": "decide", "recommended": "dash", "because": "URLs"}
        ],
        "risky": [{"step": "1", "why": "publishes to the site", "sort": "checks", "because": "it reaches the live site"}],
        "setup": [],
        "order": [{"step": "1", "after": ["4"], "why": "docs last"}]
    })
    .to_string();
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let n = asked.clone();
    env.set_agent(move |c| {
        assert!(planner(c), "only the check's planner turn runs here");
        n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(
            c.prompt.contains("Run rules for this project"),
            "{}",
            c.prompt
        );
        assert!(
            c.prompt.contains("\"Check: test build\" means Qanuvi"),
            "{}",
            c.prompt
        );
        assert!(
            !c.prompt.contains("Done before"),
            "done steps are not checked"
        );
        AgentReply {
            result: Some(answer.clone()),
            ..Default::default()
        }
    });
    let setup = crate::plan::run::test_setup(FILE, options());
    let got = crate::plan::headless_check::check_before_run(&*env, &setup).unwrap();
    assert_eq!(got.read, 4);
    let extra: Vec<&str> = got.extra_checks.iter().map(|x| x.step.as_str()).collect();
    assert_eq!(extra, ["1"]);
    let mut held: Vec<&str> = got.wait_before.iter().map(|x| x.step.as_str()).collect();
    held.sort();
    assert_eq!(held, ["2", "3"], "{:?}", got.wait_before);
    assert!(
        got.wait_before
            .iter()
            .any(|w| w.why.contains("DB_PASSWORD"))
    );
    // Kept with the plan as the window keeps it: the decided choice as a note, the steps checked, the order.
    let notes = env
        .plan_data_get(FILE, crate::plan_data::Kind::Notes)
        .unwrap();
    assert!(
        notes
            .to_string()
            .contains("Decided for you for step 4: Slug separator? → dash"),
        "{notes}"
    );
    let state = env
        .plan_data_get(FILE, crate::plan_data::Kind::State)
        .unwrap();
    assert_eq!(state["checked"].as_array().map(Vec::len), Some(4));
    assert!(
        env.text(FILE)
            .contains("**1. Deploy docs**\n  - Push the docs to the site.\n  - Needs: 4\n")
    );
    // Started again with nothing changed: nothing is asked again.
    let again = crate::plan::headless_check::check_before_run(&*env, &setup).unwrap();
    assert_eq!(again.read, 0);
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn y10_work_left_for_later_goes_to_its_step_and_with_no_step_the_run_is_not_finished() {
    let env = world(
        "# Plan\n\n- [ ] **1. Port it**\n  - Port the runner.\n- [ ] **2. Docs**\n  - Write it down.\n- [ ] **3. Wire it**\n  - Wire the turn.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    env.set_settings(s);
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2", "3"]);
        assert!(c.prompt.contains("- Left for later: <what> (step N)"), "{}", c.prompt);
        if n == "1" {
            let t = c.env.text(FILE).replace(
                "  - Port the runner.",
                "  - Port the runner.\n  - Left for later: wire the real agent turn (step 3)\n  - Left for later: a faster index",
            );
            c.env.put(FILE, &t);
        }
        if n == "3" {
            assert!(
                c.prompt.contains("Promised by step 1: wire the real agent turn"),
                "{}",
                c.prompt
            );
            assert!(c.prompt.contains("Earlier steps left work to this step"));
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert!(
        r.steps.iter().all(|s| s.status == RunStepStatus::Done),
        "{:?}",
        statuses(&r)
    );
    assert_eq!(r.status, RunStatus::NeedsYou, "{}", r.message);
    assert!(r.message.contains("a faster index"), "{}", r.message);
    assert!(
        !r.message.contains("wire the real agent turn"),
        "{}",
        r.message
    );
    let rep = r.report.as_ref().unwrap();
    assert!(rep.look.iter().any(|l| l.why.contains("a faster index")));
}

#[test]
fn y10_what_the_check_settled_reaches_its_steps() {
    let env = world(
        "# Plan\n\n- [ ] **1. Tray menu**\n  - Add it.\n  - Check: the tray menu looks right\n- [ ] **2. Remove the old runner**\n  - Remove it.\n",
    );
    let mut s = settings("");
    s.test_command = String::new();
    env.set_settings(s);
    env.plan_data_set(
        FILE,
        Kind::Check,
        &json!({
            "grants": [{"step": "2", "kind": "delete"}],
            "eyes": [{"step": "1", "what": "the tray menu", "do": "run", "how": "the test build and a screenshot"}]
        }),
    )
    .unwrap();
    env.set_agent(|c| {
        if planner(c) {
            return ok_review(c);
        }
        let n = step_of(c.prompt, &["1", "2"]);
        if n == "1" {
            assert!(!c.delete_inside);
            assert!(
                c.prompt.contains("yours to do in this step, decided before the run: the tray menu (how: the test build and a screenshot)"),
                "{}",
                c.prompt
            );
        } else {
            assert!(c.delete_inside, "step 2 has leave to delete");
        }
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
    let r = run(&env, options());
    assert_eq!(
        r.status,
        RunStatus::Done,
        "{:?} {}",
        statuses(&r),
        r.message
    );
    // The tray check was the run's own: nothing on the list to look at.
    assert!(r.report.as_ref().unwrap().look.is_empty());
}

#[test]
fn y10_the_check_settles_grants_eyes_the_end_check_promises_and_tries_what_steps_need() {
    let env = world(
        "# Plan\n\n- [ ] **1. Port it**\n  - Port it; wiring until step 3.\n- [ ] **2. Remove the old runner**\n  - Remove it.\n- [ ] **3. Wire it**\n  - Wire the turn.\n  - Check: the tray menu looks right\n- [ ] **4. Browser check**\n  - Open the page in chromium.\n",
    );
    let answer = json!({
        "secrets": [], "questions": [], "setup": [], "order": [],
        "risky": [{"step": "2", "why": "removes the old runner files", "sort": "checks", "because": "inside the project", "grant": "delete"}],
        "eyes": [{"step": "3", "what": "the tray menu", "do": "run", "how": "the test build"}],
        "endCheck": {"step": "3", "check": "cargo run --bin qanuvi-master -- plan run demo.md", "why": "the real run"},
        "promises": [{"from": "1", "what": "wire the real agent turn", "step": "3"}],
        "needs": [{"kind": "program", "name": "chromium", "steps": ["4"]}, {"kind": "program", "name": "cargo", "steps": ["3"]}]
    })
    .to_string();
    env.set_agent(move |c| {
        assert!(planner(c));
        assert!(
            c.prompt.contains("the files they will change"),
            "{}",
            c.prompt
        );
        assert!(c.prompt.contains("Never-list forecast"));
        assert!(
            c.prompt
                .contains("look for the same: a step that removes files needs leave"),
            "{}",
            c.prompt
        );
        AgentReply {
            result: Some(answer.clone()),
            ..Default::default()
        }
    });
    crate::plan::check_more::keep_lessons(
        &*env,
        &["a step that removes files needs leave".to_string()],
    );
    let setup = crate::plan::run::test_setup(FILE, options());
    let got = crate::plan::headless_check::check_before_run(&*env, &setup).unwrap();
    // d5: chromium is not on this machine: only step 4 holds; cargo is.
    let held: Vec<&str> = got.wait_before.iter().map(|x| x.step.as_str()).collect();
    assert_eq!(held, ["4"], "{:?}", got.wait_before);
    assert!(got.wait_before[0].why.contains("chromium"));
    // d1 + d2: kept for the runner.
    let rules = crate::plan::check_more::CheckRules::load(&*env, FILE);
    assert_eq!(rules.grants_of("2"), ["delete"]);
    assert_eq!(rules.eyes_of("3").len(), 1);
    // d3 + d4: written into step 3.
    let t = env.text(FILE);
    assert!(
        t.contains("**3. Wire it**\n  - Wire the turn.\n  - Check: the tray menu looks right\n  - Check: `cargo run --bin qanuvi-master -- plan run demo.md`\n  - Promised by step 1: wire the real agent turn\n"),
        "{t}"
    );
    let notes = env.plan_data_get(FILE, Kind::Notes).unwrap().to_string();
    assert!(
        notes.contains("Step 3 does its check itself: the tray menu"),
        "{notes}"
    );
}
