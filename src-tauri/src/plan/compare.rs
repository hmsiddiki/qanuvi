//! The comparison check for 24z step A (R1): `compare.json` holds inputs and the TypeScript plan logic's outputs
//! (kept current by `src/lib/plan/compare.test.ts`); this runs the same inputs through the Rust port, which must give
//! the same outputs.

use serde_json::Value;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
use super::{
    costs, decider, finishing, holdask, limits, picks, prompts, routing, runner, setaside, tasklist,
};
#[cfg(test)]
use std::collections::{HashMap, HashSet};

/// One case through the Rust port: the function's name (the TypeScript one) and its arguments as JSON.
#[cfg(test)]
fn run(name: &str, a: &[Value]) -> Value {
    use decider::*;
    fn de<T: serde::de::DeserializeOwned>(v: &Value) -> T {
        serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("bad input {v}: {e}"))
    }
    fn to(v: impl serde::Serialize) -> Value {
        serde_json::to_value(v).unwrap()
    }
    let s = |i: usize| a[i].as_str().unwrap_or_default().to_string();
    let line = |v: &Value| v.as_u64().unwrap() as usize;
    // An edit's text, or the error the TypeScript throws.
    let tried = |r: Result<String, String>| match r {
        Ok(t) => json!(t),
        Err(e) => json!({ "error": e }),
    };
    let ids = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .map(|m| m["id"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    match name {
        "parseTeam" => to(parse_team(&a[0])),
        "deciderSeat" => {
            let paid = a[0].as_object().map(|p| {
                (
                    p["agent"].as_str().unwrap_or(""),
                    p["model"].as_str().unwrap_or(""),
                )
            });
            to(decider_seat(paid, &de::<Vec<FreeModel>>(&a[1])))
        }
        "freeWorks" => json!(free_works(parse_team(&a[0]), a[1] == true, a[2] == true)),
        "assignPrompt" => json!(assign_prompt(&de::<Vec<StepBrief>>(&a[0]), de(&a[1]))),
        "parseAssignment" => to(parse_assignment(
            &s(0),
            &de::<Vec<StepBrief>>(&a[1]),
            de(&a[2]),
        )),
        "pickPrompt" => {
            let o = &a[1];
            let profiles: Profiles = o["profiles"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), de(v)))
                .collect();
            let models: Vec<ModelChoice> = de(&o["models"]);
            json!(pick_prompt(
                &de::<Vec<StepBrief>>(&a[0]),
                &PickOptions {
                    priority: de(&o["priority"]),
                    free_first: o["freeFirst"] == true,
                    models: &models,
                    profiles: &profiles,
                }
            ))
        }
        "parsePicks" => {
            let o = &a[2];
            to(parse_picks(
                &s(0),
                &de::<Vec<StepBrief>>(&a[1]),
                de(&o["priority"]),
                o["freeFirst"] == true,
                &de::<Vec<String>>(&o["modelIds"]),
            ))
        }
        "reviewPrompt" => json!(review_prompt(
            a[0]["title"].as_str().unwrap_or(""),
            &de::<Vec<String>>(&a[0]["details"]),
            &s(1),
            &s(2)
        )),
        "parseReview" => to(parse_review(&s(0))),
        "chatModelPrompt" => json!(chat_model_prompt(&s(0), &de::<Vec<ModelChoice>>(&a[1]))),
        "parseChatModel" => to(parse_chat_model(&s(0), &ids(&a[1]))),
        "stepDiff" => to(step_diff(&s(0), &s(1), &s(2))),
        "overruleNote" => json!(overrule_note(&s(0), &s(1))),
        "withoutOverruleNote" => json!(without_overrule_note(&s(0))),
        "overruleIn" => to(overrule_in(&s(0))),
        "strongerModel" => json!(stronger_model(&ids(&a[0]), &s(1))),
        "isLimit" => json!(limits::is_limit(&s(0))),
        "limitResetAt" => {
            let now = jiff::Timestamp::from_millisecond(a[1].as_i64().unwrap()).unwrap();
            to(limits::limit_reset_at(&s(0), now))
        }
        "holdKey" => json!(holdask::hold_key(&s(0), &s(1), &s(2))),
        "asksSecret" => json!(holdask::asks_secret(&s(0))),
        "holdForm" => to(holdask::hold_form(de(&a[0]), &s(1))),
        "holdReply" => to(holdask::hold_reply(&de(&a[0]))),
        "isClosingStep" => json!(setaside::is_closing_step(a[0].as_str())),
        "mentionedFiles" => to(setaside::mentioned_files(&s(0))),
        "planDependence" => to(setaside::plan_dependence(&de(&a[0]), &de(&a[1]))),
        "dependencePrompt" => json!(setaside::dependence_prompt(
            &de::<Vec<setaside::HeldStep>>(&a[0]),
            &de::<Vec<setaside::DepStep>>(&a[1])
        )),
        "parseDependence" => to(setaside::parse_dependence(
            &s(0),
            &de::<Vec<String>>(&a[1]),
            &de::<Vec<String>>(&a[2]),
        )),
        "waitingLine" => json!(setaside::waiting_line(a[0].as_u64().unwrap() as usize)),
        "closingRerunNote" => json!(setaside::closing_rerun_note(&de::<Vec<String>>(&a[0]))),
        "setAsideBlock" => json!(setaside::set_aside_block(&de::<Vec<setaside::AsideItem>>(
            &a[0]
        ))),
        "toolKinds" => to(costs::tool_kinds(&de::<Vec<String>>(&a[0]))),
        "finishedCost" => to(costs::finished_cost(&a[0])),
        "planHistory" => to(costs::plan_history(
            &de::<Vec<costs::RunRecord>>(&a[0]),
            &s(1),
            &s(2),
        )),
        "overLimit" => to(costs::over_limit(
            a[0].as_f64().unwrap(),
            a[1].as_f64().unwrap(),
            de(&a[2]),
        )),
        "pickFor" => {
            let saved: Option<picks::SavedPicks> = de(&a[0]);
            let step = &a[1];
            to(picks::pick_for(
                saved.as_ref(),
                step["number"].as_str().unwrap_or(""),
                step["title"].as_str().unwrap_or(""),
                &s(2),
            ))
        }
        "taskListPrompt" => json!(prompts::task_list_prompt(
            &s(0),
            &prompts::CLAUDE_CODE_MODELS
        )),
        "parsePlan" => to(tasklist::parse_plan(&s(0))),
        "setStepStatus" => tried(tasklist::set_step_status(
            &s(0),
            line(&a[1]),
            de(&a[2]),
            a[3].as_str(),
        )),
        "setStepTitle" => tried(tasklist::set_step_title(
            &s(0),
            line(&a[1]),
            &s(2),
            a[3].as_str(),
        )),
        "setStepModel" => tried(tasklist::set_step_model(
            &s(0),
            line(&a[1]),
            a[2].as_str(),
            a[3].as_str(),
        )),
        "setStepSkipped" => tried(tasklist::set_step_skipped(
            &s(0),
            line(&a[1]),
            a[2].as_str(),
            a[3].as_str(),
        )),
        "nextStatus" => to(tasklist::next_status(de(&a[0]))),
        "findTaskLists" => to(tasklist::find_task_lists(&de::<Vec<String>>(&a[0]))),
        "clearStaleSkips" => json!(tasklist::clear_stale_skips(
            &s(0),
            &de::<Vec<String>>(&a[1])
        )),
        "jobType" => to(routing::job_type(&s(0))),
        "learn" => {
            let runs: Vec<Vec<runner::StepRun>> = a[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| de(&r["steps"]))
                .collect();
            to(routing::learn(
                runs.iter().map(Vec::as_slice),
                routing::job_type,
            ))
        }
        "reliability" => json!(routing::reliability(
            a[0]["tries"].as_u64().unwrap() as u32,
            a[0]["successes"].as_u64().unwrap() as u32
        )),
        "learnedModel" => to(routing::learned_model(
            &de(&a[0]),
            de(&a[1]),
            &de::<Vec<_>>(&a[2]),
        )),
        "modelProfiles" => to(routing::model_profiles(
            &de(&a[0]),
            de(&a[1]),
            &de::<Vec<_>>(&a[2]),
        )),
        "budgetState" => to(routing::budget_state(de(&a[0]))),
        "chooseModel" => {
            let o = &a[0];
            let rules: HashMap<routing::JobType, String> = de(&o["rules"]);
            let stats: routing::Stats = de(&o["stats"]);
            let models: Vec<routing::AgentModel> = de(&o["models"]);
            let picked: Option<routing::Picked> = de(o.get("picked").unwrap_or(&Value::Null));
            to(routing::choose_model(&routing::ChooseOptions {
                step_model: o["stepModel"].as_str(),
                job: de(&o["type"]),
                rules: &rules,
                stats: &stats,
                models: &models,
                default_model: o["defaultModel"].as_str().unwrap(),
                budget: de(&o["budget"]),
                picked: picked.as_ref(),
            }))
        }
        "freeFirstFor" => json!(runner::free_first_for(
            de(&a[0]),
            a[1] == true,
            a[2] == true
        )),
        "freeWantedButMissing" => json!(runner::free_wanted_but_missing(
            de(&a[0]),
            de(&a[1]),
            a[2] == true,
            a[3] == true
        )),
        "costRatio" => to(runner::cost_ratio(
            &de::<Vec<_>>(&a[0]),
            &de::<Vec<_>>(&a[1]),
            &s(2),
        )),
        "stepsToRun" => {
            let plan = tasklist::parse_plan(&s(0));
            let ranges = runner::parse_ranges(a[3].as_str()).ranges;
            let picked = runner::steps_to_run(&plan.steps, line(&a[1]), a[2] == true, &ranges);
            to(picked.iter().map(|s| s.line).collect::<Vec<_>>())
        }
        "parseRanges" => to(runner::parse_ranges(a[0].as_str())),
        "inRanges" => json!(runner::in_ranges(
            a[0].as_str(),
            &runner::parse_ranges(a[1].as_str()).ranges
        )),
        "unmetNeed" => {
            let others: HashMap<String, HashSet<String>> = de(&a[2]);
            to(runner::unmet_need(
                &de::<Vec<_>>(&a[0]),
                &de(&a[1]),
                &others,
            ))
        }
        "endStatus" => to(runner::end_status(&de::<Vec<_>>(&a[0]))),
        "runSummary" => json!(runner::run_summary(&de::<Vec<_>>(&a[0]))),
        "startModel" => json!(runner::start_model(a[0].as_str(), &s(1))),
        "nextModel" => to(runner::next_model::<runner::Attempt>(
            &de::<Vec<_>>(&a[0]),
            &de::<Vec<_>>(&a[1]),
            &s(2),
            a[3].as_u64().unwrap() as u32,
        )),
        "stepOutcome" => to(runner::step_outcome(
            &s(0),
            a[1]["number"].as_str(),
            a[1]["title"].as_str().unwrap(),
        )),
        "stepPrompt" => {
            let plan = tasklist::parse_plan(&s(1));
            json!(runner::step_prompt(
                &s(0),
                &plan.steps[line(&a[2])],
                &de::<Vec<_>>(&a[3])
            ))
        }
        "runTotals" => {
            let calls: Option<Vec<runner::DeciderCall>> = de(&a[1]);
            let finish: Option<finishing::FinishPass> = de(&a[2]);
            let t = runner::run_totals(
                &de::<Vec<_>>(&a[0]),
                &calls.unwrap_or_default(),
                finish.as_ref(),
            );
            let mut v = to(&t);
            v["byModel"] =
                Value::Object(t.by_model.iter().map(|(k, m)| (k.clone(), to(m))).collect());
            v
        }
        "nextModelFreeFirst" => to(runner::next_model_free_first::<runner::Attempt>(
            &de::<Vec<_>>(&a[0]),
            &de::<Vec<_>>(&a[1]),
            &s(2),
            &s(3),
            a[4].as_u64().unwrap() as u32,
        )),
        "freeSavings" => to(runner::free_savings(&de::<Vec<_>>(&a[0]), &de(&a[1]))),
        "paidPerStep" => to(runner::paid_per_step(&de(&a[0]))),
        "finishEstimate" => to(finishing::finish_estimate(&de::<Vec<_>>(&a[0]), de(&a[1]))),
        "offersFinish" => {
            let finish: Option<finishing::FinishPass> = de(&a[2]);
            json!(finishing::offers_finish(
                de(&a[0]),
                &de::<Vec<_>>(&a[1]),
                finish.as_ref()
            ))
        }
        "finishReviewPrompt" => {
            let checks: Option<finishing::Checks> = de(&a[3]);
            json!(finishing::finish_review_prompt(
                &s(0),
                &de::<Vec<_>>(&a[1]),
                &s(2),
                checks.as_ref()
            ))
        }
        "parseFinishReview" => to(finishing::parse_finish_review(&s(0))),
        "finishFullPrompt" => {
            let checks: Option<finishing::Checks> = de(&a[3]);
            json!(finishing::finish_full_prompt(
                &s(0),
                &de::<Vec<_>>(&a[1]),
                &de(&a[2]),
                checks.as_ref(),
                &de::<Vec<_>>(&a[4])
            ))
        }
        "stillOpenIn" => to(finishing::still_open_in(&s(0))),
        "doneNotes" => to(finishing::done_notes(&de::<Vec<_>>(&a[0]))),
        "isPlanFile" => json!(finishing::is_plan_file(&s(0), &s(1), &s(2))),
        "finishEntry" => json!(finishing::finish_entry(&de(&a[0]))),
        other => panic!("no Rust function for {other}"),
    }
}

/// JSON equality with numbers compared by value (TypeScript has one number type: 3 and 3.0 are the same).
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rust_port_gives_the_typescript_outputs() {
        let cases: Vec<Value> = serde_json::from_str(include_str!("compare.json")).unwrap();
        assert!(!cases.is_empty());
        let mut wrong = vec![];
        for c in &cases {
            let name = c["fn"].as_str().unwrap();
            let args = c["args"].as_array().unwrap();
            let got = run(name, args);
            if !same(&got, &c["out"]) {
                wrong.push(format!(
                    "{name}({})\n  TypeScript: {}\n  Rust:       {got}",
                    c["args"], c["out"]
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} cases differ:\n{}",
            wrong.len(),
            cases.len(),
            wrong.join("\n")
        );
    }
}
