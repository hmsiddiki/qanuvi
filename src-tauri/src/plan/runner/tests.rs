//! Ported from `src/lib/plan/runner.test.ts`.

use super::*;
use serde_json::json;

const PLAN: &str = "## Steps\n\n- [x] **1. Already done**\n- [ ] **2. Add eggs** (to the list)\n  - Edit notes.txt.\n  \
- Model: claude-haiku-5-5\n- [!] **3. Waiting for the owner**\n- [-] **4. Sort the list**";

const H: &str = "claude-haiku-5-5";
const S: &str = "claude-sonnet-5-5";
const O: &str = "claude-opus-5-5";
const F: &str = "claude-fable-5-2";

/// Claude Code's models with their prices, strongest first.
pub(crate) fn models() -> Vec<AgentModel> {
    serde_json::from_value(json!([
        { "id": F, "label": "Fable 5.2", "price": [8, 40] },
        { "id": O, "label": "Opus 5.5", "price": [4, 20] },
        { "id": S, "label": "Sonnet 5.5", "price": [2, 10] },
        { "id": H, "label": "Haiku 5.5", "price": [0.1, 0.5] },
    ]))
    .unwrap()
}

fn attempt(model: &str, outcome: Outcome, cost: f64) -> Attempt {
    serde_json::from_value(json!({
        "model": model, "outcome": outcome, "costUsd": cost, "sessionId": "s", "inputTokens": 100,
        "outputTokens": 10, "seconds": 2, "note": ""
    }))
    .unwrap()
}

fn fail(model: &str) -> Attempt {
    attempt(model, Outcome::Open, 0.0)
}

fn step_run(number: &str, status: RunStepStatus, reason: Option<&str>) -> StepRun {
    serde_json::from_value(json!({
        "line": 0, "number": number, "title": number, "attempts": [], "status": status, "reason": reason
    }))
    .unwrap()
}

fn numbers(steps: &[Step]) -> Vec<&str> {
    steps
        .iter()
        .map(|s| s.number.as_deref().unwrap_or(""))
        .collect()
}

#[test]
fn takes_open_steps_from_the_chosen_one() {
    let steps = parse_plan(PLAN).steps;
    assert_eq!(numbers(&steps_to_run(&steps, 0, false, &[])), ["2", "4"]);
    assert_eq!(
        numbers(&steps_to_run(&steps, steps[3].line, false, &[])),
        ["4"]
    );
    assert_eq!(
        numbers(&steps_to_run(&steps, steps[1].line, true, &[])),
        ["2"]
    );
    assert_eq!(start_model(steps[1].model.as_deref(), S), H);
    assert_eq!(start_model(steps[3].model.as_deref(), S), S);
}

#[test]
fn moves_up_the_models_after_failures() {
    let m = models();
    let none: [Attempt; 0] = [];
    assert_eq!(next_model(&m, &none, H, 2).as_deref(), Some(H));
    assert_eq!(next_model(&m, &[fail(H)], H, 2).as_deref(), Some(H));
    assert_eq!(
        next_model(&m, &[fail(H), fail(H)], H, 2).as_deref(),
        Some(S)
    );
    assert_eq!(
        next_model(&m, &[fail(H), fail(H), fail(S), fail(S)], H, 2).as_deref(),
        Some(O)
    );
    assert_eq!(next_model(&m, &[fail(F)], F, 1), None);
    assert_eq!(
        next_model(&m, &[fail("custom-model")], "custom-model", 1),
        None
    );
    assert_eq!(next_model(ladder(&m, Some(false)), &[fail(S)], S, 1), None);
    assert_eq!(
        next_model(ladder(&m, Some(true)), &[fail(S)], S, 1).as_deref(),
        Some(O)
    );
}

#[test]
fn reads_the_steps_status_after_an_attempt() {
    assert_eq!(step_outcome(PLAN, Some("2"), "Add eggs"), StepOutcome::Open);
    let done = PLAN
        .replace("- [ ] **2. Add eggs**", "- [x] **2. Add eggs**")
        .replace("## Steps", "## Steps\n\nIntro line.");
    assert_eq!(
        step_outcome(&done, Some("2"), "Add eggs"),
        StepOutcome::Done
    );
    assert_eq!(
        step_outcome(
            &PLAN.replace("- [ ] **2.", "- [!] **2."),
            Some("2"),
            "Add eggs"
        ),
        StepOutcome::Blocked
    );
    assert_eq!(
        step_outcome("# empty", Some("2"), "Add eggs"),
        StepOutcome::Missing
    );
}

#[test]
fn the_step_prompt_has_the_step_and_the_rules() {
    let step = &parse_plan(PLAN).steps[1];
    let done = [StepName {
        number: Some("1".into()),
        title: "Already done".into(),
    }];
    let p = step_prompt("TASKLIST.md", step, &done);
    assert!(p.contains("**2. Add eggs** (to the list)"));
    assert!(p.contains("- Edit notes.txt."));
    assert!(!p.contains("Model:"));
    assert!(p.contains("- 1. Already done"));
    assert!(p.contains("change its line in TASKLIST.md to \"- [x]\""));
}

#[test]
fn adds_up_totals_with_the_decider() {
    let mut a = step_run("1", RunStepStatus::Done, None);
    a.attempts = vec![
        attempt("h", Outcome::Done, 0.01),
        attempt("s", Outcome::Done, 0.05),
    ];
    let mut b = step_run("2", RunStepStatus::Done, None);
    b.attempts = vec![attempt("h", Outcome::Done, 0.02)];
    let t = run_totals(&[a, b], &[], None);
    assert!((t.cost_usd - 0.08).abs() < 1e-9);
    assert_eq!(t.input_tokens, 300.0);
    assert_eq!(t.by_model[0].0, "h");
    assert_eq!(t.by_model[0].1.attempts, 2);
    assert!((t.by_model[0].1.cost_usd - 0.03).abs() < 1e-9);
    let calls = [
        DeciderCall {
            model: S.into(),
            cost_usd: 0.01,
        },
        DeciderCall {
            model: S.into(),
            cost_usd: 0.02,
        },
    ];
    let t = run_totals(&[], &calls, None);
    assert!((t.cost_usd - 0.03).abs() < 1e-9);
    assert_eq!(t.by_model, vec![(format!("decider:{S}"), t.by_model[0].1)]);
    assert_eq!(t.by_model[0].1.attempts, 2);
}

#[test]
fn free_models_go_first() {
    let m = models();
    let none: [Attempt; 0] = [];
    assert_eq!(
        next_model_free_first(&m, &none, FREE_MODEL, S, 1).as_deref(),
        Some(FREE_MODEL)
    );
    assert_eq!(
        next_model_free_first(&m, &[fail(FREE_MODEL)], FREE_MODEL, S, 2).as_deref(),
        Some(FREE_MODEL)
    );
    assert_eq!(
        next_model_free_first(&m, &[fail(FREE_MODEL)], FREE_MODEL, S, 1).as_deref(),
        Some(S)
    );
    let tried = [attempt(FREE_MODEL, Outcome::Error, 0.0), fail(S)];
    assert_eq!(
        next_model_free_first(&m, &tried, FREE_MODEL, S, 1).as_deref(),
        Some(O)
    );

    let stats = super::super::routing::Stats::from([
        (
            format!("docs|{S}"),
            serde_json::from_value(json!({"tries": 4, "successes": 4, "costUsd": 0.4})).unwrap(),
        ),
        (
            "docs|qanuvi:auto".into(),
            serde_json::from_value(json!({"tries": 3, "successes": 3, "costUsd": 0})).unwrap(),
        ),
    ]);
    let per = paid_per_step(&stats);
    assert!((per["docs"] - 0.1).abs() < 1e-9);
    let mut docs = step_run("1", RunStepStatus::Done, None);
    docs.job_type = Some(JobType::Docs);
    docs.attempts = vec![attempt(FREE_MODEL, Outcome::Done, 0.0)];
    let mut feature = step_run("2", RunStepStatus::Done, None);
    feature.job_type = Some(JobType::Feature);
    feature.attempts = vec![
        attempt(FREE_MODEL, Outcome::Open, 0.0),
        attempt(S, Outcome::Done, 0.3),
    ];
    let mut steps = vec![docs, feature];
    let s = free_savings(&steps, &per);
    assert_eq!(s.steps, 1);
    assert!((s.estimate_usd.unwrap() - 0.1).abs() < 1e-9);
    // No history for the kind: this run's paid steps are the measure; none at all: unknown.
    steps[0].job_type = Some(JobType::Ui);
    assert!((free_savings(&steps, &per).estimate_usd.unwrap() - 0.3).abs() < 1e-9);
    assert_eq!(
        free_savings(&steps[..1], &HashMap::new()).estimate_usd,
        None
    );
}

#[test]
fn reads_ranges_and_needs() {
    let r = parse_ranges(Some("2-6, 10-15, 20"));
    assert_eq!(
        r,
        Ranges {
            ranges: vec![(2, 6), (10, 15), (20, 20)],
            error: String::new()
        }
    );
    assert_eq!(parse_ranges(Some("6-2")).ranges, vec![(2, 6)]);
    assert!(parse_ranges(Some("2-6, x")).error.contains('x'));
    assert!(parse_ranges(Some("")).ranges.is_empty());

    let plan = parse_plan(
        "- [ ] **1. A**\n- [ ] **2. B**\n- [x] **3. C**\n- [ ] **4. D**\n- [ ] **10b. E**\n- [ ] **F**\n",
    );
    let picked = steps_to_run(&plan.steps, 0, false, &parse_ranges(Some("2-4, 10")).ranges);
    assert_eq!(numbers(&picked), ["2", "4", "10b"]);

    let done: HashSet<String> = ["1", "2"].map(String::from).into();
    let none = HashMap::new();
    let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(unmet_need(&n(&["1", "2"]), &done, &none), None);
    assert_eq!(
        unmet_need(&n(&["1", "3"]), &done, &none).as_deref(),
        Some("needs step 3, which is not done")
    );
    assert!(
        unmet_need(&n(&["docs/B.md#4"]), &done, &none)
            .unwrap()
            .contains("could not be read")
    );
    let others = HashMap::from([("docs/B.md".to_string(), HashSet::from(["4".to_string()]))]);
    assert_eq!(unmet_need(&n(&["docs/B.md#4"]), &done, &others), None);
}

#[test]
fn reports_and_ends_a_run() {
    let steps = [
        step_run("1", RunStepStatus::Done, None),
        step_run("2", RunStepStatus::Blocked, Some("needs a key")),
        step_run(
            "3",
            RunStepStatus::Skipped,
            Some("needs step 2, which is not done"),
        ),
    ];
    assert_eq!(
        run_summary(&steps),
        "1 done · 1 skipped: step 3 (needs step 2, which is not done) · 1 blocked: step 2 (needs a key)"
    );
    let s = |st| step_run("1a", st, None);
    assert_eq!(
        end_status(&[s(RunStepStatus::Done), s(RunStepStatus::Held)]),
        RunStatus::NeedsYou
    );
    assert_eq!(
        end_status(&[s(RunStepStatus::Done), s(RunStepStatus::SetAside)]),
        RunStatus::NeedsYou
    );
    assert_eq!(
        end_status(&[s(RunStepStatus::Done), s(RunStepStatus::Skipped)]),
        RunStatus::Done
    );
}

#[test]
fn run_settings_one_model_and_team() {
    let m = models();
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!((cost_ratio(&m, &ids(&[O, H]), S).unwrap() - 0.976).abs() < 0.005);
    assert_eq!(cost_ratio(&m, &ids(&[S]), F), Some(4.0));
    assert_eq!(cost_ratio(&m, &ids(&["unknown"]), F), None);
    assert_eq!(cost_ratio(&m, &[], F), None);

    use RunTeam::*;
    assert!(!free_first_for(Some(Paid), true, true));
    assert!(free_first_for(Some(Free), false, true));
    assert!(free_first_for(Some(Settings), true, true));
    assert!(!free_first_for(Some(Settings), false, true));
    assert!(!free_first_for(Some(Free), true, false));

    assert!(free_wanted_but_missing(
        Some(Settings),
        TeamMode::PaidDecides,
        false,
        false
    ));
    assert!(free_wanted_but_missing(
        Some(Free),
        TeamMode::Direct,
        false,
        false
    ));
    assert!(!free_wanted_but_missing(
        Some(Settings),
        TeamMode::Direct,
        false,
        false
    ));
    assert!(!free_wanted_but_missing(
        Some(Paid),
        TeamMode::PaidDecides,
        false,
        false
    ));
    assert!(!free_wanted_but_missing(
        Some(Settings),
        TeamMode::PaidDecides,
        true,
        false
    ));
    assert!(!free_wanted_but_missing(
        Some(Settings),
        TeamMode::PaidDecides,
        false,
        true
    ));
}
