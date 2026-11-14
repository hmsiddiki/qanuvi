//! The decider's model picks for a plan, made before a run ("Pick models" in the Plan view), ported from
//! `src/lib/plan/picks.ts`. A run uses a pick's model while it still fits: same agent, same step title. A model the
//! owner set on the step (its Model: line) is never replaced. Storing picks moves to the database in R2.

use super::decider::Worker;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepPick {
    pub title: String,
    pub worker: Worker,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPicks {
    pub at: f64,
    pub provider_id: String,
    pub free_first: bool,
    /// By step number (or line, for a step without one).
    pub picks: BTreeMap<String, StepPick>,
}

/// The saved pick for a step, while it still fits this run; None when the step changed or the run differs.
pub fn pick_for<'a>(
    saved: Option<&'a SavedPicks>,
    number: &str,
    title: &str,
    provider_id: &str,
) -> Option<&'a StepPick> {
    let saved = saved.filter(|s| s.provider_id == provider_id)?;
    if let Some(p) = saved.picks.get(number).filter(|p| p.title == title) {
        return Some(p);
    }
    // A step without a number is kept by its line, and lines move when a run writes into the plan ("Done" lines):
    // the same title, on one pick only, is still that step.
    let mut same = saved.picks.values().filter(|p| p.title == title);
    match (same.next(), same.next()) {
        (Some(p), None) => Some(p),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const O: &str = "claude-opus-5-5";

    fn pick(title: &str) -> StepPick {
        StepPick {
            title: title.into(),
            worker: Worker::Paid,
            model: Some(O.into()),
            why: "x".into(),
        }
    }

    fn saved(picks: &[(&str, StepPick)]) -> SavedPicks {
        SavedPicks {
            at: 1.0,
            provider_id: "claude-code".into(),
            free_first: false,
            picks: picks
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn are_used_only_for_the_same_agent_and_step_title() {
        let s = saved(&[("95a", pick("Erasure"))]);
        let back: SavedPicks = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let model = |p: Option<&StepPick>| p.and_then(|p| p.model.clone());
        assert_eq!(
            model(pick_for(Some(&back), "95a", "Erasure", "claude-code")).as_deref(),
            Some(O)
        );
        assert_eq!(
            pick_for(Some(&back), "95a", "Erasure, changed", "claude-code"),
            None
        );
        assert_eq!(pick_for(Some(&back), "95a", "Erasure", "codex"), None);
        assert_eq!(pick_for(None, "95a", "Erasure", "claude-code"), None);
    }

    #[test]
    fn follow_a_moved_step_by_its_title_while_unique() {
        let s = saved(&[
            ("5", pick("Add tests")),
            ("6", pick("Write docs")),
            ("7", pick("Write docs")),
        ]);
        assert!(pick_for(Some(&s), "9", "Add tests", "claude-code").is_some());
        assert_eq!(pick_for(Some(&s), "9", "Write docs", "claude-code"), None);
        assert_eq!(
            pick_for(Some(&s), "9", "Something else", "claude-code"),
            None
        );
    }
}
