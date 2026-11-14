//! Learned routing (Milestone 13), ported from `src/lib/plan/routing.ts`: which model a step starts on when the step
//! does not name one.
//!
//! Order: the step's own "- Model:" line > the project's rules > what earlier runs showed > the agent's default.
//! A monthly budget then may move the choice one model cheaper (never for money and security work).

use super::runner::{Outcome, StepRun};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// One of an agent's models (as in `src/lib/agents/providers.ts`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentModel {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// API price in US dollars per million tokens, [input, output], when known (for cost comparisons).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<[f64; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    Docs,
    Tests,
    Ui,
    Security,
    Bugfix,
    Refactor,
    Feature,
}

pub const JOB_TYPES: [JobType; 7] = [
    JobType::Docs,
    JobType::Tests,
    JobType::Ui,
    JobType::Security,
    JobType::Bugfix,
    JobType::Refactor,
    JobType::Feature,
];

impl JobType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Docs => "Docs and wording",
            Self::Tests => "Tests",
            Self::Ui => "Interface and styling",
            Self::Security => "Money and security",
            Self::Bugfix => "Bug fixes",
            Self::Refactor => "Refactoring",
            Self::Feature => "Features",
        }
    }

    /// The name used in run records and the comparison check ("docs", "security", ...).
    pub fn id(self) -> &'static str {
        match self {
            Self::Docs => "docs",
            Self::Tests => "tests",
            Self::Ui => "ui",
            Self::Security => "security",
            Self::Bugfix => "bugfix",
            Self::Refactor => "refactor",
            Self::Feature => "feature",
        }
    }
}

/// `\b` and `\w` as JavaScript has them (ASCII only).
fn rule(body: &str) -> Regex {
    let b = r"(?-u:\b)";
    Regex::new(&format!(
        "(?i){}",
        body.replace(r"\b", b).replace(r"\w", "[A-Za-z0-9_]")
    ))
    .unwrap()
}

static RULES: LazyLock<Vec<(JobType, Regex)>> = LazyLock::new(|| {
    vec![
        (
            JobType::Security,
            rule(
                r"\b(payments?|pay|prices?|pricing|money|refunds?|wallets?|gift ?cards?|coupons?|discounts?|checkout|invoices?|tax(es)?|vat|auth\w*|log ?in|passwords?|tokens?|secrets?|permissions?|security|encrypt\w*|csrf|xss|sql injection|nonces?)\b",
            ),
        ),
        (
            JobType::Docs,
            rule(
                r"\b(readme|docs?|documentation|documents?|wording|typos?|comments?|changelog|guides?)\b|\.md\b",
            ),
        ),
        (
            JobType::Tests,
            rule(r"\b(tests?|specs?|coverage|e2e|playwright|vitest|phpunit|pytest|jest)\b"),
        ),
        (
            JobType::Ui,
            rule(
                r"\b(ui|css|styles?|styling|restyle|layouts?|buttons?|modals?|components?|svelte|vue|react|themes?|templates?|responsive|icons?)\b",
            ),
        ),
        (
            JobType::Bugfix,
            rule(
                r"\b(fix(es|ed)?|bugs?|broken|crash(es)?|errors?|regressions?|wrong|fails?|failing)\b",
            ),
        ),
        (
            JobType::Refactor,
            rule(
                r"\b(refactor\w*|rename\w*|move|moving|clean ?up|simplif\w*|split|extract\w*|reorganis\w*|reorganiz\w*)\b",
            ),
        ),
    ]
});

/// The kind of work a step is, from its title and details. Money and security win over everything else.
pub fn job_type(text: &str) -> JobType {
    RULES
        .iter()
        .find(|(_, re)| re.is_match(text))
        .map_or(JobType::Feature, |(t, _)| *t)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStats {
    pub tries: u32,
    pub successes: u32,
    pub cost_usd: f64,
    /// Tokens read and written over all tries (what a model burns, apart from its price).
    #[serde(default)]
    pub tokens: f64,
}

/// Per "type|model": how many tries, how many succeeded, what they cost — from saved runs.
pub type Stats = HashMap<String, ModelStats>;

fn key(t: JobType, model: &str) -> String {
    format!("{}|{model}", t.id())
}

/// Per job type and model: tries, successes and cost, from the steps of saved runs.
pub fn learn<'a>(
    runs: impl IntoIterator<Item = &'a [StepRun]>,
    type_of: impl Fn(&str) -> JobType,
) -> Stats {
    let mut stats = Stats::new();
    for steps in runs {
        for step in steps {
            let t = step.job_type.unwrap_or_else(|| type_of(&step.title));
            for a in &step.attempts {
                if a.outcome == Outcome::Stopped {
                    continue; // the user stopped it: says nothing about the model
                }
                let s = stats.entry(key(t, &a.model)).or_default();
                s.tries += 1;
                if a.outcome == Outcome::Done {
                    s.successes += 1;
                }
                s.cost_usd += a.cost_usd;
                s.tokens += a.input_tokens + a.output_tokens;
            }
        }
    }
    stats
}

pub const MIN_TRIES: u32 = 3;
pub const MIN_SUCCESS: f64 = 0.8;

/// How sure the record makes us that a model finishes this kind of work: the lower end of the 95% range around its
/// success rate (Wilson). Many good tries count for more than a few: 33 of 34 is surer than 5 of 5.
pub fn reliability(tries: u32, successes: u32) -> f64 {
    if tries == 0 {
        return 0.0;
    }
    let z = 1.96;
    let n = tries as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    (p + z2 / (2.0 * n) - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt()) / (1.0 + z2 / n)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Learned {
    pub model: String,
    pub sure: f64,
    pub per_success: f64,
    pub s: ModelStats,
}

fn qualifies(s: &ModelStats) -> bool {
    s.tries >= MIN_TRIES && s.successes as f64 / s.tries as f64 >= MIN_SUCCESS
}

/// The most reliable model on this kind of job (quality first: code quality before saving), among those that
/// succeeded at least 80% of the time with at least three tries; cost per success only breaks a tie.
pub fn learned_model(stats: &Stats, t: JobType, models: &[AgentModel]) -> Option<Learned> {
    let mut best: Option<Learned> = None;
    for m in models {
        let Some(s) = stats.get(&key(t, &m.id)).filter(|s| qualifies(s)) else {
            continue;
        };
        let sure = reliability(s.tries, s.successes);
        let per_success = s.cost_usd / (s.successes.max(1) as f64);
        let better = best.as_ref().is_none_or(|b| {
            sure > b.sure + 1e-9 || ((sure - b.sure).abs() <= 1e-9 && per_success < b.per_success)
        });
        if better {
            best = Some(Learned {
                model: m.id.clone(),
                sure,
                per_success,
                s: *s,
            });
        }
    }
    best
}

/// One model's record on one kind of work: what the decider weighs when it picks models.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    pub model: String,
    pub tries: u32,
    pub successes: u32,
    pub tokens_per_success: f64,
    pub cost_per_success: f64,
    /// Burns far more tokens than the lightest model that also finishes this kind of work (1.5x the tokens per
    /// finished step, with at least three tries): picked only when a step really needs it. Tokens, not price.
    pub heavy: bool,
}

pub const HEAVY_FACTOR: f64 = 1.5;

/// Per model, for one kind of work: tries, successes, tokens and cost per finished step, and whether it is heavy.
pub fn model_profiles(stats: &Stats, t: JobType, models: &[AgentModel]) -> Vec<ModelProfile> {
    let mut out: Vec<ModelProfile> = models
        .iter()
        .filter_map(|m| {
            let s = stats.get(&key(t, &m.id)).filter(|s| s.tries > 0)?;
            let done = s.successes.max(1) as f64;
            Some(ModelProfile {
                model: m.id.clone(),
                tries: s.tries,
                successes: s.successes,
                tokens_per_success: super::js::round(s.tokens / done),
                cost_per_success: s.cost_usd / done,
                heavy: false,
            })
        })
        .collect();
    // Compared only with models that do finish this kind of work (a model that always fails is not "light").
    let min_tokens = out
        .iter()
        .filter(|p| p.tries >= MIN_TRIES && p.successes as f64 / p.tries as f64 >= MIN_SUCCESS)
        .map(|p| p.tokens_per_success)
        .filter(|t| *t > 0.0)
        .fold(f64::INFINITY, f64::min);
    if min_tokens.is_finite() {
        for p in out.iter_mut().filter(|p| p.tries >= MIN_TRIES) {
            p.heavy = p.tokens_per_success >= min_tokens * HEAVY_FACTOR;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    /// Monthly limit in US dollars; 0 means none.
    pub monthly_usd: f64,
    /// Spent so far this month by all agent sessions.
    pub spent_usd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetState {
    Ok,
    /// 80% or more used: prefer cheaper models.
    Tight,
    /// Stop and ask.
    Over,
}

pub fn budget_state(b: Budget) -> BudgetState {
    if b.monthly_usd <= 0.0 {
        BudgetState::Ok
    } else if b.spent_usd >= b.monthly_usd {
        BudgetState::Over
    } else if b.spent_usd >= b.monthly_usd * 0.8 {
        BudgetState::Tight
    } else {
        BudgetState::Ok
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub model: String,
    /// Why, in a few words, for the step's chip and the run record.
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Picked {
    pub model: String,
    #[serde(default)]
    pub why: String,
}

pub struct ChooseOptions<'a> {
    pub step_model: Option<&'a str>,
    pub job: JobType,
    pub rules: &'a HashMap<JobType, String>,
    pub stats: &'a Stats,
    /// Strongest first.
    pub models: &'a [AgentModel],
    pub default_model: &'a str,
    pub budget: Budget,
    /// The decider's pick for this step (it read the step and the token record), when it made one.
    pub picked: Option<&'a Picked>,
}

/// The model a step starts on, and why.
pub fn choose_model(o: &ChooseOptions) -> Choice {
    if let Some(m) = o.step_model.map(super::js::trim).filter(|m| !m.is_empty()) {
        return Choice {
            model: m.into(),
            reason: "set on the step".into(),
        };
    }
    let label = o.job.label().to_lowercase();
    let rule = o.rules.get(&o.job).filter(|r| !r.is_empty());
    let choice = if let Some(r) = rule {
        Choice {
            model: r.clone(),
            reason: format!("project rule for {label}"),
        }
    } else if let Some(p) = o
        .picked
        .filter(|p| o.models.iter().any(|m| m.id == p.model))
    {
        let why = if p.why.is_empty() {
            "picked for this step"
        } else {
            &p.why
        };
        Choice {
            model: p.model.clone(),
            reason: format!("decider: {why}"),
        }
    } else if let Some(l) = learned_model(o.stats, o.job, o.models) {
        Choice {
            model: l.model,
            reason: format!(
                "learned: most reliable on {label} ({}/{} done)",
                l.s.successes, l.s.tries
            ),
        }
    } else {
        Choice {
            model: o.default_model.into(),
            reason: "default model".into(),
        }
    };

    if budget_state(o.budget) == BudgetState::Tight
        && o.job != JobType::Security
        && let Some(i) = o.models.iter().position(|m| m.id == choice.model)
        && i + 1 < o.models.len()
    {
        return Choice {
            model: o.models[i + 1].id.clone(),
            reason: format!("{}; one cheaper (monthly budget 80% used)", choice.reason),
        };
    }
    choice
}

/// Start of the current month (seconds since 1970, local time), for "spent this month".
pub fn month_start(now: &jiff::Zoned) -> i64 {
    now.date()
        .first_of_month()
        .to_zoned(now.time_zone().clone())
        .map_or(0, |z| z.timestamp().as_second())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::runner::Attempt;

    const H: &str = "claude-haiku-5-5";
    const S: &str = "claude-sonnet-5-5";
    const F: &str = "claude-fable-5-2";

    fn models() -> Vec<AgentModel> {
        crate::plan::prompts::CLAUDE_CODE_MODELS
            .iter()
            .map(|id| AgentModel {
                id: id.to_string(),
                label: String::new(),
                price: None,
            })
            .collect()
    }

    #[test]
    fn sorts_steps_by_the_kind_of_work() {
        for (text, t) in [
            ("Update the README with install steps", JobType::Docs),
            ("Add tests for the cart", JobType::Tests),
            ("Restyle the layout of the settings modal", JobType::Ui),
            ("Fix the crash when the list is empty", JobType::Bugfix),
            ("Rename CartService to Cart and move it", JobType::Refactor),
            ("Add a wishlist page", JobType::Feature),
            ("Round discounted prices", JobType::Security),
            ("Coupons", JobType::Security),
            ("Update the docs and comments", JobType::Docs),
            ("Restyle the buttons", JobType::Ui),
            ("Fixes for two crashes", JobType::Bugfix),
            ("Fix the refund docs and add tests", JobType::Security),
            ("Checkout: pay what you want", JobType::Security),
        ] {
            assert_eq!(job_type(text), t, "{text}");
        }
    }

    fn attempt(model: &str, outcome: Outcome, cost: f64) -> Attempt {
        serde_json::from_value(serde_json::json!({
            "model": model, "outcome": outcome, "costUsd": cost, "sessionId": "", "inputTokens": 0,
            "outputTokens": 0, "seconds": 0, "note": ""
        }))
        .unwrap()
    }

    fn step(title: &str, attempts: Vec<Attempt>) -> StepRun {
        let mut s: StepRun = serde_json::from_value(serde_json::json!({
            "line": 1, "number": "1", "title": title, "status": "done", "attempts": []
        }))
        .unwrap();
        s.attempts = attempts;
        s
    }

    fn runs() -> Vec<StepRun> {
        vec![
            step("Update README", vec![attempt(H, Outcome::Done, 0.01)]),
            step("Docs for setup", vec![attempt(H, Outcome::Done, 0.012)]),
            step("Fix typo in docs", vec![attempt(H, Outcome::Done, 0.008)]),
            step(
                "Add a wishlist page",
                vec![
                    attempt(H, Outcome::Open, 0.01),
                    attempt(S, Outcome::Done, 0.06),
                    attempt(H, Outcome::Stopped, 0.01),
                ],
            ),
        ]
    }

    #[test]
    fn learns_and_chooses() {
        let r = runs();
        let stats = learn([r.as_slice()], job_type);
        let docs = stats[&format!("docs|{H}")];
        assert_eq!((docs.tries, docs.successes), (3, 3));
        assert!((docs.cost_usd - 0.03).abs() < 1e-9);
        let f = stats[&format!("feature|{H}")];
        assert_eq!((f.tries, f.successes, f.cost_usd), (1, 0, 0.01));
        let m = models();
        assert_eq!(learned_model(&stats, JobType::Docs, &m).unwrap().model, H);
        assert!(learned_model(&stats, JobType::Feature, &m).is_none());

        let none = HashMap::new();
        let base = |job, step_model, rules, budget| ChooseOptions {
            step_model,
            job,
            rules,
            stats: &stats,
            models: &m,
            default_model: S,
            budget,
            picked: None,
        };
        let free = Budget::default();
        assert_eq!(
            choose_model(&base(JobType::Docs, Some(F), &none, free)),
            Choice {
                model: F.into(),
                reason: "set on the step".into()
            }
        );
        let rules = HashMap::from([(JobType::Docs, S.to_string())]);
        assert_eq!(
            choose_model(&base(JobType::Docs, None, &rules, free)).reason,
            "project rule for docs and wording"
        );
        assert_eq!(
            choose_model(&base(JobType::Docs, None, &none, free)).reason,
            "learned: most reliable on docs and wording (3/3 done)"
        );
        assert_eq!(
            choose_model(&base(JobType::Feature, None, &none, free)).reason,
            "default model"
        );
        let tight = Budget {
            monthly_usd: 10.0,
            spent_usd: 9.0,
        };
        assert_eq!(
            choose_model(&base(JobType::Feature, None, &none, tight)).model,
            H
        );
        assert_eq!(
            choose_model(&base(JobType::Security, None, &none, tight)).model,
            S
        );
        assert_eq!(
            choose_model(&base(JobType::Feature, Some(F), &none, tight)).model,
            F
        );
    }

    #[test]
    fn budget_states() {
        let b = |m, s| {
            budget_state(Budget {
                monthly_usd: m,
                spent_usd: s,
            })
        };
        assert_eq!(b(0.0, 999.0), BudgetState::Ok);
        assert_eq!(b(10.0, 7.9), BudgetState::Ok);
        assert_eq!(b(10.0, 8.0), BudgetState::Tight);
        assert_eq!(b(10.0, 10.0), BudgetState::Over);
    }

    #[test]
    fn month_start_is_the_first_moment_of_the_month() {
        let tz = jiff::tz::TimeZone::get("Asia/Dhaka").unwrap();
        let now = jiff::civil::date(2026, 11, 10)
            .at(15, 30, 0, 0)
            .to_zoned(tz.clone())
            .unwrap();
        let start = jiff::Timestamp::from_second(month_start(&now))
            .unwrap()
            .to_zoned(tz);
        assert_eq!(
            (start.year(), start.month(), start.day(), start.hour()),
            (2026, 11, 1, 0)
        );
    }
}
