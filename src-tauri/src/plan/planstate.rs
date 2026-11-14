//! What a plan keeps between runs (24u part 2), ported from `src/lib/plan/planstate.ts`: the holds from the check
//! before a run, the owner's answers to held steps, closing steps to run again, and decided-for-you choices. Storing
//! it moves from localStorage to the DB in R2; `from_json` reads a stored copy as the TypeScript's load does.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

use super::preflight::{DecidedForYou, Hold, HoldBy, StepWhy};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptHold {
    /// The step's number, else its title.
    pub step: String,
    /// What the hold is about (the check's risk or question).
    pub why: String,
    pub by: HoldBy,
    pub at: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerChoice {
    Go,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptAnswer {
    pub step: String,
    pub choice: AnswerChoice,
    pub text: String,
    pub at: f64,
}

/// A section's closing step ran while some of its steps were set aside: it runs once more after they are done.
/// `late`: those done since, not yet seen by the closing step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptClosing {
    pub closing: String,
    pub waiting: Vec<String>,
    pub late: Vec<String>,
}

/// A choice Qanuvi made for the owner in the check; `changed`: the owner's later answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptDecided {
    #[serde(flatten)]
    pub decided: DecidedForYou,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<String>,
    pub at: f64,
}

impl KeptDecided {
    /// Whether the owner changed it (an empty answer counts as not, as in the TypeScript).
    fn is_changed(&self) -> bool {
        self.changed.as_deref().is_some_and(|c| !c.is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanState {
    pub holds: Vec<KeptHold>,
    pub answers: Vec<KeptAnswer>,
    pub closings: Vec<KeptClosing>,
    pub decided: Vec<KeptDecided>,
    /// Steps that count as done only after the decider's review and passing tests (24u part 3).
    pub checks: Vec<StepWhy>,
}

impl PlanState {
    /// Nothing kept (the stored copy is removed then).
    pub fn is_empty(&self) -> bool {
        self.holds.is_empty()
            && self.answers.is_empty()
            && self.closings.is_empty()
            && self.decided.is_empty()
            && self.checks.is_empty()
    }

    /// A stored copy, keeping only the entries that can be read (anything else gives an empty state).
    pub fn from_json(v: &Value) -> PlanState {
        fn list<T: serde::de::DeserializeOwned>(
            v: &Value,
            key: &str,
            ok: impl Fn(&Value) -> bool,
        ) -> Vec<T> {
            v.get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|x| ok(x))
                .filter_map(|x| serde_json::from_value(x.clone()).ok())
                .collect()
        }
        let s = |x: &Value, k: &str| x.get(k).is_some_and(Value::is_string);
        let a = |x: &Value, k: &str| x.get(k).is_some_and(Value::is_array);
        PlanState {
            holds: list(v, "holds", |x| s(x, "step")),
            answers: list(v, "answers", |x| s(x, "step")),
            closings: list(v, "closings", |x| {
                s(x, "closing") && a(x, "waiting") && a(x, "late")
            }),
            decided: list(v, "decided", |x| s(x, "step") && s(x, "answer")),
            checks: list(v, "checks", |x| s(x, "step")),
        }
    }
}

/// The hold's line, saying who set it.
pub fn hold_wording(why: &str, by: HoldBy) -> String {
    match by {
        HoldBy::You => format!("you asked to see it first ({why})"),
        HoldBy::Default => format!("Qanuvi's default: {why}"),
    }
}

/// New holds from a check replace the kept ones of the same steps. A step the owner already answered is not held
/// again: the answer stands.
pub fn keep_holds(state: &PlanState, holds: &[Hold], now: f64) -> PlanState {
    let answered: HashSet<&str> = state.answers.iter().map(|a| a.step.as_str()).collect();
    let fresh: Vec<&Hold> = holds
        .iter()
        .filter(|h| !answered.contains(h.step.as_str()))
        .collect();
    let steps: HashSet<&str> = fresh.iter().map(|h| h.step.as_str()).collect();
    let mut out = state.clone();
    out.holds.retain(|h| !steps.contains(h.step.as_str()));
    out.holds.extend(fresh.into_iter().map(|h| KeptHold {
        step: h.step.clone(),
        why: h.why.clone(),
        by: h.by,
        at: now,
    }));
    out
}

/// The owner answered a held step: the answer is kept and the step's hold goes.
pub fn keep_answer(
    state: &PlanState,
    step: &str,
    choice: AnswerChoice,
    text: &str,
    now: f64,
) -> PlanState {
    let mut out = state.clone();
    out.holds.retain(|h| h.step != step);
    out.answers.retain(|a| a.step != step);
    out.answers.push(KeptAnswer {
        step: step.into(),
        choice,
        text: text.into(),
        at: now,
    });
    out
}

/// Drops what belongs to steps no longer open (done, or gone from the plan); a closing record stays while its
/// closing step is in the plan and something is still waiting or done late. `all`: every step in the plan.
pub fn prune_state(state: &PlanState, open: &HashSet<String>, all: &HashSet<String>) -> PlanState {
    PlanState {
        holds: state
            .holds
            .iter()
            .filter(|h| open.contains(&h.step))
            .cloned()
            .collect(),
        answers: state
            .answers
            .iter()
            .filter(|a| open.contains(&a.step))
            .cloned()
            .collect(),
        // Decided choices stay while their step is in the plan (the owner may still want to see or change them).
        decided: state
            .decided
            .iter()
            .filter(|d| all.contains(&d.decided.step))
            .cloned()
            .collect(),
        checks: state
            .checks
            .iter()
            .filter(|c| open.contains(&c.step))
            .cloned()
            .collect(),
        closings: state
            .closings
            .iter()
            .filter(|c| all.contains(&c.closing))
            // A waited-for step done meanwhile (by hand, or outside a run) counts as late; one gone from the plan
            // drops.
            .map(|c| {
                let mut late = c.late.clone();
                late.extend(
                    c.waiting
                        .iter()
                        .filter(|w| all.contains(*w) && !open.contains(*w) && !c.late.contains(w))
                        .cloned(),
                );
                KeptClosing {
                    closing: c.closing.clone(),
                    waiting: c
                        .waiting
                        .iter()
                        .filter(|w| open.contains(*w))
                        .cloned()
                        .collect(),
                    late,
                }
            })
            .filter(|c| !c.waiting.is_empty() || !c.late.is_empty())
            .collect(),
    }
}

/// A closing step finished: what of its section is still set aside is what it waits for now; late ones are seen.
pub fn closing_ran(state: &PlanState, closing: &str, waiting: &[String]) -> PlanState {
    let mut out = state.clone();
    out.closings.retain(|c| c.closing != closing);
    if !waiting.is_empty() {
        out.closings.push(KeptClosing {
            closing: closing.into(),
            waiting: waiting.to_vec(),
            late: vec![],
        });
    }
    out
}

/// A step finished: closing steps that ran without it note it as late.
pub fn mark_late(state: &PlanState, step: &str) -> PlanState {
    let mut out = state.clone();
    for c in &mut out.closings {
        if c.waiting.iter().any(|w| w == step) {
            c.waiting.retain(|w| w != step);
            c.late.retain(|l| l != step);
            c.late.push(step.into());
        }
    }
    out
}

/// The closing steps to run again now: something came in late and nothing they wait for is still pending (once for
/// the whole section, not once per late step). `ending`: the run is about to end, so pending steps no longer count.
pub fn closings_due(
    state: &PlanState,
    pending: impl Fn(&str) -> bool,
    ending: bool,
) -> Vec<KeptClosing> {
    state
        .closings
        .iter()
        .filter(|c| !c.late.is_empty() && (ending || !c.waiting.iter().any(|w| pending(w))))
        .cloned()
        .collect()
}

/// A check's decided-for-you choices and extra-check steps: they replace the kept ones of the same steps. A decision
/// the owner already changed stays changed.
pub fn keep_decided(
    state: &PlanState,
    decided: &[DecidedForYou],
    checks: &[StepWhy],
    cleared: &[String],
    now: f64,
) -> PlanState {
    let touched: HashSet<&str> = decided
        .iter()
        .map(|d| d.step.as_str())
        .chain(checks.iter().map(|c| c.step.as_str()))
        .chain(cleared.iter().map(String::as_str))
        .collect();
    let changed: Vec<&KeptDecided> = state.decided.iter().filter(|d| d.is_changed()).collect();
    let fresh = decided
        .iter()
        .filter(|d| {
            !changed
                .iter()
                .any(|c| c.decided.step == d.step && c.decided.what == d.what)
        })
        .map(|d| KeptDecided {
            decided: d.clone(),
            changed: None,
            at: now,
        });
    let mut out = state.clone();
    out.decided
        .retain(|d| d.is_changed() || !touched.contains(d.decided.step.as_str()));
    out.decided.extend(fresh);
    out.checks.retain(|c| !touched.contains(c.step.as_str()));
    out.checks.extend(checks.iter().cloned());
    out
}

/// The owner changed a decided-for-you choice: kept, so later runs and the panel show their answer.
pub fn change_decided(state: &PlanState, step: &str, what: &str, answer: &str) -> PlanState {
    let mut out = state.clone();
    for d in &mut out.decided {
        if d.decided.step == step && d.decided.what == what {
            d.changed = Some(answer.into());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hold(step: &str, why: &str, by: HoldBy) -> Hold {
        Hold {
            step: step.into(),
            why: why.into(),
            by,
        }
    }
    fn set(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn says_who_set_a_hold() {
        assert_eq!(
            hold_wording("pushes to the server", HoldBy::You),
            "you asked to see it first (pushes to the server)"
        );
        assert_eq!(
            hold_wording("a question was left open (dash?)", HoldBy::Default),
            "Qanuvi's default: a question was left open (dash?)"
        );
    }

    #[test]
    fn replaces_holds_of_the_same_step() {
        let s = keep_holds(
            &PlanState::default(),
            &[hold("1a", "old", HoldBy::Default)],
            1.0,
        );
        let s = keep_holds(
            &s,
            &[
                hold("1a", "new", HoldBy::You),
                hold("2a", "x", HoldBy::Default),
            ],
            2.0,
        );
        let got: Vec<_> = s
            .holds
            .iter()
            .map(|h| format!("{}:{}:{:?}", h.step, h.why, h.by))
            .collect();
        assert_eq!(got, ["1a:new:You", "2a:x:Default"]);
    }

    #[test]
    fn an_answer_removes_the_hold_and_is_not_held_again() {
        let s = keep_holds(
            &PlanState::default(),
            &[hold("1a", "q", HoldBy::Default)],
            1.0,
        );
        let s = keep_answer(&s, "1a", AnswerChoice::Go, "dash", 2.0);
        assert!(s.holds.is_empty());
        assert_eq!(
            s.answers,
            [KeptAnswer {
                step: "1a".into(),
                choice: AnswerChoice::Go,
                text: "dash".into(),
                at: 2.0
            }]
        );
        let s = keep_holds(&s, &[hold("1a", "q", HoldBy::Default)], 3.0);
        assert!(s.holds.is_empty());
        let s = keep_answer(&s, "1a", AnswerChoice::Skip, "", 4.0);
        assert_eq!(s.answers.len(), 1);
        assert_eq!(s.answers[0].choice, AnswerChoice::Skip);
    }

    #[test]
    fn drops_entries_of_steps_no_longer_open() {
        let s = keep_holds(
            &PlanState::default(),
            &[hold("1a", "q", HoldBy::Default)],
            1.0,
        );
        let s = keep_answer(&s, "2a", AnswerChoice::Go, "", 2.0);
        let open = set(&["1a"]);
        let s = prune_state(&s, &open, &open);
        assert_eq!(s.holds.len(), 1);
        assert!(s.answers.is_empty());
    }

    #[test]
    fn runs_a_closing_step_again_once_for_its_section() {
        let s = closing_ran(&PlanState::default(), "1z", &strings(&["1a", "1b"]));
        let s = mark_late(&s, "1a");
        assert_eq!(
            s.closings,
            [KeptClosing {
                closing: "1z".into(),
                waiting: strings(&["1b"]),
                late: strings(&["1a"])
            }]
        );
        assert!(closings_due(&s, |x| x == "1b", false).is_empty());
        assert_eq!(closings_due(&s, |x| x == "1b", true)[0].closing, "1z");
        let s = mark_late(&s, "1b");
        assert_eq!(closings_due(&s, |_| false, false)[0].late, ["1a", "1b"]);
        assert!(closing_ran(&s, "1z", &[]).closings.is_empty());
    }

    #[test]
    fn keeps_a_closing_record_across_runs() {
        let s = closing_ran(&PlanState::default(), "1z", &strings(&["1a", "1b"]));
        let kept = prune_state(&s, &set(&["1b"]), &set(&["1a", "1b", "1z"]));
        assert_eq!(
            kept.closings,
            [KeptClosing {
                closing: "1z".into(),
                waiting: strings(&["1b"]),
                late: strings(&["1a"])
            }]
        );
        assert!(
            prune_state(&s, &set(&[]), &set(&["1a"]))
                .closings
                .is_empty()
        );
    }

    fn d(step: &str, answer: &str) -> DecidedForYou {
        DecidedForYou {
            step: step.into(),
            what: "Separator?".into(),
            answer: answer.into(),
            because: String::new(),
        }
    }

    #[test]
    fn a_later_check_replaces_decisions_of_the_same_steps() {
        let s = keep_decided(
            &PlanState::default(),
            &[d("1a", "dash"), d("1b", "dash")],
            &[StepWhy {
                step: "2a".into(),
                why: "migrates".into(),
            }],
            &[],
            1.0,
        );
        let s = keep_decided(&s, &[d("1a", "underscore")], &[], &strings(&["2a"]), 2.0);
        let got: Vec<_> = s
            .decided
            .iter()
            .map(|x| (x.decided.step.as_str(), x.decided.answer.as_str()))
            .collect();
        assert_eq!(got, [("1b", "dash"), ("1a", "underscore")]);
        assert!(s.checks.is_empty());
    }

    #[test]
    fn an_owners_change_stays() {
        let s = keep_decided(&PlanState::default(), &[d("1a", "dash")], &[], &[], 1.0);
        let s = change_decided(&s, "1a", "Separator?", "dot");
        let s = keep_decided(&s, &[d("1a", "dash")], &[], &[], 2.0);
        assert_eq!(s.decided.len(), 1);
        assert_eq!(s.decided[0].changed.as_deref(), Some("dot"));
    }

    #[test]
    fn keeps_decisions_while_in_the_plan_and_checks_while_open() {
        let s = keep_decided(
            &PlanState::default(),
            &[d("1a", "dash")],
            &[StepWhy {
                step: "1a".into(),
                why: "x".into(),
            }],
            &[],
            1.0,
        );
        let p = prune_state(&s, &set(&[]), &set(&["1a"]));
        assert_eq!(p.decided.len(), 1);
        assert!(p.checks.is_empty());
        assert!(prune_state(&s, &set(&[]), &set(&[])).decided.is_empty());
    }

    #[test]
    fn reads_a_stored_copy() {
        let s = PlanState::from_json(&json!({
            "holds": [{ "step": "1a", "why": "q", "by": "you", "at": 1 }, { "step": 2 }],
            "closings": [{ "closing": "1z", "waiting": [], "late": ["1a"] }, { "closing": "2z", "waiting": [] }],
            "decided": [{ "step": "1a", "what": "w", "answer": "a", "because": "", "at": 1 }, { "step": "1b" }],
        }));
        assert_eq!(s.holds.len(), 1);
        assert_eq!(s.closings.len(), 1);
        assert_eq!(s.decided.len(), 1);
        assert!(PlanState::from_json(&json!(null)).is_empty());
        assert!(!s.is_empty());
    }
}
