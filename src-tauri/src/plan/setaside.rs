//! Runs that keep going (24u part 1), ported from `src/lib/plan/setaside.ts`: a step that waits for the owner is set
//! aside, and so are the steps that need it; the run goes on with the rest and waits, spending nothing, when only
//! set-aside steps are left.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepStep {
    pub number: Option<String>,
    pub title: String,
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Files the step changed so far (a failed or blocked step may have started).
    #[serde(default)]
    pub changed: Vec<String>,
}

/// Why a step waits for the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeldKind {
    Hold,
    Blocked,
    Failed,
}

/// A section's closing step (such as "95z"): it never waits for others, it does what it can and writes what is open.
pub fn is_closing_step(number: Option<&str>) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\d+z$").unwrap());
    number.is_some_and(|n| !n.is_empty() && RE.is_match(n.trim()))
}

/// File names a text mentions (lower case, forward slashes). Plan notes (.md) are left out: every step names them.
pub fn mentioned_files(text: &str) -> Vec<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:[A-Za-z0-9_.-]+[/\\])*[A-Za-z0-9_-]+\.(?:php|ts|tsx|js|jsx|mjs|svelte|vue|rs|css|scss|html|sql|py|go|java|kt|json|toml|ya?ml|sh|ps1)(?-u:\b)").unwrap()
    });
    let mut out: Vec<String> = vec![];
    for m in RE.find_iter(text) {
        let f = m.as_str().replace('\\', "/").to_lowercase();
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn base(p: &str) -> &str {
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// `parseInt(s, 10)` for a step number such as "95b".
fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Whether the plan itself says `later` needs `held`: its Needs line, the same step number (95b after 95a), or a
/// file both touch. None when the plan does not say; the decider is asked then.
pub fn plan_dependence(held: &DepStep, later: &DepStep) -> Option<String> {
    static SUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\d+[a-y]$").unwrap());
    let h = held
        .number
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(h) = h
        && later.needs.iter().any(|n| n.trim() == h)
    {
        return Some(format!("its Needs line names step {h}"));
    }
    let l = later
        .number
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let (Some(h), Some(l)) = (h, l)
        && h != l
        && SUB.is_match(h)
        && SUB.is_match(l)
        && leading_number(h) == leading_number(l)
    {
        return Some(format!(
            "same step number ({})",
            leading_number(h).unwrap_or(0)
        ));
    }
    let text = |s: &DepStep| {
        let mut v = vec![s.title.clone()];
        v.extend(s.details.iter().cloned());
        v.join("\n")
    };
    let held_files: Vec<String> = mentioned_files(&text(held))
        .into_iter()
        .chain(held.changed.iter().cloned())
        .map(|f| f.replace('\\', "/").to_lowercase())
        .collect();
    for f in mentioned_files(&text(later)) {
        let hit = held_files.iter().find(|g| {
            **g == f
                || g.ends_with(&format!("/{f}"))
                || f.ends_with(&format!("/{g}"))
                || base(g) == base(&f)
        });
        if let Some(hit) = hit {
            return Some(format!(
                "both work on {}",
                if f.contains('/') { &f } else { hit }
            ));
        }
    }
    None
}

fn label(s: &DepStep) -> String {
    match s.number.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => format!("{n}. {}", s.title),
        None => s.title.clone(),
    }
}

/// A set-aside step and why it waits, for the decider's question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldStep {
    #[serde(flatten)]
    pub step: DepStep,
    pub why: String,
}

/// One short question to the decider: which of these open steps need one of the set-aside steps?
pub fn dependence_prompt(held: &[HeldStep], open: &[DepStep]) -> String {
    let mut out: Vec<String> =
        vec!["A plan run set these steps aside (they wait for the owner):".into()];
    out.extend(
        held.iter()
            .map(|s| format!("- {} ({})", label(&s.step), s.why)),
    );
    out.push(String::new());
    out.push("Which of these open steps cannot be done properly before one of them is done? A step only needs another when it".into());
    out.push(
        "uses what that step makes or changes; working on the same project is not enough.".into(),
    );
    out.extend(open.iter().map(|s| {
        let details = if s.details.is_empty() {
            String::new()
        } else {
            let d: Vec<&str> = s.details.iter().take(3).map(String::as_str).collect();
            format!(": {}", super::js::cut(&d.join(" "), 300))
        };
        format!("- {}{details}", label(s))
    }));
    out.push(String::new());
    out.push(r#"Answer with JSON only: {"needs":[{"step":"72a","on":"95b","why":"..."}]} (an empty list when none do)."#.into());
    out.join("\n")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub on: String,
    pub why: String,
}

/// Reads the decider's answer: open step number to the set-aside step it needs and why, in the answer's order.
/// Unreadable: none.
pub fn parse_dependence(text: &str, open: &[String], held: &[String]) -> Vec<(String, Need)> {
    let mut out: Vec<(String, Need)> = vec![];
    let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) else {
        return out;
    };
    if end <= start {
        return out;
    }
    let Ok(raw) = serde_json::from_str::<Value>(&text[start..=end]) else {
        return out;
    };
    let Some(needs) = raw.get("needs").and_then(Value::as_array) else {
        return out;
    };
    for n in needs {
        let s = |k: &str| {
            n.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let (step, on) = (s("step"), s("on"));
        let why = super::js::cut(&s("why"), 200).to_string();
        if open.contains(&step) && held.contains(&on) && !out.iter().any(|(k, _)| *k == step) {
            out.push((step, Need { on, why }));
        }
    }
    out
}

/// The line the run shows while only set-aside steps are left.
pub fn waiting_line(answers: usize) -> String {
    format!(
        "Waiting for you ({answers} answer{}). Nothing is spent meanwhile; the run goes on by itself once you answer.",
        if answers == 1 { "" } else { "s" }
    )
}

/// What a closing step that runs again is told: these steps were set aside when it ran and are done now.
pub fn closing_rerun_note(late: &[String]) -> String {
    let one = late.len() == 1;
    [
        String::new(),
        String::new(),
        format!(
            "You already ran once in this run while {} {} {} set aside; {} done now.",
            if one { "step" } else { "steps" },
            late.join(", "),
            if one { "was" } else { "were" },
            if one { "it is" } else { "they are" }
        ),
        "Qanuvi opened this step again in the plan on purpose (it shows [ ] instead of [x]); mark it done again when".into(),
        "finished. Bring your step's work up to date with them (and take them off your log's still-open list); do not".into(),
        "redo the rest.".into(),
    ]
    .join("\n")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsideItem {
    pub label: String,
    pub why: String,
}

/// What a section's closing step is told about the steps set aside in this run.
pub fn set_aside_block(aside: &[AsideItem]) -> String {
    if aside.is_empty() {
        return String::new();
    }
    let list: Vec<String> = aside
        .iter()
        .map(|a| format!("{} ({})", a.label, a.why))
        .collect();
    [
        String::new(),
        String::new(),
        format!("Set aside in this run (they wait for the owner): {}.", list.join("; ")),
        "Do what you can without them, write in your log what is still open because of them, and mark this step done".into(),
        "([x]) for what you did: do not mark it blocked for them. Qanuvi runs this step again once they are done.".into(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(number: &str, title: &str) -> DepStep {
        DepStep {
            number: Some(number.into()),
            title: title.into(),
            ..Default::default()
        }
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn knows_a_sections_closing_step() {
        assert!(is_closing_step(Some("95z")));
        assert!(!is_closing_step(Some("95b")));
        assert!(!is_closing_step(None));
    }

    #[test]
    fn finds_file_names_leaving_plan_notes_out() {
        assert_eq!(
            mentioned_files(r"Change includes\Checkout.php and src/a.ts; see MAP.md"),
            ["includes/checkout.php", "src/a.ts"]
        );
    }

    #[test]
    fn reads_dependence_from_the_plan() {
        let held = DepStep {
            details: strings(&["edit includes/refund.php"]),
            ..step("95b", "Refunds")
        };
        let needs = DepStep {
            needs: strings(&["95b"]),
            ..step("72a", "x")
        };
        assert!(plan_dependence(&held, &needs).unwrap().contains("Needs"));
        assert!(
            plan_dependence(&held, &step("95c", "x"))
                .unwrap()
                .contains("same step number")
        );
        assert!(
            plan_dependence(&held, &step("72a", "Log refund.php errors"))
                .unwrap()
                .contains("refund.php")
        );
        assert_eq!(plan_dependence(&held, &step("72a", "Docs only")), None);
        let changed = DepStep {
            details: vec![],
            changed: strings(&["src/x.ts"]),
            ..held.clone()
        };
        assert!(
            plan_dependence(&changed, &step("1", "fix x.ts"))
                .unwrap()
                .contains("x.ts")
        );
        assert_eq!(plan_dependence(&step("95a", "a"), &step("96a", "b")), None);
    }

    #[test]
    fn asks_the_decider_once_and_reads_only_known_steps() {
        let p = dependence_prompt(
            &[HeldStep {
                step: step("95b", "Refunds"),
                why: "a question".into(),
            }],
            &[step("72a", "Mail")],
        );
        assert!(p.contains("95b. Refunds (a question)"));
        assert!(p.contains("72a. Mail"));
        let m = parse_dependence(
            r#"ok {"needs":[{"step":"72a","on":"95b","why":"mails the refund"},{"step":"99","on":"95b"},{"step":"72a","on":"1"}]}"#,
            &strings(&["72a"]),
            &strings(&["95b"]),
        );
        assert_eq!(
            m,
            [(
                "72a".to_string(),
                Need {
                    on: "95b".into(),
                    why: "mails the refund".into()
                }
            )]
        );
        assert!(parse_dependence("no", &strings(&["72a"]), &strings(&["95b"])).is_empty());
    }

    #[test]
    fn words_the_wait_and_the_closing_steps_note() {
        assert!(waiting_line(1).starts_with("Waiting for you (1 answer)"));
        assert!(waiting_line(3).contains("3 answers"));
        assert_eq!(set_aside_block(&[]), "");
        let b = set_aside_block(&[AsideItem {
            label: "95b".into(),
            why: "waits for you".into(),
        }]);
        assert!(b.contains("95b (waits for you)"));
        assert!(
            closing_rerun_note(&strings(&["1a"])).contains("step 1a was set aside; it is done now")
        );
        assert!(
            closing_rerun_note(&strings(&["1a", "1b"]))
                .contains("steps 1a, 1b were set aside; they are done now")
        );
    }
}
