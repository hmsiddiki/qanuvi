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

/// A login, key or address the steps need; `what`, `example` and `if_empty` are written for someone who does not know
/// the code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightSecret {
    pub name: String,
    /// What it is, in plain words.
    #[serde(default)]
    pub what: String,
    /// An example value (never a real password).
    #[serde(default)]
    pub example: String,
    /// What happens if it is left empty.
    #[serde(default)]
    pub if_empty: String,
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
    /// PM a: "project" when the answer holds for the whole project (a test site, how tests run), so the owner's
    /// answer goes into This project memory; "plan" when it is only this plan's choice.
    #[serde(default = "plan_about")]
    pub about: String,
    /// PM c: the project memory already answers it: recommended is the memory's answer, shown "from memory".
    #[serde(default, rename = "fromMemory")]
    pub from_memory: bool,
}

fn plan_about() -> String {
    "plan".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreflightRisk {
    pub step: String,
    pub why: String,
    pub sort: AskSort,
    pub because: String,
    /// Y10 d1: "delete" when going ahead gives this step alone leave to delete files inside the project; else "".
    #[serde(default)]
    pub grant: String,
}

/// Y10 d2: a check that needs a person or a live drive, and what the run does about it: "run" (the step does it,
/// `how` says with what), "end" (the owner's list to look at, after the run) or "skip".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eyes {
    pub step: String,
    pub what: String,
    #[serde(rename = "do")]
    pub act: String,
    pub how: String,
}

/// Y10 d3: the one real end-to-end check the closing step must pass (no stand-ins).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndCheck {
    pub step: String,
    pub check: String,
    pub why: String,
}

/// Y10 d4: part of a step's work left for later, and the open step that will do it ("" when none does).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Promise {
    pub from: String,
    pub what: String,
    pub step: String,
}

/// Y10 d5: a program, address or secret the steps cannot run without; Qanuvi tries it before the run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub kind: String,
    pub name: String,
    pub steps: Vec<String>,
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
    /// Steps that must run after steps the file lists after them (R2e g): each becomes a "- Needs:" line.
    #[serde(default)]
    pub order: Vec<Order>,
    /// Y10: read by the core only (the window shows them as the core reads them).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eyes: Vec<Eyes>,
    #[serde(default, rename = "endCheck", skip_serializing_if = "Option::is_none")]
    pub end_check: Option<EndCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promises: Vec<Promise>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<Need>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub step: String,
    pub after: Vec<String>,
    pub why: String,
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
    /// PM c: what This project's memory knows, one line per entry.
    #[serde(default)]
    pub memory: Vec<String>,
}

/// f4: an example in the owner's questions never uses their own name (it once came from the folder path).
pub const NEUTRAL_EXAMPLES: &str = "Examples use made-up neutral names (for example \"Example Shop Ltd\"), never the owner's own name or a name taken from a folder path, user name or account.";

/// Y10 (d1-d5): what the check also finds before the run, so the run does not hold for it later.
const CHECK_MORE: &[&str] = &[
    "Never-list forecast: for each step, think which commands its work will need. One the run rules refuse (deleting",
    "files, pushing, publishing, ssh, installs, outside sites) is a risky item that names the part in plain words (for",
    r#"example "removes the old test files"). Deleting files inside this project is the only thing a step can be given:"#,
    r#"set "grant": "delete" and sort it "checks" (Qanuvi keeps a copy to undo it). Anything else the rules refuse is"#,
    r#"sorted "you", with no grant."#,
    "eyes: every check a person must look at or that needs a live drive (a window, a tray icon, a dialog, another",
    r#"machine). "do": "run" when the run can do it itself with what exists here (a test build, a script, a command; say"#,
    r#"how), "end" when only a person can judge it (it goes on the owner's list to look at after the run), "skip" when"#,
    "it is not worth doing. Using the plan's real thing end to end is never eyes \"skip\": it is the endCheck.",
    "endCheck: when the plan builds something that runs (an app, a server, a runner), one command that uses the real",
    "thing end to end as a user would after the last step (for a plan runner: run a small plan with the real agent and",
    "check that it finished), never only that it starts and never with stand-ins or fakes; else null. Use a command",
    "the run rules allow when one does it (a new folder under the system's temp folder instead of removing one). When",
    "only a command they refuse uses it for real (ssh to another machine, a download), give that one: Qanuvi asks the",
    "owner about it before the run. Never a weaker local check in its place.",
    r#"promises: a step that leaves part of its work for later or "until step N": from = that step, step = the open step"#,
    "that will do it (empty when no step does).",
    "needs: the programs, addresses (http or host:port) and secrets the steps cannot run without; Qanuvi tries each on",
    "this machine before the run.",
    "A program a step needs that is neither in this project nor on the PATH (for example one found inside another",
    "project's folder) is not used quietly: add a setup item that names where it was found, that the run depends on",
    "that folder staying, and recommends adding it to this project.",
];

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
        format!("Before an unattended run of {file}, read these open steps and the files they will change (read-only: open as"),
        "many as you need, but take minutes, not a step's work) and list".into(),
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
    if !known.memory.is_empty() {
        out.push("- This project's memory (the owner's own facts about the project):".into());
        out.extend(known.memory.iter().map(|m| format!("  - {m}")));
    }
    out.extend([
        String::new(),
        "Open steps:".into(),
        list,
        String::new(),
        "Answer with JSON only, in this shape (empty lists when nothing is needed):".into(),
        r#"{"secrets":[{"name":"ENV_NAME","what":"what it is, in plain words","example":"an example value, never a real password","ifEmpty":"what happens if it is left empty","why":"...","steps":["82e"]}],"#.into(),
        r#" "questions":[{"step":"83c","question":"a choice the steps leave open","sort":"decide","recommended":"the answer you advise","because":"...","about":"plan","fromMemory":false}],"#.into(),
        r#" "risky":[{"step":"84a","why":"deletes data / migrates / publishes / live payments / pushes","sort":"checks","because":"...","grant":""}],"#.into(),
        r#" "setup":[{"what":"something that must run or be set","why":"..."}],"#.into(),
        r#" "order":[{"step":"85b","after":["85c"],"why":"..."}],"#.into(),
        r#" "eyes":[{"step":"86a","what":"a check that needs a person or a live drive","do":"run","how":"how the run does it itself"}],"#.into(),
        r#" "endCheck":{"step":"87","check":"one command that runs the real thing end to end","why":"..."},"#.into(),
        r#" "promises":[{"from":"86b","what":"what it leaves for later","step":"87"}],"#.into(),
        r#" "needs":[{"kind":"program","name":"chromium","steps":["86a"]}]}"#.into(),
        "Rules: secrets only for logins, keys, passwords and addresses the steps cannot run without and that are not".into(),
        "already known. Before asking, look at how the project's test command or test runner reads its settings (its docs".into(),
        "say what to set): a variable the runner fills in by itself is not missing, ask for the one the runner reads; a".into(),
        "need a known secret already covers is not asked again.".into(),
        "Write every secret, question and risky item for someone who does not know the code: plain words, what it is for".into(),
        "and what each answer changes, with one concrete example; never quote a plan line or a code name without saying".into(),
        "what it means.".into(),
        NEUTRAL_EXAMPLES.into(),
        "Questions only for real choices the steps leave open, not for things you can decide from the code; risky only".into(),
        "for steps that are hard to undo or reach outside this computer; order only when a step must run after steps".into(),
        "listed below it (the run goes in file order otherwise; each becomes a Needs line in the plan). At most 8 items".into(),
        "in each list.".into(),
        "Sort every question and risky step (sort):".into(),
        r#"- "decide": a sensible default exists that can be changed later (a setting, not hard-coded); give it as"#.into(),
        "  recommended and say why. Most naming, format, layout and library choices are this.".into(),
        r#"- "checks": it may go ahead if the decider reviews its diff and the tests pass first (migrations, deletions on"#.into(),
        "  this computer or a test database, refactors of shared code).".into(),
        r#"- "you": truly needs the owner: secrets, actions on outside accounts (real payments, publishing, pushing, mail"#.into(),
        "  to real people), or anything that leaves this computer. Still give a recommended answer when you have one.".into(),
        "Judge risk by where the step runs: money on a local or test database is not real money.".into(),
        r#"A "decide" answer must be something the run may do under the run rules above (nothing they hold or refuse,"#.into(),
        r#"such as a download, an install or an outside site); when only such a way exists, sort it "you"."#.into(),
        r#""about": "project" when the answer holds for the whole project beyond this plan (which site or address to test"#.into(),
        r#"on, how the tests run, how to get a test login, where something is), else "plan"."#.into(),
        r#"A question the project memory or a known secret already answers is not asked of the owner: give it as "decide""#.into(),
        r#"with the memory's answer as recommended and "fromMemory": true."#.into(),
    ]);
    out.extend(CHECK_MORE.iter().map(|l| l.to_string()));
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

/// `typeof v === "string" ? clipReason(v.trim()) : ""`.
fn text(v: &Value) -> String {
    v.as_str()
        .filter(|s| !is_placeholder(js::trim(s)))
        .map(|s| clip_reason(js::trim(s), 300))
        .unwrap_or_default()
}

/// A placeholder in place of the words (seen in a check's answer: "<<ccr:0bd3c8834676,string,348B>>") is no text:
/// it is never shown to the owner as a question.
pub fn is_placeholder(s: &str) -> bool {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^<<[a-z]+:[^<>]*>>$").unwrap());
    RE.is_match(s)
}

/// A text longer than `n` characters (UTF-16 units) ends at a word (when one ends at 200 or later) with "…"
/// (`clipReason`).
pub fn clip_reason(s: &str, n: usize) -> String {
    if js::len(s) <= n {
        return s.to_string();
    }
    let mut head = js::cut(s, n - 1);
    if let Some(space) = head.rfind(' ')
        && js::len(&head[..space]) >= 200
    {
        head = &head[..space];
    }
    format!(
        "{}…",
        head.trim_end_matches(|c: char| c.is_whitespace() && c != '\u{85}')
    )
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
                what: text(&s["what"]),
                example: text(&s["example"]),
                if_empty: text(&s["ifEmpty"]),
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
                    about: if q["about"].as_str() == Some("project") {
                        "project".into()
                    } else {
                        "plan".into()
                    },
                    from_memory: q["fromMemory"].as_bool() == Some(true) && !recommended.is_empty(),
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
                grant: if r["grant"].as_str() == Some("delete") {
                    "delete".into()
                } else {
                    String::new()
                },
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
        order: items(field("order"))
            .into_iter()
            .map(|o| {
                let step = text(&o["step"]);
                let list: Vec<Value> = match &o["after"] {
                    Value::Array(a) => a.clone(),
                    s @ Value::String(_) => vec![s.clone()],
                    _ => vec![],
                };
                let mut after: Vec<String> = Vec::new();
                for a in list.iter().map(text) {
                    // JavaScript's \s and the comma: one label per item.
                    let bad = a.chars().any(|c| {
                        c == ',' || (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
                    });
                    if !a.is_empty() && a != step && !bad && !after.contains(&a) {
                        after.push(a);
                    }
                }
                after.truncate(10);
                Order {
                    step,
                    after,
                    why: text(&o["why"]),
                }
            })
            .filter(|o| !o.step.is_empty() && !o.after.is_empty())
            .take(8)
            .collect(),
        eyes: items(field("eyes"))
            .into_iter()
            .map(|e| Eyes {
                step: text(&e["step"]),
                what: text(&e["what"]),
                act: match e["do"].as_str() {
                    Some("run") => "run".into(),
                    Some("skip") => "skip".into(),
                    _ => "end".into(),
                },
                how: text(&e["how"]),
            })
            .filter(|e| !e.step.is_empty() && !e.what.is_empty())
            .take(12)
            .collect(),
        end_check: Some(field("endCheck"))
            .filter(|v| v.is_object())
            .map(|v| EndCheck {
                step: text(&v["step"]),
                check: text(&v["check"]),
                why: text(&v["why"]),
            })
            .filter(|e| !e.check.is_empty()),
        promises: items(field("promises"))
            .into_iter()
            .map(|p| Promise {
                from: text(&p["from"]),
                what: text(&p["what"]),
                step: text(&p["step"]),
            })
            .filter(|p| !p.what.is_empty())
            .take(12)
            .collect(),
        needs: items(field("needs"))
            .into_iter()
            .map(|n| Need {
                kind: text(&n["kind"]),
                name: text(&n["name"]),
                steps: n["steps"]
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
            .filter(|n| {
                !n.name.is_empty() && matches!(n.kind.as_str(), "program" | "address" | "secret")
            })
            .take(12)
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
/// How a hold for a question the check left open starts (Qanuvi decides it after a while with no answer).
pub const OPEN_QUESTION: &str = "a question was left open";
/// How the hold for a real end-to-end check the run rules refuse starts: only the owner answers it (never decided
/// after a wait, no "Let Qanuvi decide").
pub const END_TO_END: &str = "the real end-to-end check needs your answer";

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
                    why: format!("{OPEN_QUESTION} ({})", q.question),
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

/// p2 (owner, 2026-11-20): the check's answers are read by meaning before anything is saved (`answersPrompt` and the
/// functions after it in `preflight.ts`). One short call reads them together and sorts each: a value, an instruction
/// (tried at once, read-only; what was found waits for the owner's yes) or unclear (not saved: the step that needs it
/// waits and asks in the Inbox, the others go on).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedAnswer {
    /// "secret:NAME" or "question:<index>".
    pub id: String,
    pub ask: String,
    pub example: String,
    pub answer: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingKind {
    Value,
    Instruction,
    Unclear,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyFrom {
    pub project: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerReading {
    pub id: String,
    pub kind: ReadingKind,
    pub value: String,
    pub r#where: String,
    pub copy_from: Option<CopyFrom>,
    pub skip: bool,
    pub reply: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtherProject {
    pub name: String,
    pub path: String,
    pub secrets: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownForAnswers {
    pub secrets: Vec<String>,
    pub others: Vec<OtherProject>,
}

/// One piece without spaces is the value itself, saved as typed and never sent to a model (a password stays out of
/// every session's record); anything with spaces is read.
pub fn secret_needs_reading(typed: &str) -> bool {
    js::trim(typed)
        .chars()
        .any(|c| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

fn secret_ask(s: &PreflightSecret) -> String {
    let what = [&s.what, &s.why, &s.name]
        .into_iter()
        .find(|x| !x.is_empty())
        .cloned()
        .unwrap_or_default();
    format!("{what} ({})", s.name)
}

fn answer_at(m: &HashMap<usize, String>, i: usize) -> &str {
    m.get(&i).map(|a| js::trim(a)).unwrap_or("")
}

/// The answers the reading call reads: secrets with a sentence in them, and choices the owner typed or changed.
pub fn typed_answers(
    check: &Preflight,
    secret_values: &HashMap<String, String>,
    answers: &HashMap<usize, String>,
    answers_touched: &HashMap<usize, bool>,
) -> Vec<TypedAnswer> {
    let mut out = Vec::new();
    for s in &check.secrets {
        let v = secret_values
            .get(&s.name)
            .map(|v| js::trim(v))
            .unwrap_or("");
        if !v.is_empty() && secret_needs_reading(v) {
            out.push(TypedAnswer {
                id: format!("secret:{}", s.name),
                ask: secret_ask(s),
                example: s.example.clone(),
                answer: clip_reason(v, 600),
            });
        }
    }
    for (i, q) in check.questions.iter().enumerate() {
        let a = answer_at(answers, i);
        if !a.is_empty() && answers_touched.get(&i).copied().unwrap_or(false) && a != q.recommended
        {
            out.push(TypedAnswer {
                id: format!("question:{i}"),
                ask: q.question.clone(),
                example: q.recommended.clone(),
                answer: clip_reason(a, 600),
            });
        }
    }
    out
}

pub fn answers_prompt(file: &str, items: &[TypedAnswer], known: &KnownForAnswers) -> String {
    let mut list: Vec<String> = Vec::new();
    for x in items {
        list.push(format!("- id: {}", x.id));
        list.push(format!("  asked: {}", x.ask));
        if !x.example.is_empty() {
            list.push(format!("  example: {}", x.example));
        }
        list.push(format!(
            "  answer: {}",
            serde_json::to_string(&x.answer).unwrap_or_default()
        ));
    }
    let mut others: Vec<String> = known
        .others
        .iter()
        .take(20)
        .map(|p| {
            let names = p.secrets.join(", ");
            format!(
                "- {} ({}): {}",
                p.name,
                p.path,
                if names.is_empty() { "none" } else { &names }
            )
        })
        .collect();
    if others.is_empty() {
        others.push("- none".into());
    }
    let secrets = known.secrets.join(", ");
    let mut out: Vec<String> = vec![
        format!("Before a run of {file}, Qanuvi asked the owner a few things. Read each answer by what it means (any wording,"),
        "spelling or language, short or long), never word by word, and sort it:".into(),
        r#"- "value": the answer itself (an address, a user name, a choice). Put it in value, without words around it"#.into(),
        r#"  ("it is localhost:3306" gives "localhost:3306")."#.into(),
        r#"- "instruction": the owner tells Qanuvi what to do instead (find it, look somewhere, use the same as another"#.into(),
        "  project, leave it, you decide). Carry it out now, read-only and quick (a few looks at most: this project's".into(),
        "  docs, config and example files, or the other projects below): put what you found in value and the file or".into(),
        "  place in where. When it is a known secret of another project, give copyFrom with that project's name and the".into(),
        "  secret's name and leave value empty (Qanuvi copies it; you never see it). skip true only when the owner clearly".into(),
        "  says to go on without it or lets Qanuvi decide.".into(),
        "  Found nothing: value empty, and reply says in one sentence where you looked.".into(),
        r#"- "unclear": the owner asks back, is unsure, or you cannot tell what they mean. In reply, answer what they asked"#.into(),
        "  in one or two plain sentences with an example, for someone who does not know code; it is asked again later.".into(),
        "  Not now, later, not known yet, or anything you cannot place is unclear too (the step waits and asks again),".into(),
        "  never skip; when unsure between skip and unclear, answer unclear.".into(),
        "Change nothing. Never guess a password.".into(),
        NEUTRAL_EXAMPLES.into(),
        String::new(),
        format!(
            "Known secrets of this project (names only): {}",
            if secrets.is_empty() { "none" } else { &secrets }
        ),
        "Other projects on this machine (their secrets, names only):".into(),
    ];
    out.extend(others);
    out.push(String::new());
    out.push("Answers:".into());
    out.extend(list);
    out.push(String::new());
    out.push("Answer with JSON only, one entry per id:".into());
    out.push(r#"{"answers":[{"id":"secret:DB_HOST","kind":"instruction","value":"localhost:3306","where":"config.php","copyFrom":null,"skip":false,"reply":""}]}"#.into());
    out.join("\n")
}

/// Reads the reading call's answer: one reading per known id; anything unreadable is left out.
pub fn parse_answer_readings(answer: &str, ids: &[String]) -> Vec<AnswerReading> {
    let (Some(start), Some(end)) = (answer.find('{'), answer.rfind('}')) else {
        return vec![];
    };
    if end <= start {
        return vec![];
    }
    let Ok(raw) = serde_json::from_str::<Value>(&answer[start..=end]) else {
        return vec![];
    };
    let mut out: Vec<AnswerReading> = Vec::new();
    for r in raw
        .get("answers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|x| x.is_object())
    {
        let id = text(&r["id"]);
        if !ids.contains(&id) || out.iter().any(|o| o.id == id) {
            continue;
        }
        let kind = match r["kind"].as_str() {
            Some("value") => ReadingKind::Value,
            Some("instruction") => ReadingKind::Instruction,
            Some("unclear") => ReadingKind::Unclear,
            _ => continue,
        };
        let is_instruction = kind == ReadingKind::Instruction;
        let c = &r["copyFrom"];
        let copy_from = (is_instruction && c.is_object())
            .then(|| CopyFrom {
                project: text(&c["project"]),
                name: text(&c["name"]),
            })
            .filter(|c| !c.project.is_empty() && !c.name.is_empty());
        out.push(AnswerReading {
            id,
            kind,
            value: text(&r["value"]),
            r#where: if is_instruction {
                text(&r["where"])
            } else {
                String::new()
            },
            copy_from,
            skip: is_instruction && r["skip"] == Value::Bool(true),
            reply: text(&r["reply"]),
        });
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundValue {
    pub id: String,
    pub ask: String,
    pub value: String,
    pub r#where: String,
    pub copy_from: Option<CopyFrom>,
    pub secret: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretValue {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretCopy {
    pub name: String,
    pub project: String,
    pub from: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadAnswers {
    pub save: Vec<SecretValue>,
    pub copy: Vec<SecretCopy>,
    pub answers: HashMap<usize, String>,
    pub answers_touched: HashMap<usize, bool>,
    pub holds: Vec<Hold>,
    pub offered: Vec<FoundValue>,
}

/// "The password ..." mid-sentence reads "the password ..."; a name such as MySQL, API or NOTES_DB stays as written
/// (`midSentence`).
fn mid_sentence(text: &str) -> String {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let word: String = chars
        .clone()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if first.is_ascii_uppercase()
        && !word.is_empty()
        && word.chars().all(|c| c.is_ascii_lowercase())
    {
        format!("{}{}", first.to_ascii_lowercase(), chars.as_str())
    } else {
        text.to_string()
    }
}

/// The hold's question for an answer that did not give what was needed: it stands on its own in the Inbox. `choice`:
/// an open choice (answered in words), else a login or value (given, or where Qanuvi can find it).
pub fn unsettled_why(ask: &str, typed: &str, reply: &str, choice: bool) -> String {
    let said = clip_reason(typed, 120);
    let more = reply.trim_end_matches(|c: char| {
        c == '.' || (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
    });
    let more = if more.is_empty() {
        String::new()
    } else {
        format!("; {more}")
    };
    let end = if choice {
        "Answer it in your own words"
    } else {
        "Give the value, or say where Qanuvi can find it"
    };
    let ask = mid_sentence(ask);
    let stop = if ask.ends_with(['.', '?', '!']) {
        ""
    } else {
        "."
    };
    format!("it needs {ask}{stop} Before the run you wrote \"{said}\"{more}. {end}")
}

/// A step held twice (a login and a choice both unsettled) asks both: the second need is added, never dropped.
fn hold_once(out: &mut ReadAnswers, steps: &[String], why: &str) {
    for step in steps {
        if step.is_empty() {
            continue;
        }
        match out.holds.iter_mut().find(|h| &h.step == step) {
            None => out.holds.push(Hold {
                step: step.clone(),
                why: why.to_string(),
                by: HoldBy::Default,
            }),
            Some(had) if !had.why.contains(why) => had.why = format!("{}; also {why}", had.why),
            Some(_) => {}
        }
    }
}

/// Applies the readings to the form (`readAnswers`): `used` is the owner's yes or no to each found value; one not
/// answered yet is offered. A secret's sentence with no reading is not saved: its step waits and asks.
pub fn read_answers(
    check: &Preflight,
    secret_values: &HashMap<String, String>,
    answers: &HashMap<usize, String>,
    answers_touched: &HashMap<usize, bool>,
    readings: &[AnswerReading],
    used: &HashMap<String, bool>,
    first_step: &str,
) -> ReadAnswers {
    let mut out = ReadAnswers {
        answers: answers.clone(),
        answers_touched: answers_touched.clone(),
        ..Default::default()
    };
    let reading = |id: &str| readings.iter().find(|r| r.id == id);
    for s in &check.secrets {
        let v = secret_values
            .get(&s.name)
            .map(|v| js::trim(v))
            .unwrap_or("");
        if v.is_empty() {
            continue;
        }
        if !secret_needs_reading(v) {
            out.save.push(SecretValue {
                name: s.name.clone(),
                value: v.to_string(),
            });
            continue;
        }
        let id = format!("secret:{}", s.name);
        let steps = if s.steps.is_empty() {
            vec![first_step.to_string()]
        } else {
            s.steps.clone()
        };
        let ask = secret_ask(s);
        match reading(&id) {
            Some(r) if r.kind == ReadingKind::Value => out.save.push(SecretValue {
                name: s.name.clone(),
                value: if r.value.is_empty() {
                    v.to_string()
                } else {
                    r.value.clone()
                },
            }),
            Some(r) if r.kind == ReadingKind::Instruction && r.skip => {}
            Some(r)
                if r.kind == ReadingKind::Instruction
                    && (!r.value.is_empty() || r.copy_from.is_some()) =>
            {
                match used.get(&id) {
                    Some(true) => match &r.copy_from {
                        Some(c) => out.copy.push(SecretCopy {
                            name: s.name.clone(),
                            project: c.project.clone(),
                            from: c.name.clone(),
                        }),
                        None => out.save.push(SecretValue {
                            name: s.name.clone(),
                            value: r.value.clone(),
                        }),
                    },
                    said => {
                        if said.is_none() {
                            out.offered.push(FoundValue {
                                id: id.clone(),
                                ask: ask.clone(),
                                value: r.value.clone(),
                                r#where: r.r#where.clone(),
                                copy_from: r.copy_from.clone(),
                                secret: true,
                            });
                        }
                        hold_once(&mut out, &steps, &unsettled_why(&ask, v, &r.reply, false));
                    }
                }
            }
            r => {
                let reply = r.map(|r| r.reply.as_str()).unwrap_or("");
                hold_once(&mut out, &steps, &unsettled_why(&ask, v, reply, false));
            }
        }
    }
    for (i, q) in check.questions.iter().enumerate() {
        let a = answer_at(answers, i);
        let id = format!("question:{i}");
        if a.is_empty() || !answers_touched.get(&i).copied().unwrap_or(false) || a == q.recommended
        {
            continue;
        }
        // No reading (no model, or it failed): a one-piece answer ("dark", "1,234.00") stands as typed; a sentence is
        // never taken word by word (it may be "plus tard" or a question back): the step waits and asks again.
        let Some(r) = reading(&id) else {
            if secret_needs_reading(a) {
                out.answers.insert(i, String::new());
                let why = unsettled_why(&format!("your answer to: {}", q.question), a, "", true);
                hold_once(&mut out, std::slice::from_ref(&q.step), &why);
            }
            continue;
        };
        if r.kind == ReadingKind::Value {
            let v = if r.value.is_empty() {
                a.to_string()
            } else {
                r.value.clone()
            };
            out.answers.insert(i, v);
            continue;
        }
        let instruction = r.kind == ReadingKind::Instruction;
        if instruction && r.skip {
            // "You decide": the recommended answer, as Qanuvi's own choice; with none, the step waits.
            out.answers.insert(i, q.recommended.clone());
            out.answers_touched.insert(i, false);
            continue;
        }
        let found = instruction && !r.value.is_empty();
        if found && used.get(&id) == Some(&true) {
            out.answers.insert(i, r.value.clone());
            continue;
        }
        if found && !used.contains_key(&id) {
            out.offered.push(FoundValue {
                id,
                ask: q.question.clone(),
                value: r.value.clone(),
                r#where: r.r#where.clone(),
                copy_from: None,
                secret: false,
            });
        }
        out.answers.insert(i, String::new());
        let why = unsettled_why(
            &format!("your answer to: {}", q.question),
            a,
            &r.reply,
            true,
        );
        hold_once(&mut out, std::slice::from_ref(&q.step), &why);
    }
    out
}

/// The settled check with the read answers' holds (`withReadHolds`): each replaces Qanuvi's default hold of the same
/// step; one the owner set stays theirs with that need added; a step held this way is no longer cleared.
pub fn with_read_holds(settled: &SettledCheck, holds: &[Hold]) -> SettledCheck {
    // The last one of a step counts, as a JavaScript Map built from the list keeps it.
    let mine = |step: &str| {
        holds
            .iter()
            .rev()
            .find(|h| h.step == step)
            .map(|h| h.why.clone())
    };
    let kept: Vec<Hold> = settled
        .holds
        .iter()
        .filter(|h| h.by == HoldBy::You || mine(&h.step).is_none())
        .map(|h| match mine(&h.step) {
            Some(why) => Hold {
                why: format!("{}; also {why}", h.why),
                ..h.clone()
            },
            None => h.clone(),
        })
        .collect();
    let mut all = kept.clone();
    for h in holds {
        if !kept.iter().any(|k| k.step == h.step) {
            all.push(h.clone());
        }
    }
    SettledCheck {
        holds: all,
        cleared: settled
            .cleared
            .iter()
            .filter(|s| mine(s).is_none())
            .cloned()
            .collect(),
        ..settled.clone()
    }
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
                memory: vec![],
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
    fn a_placeholder_in_place_of_a_question_is_never_asked() {
        let r = parse_preflight(
            r#"{"questions":[{"step":"2","question":"<<ccr:0bd3c8834676,string,348B>>","sort":"you"},
                {"step":"2","question":"What if the connection fails?","sort":"decide","recommended":"<<ccr:ab12,string,9B>>"}]}"#,
        );
        assert_eq!(r.questions.len(), 1);
        assert_eq!(r.questions[0].question, "What if the connection fails?");
        assert_eq!(r.questions[0].recommended, "");
        assert!(!is_placeholder("<<the owner's words>>"));
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

    #[test]
    fn ends_a_long_reason_at_a_word_with_an_ellipsis() {
        let long = "word ".repeat(80).trim().to_string();
        let c = clip_reason(&long, 300);
        assert!(js::len(&c) <= 300);
        assert!(c.ends_with("word…"));
        assert_eq!(clip_reason("short", 300), "short");
        assert_eq!(
            clip_reason(&"x".repeat(400), 300),
            format!("{}…", "x".repeat(299))
        );
    }
}
