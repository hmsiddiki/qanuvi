//! The finishing pass at the end of a plan run (24o), ported from `src/lib/plan/finishing.ts`. The steps followed
//! their own instructions; one smarter pass then looks at the whole. Light: the project's full checks and a review of
//! the whole run's diff. Full: also a session that fixes what was found, runs live tests where the project defines
//! them (scratch copies only) and checks the task list's Done notes against what was built. Both end with a short
//! "still open" list.

use super::js;
use super::runner::{RunStatus, RunStepStatus, StepRun};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishLevel {
    Light,
    Full,
}

/// What a run does at its end: offer the pass, or run one by itself (a plan's run setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishAuto {
    Ask,
    Light,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishStatus {
    Running,
    Done,
    Failed,
    Stopped,
}

/// The project's full checks, when it has a test command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checks {
    pub command: String,
    pub ok: bool,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishPass {
    pub level: FinishLevel,
    pub status: FinishStatus,
    /// The decider's model the pass ran on.
    pub model: String,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub cost_usd: f64,
    pub seconds: f64,
    pub checks: Option<Checks>,
    /// What the review found (light), or what the full pass found and did.
    #[serde(default)]
    pub findings: Vec<String>,
    /// Left for the owner.
    #[serde(default)]
    pub still_open: Vec<String>,
    /// The full pass's session (to open it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The key the pass's cost has in the run's lines per model.
pub const FINISHER: &str = "finish:";

fn median(xs: &[f64]) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let m = s.len() / 2;
    if s.len() % 2 == 1 {
        s[m]
    } else {
        (s[m - 1] + s[m]) / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub cost_usd: Option<f64>,
    pub seconds: Option<f64>,
}

/// About what the pass costs and how long it takes, from the run's own finished steps: a light pass is about half a
/// typical step, a full one about 1.5 typical steps, more for long runs. Cost is None when no finished step was paid
/// for (free steps tell nothing about the paid decider's price).
pub fn finish_estimate(steps: &[StepRun], level: FinishLevel) -> Estimate {
    let done: Vec<(f64, f64)> = steps
        .iter()
        .filter(|s| s.status == RunStepStatus::Done && !s.attempts.is_empty())
        .map(|s| {
            s.attempts
                .iter()
                .fold((0.0, 0.0), |(c, t), a| (c + a.cost_usd, t + a.seconds))
        })
        .collect();
    if done.is_empty() {
        return Estimate {
            cost_usd: None,
            seconds: None,
        };
    }
    let paid: Vec<f64> = done.iter().map(|d| d.0).filter(|c| *c > 0.0).collect();
    let factor = match level {
        FinishLevel::Light => 0.5,
        FinishLevel::Full => (1.5 + 0.1 * done.len() as f64).min(4.0),
    };
    Estimate {
        cost_usd: (!paid.is_empty()).then(|| median(&paid) * factor),
        seconds: Some(median(&done.iter().map(|d| d.1).collect::<Vec<_>>()) * factor),
    }
}

/// Whether a finished run gets the offer: it ended done, some step got done in it, and no pass ran yet.
pub fn offers_finish(status: RunStatus, steps: &[StepRun], finish: Option<&FinishPass>) -> bool {
    status == RunStatus::Done
        && finish.is_none()
        && steps
            .iter()
            .any(|s| s.status == RunStepStatus::Done && !s.attempts.is_empty())
}

const DIFF_LIMIT: usize = 40_000;

/// A step the run finished, with the Done notes it left in the task list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DoneBrief {
    pub number: Option<String>,
    pub title: String,
    pub done: Vec<String>,
}

fn brief_lines(steps: &[DoneBrief]) -> Vec<String> {
    steps
        .iter()
        .flat_map(|s| {
            let n = s
                .number
                .as_ref()
                .map(|n| format!("{n}. "))
                .unwrap_or_default();
            std::iter::once(format!("- {n}{}", s.title)).chain(
                s.done
                    .iter()
                    .take(4)
                    .map(|d| format!("    {}", js::cut(d, 400))),
            )
        })
        .collect()
}

fn checks_line(checks: Option<&Checks>, label: &str, none: &str) -> String {
    match checks {
        Some(c) => format!(
            "The project's full checks ({}){label}: {}",
            c.command,
            if c.ok {
                "passed".to_string()
            } else {
                format!("FAILED\n{}", c.summary)
            }
        ),
        None => none.into(),
    }
}

/// The light pass: one review of the whole run's diff, with the checks' result.
pub fn finish_review_prompt(
    file: &str,
    steps: &[DoneBrief],
    diff: &str,
    checks: Option<&Checks>,
) -> String {
    let shown = if js::len(diff) > DIFF_LIMIT {
        format!("{}\n… (cut: the diff is longer)", js::cut(diff, DIFF_LIMIT))
    } else {
        diff.to_string()
    };
    let mut lines: Vec<String> = vec![
        format!("A plan run in this project just finished the steps below of {file}. Each step was checked on its own; you look"),
        "at the whole once, before the run is called done. Look for:".into(),
        "- leftovers: debug output, commented-out code, temporary files, TODOs the steps added and did not resolve;".into(),
        "- steps whose changes do not fit together (one changes what another relies on, names or formats that disagree);".into(),
        "- a Done note that claims more than the diff shows;".into(),
        "- bugs a reviewer of the whole would catch. Style alone is no finding.".into(),
        r#"Answer JSON only: {"ok": true or false, "findings": ["file: what is wrong, and the fix"], "stillOpen": ["what"#.into(),
        r#"the owner still has to do or decide"]}. Empty lists when there is nothing. Short, concrete items."#.into(),
        String::new(),
        "Steps done in this run:".into(),
    ];
    lines.extend(brief_lines(steps));
    lines.push(String::new());
    lines.push(checks_line(
        checks,
        "",
        "The project has no test command: no checks were run.",
    ));
    lines.push(String::new());
    lines.push("The run's changes (git diff; the task list and its logs left out):".into());
    lines.push(if shown.is_empty() {
        "(no change)".into()
    } else {
        shown
    });
    lines.join("\n")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishReview {
    pub ok: bool,
    pub findings: Vec<String>,
    pub still_open: Vec<String>,
}

/// Reads the review. Anything unreadable is a finding, never a quiet pass.
pub fn parse_finish_review(answer: &str) -> FinishReview {
    let v = js::json_in(answer);
    let list = |x: Option<&Value>| -> Vec<String> {
        match x {
            Some(Value::Array(a)) => a
                .iter()
                .map(|i| js::trim(&js::string(Some(i))).to_string())
                .filter(|i| !i.is_empty())
                .take(20)
                .map(|i| js::cut(&i, 400).to_string())
                .collect(),
            _ => vec![],
        }
    };
    match v
        .as_ref()
        .and_then(|v| v.get("ok"))
        .and_then(Value::as_bool)
    {
        Some(ok) => FinishReview {
            ok,
            findings: list(v.as_ref().and_then(|v| v.get("findings"))),
            still_open: list(v.as_ref().and_then(|v| v.get("stillOpen"))),
        },
        None => FinishReview {
            ok: false,
            findings: vec![
                "The review's answer could not be read; its reply is in the agent pane.".into(),
            ],
            still_open: vec![],
        },
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFound {
    pub findings: Vec<String>,
    pub still_open: Vec<String>,
}

/// The full pass: a session that fixes what the review found and checks the rest.
pub fn finish_full_prompt(
    file: &str,
    steps: &[DoneBrief],
    review: &ReviewFound,
    checks: Option<&Checks>,
    diff_files: &[String],
) -> String {
    let mut lines: Vec<String> = vec![
        format!("A plan run in this project just finished the steps below of {file}. Each step was checked on its own. You do the"),
        "finishing pass over the whole run before it is called done:".into(),
        "1. Fix what the review below found, where it is right (say so where it is not). Keep each fix small.".into(),
        "2. Remove leftovers the steps added: debug output, commented-out code, temporary files.".into(),
        format!("3. Check each Done note below against the code: what a note claims must be there. Correct a note in {file} that"),
        "   says more than was built; do not reopen or re-mark steps.".into(),
        "4. Live tests: only where the project or the plan defines them (a live-test folder, script or a plan rule), and".into(),
        "   only on a scratch copy or the test environment the plan names; never on the owner's real folders, data or live".into(),
        "   sites. None defined: say so, and do not invent any.".into(),
        "5. Run the project's full checks at the end.".into(),
        "End your last message with a section headed exactly `Still open:` and one `- ` line per thing the owner still has".into(),
        "to do or decide (`- none` when nothing is left).".into(),
        String::new(),
        "Steps done in this run:".into(),
    ];
    lines.extend(brief_lines(steps));
    lines.push(String::new());
    lines.push("Files the run changed:".into());
    if diff_files.is_empty() {
        lines.push("- (none found)".into());
    } else {
        lines.extend(diff_files.iter().take(60).map(|f| format!("- {f}")));
    }
    lines.push(String::new());
    lines.push(checks_line(
        checks,
        " before this pass",
        "The project has no test command set.",
    ));
    lines.push(String::new());
    lines.push("The review of the whole diff found:".into());
    if review.findings.is_empty() {
        lines.push("- nothing".into());
    } else {
        lines.extend(review.findings.iter().map(|f| format!("- {f}")));
    }
    if !review.still_open.is_empty() {
        lines.push(String::new());
        lines.push("It left open:".into());
        lines.extend(review.still_open.iter().map(|f| format!("- {f}")));
    }
    lines.join("\n")
}

/// The `Still open:` list at the end of the full pass's answer; None when the answer has none.
pub fn still_open_in(text: &str) -> Option<Vec<String>> {
    static HEAD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(?:#+\s*)?\**\s*still open\s*:?\s*\**\s*:?\s*(.*)$").unwrap()
    });
    static INLINE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)still open\s*:?\s*\**\s*:?\s*(.*)$").unwrap());
    static ITEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[-*•]|[0-9]+[.)])\s+(.*)$").unwrap());
    static NONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^none\.?$").unwrap());
    let (lines, _) = super::tasklist::split_lines(text);
    let at = lines.iter().rposition(|l| HEAD.is_match(l))?;
    let inline = INLINE
        .captures(&lines[at])
        .and_then(|c| c.get(1))
        .map_or(String::new(), |m| js::trim(m.as_str()).to_string());
    let mut items: Vec<String> = vec![];
    if !inline.is_empty() && !NONE.is_match(&inline) {
        items.push(inline);
    }
    for l in &lines[at + 1..] {
        let Some(m) = ITEM.captures(l) else {
            if !js::trim(l).is_empty() && !items.is_empty() {
                break;
            }
            continue;
        };
        let item = js::trim(&m[1]);
        if NONE.is_match(item) {
            continue;
        }
        items.push(js::cut(item, 400).to_string());
    }
    items.truncate(20);
    Some(items)
}

/// Done notes under a step's line ("- Done: ...").
pub fn done_notes(details: &[String]) -> Vec<String> {
    static DONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^- Done:").unwrap());
    details
        .iter()
        .map(|d| js::trim(d).to_string())
        .filter(|d| DONE.is_match(d))
        .collect()
}

/// Plan files and their logs are no part of the run's code changes.
pub fn is_plan_file(path: &str, plan_file: &str, log_dir_path: &str) -> bool {
    static MD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.md$").unwrap());
    let p = path.replace('\\', "/");
    let plan = plan_file.replace('\\', "/");
    let archive = MD.replace(&plan, ".archive.md");
    p == plan
        || p.ends_with(&format!("/{plan}"))
        || p == archive
        || p.starts_with(&format!("{log_dir_path}/"))
}

/// The pass as an entry for the plan's FINISH.md log.
pub fn finish_entry(f: &FinishPass) -> String {
    let when = jiff::Timestamp::from_millisecond(f.started_at as i64)
        .map(|t| t.strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let level = match f.level {
        FinishLevel::Light => "light",
        FinishLevel::Full => "full",
    };
    let status = match f.status {
        FinishStatus::Running => "running",
        FinishStatus::Done => "done",
        FinishStatus::Failed => "failed",
        FinishStatus::Stopped => "stopped",
    };
    let mut lines = vec![
        format!("## Finishing pass ({level}), {when}: {status}"),
        format!(
            "- Model: {}; cost ${}; {} s",
            f.model,
            js::to_fixed(f.cost_usd, 2),
            js::number(js::round(f.seconds))
        ),
        match &f.checks {
            Some(c) => format!(
                "- Checks ({}): {}",
                c.command,
                if c.ok { "passed" } else { "failed" }
            ),
            None => "- Checks: no test command".into(),
        },
    ];
    if f.findings.is_empty() {
        lines.push("- Found: nothing".into());
    } else {
        lines.push("- Found:".into());
        lines.extend(f.findings.iter().map(|x| format!("  - {x}")));
    }
    if f.still_open.is_empty() {
        lines.push("- Still open: nothing".into());
    } else {
        lines.push("- Still open:".into());
        lines.extend(f.still_open.iter().map(|x| format!("  - {x}")));
    }
    if let Some(n) = f.note.as_ref().filter(|n| !n.is_empty()) {
        lines.push(format!("- Note: {n}"));
    }
    lines.push(String::new());
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::runner::{Attempt, run_totals};
    use serde_json::json;

    fn step(status: &str, attempts: &[(f64, f64, &str)]) -> StepRun {
        let a: Vec<Attempt> = attempts
            .iter()
            .map(|(c, s, o)| {
                serde_json::from_value(
                    json!({"model": "m", "outcome": o, "costUsd": c, "seconds": s}),
                )
                .unwrap()
            })
            .collect();
        let mut s: StepRun = serde_json::from_value(
            json!({"line": 1, "number": "1", "title": "t", "status": status}),
        )
        .unwrap();
        s.attempts = a;
        s
    }

    #[test]
    fn estimates_the_pass() {
        let run = [
            step("done", &[(1.0, 100.0, "done")]),
            step("done", &[(3.0, 300.0, "done")]),
        ];
        assert_eq!(
            finish_estimate(&run, FinishLevel::Light),
            Estimate {
                cost_usd: Some(1.0),
                seconds: Some(100.0)
            }
        );
        let one = [step("done", &[(2.0, 60.0, "done")])];
        assert!((finish_estimate(&one, FinishLevel::Full).cost_usd.unwrap() - 3.2).abs() < 1e-9);
        let long: Vec<StepRun> = (0..40)
            .map(|_| step("done", &[(1.0, 60.0, "done")]))
            .collect();
        assert_eq!(
            finish_estimate(&long, FinishLevel::Full).cost_usd,
            Some(4.0)
        );
        assert_eq!(
            finish_estimate(&[step("done", &[(0.0, 50.0, "done")])], FinishLevel::Light),
            Estimate {
                cost_usd: None,
                seconds: Some(25.0)
            }
        );
        assert_eq!(
            finish_estimate(&[step("failed", &[(1.0, 1.0, "open")])], FinishLevel::Light),
            Estimate {
                cost_usd: None,
                seconds: None
            }
        );
        let done = [step("done", &[(1.0, 1.0, "done")])];
        assert!(offers_finish(RunStatus::Done, &done, None));
        assert!(!offers_finish(RunStatus::Stopped, &done, None));
        assert!(!offers_finish(RunStatus::Done, &[step("done", &[])], None));
    }

    #[test]
    fn prompts_and_answers() {
        let steps = [DoneBrief {
            number: Some("3".into()),
            title: "Add the cart".into(),
            done: vec!["- Done: cart added, tests pass".into()],
        }];
        let checks = Checks {
            command: "npm test".into(),
            ok: false,
            summary: "1 failed".into(),
        };
        let p = finish_review_prompt("PLAN.md", &steps, "diff --git a/x", Some(&checks));
        for part in [
            "3. Add the cart",
            "- Done: cart added",
            "npm test): FAILED\n1 failed",
            "diff --git a/x",
            "\"stillOpen\"",
        ] {
            assert!(p.contains(part), "{part}");
        }
        let p = finish_review_prompt("PLAN.md", &steps, &"x".repeat(50_000), None);
        assert!(p.contains("(cut: the diff is longer)") && p.contains("no test command"));
        let review = ReviewFound {
            findings: vec!["a.ts: debug log left".into()],
            still_open: vec![],
        };
        let p = finish_full_prompt("PLAN.md", &steps, &review, None, &["a.ts".into()]);
        for part in [
            "never on the owner's real folders",
            "`Still open:`",
            "- a.ts: debug log left",
            "- a.ts",
        ] {
            assert!(p.contains(part), "{part}");
        }
        assert_eq!(
            parse_finish_review(
                "```json\n{\"ok\": false, \"findings\": [\"a: b\"], \"stillOpen\": [\"set the key\"]}\n```"
            ),
            FinishReview {
                ok: false,
                findings: vec!["a: b".into()],
                still_open: vec!["set the key".into()]
            }
        );
        let r = parse_finish_review("Looks good to me");
        assert!(!r.ok && r.findings[0].contains("could not be read"));
    }

    #[test]
    fn reads_the_still_open_list() {
        let s = |t: &str| still_open_in(t);
        let v = |x: &[&str]| Some(x.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            s("Fixed two things.\n\nStill open:\n- set the SMTP login\n- decide the price\n"),
            v(&["set the SMTP login", "decide the price"])
        );
        assert_eq!(s("## Still open\n1. one\n2) two"), v(&["one", "two"]));
        assert_eq!(s("**Still open:**\n* x"), v(&["x"]));
        assert_eq!(s("Still open:\n- none"), v(&[]));
        assert_eq!(s("Still open: none"), v(&[]));
        assert_eq!(s("All done."), None);
        assert_eq!(s("Still open:\n- a\n\nThat is all.\n- not this"), v(&["a"]));
    }

    #[test]
    fn helpers() {
        let d = ["  - Model: x", "  - Done: did it", "- done: lower"].map(String::from);
        assert_eq!(done_notes(&d), vec!["- Done: did it", "- done: lower"]);
        let logs = ".qanuvi/plans/docs_PLAN";
        assert!(is_plan_file("docs/PLAN.md", "docs/PLAN.md", logs));
        assert!(is_plan_file("docs/PLAN.archive.md", "docs/PLAN.md", logs));
        assert!(is_plan_file(
            ".qanuvi/plans/docs_PLAN/3.md",
            "docs/PLAN.md",
            logs
        ));
        assert!(!is_plan_file("src/cart.ts", "docs/PLAN.md", logs));
        let f: FinishPass = serde_json::from_value(json!({
            "level": "light", "status": "done", "model": "m", "startedAt": 0, "endedAt": 1000, "costUsd": 0.4,
            "seconds": 61, "checks": {"command": "npm test", "ok": true, "summary": ""}, "findings": ["x"],
            "stillOpen": []
        }))
        .unwrap();
        let e = finish_entry(&f);
        for part in [
            "## Finishing pass (light), 1970-01-01 00:00: done",
            "$0.40",
            "Checks (npm test): passed",
            "  - x",
            "Still open: nothing",
        ] {
            assert!(e.contains(part), "{part}");
        }
        let mut pass = f.clone();
        pass.cost_usd = 0.5;
        pass.seconds = 10.0;
        let t = run_totals(&[step("done", &[(1.0, 5.0, "done")])], &[], Some(&pass));
        assert!((t.cost_usd - 1.5).abs() < 1e-9);
        assert_eq!(t.by_model[1], ("finish:m".to_string(), t.by_model[1].1));
        assert_eq!(t.by_model[1].1.cost_usd, 0.5);
    }
}
