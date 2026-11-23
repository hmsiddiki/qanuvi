//! The check before a run when nobody is at a window (y9-1): a run started from a terminal (`qanuvi-master plan run`)
//! or by a master on its own gets the same check the window's Run panel makes, decided the way "Check and start"
//! decides it: the recommended answers are taken, risky steps go ahead with extra checks, and only what truly needs
//! the owner (or a login the project lacks) holds its step, with the question in the Inbox once the run reaches it.
//!
//! What it settles is kept with the plan as the window keeps it (notes, decided-for-you choices, holds, the steps it
//! went through), so a later start, from a window or a terminal, does not ask again about unchanged steps.

use super::env::{MemoryUse, PlannerAsk, RunEnv, ask_planner};
use super::planstate::{KeptChecked, PlanState, hold_wording, keep_decided, keep_holds, step_key};
use super::preflight::{
    self, Known, Preflight, PreflightStep, RiskChoice, decided_note, parse_preflight,
    preflight_prompt, recommended_risk, settle_check,
};
use super::run::RunSetup;
use super::runner::{StepWhy, in_ranges, parse_ranges};
use super::tasklist::{Step, StepStatus, add_order_needs, parse_plan};
use crate::plan_data::Kind;
use crate::run_rules::RunRules;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::LazyLock;

/// What the check gives the run: steps held until the owner answers, and steps that go ahead with extra checks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checked {
    pub wait_before: Vec<StepWhy>,
    pub extra_checks: Vec<StepWhy>,
    /// How many steps it read now (0: all were checked before and are unchanged).
    pub read: usize,
}

/// Lines a run writes under a step (Needs, Progress, Done ...): they do not change what the step asks.
static RUN_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[-*]?\s*(needs|skipped|done|progress|model|blocked|note)\b[^:]{0,40}:")
        .unwrap()
});

/// A fingerprint of a step's text (its title and its author's detail lines), as the window's `stepSig` makes it.
pub fn step_sig(title: &str, details: &[String]) -> String {
    let own: Vec<&str> = details
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !RUN_LINE.is_match(l))
        .collect();
    let text = std::iter::once(title.trim())
        .chain(own)
        .collect::<Vec<_>>()
        .join("\n");
    // djb2 over UTF-16 code units, as JavaScript's charCodeAt reads them.
    let mut h: u32 = 5381;
    let mut len = 0usize;
    for u in text.encode_utf16() {
        h = (h << 5).wrapping_add(h).wrapping_add(u32::from(u));
        len += 1;
    }
    format!("{}-{}", base36(len as u64), base36(u64::from(h)))
}

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The steps a start covers, as the Run panel counts them: open steps from the start line, in the ranges if any.
pub fn covered(
    steps: &[Step],
    from_line: usize,
    ranges: Option<&str>,
    only_one: bool,
) -> Vec<Step> {
    let r = parse_ranges(ranges);
    let all: Vec<Step> = steps
        .iter()
        .filter(|s| {
            !matches!(s.status, StepStatus::Done | StepStatus::Blocked) && s.line >= from_line
        })
        .cloned()
        .collect();
    let mut inr: Vec<Step> = if r.ranges.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|s| s.number.is_some() && in_ranges(s.number.as_deref(), &r.ranges))
            .collect()
    };
    if only_one {
        inr.truncate(1);
    }
    inr.truncate(40);
    inr
}

/// What the run rules say, for the check (a question they answer is decided, not asked): the window's `rulesFacts`.
pub fn rules_facts(r: &RunRules, auto: bool) -> String {
    let mut lines = vec![
        "Run rules for this project (standing answers: a question they answer is decided, not asked):".to_string(),
        "- The run may change files in this project and run its tests, checks and builds.".into(),
        "- It never publishes, deletes outside the project, uses passwords, pushes, or changes the main branch.".into(),
        if r.branch {
            "- The plan's work is kept on its own git branch.".into()
        } else {
            "- Nothing goes into git; Qanuvi keeps its own copies to undo a step.".into()
        },
    ];
    if !r.allow.is_empty() {
        lines.push(format!("- Also allowed: {}", r.allow.join("; ")));
    }
    if !r.never.is_empty() {
        lines.push(format!("- Never: {}", r.never.join("; ")));
    }
    lines.push(if auto {
        "- Permissions are Auto: any other command the agent judges safe may run; nothing needs to be allowed first."
            .into()
    } else {
        "- Any other command is refused while nobody is there: its step is set aside with the command named.".into()
    });
    lines.join("\n")
}

/// y9-3: what "Check: test build" means here, so the check never asks what it is.
pub fn test_build_fact(recipe: &crate::test_build::TestBuild) -> String {
    if recipe.is_set() {
        format!(
            "A step line \"Check: test build\" means Qanuvi itself test-builds the app after the step with the project's \
             recipe (build: {}; start: {}; check: {}), into its own folder and on fresh data, never over the installed \
             app. Nothing needs to be asked about it.",
            or_none(&recipe.build),
            or_none(&recipe.start),
            or_none(&recipe.check)
        )
    } else {
        "A step line \"Check: test build\" means Qanuvi test-builds the app after the step with the project's test build \
         recipe (Project settings, Test build, or the plan's own \"Test build\" section). This project has none yet: a \
         step that asks for one is set aside until it is set."
            .into()
    }
}

fn or_none(s: &str) -> &str {
    if s.trim().is_empty() {
        "none"
    } else {
        s.trim()
    }
}

/// Runs the check for the start's steps that it has not gone through before (or that changed since), decides it as
/// "Check and start" does, keeps what it settled with the plan, and answers what the run gets.
pub fn check_before_run(env: &dyn RunEnv, setup: &RunSetup) -> Result<Checked, String> {
    let file = setup.file.clone();
    let text = env.read(&file)?.text;
    let plan = parse_plan(&text);
    let o = &setup.options;
    let steps = covered(&plan.steps, o.from_line, o.ranges.as_deref(), o.only_one);
    let state = env
        .plan_data_get(&file, Kind::State)
        .map(|v| PlanState::from_json(&v))
        .unwrap_or_default();
    let seen: std::collections::HashSet<(String, String)> = state
        .checked
        .iter()
        .map(|c| (c.step.clone(), c.sig.clone()))
        .collect();
    let fresh: Vec<&Step> = steps
        .iter()
        .filter(|s| {
            !seen.contains(&(
                step_key(s.number.as_deref(), &s.title),
                step_sig(&s.title, &s.details),
            ))
        })
        .collect();
    if fresh.is_empty() {
        return Ok(Checked::default());
    }
    let settings = env.project_settings().unwrap_or_default();
    let plan_rules = crate::run_rules::plan_rules(&text);
    let rules = plan_rules
        .clone()
        .unwrap_or_else(|| settings.run_rules.clone());
    let recipe = crate::test_build::plan_test_build(&text)
        .filter(crate::test_build::TestBuild::is_set)
        .unwrap_or_else(|| settings.test_build.clone());
    let mut notes = notes_of(env, &file);
    let known = Known {
        secrets: env.secret_names(),
        test_command: settings.test_command.clone(),
        preview_url: settings.preview_url.clone(),
        notes: notes
            .iter()
            .cloned()
            .chain([
                rules_facts(&rules, o.mode == "auto"),
                test_build_fact(&recipe),
            ])
            // Y10 d7: what earlier checks here missed.
            .chain(super::check_more::lessons_fact(
                &super::check_more::lessons(env),
            ))
            .collect(),
        memory: env.project_memory(),
    };
    let asked: Vec<PreflightStep> = fresh
        .iter()
        .map(|s| PreflightStep {
            number: s.number.clone(),
            title: s.title.clone(),
            details: s.details.clone(),
        })
        .collect();
    // Y10 d6: the check reads the files the steps change, on the agent's strongest model.
    let model = [
        setup
            .models
            .first()
            .map(|m| m.id.clone())
            .unwrap_or_default(),
        setup.planner_model.clone(),
        o.model.clone().unwrap_or_default(),
        setup.default_model.clone(),
    ]
    .into_iter()
    .find(|m| !m.trim().is_empty())
    .unwrap_or_default();
    let answer = ask_planner(
        env,
        MemoryUse::default(),
        &PlannerAsk {
            prompt: preflight_prompt(&file, &asked, &known),
            model,
            title: "Before the run: what it needs".into(),
            agent: setup.provider.clone(),
            base_url: setup.turns.base_url.clone(),
            effort: None,
            lean: true,
            index_tools: false,
            setup: setup.turns.setups.get(&setup.provider).cloned(),
        },
        &mut |_| {},
    )?;
    let mut found: Preflight = parse_preflight(&answer.text);
    found.secrets.retain(|s| !known.secrets.contains(&s.name));
    // "Check and start" with nothing touched: Qanuvi's recommended answers, except where the owner is truly needed.
    let answers: HashMap<usize, String> = found
        .questions
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let a = if q.sort == preflight::AskSort::You {
                String::new()
            } else {
                q.recommended.clone()
            };
            (i, a)
        })
        .collect();
    let risk: HashMap<usize, RiskChoice> = found
        .risky
        .iter()
        .enumerate()
        .map(|(i, x)| (i, recommended_risk(x.sort)))
        .collect();
    let mut settled = settle_check(&found, &answers, &HashMap::new(), &risk, &HashMap::new());
    // A login the project lacks holds the first step that needs it; its question comes when the run reaches it.
    for s in &found.secrets {
        let step = s
            .steps
            .first()
            .cloned()
            .or_else(|| {
                fresh
                    .first()
                    .map(|f| step_key(f.number.as_deref(), &f.title))
            })
            .unwrap_or_default();
        if !step.is_empty() {
            settled.holds.push(preflight::Hold {
                step,
                why: format!(
                    "it needs {} ({}); add it in the project's settings under Secrets, then Try again",
                    s.name, s.why
                ),
                by: preflight::HoldBy::Default,
            });
        }
    }
    // Y10 d5: what the steps need is tried here first; what is missing holds only its steps.
    let first = fresh
        .first()
        .map(|f| step_key(f.number.as_deref(), &f.title))
        .unwrap_or_default();
    let probed: Vec<preflight::Hold> = super::check_more::probe(env, &found, &steps, &first)
        .into_iter()
        .chain(super::check_more::probe_test_build(env, &recipe, &steps))
        .collect();
    for h in probed {
        if !settled.holds.iter().any(|x| x.step == h.step) {
            settled.holds.push(h);
        }
    }
    // Y10 d1-d4: grants, eyes, the end check and promises.
    let applied = super::check_more::apply(env, &file, &found, &risk, &o.mode, plan_rules.as_ref());
    notes.extend(applied.notes.iter().cloned());
    // Kept as the window keeps it.
    notes.extend(settled.notes.iter().cloned());
    notes.extend(settled.decided.iter().map(decided_note));
    let _ = env.plan_data_set(&file, Kind::Notes, &json!(notes));
    let now = env.now_ms() as f64;
    let mut state = keep_decided(
        &state,
        &settled.decided,
        &settled.extra_checks,
        &settled.cleared,
        now,
    );
    let cleared: std::collections::HashSet<&str> =
        settled.cleared.iter().map(String::as_str).collect();
    state.holds.retain(|h| {
        !cleared.contains(h.step.as_str()) || settled.holds.iter().any(|n| n.step == h.step)
    });
    state = keep_holds(&state, &settled.holds, now);
    let keys: std::collections::HashSet<String> = fresh
        .iter()
        .map(|s| step_key(s.number.as_deref(), &s.title))
        .collect();
    state.checked.retain(|c| !keys.contains(&c.step));
    state.checked.extend(fresh.iter().map(|s| KeptChecked {
        step: step_key(s.number.as_deref(), &s.title),
        sig: step_sig(&s.title, &s.details),
    }));
    let over = state.checked.len().saturating_sub(500);
    state.checked.drain(..over);
    if let Ok(v) = serde_json::to_value(&state) {
        let _ = env.plan_data_set(&file, Kind::State, &v);
    }
    // The check's order decisions become Needs lines, so the run keeps that order.
    if !found.order.is_empty()
        && let Ok(now) = env.read(&file)
    {
        let order: Vec<(String, Vec<String>)> = found
            .order
            .iter()
            .map(|x| (x.step.clone(), x.after.clone()))
            .collect();
        let (next, added) = add_order_needs(&now.text, &order);
        if !added.is_empty() {
            let _ = env.write(&file, &next, Some(&now.hash));
        }
    }
    Ok(Checked {
        wait_before: settled
            .holds
            .iter()
            .map(|h| StepWhy {
                step: h.step.clone(),
                why: hold_wording(&h.why, h.by),
            })
            .collect(),
        extra_checks: settled
            .extra_checks
            .iter()
            .map(|x| StepWhy {
                step: x.step.clone(),
                why: x.why.clone(),
            })
            .collect(),
        read: fresh.len(),
    })
}

fn notes_of(env: &dyn RunEnv, file: &str) -> Vec<String> {
    match env.plan_data_get(file, Kind::Notes) {
        Some(Value::Array(a)) => a
            .into_iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_a_step_as_the_window_does() {
        // Same as stepSig in src/lib/plan/planstate.ts for these inputs.
        assert_eq!(step_sig("One", &[]), "3-376nwn");
        let a = step_sig(
            "Cart page",
            &[
                "Add it.".into(),
                "- Needs: 2".into(),
                "Progress: half".into(),
            ],
        );
        assert_eq!(a, step_sig("Cart page", &["Add it.".into()]));
        assert_ne!(a, step_sig("Cart page", &["Add it now.".into()]));
    }

    #[test]
    fn says_the_rules_and_the_test_build_for_the_check() {
        let r = RunRules {
            branch: true,
            allow: vec!["cargo".into()],
            never: vec!["ssh".into()],
            ..RunRules::default()
        };
        let f = rules_facts(&r, true);
        assert!(
            f.contains("own git branch")
                && f.contains("Also allowed: cargo")
                && f.contains("Never: ssh")
        );
        assert!(f.ends_with("nothing needs to be allowed first."));
        let t = crate::test_build::TestBuild {
            build: "make".into(),
            check: "make check".into(),
            ..Default::default()
        };
        assert!(test_build_fact(&t).contains("build: make; start: none; check: make check"));
        assert!(test_build_fact(&Default::default()).contains("has none yet"));
    }
}
