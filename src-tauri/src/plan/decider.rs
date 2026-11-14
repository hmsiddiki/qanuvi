//! Qanuvi core's decision layer (24b), ported from `src/lib/core/decider.ts`. One model decides (what kind of job,
//! which worker does it, whether a result is good enough) and the others work. Plain functions: prompts, parsing and
//! fallbacks; the runner calls the models.
//!
//! The decider and the workers are plain "seats", so a personal model (25) or another paid agent takes a seat later
//! without the rest changing. Every decision is kept as a record with its reason.

use super::js::{self, json_in};
use super::routing::ModelProfile;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

/// How the team works (the owner's design, 2026-11-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TeamMode {
    /// The paid model decides; free models work; the paid one does what they could not, or steps in on trouble.
    PaidDecides,
    /// Like paid-decides, leaning by `priority`.
    Priority,
    /// Only the paid model works; Qanuvi core gives it context.
    Direct,
    /// A free model decides what to send to the paid one.
    FreeDecides,
    /// Free models decide and work; the paid agent is not used.
    FreeOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Save,
    Balanced,
    Solve,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamSettings {
    pub mode: TeamMode,
    pub priority: Priority,
}

pub const DEFAULT_TEAM: TeamSettings = TeamSettings {
    mode: TeamMode::PaidDecides,
    priority: Priority::Balanced,
};

/// Team settings from stored JSON; anything unknown takes the default.
pub fn parse_team(raw: &Value) -> TeamSettings {
    let pick = |k: &str| raw.get(k).cloned().unwrap_or(Value::Null);
    TeamSettings {
        mode: serde_json::from_value(pick("mode")).unwrap_or(DEFAULT_TEAM.mode),
        priority: serde_json::from_value(pick("priority")).unwrap_or(DEFAULT_TEAM.priority),
    }
}

/// A seat at the table: who decides or works.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Seat {
    Paid { agent: String, model: String },
    Free { models: Vec<String> },
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeModel {
    pub id: String,
    pub local: bool,
}

/// Who decides: any paid agent must; with none, the free models, online first, local last (the weakest judges).
pub fn decider_seat(paid: Option<(&str, &str)>, free: &[FreeModel]) -> Seat {
    if let Some((agent, model)) = paid {
        return Seat::Paid {
            agent: agent.into(),
            model: model.into(),
        };
    }
    let models: Vec<String> = free
        .iter()
        .filter(|m| !m.local)
        .chain(free.iter().filter(|m| m.local))
        .map(|m| m.id.clone())
        .collect();
    if models.is_empty() {
        Seat::None
    } else {
        Seat::Free { models }
    }
}

/// Whether free models get work at all in this mode (Direct: only the paid model works).
pub fn free_works(team: TeamSettings, has_free: bool, has_paid: bool) -> bool {
    if !has_free {
        return false;
    }
    if !has_paid {
        return true;
    }
    matches!(team.mode, TeamMode::PaidDecides | TeamMode::Priority)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepBrief {
    pub number: String,
    pub title: String,
    pub details: Vec<String>,
    /// The kind of work (docs, tests, ui, security, bugfix, refactor, feature).
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Worker {
    Free,
    Paid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub number: String,
    pub worker: Worker,
    pub why: String,
    /// The paid model the decider picked for the step, when it named one from the list.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
}

fn lean(p: Priority) -> &'static str {
    match p {
        Priority::Save => {
            "Save the paid model's tokens: give free models every step they can plausibly do."
        }
        Priority::Balanced => {
            "Balance: free models for routine steps, the paid model for steps that need real judgement."
        }
        Priority::Solve => {
            "Solving the problem comes first: give the paid model every step that is not routine; free models only for plain, low-risk steps."
        }
    }
}

fn step_list(steps: &[StepBrief]) -> String {
    steps
        .iter()
        .map(|s| {
            let details = if s.details.is_empty() {
                String::new()
            } else {
                let d: Vec<&str> = s.details.iter().take(4).map(String::as_str).collect();
                format!("\n   {}", d.join("\n   "))
            };
            format!("{}. {} [{}]{details}", s.number, s.title, s.kind)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The decider's question for a whole plan, one call: which steps free models can do and which need the paid model.
pub fn assign_prompt(steps: &[StepBrief], priority: Priority) -> String {
    [
        "You are the decider in a coding team. Free models (capable but weaker, may break tool calls) and one paid model (strong) can do each step of this plan.",
        lean(priority),
        "Money, payments, security and authentication work always goes to the paid model.",
        "Look at the steps, then answer with JSON only, in this shape:",
        r#"{"steps":[{"number":"1","worker":"free","why":"a short reason"}]}"#,
        "",
        "Steps:",
        &step_list(steps),
    ]
    .join("\n")
}

fn answer_steps(answer: &str) -> Vec<Value> {
    match json_in(answer) {
        Some(v) => v
            .get("steps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        None => vec![],
    }
}

/// Reads the decider's answer. Steps it left out, or answered unusably, get the safe default: security work paid;
/// otherwise free when saving, paid when solving, by kind when balanced.
pub fn parse_assignment(answer: &str, steps: &[StepBrief], priority: Priority) -> Vec<Assignment> {
    let mut given: HashMap<String, Assignment> = HashMap::new();
    for s in answer_steps(answer) {
        let number = js::string(s.get("number")).trim().to_string();
        let worker = match s.get("worker").and_then(Value::as_str) {
            Some("free") => Some(Worker::Free),
            Some("paid") => Some(Worker::Paid),
            _ => None,
        };
        if let (false, Some(worker)) = (number.is_empty(), worker) {
            let why = js::cut(&js::string(s.get("why")), 200).to_string();
            given.insert(
                number.clone(),
                Assignment {
                    number,
                    worker,
                    why,
                    model: None,
                },
            );
        }
    }
    steps
        .iter()
        .map(|s| {
            if s.kind == "security" {
                return Assignment {
                    number: s.number.clone(),
                    worker: Worker::Paid,
                    why: "money or security work".into(),
                    model: None,
                };
            }
            given.get(&s.number).cloned().unwrap_or_else(|| Assignment {
                number: s.number.clone(),
                worker: fallback_worker(s, priority),
                why: "Qanuvi core's rule (the decider gave no answer for it)".into(),
                model: None,
            })
        })
        .collect()
}

/// One kind of work's record per model (plan/routing model profiles), in the order the kinds were first seen.
pub type Profiles = Vec<(String, Vec<ModelProfile>)>;

fn profile_lines(profiles: &Profiles) -> Vec<String> {
    profiles
        .iter()
        .filter(|(_, list)| !list.is_empty())
        .map(|(kind, list)| {
            let parts: Vec<String> = list
                .iter()
                .map(|p| {
                    format!(
                        "{} {}/{} done, {}k tokens and ${} per finished step{}",
                        p.model,
                        p.successes,
                        p.tries,
                        js::number(js::round(p.tokens_per_success / 1000.0)),
                        js::to_fixed(p.cost_per_success, 2),
                        if p.heavy { " HEAVY" } else { "" }
                    )
                })
                .collect();
            format!("- {kind}: {}", parts.join("; "))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
}

pub struct PickOptions<'a> {
    pub priority: Priority,
    pub free_first: bool,
    pub models: &'a [ModelChoice],
    pub profiles: &'a Profiles,
}

/// The decider's question for a whole plan, one call: the model for each step (and, when free models work first,
/// which steps they take). It sees what each model burned before and keeps heavy models for steps that need them.
pub fn pick_prompt(steps: &[StepBrief], opts: &PickOptions) -> String {
    let record = profile_lines(opts.profiles);
    let mut out: Vec<String> =
        vec!["You are the decider in a coding team. Pick, for each step of this plan, the model that does it.".into()];
    if opts.free_first {
        out.push(format!(
            "Free models (capable but weaker, may break tool calls) can take routine steps: worker \"free\". {}",
            lean(opts.priority)
        ));
    }
    out.push("Paid models, strongest first:".into());
    out.extend(
        opts.models
            .iter()
            .map(|m| format!("- {} ({})", m.id, m.label)),
    );
    out.push("High-quality code comes first, saving tokens second. For each step pick the paid model that will do it well; only among models that would do it equally well, pick the lighter one. Never pick a weaker model to save tokens on a step that needs real judgement (money logic, security, tricky bugs, changes across many files).".into());
    if !record.is_empty() {
        out.push("What earlier runs showed, per kind of work (steps done of tried, tokens and cost per finished step). A model marked HEAVY burned far more tokens than another model that also finished that kind of work: pick it only when the step truly needs what it does better, and say why in the reason. A model that often did not finish a kind of work is a quality risk there, whatever it costs.".into());
        out.extend(record);
    }
    out.push("Money, payments, security and authentication work always gets a paid model.".into());
    out.push("Answer with JSON only, in this shape:".into());
    out.push(format!(
        r#"{{"steps":[{{"number":"1","worker":"{}","model":"<model id>","why":"a short reason"}}]}}"#,
        if opts.free_first { "free" } else { "paid" }
    ));
    out.push(String::new());
    out.push("Steps:".into());
    out.push(step_list(steps));
    out.join("\n")
}

/// Reads the decider's picks. A model not in the list is dropped (the step then follows the rules); without free
/// models first every step is paid; steps left out get `parse_assignment`'s safe default.
pub fn parse_picks(
    answer: &str,
    steps: &[StepBrief],
    priority: Priority,
    free_first: bool,
    model_ids: &[String],
) -> Vec<Assignment> {
    let mut models: HashMap<String, String> = HashMap::new();
    for s in answer_steps(answer) {
        let number = js::string(s.get("number")).trim().to_string();
        let model = js::string(s.get("model")).trim().to_string();
        if !number.is_empty() && model_ids.contains(&model) {
            models.insert(number, model);
        }
    }
    parse_assignment(answer, steps, priority)
        .into_iter()
        .map(|a| Assignment {
            worker: if free_first { a.worker } else { Worker::Paid },
            model: models.get(&a.number).cloned(),
            ..a
        })
        .collect()
}

fn fallback_worker(s: &StepBrief, priority: Priority) -> Worker {
    let free = match priority {
        Priority::Save => true,
        Priority::Solve => s.kind == "docs",
        Priority::Balanced => ["docs", "tests", "ui", "refactor"].contains(&s.kind.as_str()),
    };
    if free { Worker::Free } else { Worker::Paid }
}

/// The decider's look at a free model's change before it is accepted (quality first).
pub fn review_prompt(title: &str, details: &[String], diff: &str, tests: &str) -> String {
    let shown = if js::len(diff) > 12_000 {
        format!("{}\n… (cut)", js::cut(diff, 12_000))
    } else {
        diff.to_string()
    };
    let mut out: Vec<String> = vec![
        "You are reviewing a change a free model made for one step of a plan. Is it correct and complete for the step,".into(),
        "without breaking anything around it? Be strict but fair; style alone is not a reason to reject.".into(),
        r#"Answer with JSON only: {"ok": true, "why": "a short reason"} or {"ok": false, "why": "what is wrong"}"#.into(),
        String::new(),
        format!("Step: {title}"),
    ];
    out.extend(details.iter().take(6).map(|d| format!("- {d}")));
    out.push(String::new());
    out.push(if tests.is_empty() {
        "Tests: none were run.".into()
    } else {
        format!("Tests: {tests}")
    });
    out.push(String::new());
    out.push("The change (git diff):".into());
    out.push(if shown.is_empty() {
        "(no change)".into()
    } else {
        shown
    });
    out.join("\n")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub ok: bool,
    pub why: String,
}

/// Reads a review. Anything unreadable counts as not accepted: quality first.
pub fn parse_review(answer: &str) -> Review {
    match json_in(answer) {
        Some(v) if v.get("ok").is_some_and(Value::is_boolean) => Review {
            ok: v["ok"].as_bool().unwrap_or(false),
            why: js::cut(&js::string(v.get("why")), 300).to_string(),
        },
        _ => Review {
            ok: false,
            why: "the review could not be read".into(),
        },
    }
}

/// Free decides, paid works (chat): which paid model a message needs.
pub fn chat_model_prompt(message: &str, models: &[ModelChoice]) -> String {
    [
        "Pick the paid model for this coding request: the cheapest one that will do it well. Stronger models first in the list.".to_string(),
        models
            .iter()
            .map(|m| format!("- {} ({})", m.id, m.label))
            .collect::<Vec<_>>()
            .join("\n"),
        r#"Answer with JSON only: {"model": "<id>", "why": "a short reason"}"#.into(),
        String::new(),
        "Request:".into(),
        js::cut(message, 4000).to_string(),
    ]
    .join("\n")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatModel {
    pub model: String,
    pub why: String,
}

pub fn parse_chat_model(answer: &str, model_ids: &[String]) -> Option<ChatModel> {
    let v = json_in(answer);
    let model = v
        .as_ref()
        .and_then(|v| v.get("model"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !model_ids.contains(&model) {
        return None;
    }
    let why = js::cut(&js::string(v.as_ref().and_then(|v| v.get("why"))), 200).to_string();
    Some(ChatModel { model, why })
}

/// A decision kept for later (shown in the run, and what a personal model will learn from).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub at: f64,
    /// assign, review, chat-model, handover, set-aside or decided.
    pub kind: String,
    pub by: String,
    pub subject: String,
    pub decision: String,
    pub why: String,
}

/// What one step changed in a file: the lines removed (-) and added (+) between two versions, with no context
/// lines. Very long files give None (the caller uses the file's whole diff instead).
pub fn step_diff(path: &str, before: &str, after: &str) -> Option<String> {
    let a: Vec<&str> = before.split('\n').collect();
    let b: Vec<&str> = after.split('\n').collect();
    let (n, m) = (a.len(), b.len());
    if n * m > 4_000_000 {
        return None;
    }
    // Longest common subsequence, then walk it.
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if a[i] == b[j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let mut out: Vec<String> = vec![];
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            i += 1;
            j += 1;
        } else if j < m && (i >= n || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j]) {
            out.push(format!("+{}", b[j]));
            j += 1;
        } else {
            out.push(format!("-{}", a[i]));
            i += 1;
        }
    }
    Some(if out.is_empty() {
        String::new()
    } else {
        format!(
            "--- {path} (before this step)\n+++ {path} (after)\n{}",
            out.join("\n")
        )
    })
}

/// The word the paid agent answers with when the free pick is too weak for the message (24e).
pub const NEEDS_STRONGER: &str = "NEEDS_STRONGER_MODEL";

/// The start of the note added for the paid agent (24e).
pub const OVERRULE_MARK: &str = "\n\n(Qanuvi: a free model chose";

/// The note added to the message, so the paid agent sees the free model's pick and can overrule it.
pub fn overrule_note(model: &str, why: &str) -> String {
    let because = if why.is_empty() {
        String::new()
    } else {
        format!(" because: {why}")
    };
    [
        String::new(),
        String::new(),
        format!("(Qanuvi: a free model chose {model} for this message{because}. If this message needs a"),
        format!("stronger model to be done well, answer only \"{NEEDS_STRONGER}: <the reason>\" and do nothing else.)"),
    ]
    .join("\n")
}

/// What the user wrote, without the note Qanuvi added for the paid agent.
pub fn without_overrule_note(text: &str) -> &str {
    text.find(OVERRULE_MARK).map_or(text, |i| &text[..i])
}

/// The paid agent's overrule, if its answer is one: the reason, else None.
pub fn overrule_in(answer: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(&format!(r"(?s)^{NEEDS_STRONGER}\s*:?\s*(.*)")).unwrap());
    let c = RE.captures(answer.trim())?;
    let reason = c[1].trim();
    Some(if reason.is_empty() {
        "no reason given".into()
    } else {
        reason.into()
    })
}

/// One model up in a list ordered strongest first; the strongest stays itself.
pub fn stronger_model(model_ids: &[String], current: &str) -> String {
    match model_ids.iter().position(|m| m == current) {
        Some(i) if i > 0 => model_ids[i - 1].clone(),
        _ => model_ids.first().cloned().unwrap_or_else(|| current.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn steps() -> Vec<StepBrief> {
        let s = |n: &str, t: &str, d: &[&str], k: &str| StepBrief {
            number: n.into(),
            title: t.into(),
            details: d.iter().map(|x| x.to_string()).collect(),
            kind: k.into(),
        };
        vec![
            s("1", "Write a README", &[], "docs"),
            s("2", "Add a checkout page", &["Stripe"], "security"),
            s("3", "Add a filter to the list", &[], "feature"),
        ]
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_decider_is_paid_when_there_is_one_else_free_online_first() {
        assert_eq!(
            decider_seat(
                Some(("claude-code", "opus")),
                &[FreeModel {
                    id: "or/a".into(),
                    local: false
                }]
            ),
            Seat::Paid {
                agent: "claude-code".into(),
                model: "opus".into()
            }
        );
        let free = [
            FreeModel {
                id: "ollama/q".into(),
                local: true,
            },
            FreeModel {
                id: "omniroute/s".into(),
                local: false,
            },
            FreeModel {
                id: "openrouter/n".into(),
                local: false,
            },
        ];
        assert_eq!(
            decider_seat(None, &free),
            Seat::Free {
                models: ids(&["omniroute/s", "openrouter/n", "ollama/q"])
            }
        );
        assert_eq!(decider_seat(None, &[]), Seat::None);
        assert_eq!(
            serde_json::to_value(Seat::None).unwrap(),
            json!({"kind": "none"})
        );
    }

    #[test]
    fn free_models_work_except_in_direct_and_free_decides_with_a_paid_agent() {
        let t = |m: &str| parse_team(&json!({ "mode": m }));
        assert!(free_works(t("paid-decides"), true, true));
        assert!(!free_works(t("direct"), true, true));
        assert!(!free_works(t("free-decides"), true, true));
        assert!(free_works(t("direct"), true, false));
        assert_eq!(
            parse_team(&json!({"mode": "nonsense", "priority": "solve"})),
            TeamSettings {
                mode: TeamMode::PaidDecides,
                priority: Priority::Solve
            }
        );
    }

    #[test]
    fn asks_once_and_keeps_security_work_paid() {
        let st = steps();
        assert!(assign_prompt(&st, Priority::Save).contains("Save the paid model's tokens"));
        let answer = "Sure:\n```json\n{\"steps\":[{\"number\":\"1\",\"worker\":\"free\",\"why\":\"plain docs\"},{\"number\":\"2\",\"worker\":\"free\",\"why\":\"easy\"}]}\n```";
        let a = parse_assignment(answer, &st, Priority::Balanced);
        assert_eq!(
            (a[0].worker, a[0].why.as_str()),
            (Worker::Free, "plain docs")
        );
        assert_eq!(
            (a[1].worker, a[1].why.as_str()),
            (Worker::Paid, "money or security work")
        );
        assert_eq!(a[2].worker, Worker::Paid);
        let w = |p| {
            parse_assignment("no idea", &st, p)
                .iter()
                .map(|x| x.worker)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            w(Priority::Save),
            [Worker::Free, Worker::Paid, Worker::Free]
        );
        assert_eq!(
            w(Priority::Solve),
            [Worker::Free, Worker::Paid, Worker::Paid]
        );
    }

    #[test]
    fn picks_keep_listed_models_only() {
        let st = steps();
        let answer = r#"{"steps":[{"number":"1","worker":"free","model":"haiku","why":"docs"},{"number":"3","model":"gpt-9"}]}"#;
        let p = parse_picks(
            answer,
            &st,
            Priority::Balanced,
            false,
            &ids(&["opus", "haiku"]),
        );
        assert_eq!(p[0].model.as_deref(), Some("haiku"));
        assert_eq!(p[0].worker, Worker::Paid);
        assert_eq!(p[2].model, None);
    }

    #[test]
    fn accepts_a_free_change_only_on_a_clear_yes() {
        assert!(review_prompt("t", &[], &"x".repeat(20_000), "").contains("(cut)"));
        assert_eq!(
            parse_review(r#"{"ok": true, "why": "fine"}"#),
            Review {
                ok: true,
                why: "fine".into()
            }
        );
        assert!(!parse_review(r#"{"ok": false, "why": "breaks the build"}"#).ok);
        assert!(!parse_review("looks good to me").ok);
    }

    #[test]
    fn picks_a_chat_model_only_from_the_list() {
        let models = [
            ModelChoice {
                id: "claude-opus-5-5".into(),
                label: "Opus".into(),
            },
            ModelChoice {
                id: "claude-haiku-5-5".into(),
                label: "Haiku".into(),
            },
        ];
        assert!(chat_model_prompt("rename a variable", &models).contains("claude-haiku-5-5"));
        let list = ids(&["claude-opus-5-5", "claude-haiku-5-5"]);
        assert_eq!(
            parse_chat_model(r#"{"model":"claude-haiku-5-5","why":"small"}"#, &list),
            Some(ChatModel {
                model: "claude-haiku-5-5".into(),
                why: "small".into()
            })
        );
        assert_eq!(parse_chat_model(r#"{"model":"gpt-9"}"#, &list), None);
    }

    #[test]
    fn step_diff_shows_only_this_steps_change() {
        let before = "Shopping list\n- milk\n- apples\n- bread\n- butter\n";
        let after = "Shopping list\n- apples\n- bread\n- butter\n- milk\n";
        let d = step_diff("notes.txt", before, after).unwrap();
        assert!(d.contains("-- milk"));
        assert!(d.contains("+- milk"));
        assert!(!d.contains("+- butter"));
        assert_eq!(step_diff("a", "x\ny", "x\ny").as_deref(), Some(""));
        let big = "l\n".repeat(3000);
        assert_eq!(step_diff("a", &big, &format!("{big}x")), None);
    }

    #[test]
    fn the_paid_agent_overrules_a_free_pick() {
        let n = overrule_note("Haiku 5.5", "a short question");
        assert!(n.contains("Haiku 5.5") && n.contains("NEEDS_STRONGER_MODEL"));
        assert_eq!(
            overrule_in("NEEDS_STRONGER_MODEL: the refactor spans many files").as_deref(),
            Some("the refactor spans many files")
        );
        assert_eq!(
            overrule_in("  NEEDS_STRONGER_MODEL").as_deref(),
            Some("no reason given")
        );
        assert_eq!(
            overrule_in("The file holds prices. NEEDS_STRONGER_MODEL is not needed."),
            None
        );
        let models = ids(&["fable", "opus", "sonnet", "haiku"]);
        assert_eq!(stronger_model(&models, "haiku"), "sonnet");
        assert_eq!(stronger_model(&models, "fable"), "fable");
        assert_eq!(
            without_overrule_note(&format!(
                "Is it sorted?{}",
                overrule_note("Haiku 5.5", "short")
            )),
            "Is it sorted?"
        );
        assert_eq!(without_overrule_note("Plain text"), "Plain text");
    }
}
