//! Measuring plan runs, ported from `src/lib/plan/costs.ts`: what each attempt spent its tool calls on, and what
//! finished steps cost in this run against the plan's earlier runs.

use super::js;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolKinds {
    /// Reading and searching: finding its way.
    pub read: u32,
    /// Writing and editing files.
    pub change: u32,
    /// Other commands: tests, builds, scripts.
    pub run: u32,
}

/// What an attempt's tool calls were for, from its session lines.
pub fn tool_kinds(lines: &[String]) -> ToolKinds {
    static READ: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(Read|Grep|Glob|LS|mcp__qanuvi__)").unwrap());
    static CHANGE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(Edit|Write|MultiEdit|NotebookEdit)$").unwrap());
    static READING: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(read|find|list|show|check|look|see|search|count|inspect|view|grep|cat|sed|head|tail|ls)(?-u:\b)").unwrap()
    });
    static CD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^cd\s+\S+\s*&&\s*").unwrap());
    let mut k = ToolKinds::default();
    for l in lines {
        if !l.contains("\"tool_use\"") {
            continue;
        }
        let Ok(m) = serde_json::from_str::<Value>(l) else {
            continue;
        };
        if m.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let content = m
            .pointer("/message/content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for b in content {
            let name = b.get("name").and_then(Value::as_str).unwrap_or("");
            if b.get("type").and_then(Value::as_str) != Some("tool_use") || name.is_empty() {
                continue;
            }
            if CHANGE.is_match(name) {
                k.change += 1;
            } else if READ.is_match(name) {
                k.read += 1;
            } else if name == "Bash" || name == "PowerShell" {
                let input = b.get("input");
                let what = input
                    .and_then(|i| i.get("description"))
                    .filter(|v| !v.is_null())
                    .or_else(|| input.and_then(|i| i.get("command")));
                let what = js::string(what);
                let what = CD.replace(&what, "");
                if READING.is_match(what.trim()) {
                    k.read += 1;
                } else {
                    k.run += 1;
                }
            } else {
                k.run += 1;
            }
        }
    }
    k
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Finished {
    pub steps: u32,
    pub cost: f64,
}

/// A run record's finished steps and what they cost.
pub fn finished_cost(data: &Value) -> Finished {
    let mut f = Finished::default();
    for s in data
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let attempts = s.get("attempts").and_then(Value::as_array);
        f.cost += attempts
            .into_iter()
            .flatten()
            .map(|a| a.get("costUsd").and_then(Value::as_f64).unwrap_or(0.0))
            .sum::<f64>();
        if s.get("status").and_then(Value::as_str) == Some("done") {
            f.steps += 1;
        }
    }
    f
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub file: String,
    pub data: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub runs: u32,
    pub steps: u32,
    pub cost: f64,
    pub per_step: Option<f64>,
}

/// This plan's earlier runs: cost per finished step, and the plan's whole spend so far.
pub fn plan_history(records: &[RunRecord], file: &str, except_id: &str) -> History {
    let mut h = History {
        runs: 0,
        steps: 0,
        cost: 0.0,
        per_step: None,
    };
    for r in records {
        if r.file != file || r.id == except_id {
            continue;
        }
        let Ok(d) = serde_json::from_str::<Value>(&r.data) else {
            continue;
        };
        let f = finished_cost(&d);
        h.runs += 1;
        h.steps += f.steps;
        h.cost += f.cost;
    }
    h.per_step = (h.steps > 0).then(|| h.cost / h.steps as f64);
    h
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepLimits {
    #[serde(default)]
    pub usd: Option<f64>,
    #[serde(default)]
    pub minutes: Option<f64>,
}

/// Whether a step went over its limits: why, or None.
pub fn over_limit(spent_usd: f64, minutes: f64, limits: StepLimits) -> Option<String> {
    if let Some(usd) = limits.usd.filter(|u| *u > 0.0)
        && spent_usd > usd
    {
        return Some(format!(
            "it cost ${}, over the limit of ${} per step",
            js::to_fixed(spent_usd, 2),
            js::to_fixed(usd, 2)
        ));
    }
    if let Some(max) = limits.minutes.filter(|m| *m > 0.0)
        && minutes > max
    {
        return Some(format!(
            "it ran {} minutes, over the limit of {} per step",
            js::number(js::round(minutes)),
            js::number(max)
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn use_(name: &str, input: Value) -> String {
        json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "x", "name": name, "input": input}]}}).to_string()
    }

    #[test]
    fn sorts_tool_calls_into_reading_changing_and_running() {
        let k = tool_kinds(&[
            use_("Read", json!({})),
            use_("mcp__qanuvi__find_symbol", json!({})),
            use_(
                "Bash",
                json!({"description": "Read CheckoutPage first half"}),
            ),
            use_(
                "Bash",
                json!({"description": "Run the embedded checkout test with the database"}),
            ),
            use_("Edit", json!({})),
            use_("Write", json!({})),
            "not json".into(),
        ]);
        assert_eq!(
            k,
            ToolKinds {
                read: 3,
                change: 2,
                run: 1
            }
        );
    }

    #[test]
    fn compares_a_run_with_the_plans_earlier_runs() {
        let run = |id: &str, file: &str, costs: &[f64], done: usize| RunRecord {
            id: id.into(),
            file: file.into(),
            data: json!({"steps": costs.iter().enumerate().map(|(i, c)| json!({
                "status": if i < done { "done" } else { "blocked" }, "attempts": [{"costUsd": c}]
            })).collect::<Vec<_>>()})
            .to_string(),
        };
        assert_eq!(
            finished_cost(
                &json!({"steps": [{"status": "done", "attempts": [{"costUsd": 2}, {"costUsd": 1}]}]})
            ),
            Finished {
                steps: 1,
                cost: 3.0
            }
        );
        let h = plan_history(
            &[
                run("a", "P.md", &[5.0, 6.0, 17.0], 2),
                run("b", "P.md", &[2.0], 1),
                run("c", "Q.md", &[9.0], 1),
                run("now", "P.md", &[1.0], 1),
            ],
            "P.md",
            "now",
        );
        assert_eq!(
            (h.runs, h.steps, h.cost, h.per_step),
            (2, 3, 30.0, Some(10.0))
        );
    }

    #[test]
    fn knows_when_a_step_went_over_its_limits() {
        let l = |usd, minutes| StepLimits { usd, minutes };
        assert!(
            over_limit(3.5, 10.0, l(Some(3.0), None))
                .unwrap()
                .contains("over the limit of $3.00")
        );
        assert!(
            over_limit(1.0, 45.0, l(Some(3.0), Some(30.0)))
                .unwrap()
                .contains("45 minutes")
        );
        assert_eq!(over_limit(1.0, 10.0, l(Some(3.0), Some(30.0))), None);
        assert_eq!(over_limit(99.0, 99.0, l(None, None)), None);
    }
}
