//! Y10: what the check before the run settles beyond its questions, so a long run needs the owner only when it is
//! truly urgent (docs/PLAN-24Y.md, "Y10 in detail").
//!
//! - d1: a step the check found deleting files inside the project, let go ahead, is given leave to (that step only).
//! - d2: checks that need eyes or a live drive: the run does it in the step, the owner looks at the end, or skipped.
//! - d3: the real end check goes into the closing step as a Check line.
//! - d4: work a step leaves for later is written into the step that will do it ("- Promised by step 5: ...").
//! - d5: the programs, addresses and secrets the steps need are tried here before the run; what is missing holds
//!   only its steps.
//! - d7: the project's check lessons (holds a later look said the check could have found), told to the next check.
//!
//! Kept with the plan as plan data "check" (the runner reads it per step), and the lessons under "(project)".

use super::env::RunEnv;
use super::preflight::{Eyes, Hold, HoldBy, Preflight, RiskChoice};
use super::tasklist::{Step, StepStatus, add_step_line, parse_plan};
use crate::plan_data::Kind;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// The plan-data file the project's check lessons are kept under.
pub const LESSONS_FILE: &str = "(project)";
/// Lessons kept at most (the newest).
pub const LESSONS_KEPT: usize = 20;

/// What the runner reads per step.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRules {
    /// Steps given leave to delete files inside the project ("delete").
    #[serde(default)]
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub eyes: Vec<Eyes>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub step: String,
    pub kind: String,
}

impl CheckRules {
    pub fn load(env: &dyn RunEnv, file: &str) -> CheckRules {
        env.plan_data_get(file, Kind::Check)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }

    /// The grants of one step ("delete").
    pub fn grants_of(&self, step: &str) -> Vec<String> {
        self.grants
            .iter()
            .filter(|g| g.step == step)
            .map(|g| g.kind.clone())
            .collect()
    }

    pub fn eyes_of(&self, step: &str) -> Vec<&Eyes> {
        self.eyes.iter().filter(|e| e.step == step).collect()
    }
}

/// The step the check named (by number, else by title), when it is in the plan.
fn find_step<'a>(steps: &'a [Step], label: &str) -> Option<&'a Step> {
    let l = label
        .trim()
        .trim_start_matches(['#', ' '])
        .trim_end_matches('.');
    if l.is_empty() {
        return None;
    }
    steps
        .iter()
        .find(|s| s.number.as_deref() == Some(l))
        .or_else(|| steps.iter().find(|s| s.title.eq_ignore_ascii_case(l)))
}

fn open(s: &Step) -> bool {
    !matches!(s.status, StepStatus::Done | StepStatus::Blocked)
}

/// The step key the run uses for a label the check gave (its number, else the label as it is).
fn label_key(steps: &[Step], label: &str) -> String {
    find_step(steps, label)
        .map(|s| super::planstate::step_key(s.number.as_deref(), &s.title))
        .unwrap_or_else(|| label.trim().to_string())
}

/// An address as the probe takes it: a URL as it is, "host:port" as a TCP address.
fn probe_url(name: &str) -> String {
    if name.contains("://") {
        name.to_string()
    } else {
        format!("tcp://{name}")
    }
}

static ENV_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{0,99}$").unwrap());

/// d5: tries what the steps need on this machine (read-only) and answers a hold for each step that cannot run here,
/// with the reason in plain words. `first` is the step a need with no steps named holds.
pub fn probe(env: &dyn RunEnv, found: &Preflight, steps: &[Step], first: &str) -> Vec<Hold> {
    let mut holds: Vec<Hold> = Vec::new();
    let secrets = env.secret_names();
    let mut hold = |labels: &[String], why: String| {
        let mut keys: Vec<String> = labels
            .iter()
            .map(|l| label_key(steps, l))
            .filter(|k| !k.is_empty())
            .collect();
        if keys.is_empty() && !first.is_empty() {
            keys.push(first.to_string());
        }
        for step in keys {
            if !holds.iter().any(|h| h.step == step) {
                holds.push(Hold {
                    step,
                    why: why.clone(),
                    by: HoldBy::Default,
                });
            }
        }
    };
    for n in &found.needs {
        let name = n.name.trim().trim_matches('`');
        match n.kind.as_str() {
            "program" => {
                let program = name.split_whitespace().next().unwrap_or("");
                if !program.is_empty() && !env.has_program(program) {
                    hold(
                        &n.steps,
                        format!(
                            "it needs the program {program}, which is not on this machine; install it, then Try again"
                        ),
                    );
                }
            }
            "address" => {
                if !env.reachable(&probe_url(name)) {
                    hold(
                        &n.steps,
                        format!(
                            "it needs {name}, which does not answer from this machine; start it or check the address, \
                             then Try again"
                        ),
                    );
                }
            }
            "secret" if ENV_NAME.is_match(name) && !secrets.iter().any(|s| s == name) => {
                hold(
                    &n.steps,
                    format!(
                        "it needs {name}; add it in the project's settings under Secrets, then Try again"
                    ),
                );
            }
            _ => {}
        }
    }
    holds
}

/// d5: the test build's own programs, for the steps that ask for one ("Check: test build").
pub fn probe_test_build(
    env: &dyn RunEnv,
    recipe: &crate::test_build::TestBuild,
    steps: &[Step],
) -> Vec<Hold> {
    let asking: Vec<&Step> = steps
        .iter()
        .filter(|s| {
            super::tasklist::step_checks(s)
                .iter()
                .any(|c| crate::test_build::names_test_build(&c.command))
        })
        .collect();
    if asking.is_empty() || !recipe.is_set() {
        return Vec::new();
    }
    let program = recipe.build.split_whitespace().next().unwrap_or("");
    if program.is_empty() || program.contains('{') || env.has_program(program) {
        return Vec::new();
    }
    asking
        .iter()
        .map(|s| Hold {
            step: super::planstate::step_key(s.number.as_deref(), &s.title),
            why: format!(
                "its test build needs the program {program}, which is not on this machine; install it, then Try again"
            ),
            by: HoldBy::Default,
        })
        .collect()
}

/// The real end-to-end check the run rules refuse: its step waits for the owner before the run reaches it, with a
/// plain question. Never a recommended skip: it is not decided after a wait and has no "Let Qanuvi decide".
pub fn end_check_hold(
    env: &dyn RunEnv,
    found: &Preflight,
    steps: &[Step],
    mode: &str,
    plan_rules: Option<&crate::run_rules::RunRules>,
) -> Option<Hold> {
    let ec = found.end_check.as_ref()?;
    let command = ec.check.trim().trim_matches('`');
    if command.is_empty()
        || matches!(
            env.rules_verdict(command, plan_rules, mode),
            crate::run_rules::Verdict::Allow
        )
    {
        return None;
    }
    let s = find_step(steps, &ec.step)
        .filter(|s| open(s))
        .or_else(|| steps.iter().rfind(|s| open(s)))?;
    let why = if ec.why.trim().is_empty() {
        String::new()
    } else {
        format!(" ({})", ec.why.trim().trim_end_matches('.'))
    };
    Some(Hold {
        step: super::planstate::step_key(s.number.as_deref(), &s.title),
        why: format!(
            "{}: the plan's work is only proven when it is used for real, `{command}`{why}, and the run rules do not \
             let a run use that command. Go ahead: the step proves it another way the rules allow, or writes under \
             the step what is still unproven. Skip this step: it is left out and the real check stays yours to do. \
             To let the run use the command itself, add it to the run rules (May also run without asking) first.",
            super::preflight::END_TO_END
        ),
        by: HoldBy::Default,
    })
}

/// Puts the end-to-end hold in: a step already held for something else keeps that reason after it, so the hold
/// still starts as the end-to-end one (an open question alone would be decided after a wait).
pub fn add_end_check_hold(holds: &mut Vec<Hold>, h: Hold) {
    match holds.iter_mut().find(|x| x.step == h.step) {
        Some(x) if x.why.starts_with(super::preflight::END_TO_END) => {}
        Some(x) => {
            x.why = format!("{}. Also: {}", h.why.trim_end_matches('.'), x.why);
            x.by = h.by;
        }
        None => holds.push(h),
    }
}

/// What `apply` did, for the notes and the "Decided for you" list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Plan notes every step reads (eyes the run does, or skipped).
    pub notes: Vec<String>,
    /// One plain line per thing settled, for the run's decisions.
    pub said: Vec<String>,
}

/// Settles the check's d1-d4 items: keeps the grants and eyes for the runner, writes the end check and the promises
/// into the plan. `choices` are the risky steps' choices (index into `found.risky`); `mode` the run's permissions.
pub fn apply(
    env: &dyn RunEnv,
    file: &str,
    found: &Preflight,
    choices: &HashMap<usize, RiskChoice>,
    mode: &str,
    plan_rules: Option<&crate::run_rules::RunRules>,
) -> Applied {
    let mut out = Applied::default();
    let Ok(now) = env.read(file) else {
        return out;
    };
    let plan = parse_plan(&now.text);
    let mut rules = CheckRules::load(env, file);
    // d1: leave to delete inside the project, for the steps let go ahead.
    for (i, r) in found.risky.iter().enumerate() {
        if r.grant != "delete" || choices.get(&i) == Some(&RiskChoice::Wait) {
            continue;
        }
        let step = label_key(&plan.steps, &r.step);
        if !rules
            .grants
            .iter()
            .any(|g| g.step == step && g.kind == "delete")
        {
            rules.grants.push(Grant {
                step: step.clone(),
                kind: "delete".into(),
            });
        }
        out.said.push(format!(
            "Step {step} may delete files inside the project ({}); Qanuvi keeps a copy to undo it",
            r.why
        ));
    }
    // d2: what the run does about checks that need eyes.
    let fresh_steps: Vec<String> = found
        .eyes
        .iter()
        .map(|e| label_key(&plan.steps, &e.step))
        .collect();
    rules.eyes.retain(|e| !fresh_steps.contains(&e.step));
    for e in &found.eyes {
        let step = label_key(&plan.steps, &e.step);
        let e = Eyes {
            step: step.clone(),
            ..e.clone()
        };
        match e.act.as_str() {
            "run" => {
                out.notes.push(format!(
                    "Step {step} does its check itself: {}{}. A \"To look at\" line does not finish it; if it truly \
                     cannot be done, say why under the step.",
                    e.what,
                    if e.how.is_empty() {
                        String::new()
                    } else {
                        format!(" (how: {})", e.how)
                    }
                ));
                out.said
                    .push(format!("Step {step}: the run does \"{}\" itself", e.what));
            }
            "skip" => {
                out.notes.push(format!(
                    "Step {step}: \"{}\" is skipped (decided before the run).",
                    e.what
                ));
                out.said
                    .push(format!("Step {step}: \"{}\" is skipped", e.what));
            }
            _ => out.said.push(format!(
                "Step {step}: \"{}\" goes on the list to look at after the run",
                e.what
            )),
        }
        rules.eyes.push(e);
    }
    if let Ok(v) = serde_json::to_value(&rules) {
        let _ = env.plan_data_set(file, Kind::Check, &v);
    }
    // d3 + d4: lines in the plan.
    let mut text = now.text.clone();
    let last_open = |t: &str| parse_plan(t).steps.into_iter().rfind(open);
    let at = |t: &str, label: &str| {
        let steps = parse_plan(t).steps;
        find_step(&steps, label).filter(|s| open(s)).cloned()
    };
    if let Some(ec) = &found.end_check {
        let command = ec.check.trim().trim_matches('`');
        let allowed = matches!(
            env.rules_verdict(command, plan_rules, mode),
            crate::run_rules::Verdict::Allow
        );
        let target = at(&text, &ec.step).or_else(|| last_open(&text));
        match (allowed, target) {
            (true, Some(s)) => {
                if let Ok(next) = add_step_line(&text, s.line, &format!("- Check: `{command}`")) {
                    if next != text {
                        out.said.push(format!(
                            "The real end check `{command}` was added to step {}",
                            super::planstate::step_key(s.number.as_deref(), &s.title)
                        ));
                    }
                    text = next;
                }
            }
            (false, target) => {
                // Not dropped unseen: the closing step still proves the plan works for real.
                let step = target
                    .map(|s| super::planstate::step_key(s.number.as_deref(), &s.title))
                    .unwrap_or_else(|| ec.step.clone());
                out.notes.push(format!(
                    "Step {step} must prove the plan's work runs for real, end to end, with no stand-ins ({}). The \
                     check before the run proposed `{command}`, which the run rules do not allow; use a way they \
                     allow, or say under the step what is still unproven.",
                    ec.why
                ));
                out.said.push(format!(
                    "The real end check `{command}` was not added (the run rules do not let a run use it); step \
                     {step} waits for your answer before it runs"
                ));
            }
            _ => {}
        }
    }
    for p in &found.promises {
        let target = at(&text, &p.step).or_else(|| last_open(&text));
        let Some(s) = target else { continue };
        let from = p.from.trim();
        let line = if from.is_empty() {
            format!("- Promised: {}", p.what)
        } else {
            format!("- Promised by step {from}: {}", p.what)
        };
        if let Ok(next) = add_step_line(&text, s.line, &line) {
            if next != text {
                out.said.push(format!(
                    "Step {} does what was left for later: {}",
                    super::planstate::step_key(s.number.as_deref(), &s.title),
                    p.what
                ));
            }
            text = next;
        }
    }
    if text != now.text {
        let _ = env.write(file, &text, Some(&now.hash));
    }
    out
}

/// d7: the project's check lessons, one line each.
pub fn lessons(env: &dyn RunEnv) -> Vec<String> {
    match env.plan_data_get(LESSONS_FILE, Kind::Check) {
        Some(v) => v["lessons"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

/// d7: the lessons as a note for the check.
pub fn lessons_fact(lessons: &[String]) -> Option<String> {
    (!lessons.is_empty()).then(|| {
        format!(
            "Earlier runs here were held for things this check could have found first; look for the same: {}",
            lessons.join("; ")
        )
    })
}

/// d7: keeps new lessons (the newest LESSONS_KEPT, no repeats).
pub fn keep_lessons(env: &dyn RunEnv, new: &[String]) {
    let mut all = lessons(env);
    for l in new.iter().map(|l| l.trim()).filter(|l| !l.is_empty()) {
        if !all.iter().any(|a| a.eq_ignore_ascii_case(l)) {
            all.push(l.to_string());
        }
    }
    let over = all.len().saturating_sub(LESSONS_KEPT);
    all.drain(..over);
    let _ = env.plan_data_set(
        LESSONS_FILE,
        Kind::Check,
        &serde_json::json!({ "lessons": all }),
    );
}

/// d7: the question asked once at the end of a run that held or set steps aside.
pub fn lessons_prompt(held: &[(String, String)]) -> String {
    let list: Vec<String> = held
        .iter()
        .map(|(step, why)| format!("- Step {step}: {why}"))
        .collect();
    [
        "A plan run held or set aside these steps while it ran:".to_string(),
        list.join("\n"),
        String::new(),
        "Before every run, a check reads the plan's open steps and the files they change and asks the owner what the \
         run will need. For each step above: could that check have found the reason before the run, from the plan \
         text and the project's files? When yes, write one short lesson for later checks in plain words (what to \
         look for), never naming this plan's steps."
            .to_string(),
        r#"Answer with JSON only: {"lessons":[{"step":"21","could":true,"lesson":"a step that removes old files needs leave to delete inside the project"}]}"#
            .to_string(),
    ]
    .join("\n")
}

pub fn parse_lessons(answer: &str) -> Vec<String> {
    let (Some(a), Some(b)) = (answer.find('{'), answer.rfind('}')) else {
        return Vec::new();
    };
    if b <= a {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&answer[a..=b]) else {
        return Vec::new();
    };
    v["lessons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|l| l["could"].as_bool() == Some(true))
        .filter_map(|l| l["lesson"].as_str())
        .map(|l| super::preflight::clip_reason(l.trim(), 200))
        .filter(|l| !l.is_empty())
        .take(5)
        .collect()
}

/// (a): "- Left for later: <what> (step N)" lines a step's agent wrote: what, and the step named (if any).
pub fn left_for_later(details: &[String]) -> Vec<(String, Option<String>)> {
    static LEFT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)^[-*]?\s*left for later:\s*(.+?)\s*(?:\(\s*step\s+([0-9A-Za-z.]+)\s*\))?\s*\.?$",
        )
        .unwrap()
    });
    details
        .iter()
        .filter_map(|d| LEFT.captures(d.trim()))
        .map(|c| {
            (
                c[1].trim().to_string(),
                c.get(2)
                    .map(|m| m.as_str().trim_end_matches('.').to_string()),
            )
        })
        .filter(|(w, _)| !w.is_empty())
        .collect()
}

/// (a): whether a step has work promised to it by an earlier step.
pub fn has_promises(details: &[String]) -> bool {
    details.iter().any(|d| {
        let t = d
            .trim()
            .trim_start_matches(['-', '*'])
            .trim()
            .to_lowercase();
        t.starts_with("promised by step") || t.starts_with("promised:")
    })
}

/// (a) + (c): the step prompt's part about work left for later and checks the run does itself.
pub fn step_prompt_part(details: &[String], eyes: &[&Eyes]) -> String {
    let mut out = String::from(
        "\n\nIf you leave part of this step's work for a later step or for later (\"until step N\", \"not yet\"), add a \
         line `- Left for later: <what> (step N)` under the step, N being the step that will do it (leave out the \
         step when none will). Qanuvi carries it to that step; the plan does not count as finished while it is open.",
    );
    if has_promises(details) {
        out.push_str(
            "\n\nEarlier steps left work to this step (its \"Promised\" lines): do it in this step. If one still \
             cannot be done, add a `- Left for later:` line for it.",
        );
    }
    let run: Vec<String> = eyes
        .iter()
        .filter(|e| e.act == "run")
        .map(|e| {
            if e.how.is_empty() {
                e.what.clone()
            } else {
                format!("{} (how: {})", e.what, e.how)
            }
        })
        .collect();
    if !run.is_empty() {
        out.push_str(&format!(
            "\n\nThese checks are yours to do in this step, decided before the run: {}. Do them for real (no \
             stand-ins); a \"To look at\" line does not finish them. If one truly cannot be done here, say why under \
             the step.",
            run.join("; ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_end_to_end_hold_leads_a_step_held_for_something_else() {
        let e2e = |s: &str| Hold {
            step: s.into(),
            why: format!("{}: use it for real.", super::super::preflight::END_TO_END),
            by: HoldBy::Default,
        };
        let mut holds = vec![Hold {
            step: "2".into(),
            why: format!("{} (Which port?)", super::super::preflight::OPEN_QUESTION),
            by: HoldBy::Default,
        }];
        add_end_check_hold(&mut holds, e2e("2"));
        add_end_check_hold(&mut holds, e2e("2"));
        add_end_check_hold(&mut holds, e2e("3"));
        assert_eq!(holds.len(), 2);
        assert!(
            holds[0]
                .why
                .starts_with(super::super::preflight::END_TO_END)
        );
        assert!(
            holds[0]
                .why
                .ends_with("Also: a question was left open (Which port?)"),
            "{}",
            holds[0].why
        );
        assert_eq!(holds[0].why.matches("for real").count(), 1);
        assert_eq!(holds[1].step, "3");
    }

    #[test]
    fn reads_left_for_later_lines() {
        let d = vec![
            "Done: ported.".to_string(),
            "- Left for later: wire the real agent turn (step 21)".to_string(),
            "- Left for later: a faster index.".to_string(),
        ];
        assert_eq!(
            left_for_later(&d),
            vec![
                (
                    "wire the real agent turn".to_string(),
                    Some("21".to_string())
                ),
                ("a faster index".to_string(), None),
            ]
        );
        assert!(has_promises(&["- Promised by step 5: wire it".to_string()]));
        assert!(!has_promises(&d));
    }

    #[test]
    fn reads_only_lessons_the_check_could_have_found() {
        let a = r#"ok {"lessons":[{"step":"21","could":true,"lesson":"look for deletes"},{"step":"9","could":false,"lesson":"x"}]}"#;
        assert_eq!(parse_lessons(a), vec!["look for deletes".to_string()]);
        assert!(parse_lessons("none").is_empty());
        assert!(
            lessons_prompt(&[("21".into(), "it deletes".into())]).contains("- Step 21: it deletes")
        );
    }

    #[test]
    fn says_in_the_step_what_the_run_does_itself() {
        let e = Eyes {
            step: "17".into(),
            what: "the tray menu".into(),
            act: "run".into(),
            how: "the test build and a screenshot".into(),
        };
        let p = step_prompt_part(&["- Promised by step 5: x".into()], &[&e]);
        assert!(p.contains("Left for later"));
        assert!(p.contains("Promised"));
        assert!(p.contains("the tray menu (how: the test build and a screenshot)"));
        let skip = Eyes {
            act: "end".into(),
            ..e
        };
        assert!(!step_prompt_part(&[], &[&skip]).contains("yours to do"));
    }
}
