use super::*;
use crate::plan::run::{run_plan, test_setup};
use crate::plan::runner::fake::{AgentCall, AgentReply, FakeEnv};
use crate::plan::runner::{RunStatus, RunTeam};
use crate::projects::ProjectSettings;
use std::time::{Duration, Instant};

const FILE: &str = "plans/PLAN.md";

fn options() -> RunOptions {
    RunOptions {
        mode: "auto".into(),
        attempts_per_model: 1,
        lean: true,
        replan: false,
        ranges: Some(String::new()),
        provider_id: Some("claude-code".into()),
        model: Some("claude-opus-5-5".into()),
        stronger_on_fail: Some(false),
        team: Some(RunTeam::Settings),
        carry_on: Some(false),
        ..Default::default()
    }
}

fn world(test_command: &str) -> Arc<FakeEnv> {
    let env = Arc::new(FakeEnv::new());
    env.set_settings(ProjectSettings {
        run_command: String::new(),
        test_command: test_command.into(),
        preview_url: String::new(),
        changes_from: String::new(),
        ..Default::default()
    });
    env.put(
        FILE,
        "# Plan\n\n- [ ] **1. First**\n  - do it\n- [ ] **2. Second**\n  - then this\n",
    );
    env
}

/// Steps write a file and tick themselves; the planner's reviews pass, and `finish` answers the finishing review.
fn agent(env: &FakeEnv, finish: &'static str) {
    env.set_agent(move |c: &AgentCall| {
        if c.mode == "plan" {
            let r = if c.title.is_some_and(|t| t.starts_with("Finishing pass")) {
                finish
            } else {
                r#"{"ok": true, "why": "fine"}"#
            };
            return AgentReply {
                result: Some(r.into()),
                cost: Some(0.01),
                ..Default::default()
            };
        }
        if c.title == Some("Finishing pass") {
            let said = json!({"type": "assistant", "message": {"content": [{"type": "text",
                "text": "Fixed the missing check.\n\nStill open:\n- The docs page"}]}});
            return AgentReply {
                lines: vec![said.to_string()],
                cost: Some(0.05),
                ..Default::default()
            };
        }
        let n = if c.prompt.contains("**1. ") { "1" } else { "2" };
        c.env
            .put(&format!("src/s{n}.php"), &format!("<?php // {n}\n"));
        c.env.mark(FILE, n, 'x');
        AgentReply::default()
    });
}

#[derive(Default)]
struct Heard {
    views: Mutex<Vec<RunView>>,
    events: Mutex<usize>,
}

impl Tell for Heard {
    fn view(&self, view: &RunView) {
        self.views.lock().unwrap().push(view.clone());
    }
    fn agent(&self, _project_id: &str, _session_id: Option<&str>, _event: &AgentEvent) {
        *self.events.lock().unwrap() += 1;
    }
}

fn wait_idle(runs: &Runs, pid: &str) {
    let until = Instant::now() + Duration::from_secs(20);
    while runs.busy(pid) {
        assert!(Instant::now() < until, "the run did not end");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_run_goes_on_its_own_thread_one_at_a_time_and_its_views_reach_the_listener() {
    let env = world("");
    agent(&env, "{}");
    let runs = Arc::new(Runs::default());
    let heard = Arc::new(Heard::default());
    let pid = env.project_id().to_string();
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let e = env.clone();
    runs.start_run(&pid, heard.clone(), move |sh| {
        let _ = wait.recv();
        run_plan(&*e, sh, test_setup(FILE, options()))
    })
    .expect("the run starts");
    assert!(runs.busy(&pid));
    assert!(runs.view(&pid).running);
    // One thing at a time per project.
    let again = runs.start_run(&pid, heard.clone(), |_| Err("not run".into()));
    assert!(again.unwrap_err().contains("plan run is going on"));
    go.send(()).unwrap();
    wait_idle(&runs, &pid);
    let v = runs.view(&pid);
    assert!(!v.running && !v.finishing);
    assert_eq!(v.run.as_ref().map(|r| r.status), Some(RunStatus::Done));
    assert_eq!(v.error, None);
    let views = heard.views.lock().unwrap();
    assert!(
        views.len() >= 3,
        "start, the steps, the end: {}",
        views.len()
    );
    assert!(views.first().unwrap().running);
    assert!(!views.last().unwrap().running);
    assert!(views.iter().any(|v| v.current_model == "claude-opus-5-5"));
    drop(views);
    assert!(*heard.events.lock().unwrap() > 0, "the steps' agent events");
    // Free again: a new start is taken.
    assert!(
        runs.start_run(&pid, heard.clone(), |_| Err("no plan".into()))
            .is_ok()
    );
    wait_idle(&runs, &pid);
    assert_eq!(runs.view(&pid).error.as_deref(), Some("no plan"));
}

#[test]
fn the_light_finishing_pass_runs_the_checks_reviews_the_runs_diff_and_reports() {
    let env = world("php tests/run.php");
    env.set_checks(|_| (false, "FAIL tests/CartTest.php: total is 0".into()));
    agent(
        &env,
        r#"{"ok": false, "findings": ["The cart total is never rounded"], "stillOpen": ["Rounding"]}"#,
    );
    let sh = Shared::default();
    let run = run_plan(&*env, &sh, test_setup(FILE, options())).unwrap();
    assert_eq!(run.status, RunStatus::Done);
    let ran = finish_pass(
        &*env,
        &sh,
        test_setup(FILE, options()),
        run,
        FinishLevel::Light,
    );
    let p = ran.finish.expect("the pass");
    assert_eq!(p.status, FinishStatus::Done);
    assert_eq!(p.level, FinishLevel::Light);
    assert!(!p.checks.as_ref().unwrap().ok);
    assert!(p.findings[0].starts_with("The full checks fail: "));
    assert!(
        p.findings
            .iter()
            .any(|f| f == "The cart total is never rounded")
    );
    assert_eq!(p.still_open, vec!["Rounding".to_string()]);
    assert!(p.cost_usd > 0.0 && p.ended_at.is_some());
    // The review saw the run's own change, not the plan file.
    let review = env
        .requests()
        .into_iter()
        .find(|r| r.title.as_deref() == Some("Finishing pass: review the whole run"))
        .expect("the review was asked")
        .prompt;
    assert!(review.contains("src/s1.php"));
    assert!(
        env.text(&format!("{}/FINISH.md", g::log_dir(FILE)))
            .contains("Rounding")
    );
    let item = env
        .inbox()
        .into_iter()
        .find(|i| i.title.contains("finishing pass"))
        .expect("the Inbox item");
    assert!(item.title.contains("finishing pass done · 1 still open"));
    assert_eq!(sh.run().unwrap().finish.unwrap().status, FinishStatus::Done);
}

#[test]
fn the_full_finishing_pass_fixes_in_a_session_of_its_own_and_reads_its_still_open_list() {
    let env = world("");
    agent(
        &env,
        r#"{"ok": false, "findings": ["A check is missing"], "stillOpen": []}"#,
    );
    let sh = Shared::default();
    let run = run_plan(&*env, &sh, test_setup(FILE, options())).unwrap();
    let ran = finish_pass(
        &*env,
        &sh,
        test_setup(FILE, options()),
        run,
        FinishLevel::Full,
    );
    let p = ran.finish.expect("the pass");
    assert_eq!(p.status, FinishStatus::Done);
    assert!(p.session_id.is_some());
    assert_eq!(p.still_open, vec!["The docs page".to_string()]);
    assert_eq!(
        p.findings,
        vec![
            "Review: A check is missing".to_string(),
            "No test command: the pass could not run the checks itself.".to_string()
        ]
    );
    assert!(
        p.cost_usd >= 0.06 - 1e-9,
        "review and session: {}",
        p.cost_usd
    );
}

#[test]
fn a_stop_before_the_pass_starts_its_review_ends_it_as_stopped() {
    let env = world("php tests/run.php");
    env.set_checks(|_| (true, String::new()));
    agent(&env, r#"{"ok": true, "findings": [], "stillOpen": []}"#);
    let sh = Shared::default();
    let run = run_plan(&*env, &sh, test_setup(FILE, options())).unwrap();
    let stop = Arc::new(Shared::default());
    let s2 = stop.clone();
    // The owner presses Stop while the checks run.
    env.set_checks(move |_| {
        s2.stop();
        (true, String::new())
    });
    let ran = finish_pass(
        &*env,
        &stop,
        test_setup(FILE, options()),
        run,
        FinishLevel::Full,
    );
    let p = ran.finish.unwrap();
    assert_eq!(p.status, FinishStatus::Stopped);
    assert_eq!(p.note.as_deref(), Some("Stopped by you."));
    assert!(
        !env.requests().iter().any(|r| r
            .title
            .as_deref()
            .is_some_and(|t| t.starts_with("Finishing pass"))),
        "no review after the Stop"
    );
}

#[test]
fn a_start_from_the_window_reads_its_camel_case_shape() {
    let start: RunStart = serde_json::from_value(json!({
        "projectName": "Shop",
        "file": "PLAN.md",
        "options": {"mode": "auto", "attemptsPerModel": 1, "lean": true, "budgetUsd": 0, "fromLine": 0,
                    "onlyOne": false, "indexTools": false, "packets": false, "replan": true},
        "provider": "claude-code",
        "providerName": "Claude Code",
        "models": [{"id": "claude-opus-5-5", "label": "Opus 5.5"}],
        "defaultModel": "claude-sonnet-5-5",
        "plannerModel": "claude-opus-5-5",
        "team": {"mode": "no-such-mode"},
        "hasFree": true,
        "rules": {"docs": "qanuvi:qwen3:4b", "code": "", "nonsense": "x"},
        "turns": {"effort": "high", "lean": true, "setups": {"qanuvi": "{}"}, "memory": {"on": true, "budget": 1200},
                  "labels": {"claude-opus-5-5": "Opus 5.5"}},
        "resume": {"sessionId": "s1", "model": "claude-opus-5-5", "base": {"cost": 0.2}, "text": "go on", "isNote": true}
    }))
    .expect("the start reads");
    let s = start.setup();
    assert_eq!(s.project_name, "Shop");
    assert_eq!(s.team, crate::plan::decider::DEFAULT_TEAM);
    assert_eq!(s.rules.len(), 1, "empty and unknown rules are left out");
    assert_eq!(
        s.rules.get(&JobType::Docs).map(String::as_str),
        Some("qanuvi:qwen3:4b")
    );
    assert_eq!(s.turns.provider, "claude-code");
    assert_eq!(s.turns.effort.as_deref(), Some("high"));
    assert!(s.turns.memory.on);
    assert_eq!(s.turns.memory.budget, 1200);
    let r = s.resume.expect("the resume");
    assert_eq!(r.session_id, "s1");
    assert!(r.is_note);
    assert!((r.base.cost - 0.2).abs() < 1e-9);
}

#[test]
fn a_run_record_left_running_when_qanuvi_closed_reads_as_stopped_and_can_be_resumed() {
    // R2e: closing the last window ends the process mid-step; its record stayed "running", so the reopened window
    // showed a step at work that nothing ran and offered Run instead of Resume.
    let env = world("");
    agent(&env, "{}");
    let sh = Shared::new(Duration::from_millis(5));
    let mut run = run_plan(&*env, &sh, test_setup(FILE, options())).expect("the run");
    run.status = RunStatus::Running;
    run.steps[0].status = RunStepStatus::Running;
    run.finish = Some(FinishPass {
        level: FinishLevel::Light,
        status: FinishStatus::Running,
        model: "claude-opus-5-5".into(),
        started_at: 1.0,
        ended_at: None,
        cost_usd: 0.0,
        seconds: 0.0,
        checks: None,
        findings: vec![],
        still_open: vec![],
        session_id: None,
        note: None,
    });
    let record = crate::runs::RunRecord {
        id: run.id.clone(),
        file: FILE.into(),
        status: "running".into(),
        data: serde_json::to_string(&run).unwrap(),
        created_at: 1,
        updated_at: 2,
    };
    let last = last_run(&[record], Some(FILE)).expect("the record reads");
    assert_eq!(last.status, RunStatus::Stopped);
    assert_eq!(last.steps[0].status, RunStepStatus::Waiting);
    let f = last.finish.expect("the pass");
    assert_eq!(f.status, FinishStatus::Stopped);
    assert!(f.note.unwrap().contains("Qanuvi closed"));
}
