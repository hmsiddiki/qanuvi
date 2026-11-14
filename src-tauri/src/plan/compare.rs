//! The comparison check for 24z step A (R1): `compare.json` holds inputs and the TypeScript plan logic's outputs
//! (kept current by `src/lib/plan/compare.test.ts`); this runs the same inputs through the Rust port, which must give
//! the same outputs.

use serde_json::Value;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
use super::{
    costs, decider, finishing, guidance, holdask, housekeeping, limits, packet, picks, planner,
    planstate, preflight, prompts, routing, runner, setaside, tasklist,
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
        other => run_2b(other, a),
    }
}

/// R1 part 2b: preflight, packet, planstate, housekeeping, planner, guidance.
#[cfg(test)]
fn run_2b(name: &str, a: &[Value]) -> Value {
    fn de<T: serde::de::DeserializeOwned>(v: &Value) -> T {
        serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("bad input {v}: {e}"))
    }
    fn to(v: impl serde::Serialize) -> Value {
        serde_json::to_value(v).unwrap()
    }
    let s = |i: usize| a[i].as_str().unwrap_or_default().to_string();
    let n = |i: usize| a[i].as_i64().unwrap();
    let f = |i: usize| a[i].as_f64().unwrap();
    let set = |v: &Value| -> HashSet<String> { de::<Vec<String>>(v).into_iter().collect() };
    let state = || de::<planstate::PlanState>(&a[0]);
    let lines = |v: Vec<&tasklist::Step>| to(v.iter().map(|s| s.line).collect::<Vec<_>>());
    match name {
        "preflightPrompt" => json!(preflight::preflight_prompt(
            &s(0),
            &de::<Vec<_>>(&a[1]),
            &de(&a[2])
        )),
        "parsePreflight" => to(preflight::parse_preflight(&s(0))),
        "recommendedRisk" => to(preflight::recommended_risk(de(&a[0]))),
        "decidedNote" => json!(preflight::decided_note(&de(&a[0]))),
        "settleCheck" => to(preflight::settle_check(
            &preflight::parse_preflight(&s(0)),
            &de(&a[1]),
            &de(&a[2]),
            &de(&a[3]),
            &de(&a[4]),
        )),
        "waitsBefore" => {
            let wait: Option<Vec<preflight::StepWhy>> = de(&a[1]);
            to(preflight::waits_before(a[0].as_str(), wait.as_deref()))
        }
        "mentions" => to(packet::mentions(&s(0))),
        "excerptAround" => to(packet::excerpt_around(&s(0), n(1), n(2), n(3))),
        "packetContextBlock" => json!(packet::context_block(&de::<Vec<_>>(&a[0]), n(1) as usize)),
        "failureSummary" => json!(packet::failure_summary(&s(0), n(1) as usize)),
        "packetPrompt" => json!(packet::packet_prompt(&s(0), &s(1), &s(2))),
        "fixPrompt" => json!(packet::fix_prompt(&s(0), &s(1), n(2) as u32, n(3) as u32)),
        "locationBlock" => json!(packet::location_block(
            &de::<Vec<_>>(&a[0]),
            &de::<Vec<_>>(&a[1]),
            &de::<Vec<_>>(&a[2]),
            n(3) as usize
        )),
        "holdWording" => json!(planstate::hold_wording(
            a[0]["why"].as_str().unwrap(),
            de(&a[0]["by"])
        )),
        "keepHolds" => to(planstate::keep_holds(&state(), &de::<Vec<_>>(&a[1]), f(2))),
        "keepAnswer" => to(planstate::keep_answer(
            &state(),
            &s(1),
            de(&a[2]),
            &s(3),
            f(4),
        )),
        "pruneState" => {
            let open = set(&a[1]);
            let all = if a[2].is_null() {
                open.clone()
            } else {
                set(&a[2])
            };
            to(planstate::prune_state(&state(), &open, &all))
        }
        "closingRan" => to(planstate::closing_ran(
            &state(),
            &s(1),
            &de::<Vec<_>>(&a[2]),
        )),
        "markLate" => to(planstate::mark_late(&state(), &s(1))),
        "closingsDue" => {
            let pending = set(&a[1]);
            to(planstate::closings_due(
                &state(),
                |x| pending.contains(x),
                a[2] == true,
            ))
        }
        "keepDecided" => to(planstate::keep_decided(
            &state(),
            &de::<Vec<_>>(&a[1]),
            &de::<Vec<_>>(&a[2]),
            &de::<Vec<_>>(&a[3]),
            f(4),
        )),
        "changeDecided" => to(planstate::change_decided(&state(), &s(1), &s(2), &s(3))),
        "archivePath" => json!(housekeeping::archive_path(&s(0))),
        "isArchive" => json!(housekeeping::is_archive(&s(0))),
        "doneNumbers" => {
            let texts: Vec<Option<&str>> = a.iter().map(Value::as_str).collect();
            to(housekeeping::done_numbers(&texts))
        }
        "finishedSections" => to(housekeeping::finished_sections(&s(0))
            .iter()
            .map(|x| json!([x.title, x.line, x.steps.len()]))
            .collect::<Vec<_>>()),
        "archiveFinished" => to(housekeeping::archive_finished(&s(0), a[1].as_str(), &s(2))),
        "mergePlans" => {
            let o = &a[4];
            let taken: Vec<String> = de(o.get("taken").unwrap_or(&json!([])));
            to(housekeeping::merge_plans(
                &s(0),
                &s(1),
                &s(2),
                &s(3),
                o["onlyOpen"] == true,
                &taken,
            ))
        }
        "stepNumbers" => to(housekeeping::step_numbers(a[0].as_str())),
        "plannerPrompt" => json!(planner::planner_prompt(&s(0), &s(1), &de::<Vec<_>>(&a[2]))),
        "replanPrompt" => {
            let plan = tasklist::parse_plan(&s(1));
            json!(planner::replan_prompt(
                &s(0),
                &plan.steps[n(2) as usize],
                &s(3),
                &de::<Vec<_>>(&a[4])
            ))
        }
        "extractSteps" => {
            let e = planner::extract_steps(&s(0));
            json!({ "block": e.block, "steps": to(&e.steps) })
        }
        "lastNumber" => json!(planner::last_number(&s(0))),
        "renumber" => {
            let start = match a[1].as_i64() {
                Some(x) => planner::Start::From(x),
                None => planner::Start::Under(a[1].as_str().unwrap()),
            };
            json!(planner::renumber(&s(0), start))
        }
        "appendSection" => json!(planner::append_section(&s(0), &s(1), &s(2))),
        "replaceStep" => match planner::replace_step(
            &s(0),
            a[1]["number"].as_str(),
            a[1]["title"].as_str().unwrap(),
            &s(2),
            &s(3),
        ) {
            Ok(t) => json!(t),
            Err(e) => json!({ "error": e }),
        },
        "labelRange" => json!(planner::label_range(&s(0))),
        "sectionTitleFrom" => json!(planner::section_title_from(&s(0))),
        "guidanceNote" => json!(guidance::guidance_note(&s(0), &s(1))),
        "withoutGuidance" => json!(guidance::without_guidance(&s(0))),
        "stoppedChatNote" => json!(guidance::stopped_chat_note(&s(0), &s(1))),
        "isGoOnOrder" => json!(guidance::is_go_on_order(&s(0))),
        "isYes" => json!(guidance::is_yes(&s(0))),
        "askedToResume" => json!(guidance::asked_to_resume(&s(0))),
        "isStopOrder" => json!(guidance::is_stop_order(&s(0))),
        "addedSteps" => {
            let plan = tasklist::parse_plan(&s(0));
            lines(guidance::added_steps(&plan.steps, &set(&a[1])))
        }
        "reopenedSteps" => {
            let plan = tasklist::parse_plan(&s(1));
            lines(guidance::reopened_steps(
                &de::<Vec<_>>(&a[0]),
                &plan.steps,
                &set(&a[2]),
            ))
        }
        "stepKey" => json!(guidance::step_key(
            a[0]["number"].as_str(),
            a[0]["title"].as_str().unwrap()
        )),
        "notesBlock" => json!(guidance::notes_block(&de::<Vec<_>>(&a[0]))),
        "notesIn" => to(guidance::notes_in(&s(0))),
        "looksSecret" => json!(guidance::looks_secret(&s(0))),
        "contextSize" => json!(guidance::context_size(&de::<Vec<_>>(&a[0]))),
        "handoverDue" => json!(guidance::handover_due(
            n(0) as u64,
            n(1) as u64,
            n(2) as u32
        )),
        "handoverRequest" => json!(guidance::handover_request(&s(0), n(1) as u64)),
        "modelSwitchRequest" => json!(guidance::model_switch_request(&s(0), &s(1))),
        "canCarry" => {
            let carry: Option<guidance::Carry> = de(&a[0]);
            json!(guidance::can_carry(
                carry.as_ref(),
                &s(1),
                &s(2),
                n(3) as u64
            ))
        }
        "needingCount" => json!(guidance::needing_count(
            &s(0),
            &de::<Vec<_>>(&a[1]),
            &set(&a[2])
        )),
        "privateBlock" => json!(guidance::private_block(&de::<Vec<_>>(&a[0]))),
        "logDir" => json!(guidance::log_dir(&s(0))),
        "logPath" => json!(guidance::log_path(
            &s(0),
            a[1]["number"].as_str(),
            a[1]["title"].as_str().unwrap()
        )),
        "attemptEntry" => json!(guidance::attempt_entry(&de(&a[0]))),
        "resultEntry" => json!(guidance::result_entry(&s(0), a[1].as_str())),
        "logsBlock" => json!(guidance::logs_block(&s(0), &s(1), a[2] == true)),
        "mapPath" => json!(guidance::map_path(&s(0))),
        "factsPath" => json!(guidance::facts_path(&s(0))),
        "apiPath" => json!(guidance::api_path(&s(0))),
        "guidanceContextBlock" => json!(guidance::context_block(
            &s(0),
            &s(1),
            &de::<Vec<_>>(&a[2]),
            a[3] == true
        )),
        "knowledgeBlock" => json!(guidance::knowledge_block(
            &s(0),
            &s(1),
            &s(2),
            &de::<Vec<_>>(&a[3]),
            a[4] == true
        )),
        "isCode" => json!(guidance::is_code(&s(0))),
        "apiSection" => json!(guidance::api_section(&s(0), &de::<Vec<_>>(&a[1]))),
        "mergeApi" => json!(guidance::merge_api(
            &s(0),
            &de::<Vec<(String, String)>>(&a[1])
        )),
        "referencedPlans" => to(guidance::referenced_plans(&s(0), &s(1))),
        "referencedBlock" => json!(guidance::referenced_block(&de::<Vec<_>>(&a[0]))),
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
