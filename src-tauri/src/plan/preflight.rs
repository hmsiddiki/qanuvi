//! The check before a run (24i), ported from `src/lib/plan/preflight.ts`: the paid agent reads the open steps once and
//! lists what the run will need from the owner (logins, decisions, risky steps, setup), so the owner answers once and
//! the run then goes on alone. Its asks are sorted (24u part 3): Qanuvi decides, goes ahead with checks, or waits.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::js;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightStep {
    pub number: Option<String>,
    pub title: String,
    #[serde(default)]
    pub details: Vec<String>,
}

/// How an ask is sorted: "decide": Qanuvi takes the recommended answer and says so; "checks": the step goes ahead,
/// but the decider reviews its diff and the tests must pass first; "you": truly needs the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskSort {
    Decide,
    Checks,
    You,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightSecret {
    pub name: String,
    pub why: String,
    pub steps: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightQuestion {
    pub step: String,
    pub question: String,
    pub sort: AskSort,
    /// The answer the check recommends (empty when it has none).
    pub recommended: String,
    /// Why that answer, or why the owner must answer.
    pub because: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightRisk {
    pub step: String,
    pub why: String,
    pub sort: AskSort,
    pub because: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    pub what: String,
    pub why: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preflight {
    pub secrets: Vec<PreflightSecret>,
    pub questions: Vec<PreflightQuestion>,
    pub risky: Vec<PreflightRisk>,
    pub setup: Vec<Setup>,
}

/// What a risky step does in the run: go ahead, go ahead with extra checks, or wait for the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskChoice {
    Go,
    Checks,
    Wait,
}

/// The choice a risky step starts on, from its sort (holds only where it truly needs the owner).
pub fn recommended_risk(sort: AskSort) -> RiskChoice {
    match sort {
        AskSort::Decide => RiskChoice::Go,
        AskSort::Checks => RiskChoice::Checks,
        AskSort::You => RiskChoice::Wait,
    }
}

/// What is already known when the check runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Known {
    pub secrets: Vec<String>,
    pub test_command: String,
    pub preview_url: String,
    pub notes: Vec<String>,
}

pub fn preflight_prompt(file: &str, steps: &[PreflightStep], known: &Known) -> String {
    let list = steps
        .iter()
        .map(|s| {
            let num = s.number.as_deref().filter(|n| !n.is_empty());
            let mut v = vec![format!(
                "- {}{}",
                num.map(|n| format!("{n}. ")).unwrap_or_default(),
                s.title
            )];
            v.extend(s.details.iter().take(8).map(|d| format!("    {d}")));
            v.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let or = |s: &str, none: &str| {
        if s.is_empty() {
            none.to_string()
        } else {
            s.to_string()
        }
    };
    let mut out: Vec<String> = vec![
        format!("Before an unattended run of {file}, read these open steps (and the code only where you must, read-only: at most"),
        "about ten quick looks; this check should take a minute or two, not a step's work) and list".into(),
        "what the run will need from the owner, so the owner answers once and the run then goes on alone. Change nothing.".into(),
        String::new(),
        "Already known:".into(),
        format!(
            "- Project secrets (environment variables every step gets): {}",
            or(&known.secrets.join(", "), "none")
        ),
        format!("- Test command: {}", or(&known.test_command, "none set")),
        format!("- Preview address: {}", or(&known.preview_url, "none set")),
    ];
    if !known.notes.is_empty() {
        out.push("- The owner's notes for this plan:".into());
        out.extend(known.notes.iter().map(|n| format!("  - {n}")));
    }
    out.extend([
        String::new(),
        "Open steps:".into(),
        list,
        String::new(),
        "Answer with JSON only, in this shape (empty lists when nothing is needed):".into(),
        r#"{"secrets":[{"name":"ENV_NAME","why":"...","steps":["82e"]}],"#.into(),
        r#" "questions":[{"step":"83c","question":"a choice the steps leave open","sort":"decide","recommended":"the answer you advise","because":"..."}],"#.into(),
        r#" "risky":[{"step":"84a","why":"deletes data / migrates / publishes / live payments / pushes","sort":"checks","because":"..."}],"#.into(),
        r#" "setup":[{"what":"something that must run or be set","why":"..."}]}"#.into(),
        "Rules: secrets only for logins, keys and passwords not already known (use the names the code or tests read);".into(),
        "questions only for real choices the steps leave open, not for things you can decide from the code; risky only".into(),
        "for steps that are hard to undo or reach outside this computer. At most 8 items in each list.".into(),
        "Sort every question and risky step (sort):".into(),
        r#"- "decide": a sensible default exists that can be changed later (a setting, not hard-coded); give it as"#.into(),
        "  recommended and say why. Most naming, format, layout and library choices are this.".into(),
        r#"- "checks": it may go ahead if the decider reviews its diff and the tests pass first (migrations, deletions on"#.into(),
        "  this computer or a test database, refactors of shared code).".into(),
        r#"- "you": truly needs the owner: secrets, actions on outside accounts (real payments, publishing, pushing, mail"#.into(),
        "  to real people), or anything that leaves this computer. Still give a recommended answer when you have one.".into(),
        "Judge risk by where the step runs: money on a local or test database is not real money.".into(),
    ]);
    out.join("\n")
}

fn sort_of(v: &Value, fallback: AskSort) -> AskSort {
    match v.as_str() {
        Some("decide") => AskSort::Decide,
        Some("checks") => AskSort::Checks,
        Some("you") => AskSort::You,
        _ => fallback,
    }
}

/// `typeof v === "string" ? v.trim().slice(0, 300) : ""`.
fn text(v: &Value) -> String {
    v.as_str()
        .map(|s| js::cut(js::trim(s), 300).to_string())
        .unwrap_or_default()
}

/// The list's items that are objects (arrays count, as `typeof` says "object"; their fields read as missing).
fn items(v: &Value) -> Vec<&Value> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|x| x.is_object() || x.is_array())
        .collect()
}

/// Reads the agent's answer; anything unreadable gives an empty check (the run is not held up by it).
pub fn parse_preflight(answer: &str) -> Preflight {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{0,99}$").unwrap());
    let (Some(start), Some(end)) = (answer.find('{'), answer.rfind('}')) else {
        return Preflight::default();
    };
    if end <= start {
        return Preflight::default();
    }
    let Ok(raw) = serde_json::from_str::<Value>(&answer[start..=end]) else {
        return Preflight::default();
    };
    let field = |name: &str| raw.get(name).unwrap_or(&Value::Null);
    Preflight {
        secrets: items(field("secrets"))
            .into_iter()
            .map(|s| PreflightSecret {
                name: text(&s["name"]),
                why: text(&s["why"]),
                steps: s["steps"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(text)
                            .filter(|x| !x.is_empty())
                            .take(20)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .filter(|s| NAME.is_match(&s.name))
            .take(8)
            .collect(),
        questions: items(field("questions"))
            .into_iter()
            .map(|q| {
                let recommended = text(&q["recommended"]);
                // Unsorted, a question with no recommended answer needs the owner; one with an answer Qanuvi decides.
                let sort = sort_of(
                    &q["sort"],
                    if recommended.is_empty() {
                        AskSort::You
                    } else {
                        AskSort::Decide
                    },
                );
                PreflightQuestion {
                    step: text(&q["step"]),
                    question: text(&q["question"]),
                    // Nothing to go ahead with: only the owner can answer it.
                    sort: if recommended.is_empty() {
                        AskSort::You
                    } else {
                        sort
                    },
                    recommended,
                    because: text(&q["because"]),
                }
            })
            .filter(|q| !q.question.is_empty())
            .take(8)
            .collect(),
        risky: items(field("risky"))
            .into_iter()
            // Unsorted risky steps wait, as before sorting.
            .map(|r| PreflightRisk {
                step: text(&r["step"]),
                why: text(&r["why"]),
                sort: sort_of(&r["sort"], AskSort::You),
                because: text(&r["because"]),
            })
            .filter(|r| !r.step.is_empty())
            .take(8)
            .collect(),
        setup: items(field("setup"))
            .into_iter()
            .map(|x| Setup {
                what: text(&x["what"]),
                why: text(&x["why"]),
            })
            .filter(|x| !x.what.is_empty())
            .take(8)
            .collect(),
    }
}

/// A choice Qanuvi made for the owner (24u part 3): shown and logged as "Decided for you", changeable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecidedForYou {
    pub step: String,
    /// The question, or the risky step's risk.
    pub what: String,
    pub answer: String,
    pub because: String,
}

/// Who set a hold: the owner chose Wait, or Qanuvi's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HoldBy {
    You,
    Default,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    pub step: String,
    pub why: String,
    pub by: HoldBy,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepWhy {
    pub step: String,
    pub why: String,
}

/// What the owner's answers to the check come to for the run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettledCheck {
    /// Steps the run holds, with who set the hold.
    pub holds: Vec<Hold>,
    /// Steps the check no longer holds (answered, or let go ahead).
    pub cleared: Vec<String>,
    /// The owner's own answers, kept as notes for the plan.
    pub notes: Vec<String>,
    pub decided: Vec<DecidedForYou>,
    /// Steps that go ahead only after the decider's review of the diff and passing tests.
    pub extra_checks: Vec<StepWhy>,
}

/// The plan note for a choice Qanuvi made, so every step and model follows it and keeps it easy to change.
pub fn decided_note(d: &DecidedForYou) -> String {
    let because = if d.because.is_empty() {
        String::new()
    } else {
        format!(", because {}", d.because)
    };
    format!(
        "Decided for you for step {}: {} → {}{because}. The owner may change it later, so keep it in one place (a setting or constant), not hard-coded.",
        d.step, d.what, d.answer
    )
}

/// Sorts the check's items into holds, decided-for-you choices and extra checks. `answers` and `risk` are the form's
/// values by item index; `*_touched` marks the ones the owner changed (an untouched recommended answer is Qanuvi's
/// decision, not theirs).
pub fn settle_check(
    check: &Preflight,
    answers: &HashMap<usize, String>,
    answers_touched: &HashMap<usize, bool>,
    risk: &HashMap<usize, RiskChoice>,
    risk_touched: &HashMap<usize, bool>,
) -> SettledCheck {
    let mut out = SettledCheck::default();
    fn clear(out: &mut SettledCheck, step: &str) {
        if !step.is_empty() && !out.cleared.iter().any(|s| s == step) {
            out.cleared.push(step.to_string());
        }
    }
    let touched = |m: &HashMap<usize, bool>, i: usize| m.get(&i).copied().unwrap_or(false);
    for (i, q) in check.questions.iter().enumerate() {
        let a = answers.get(&i).map(|a| js::trim(a)).unwrap_or("");
        if a.is_empty() {
            if !q.step.is_empty() {
                out.holds.push(Hold {
                    step: q.step.clone(),
                    why: format!("a question was left open ({})", q.question),
                    by: HoldBy::Default,
                });
            }
            continue;
        }
        if a == q.recommended && !touched(answers_touched, i) && q.sort != AskSort::You {
            out.decided.push(DecidedForYou {
                step: q.step.clone(),
                what: q.question.clone(),
                answer: a.to_string(),
                because: q.because.clone(),
            });
        } else {
            out.notes.push(format!(
                "The owner's answer for step {}: {} → {a}",
                q.step, q.question
            ));
        }
        if q.sort == AskSort::Checks && !q.step.is_empty() {
            out.extra_checks.push(StepWhy {
                step: q.step.clone(),
                why: q.question.clone(),
            });
        }
        clear(&mut out, &q.step);
    }
    for (i, x) in check.risky.iter().enumerate() {
        let choice = risk
            .get(&i)
            .copied()
            .unwrap_or_else(|| recommended_risk(x.sort));
        if choice == RiskChoice::Wait {
            out.holds.push(Hold {
                step: x.step.clone(),
                why: x.why.clone(),
                by: if touched(risk_touched, i) {
                    HoldBy::You
                } else {
                    HoldBy::Default
                },
            });
            continue;
        }
        if choice == RiskChoice::Checks {
            out.extra_checks.push(StepWhy {
                step: x.step.clone(),
                why: x.why.clone(),
            });
        }
        if !touched(risk_touched, i) {
            out.decided.push(DecidedForYou {
                step: x.step.clone(),
                what: x.why.clone(),
                answer: if choice == RiskChoice::Checks {
                    "go ahead with extra checks".into()
                } else {
                    "go ahead".into()
                },
                because: x.because.clone(),
            });
        }
        clear(&mut out, &x.step);
    }
    // A step held for one item stays held whatever another item cleared.
    let held: Vec<String> = out.holds.iter().map(|h| h.step.clone()).collect();
    out.cleared.retain(|s| !held.contains(s));
    out
}

/// Whether the run pauses before this step (a risky step to wait at, or a question left open).
pub fn waits_before<'a>(step: Option<&str>, wait: Option<&'a [StepWhy]>) -> Option<&'a str> {
    let step = step.filter(|s| !s.is_empty())?;
    wait?
        .iter()
        .find(|w| w.step == step)
        .map(|w| w.why.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Preflight {
        parse_preflight(&v.to_string())
    }

    #[test]
    fn asks_with_what_is_known_and_the_open_steps() {
        let p = preflight_prompt(
            "PLAN-D.md",
            &[PreflightStep {
                number: Some("82e".into()),
                title: "Short field set".into(),
                details: vec!["- Needs: 82d".into()],
            }],
            &Known {
                secrets: vec!["DB_USER".into()],
                test_command: "php tests/run.php".into(),
                preview_url: String::new(),
                notes: vec!["use the shop currency".into()],
            },
        );
        assert!(p.contains("DB_USER"));
        assert!(p.contains("82e. Short field set"));
        assert!(p.contains("use the shop currency"));
        assert!(p.contains("Change nothing"));
    }

    #[test]
    fn reads_the_answer_and_drops_what_is_not_usable() {
        let r = parse_preflight(concat!(
            r#"Here: {"secrets":[{"name":"QCOMMERCE_TEST_PASSWORD","why":"db tests","steps":["82e"]},{"name":"bad name","why":"x"}],"#,
            r#""questions":[{"step":"83c","question":"Which currency format?"},{"step":"x","question":""}],"#,
            r#""risky":[{"step":"84a","why":"refunds reach the payment provider"}],"setup":[{"what":"MySQL running","why":"tests"}]}"#
        ));
        assert_eq!(
            r.secrets
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["QCOMMERCE_TEST_PASSWORD"]
        );
        assert_eq!(r.questions.len(), 1);
        assert_eq!(r.risky[0].step, "84a");
        assert_eq!(r.setup[0].what, "MySQL running");
        assert_eq!(parse_preflight("no json here"), Preflight::default());
    }

    #[test]
    fn knows_the_steps_to_wait_at() {
        let wait = [StepWhy {
            step: "84a".into(),
            why: "refunds".into(),
        }];
        assert_eq!(waits_before(Some("84a"), Some(&wait)), Some("refunds"));
        assert_eq!(waits_before(Some("84b"), Some(&wait)), None);
        assert_eq!(waits_before(None, Some(&wait)), None);
    }

    #[test]
    fn sorts_asks() {
        let r = parse(json!({
            "questions": [
                { "step": "1a", "question": "Separator?", "sort": "decide", "recommended": "dash", "because": "URLs" },
                { "step": "1b", "question": "Which bank?", "sort": "decide" },
                { "step": "1c", "question": "Format?", "recommended": "ISO" },
            ],
            "risky": [
                { "step": "2a", "why": "migrates", "sort": "checks", "because": "test database" },
                { "step": "2b", "why": "pushes" },
                { "step": "2c", "why": "odd", "sort": "maybe" },
            ],
        }));
        use AskSort::*;
        assert_eq!(
            r.questions.iter().map(|q| q.sort).collect::<Vec<_>>(),
            [Decide, You, Decide]
        );
        assert_eq!(r.questions[0].recommended, "dash");
        assert_eq!(
            r.risky.iter().map(|x| x.sort).collect::<Vec<_>>(),
            [Checks, You, You]
        );
        assert_eq!(
            r.risky
                .iter()
                .map(|x| recommended_risk(x.sort))
                .collect::<Vec<_>>(),
            [RiskChoice::Checks, RiskChoice::Wait, RiskChoice::Wait]
        );
        assert_eq!(recommended_risk(Decide), RiskChoice::Go);
    }

    #[test]
    fn asks_to_sort_and_judge_risk_by_where_the_step_runs() {
        let p = preflight_prompt("P.md", &[], &Known::default());
        assert!(p.contains("\"decide\""));
        assert!(p.contains("not real money"));
    }

    #[test]
    fn settles_the_answers() {
        let check = parse(json!({
            "questions": [
                { "step": "1a", "question": "Separator?", "sort": "decide", "recommended": "dash", "because": "URLs" },
                { "step": "1b", "question": "Bank?", "sort": "you", "recommended": "test bank" },
                { "step": "1c", "question": "Column name?", "sort": "checks", "recommended": "slug" },
                { "step": "1d", "question": "Theme?", "sort": "decide", "recommended": "dark" },
            ],
            "risky": [
                { "step": "2a", "why": "migrates", "sort": "checks", "because": "test database" },
                { "step": "2b", "why": "pushes", "sort": "you" },
                { "step": "2c", "why": "deletes cache", "sort": "decide" },
                { "step": "1a", "why": "renames files", "sort": "you" },
            ],
        }));
        let answers: HashMap<usize, String> = [(0, "dash"), (1, ""), (2, "slug"), (3, "light")]
            .into_iter()
            .map(|(i, a)| (i, a.to_string()))
            .collect();
        use RiskChoice::*;
        let risk: HashMap<usize, RiskChoice> = [(0, Checks), (1, Wait), (2, Go), (3, Wait)].into();
        let r = settle_check(
            &check,
            &answers,
            &[(3, true)].into(),
            &risk,
            &[(3, true)].into(),
        );
        assert_eq!(
            r.decided
                .iter()
                .map(|d| d.step.as_str())
                .collect::<Vec<_>>(),
            ["1a", "1c", "2a", "2c"]
        );
        assert_eq!(
            r.decided.iter().find(|d| d.step == "2a").unwrap().answer,
            "go ahead with extra checks"
        );
        assert_eq!(r.notes, ["The owner's answer for step 1d: Theme? → light"]);
        assert_eq!(
            r.extra_checks
                .iter()
                .map(|c| c.step.as_str())
                .collect::<Vec<_>>(),
            ["1c", "2a"]
        );
        let hold = |step: &str, why: &str, by| Hold {
            step: step.into(),
            why: why.into(),
            by,
        };
        assert_eq!(
            r.holds,
            [
                hold("1b", "a question was left open (Bank?)", HoldBy::Default),
                hold("2b", "pushes", HoldBy::Default),
                hold("1a", "renames files", HoldBy::You),
            ]
        );
        assert_eq!(r.cleared, ["1c", "1d", "2a", "2c"]);
    }

    #[test]
    fn an_accepted_answer_for_a_step_that_needs_the_owner_is_theirs() {
        let check = parse(
            json!({ "questions": [{ "step": "1b", "question": "Bank?", "sort": "you", "recommended": "test bank" }] }),
        );
        let r = settle_check(
            &check,
            &[(0, "test bank".to_string())].into(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert!(r.decided.is_empty());
        assert_eq!(r.notes.len(), 1);
    }

    #[test]
    fn writes_a_decided_choice_as_a_changeable_note() {
        let n = decided_note(&DecidedForYou {
            step: "1a".into(),
            what: "Separator?".into(),
            answer: "dash".into(),
            because: "URLs".into(),
        });
        assert!(n.contains("Decided for you for step 1a: Separator? → dash, because URLs"));
        assert!(n.contains("not hard-coded"));
    }
}
