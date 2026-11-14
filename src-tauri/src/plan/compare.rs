//! The comparison check for 24z step A (R1): `compare.json` holds inputs and the TypeScript plan logic's outputs
//! (kept current by `src/lib/plan/compare.test.ts`); this runs the same inputs through the Rust port, which must give
//! the same outputs.

use serde_json::Value;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
use super::{costs, decider, holdask, limits, picks, prompts, setaside};

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
