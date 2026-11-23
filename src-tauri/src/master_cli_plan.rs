// A master's plan runs from its own terminal (migration M7c): `qanuvi-master plan run <file>`, `plan follow`, `plan
// list` and `plan stop`. A thin client of the running service with the plan_run_* commands a window sends, so a run
// started here is the master's own: it shows in any window showing this master, and Ctrl+C only leaves the terminal
// (the run goes on until `plan stop`).
//
// A terminal has no window settings, so a run started here takes the setup of the last run a window started on this
// master (agent, models, team, permission mode, budget; kept in run-starts/ for M5b), with the new plan file. The
// project's own one first, else the newest of another project (its model rules left out).

use super::cli::{call, words};
use super::cli_inbox::ago;
use super::cli_status::find_project;
use super::{Value, ensure_service, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "Usage: qanuvi-master plan run <plan file> [--steps <for example 2-6, 9>] [--budget <dollars>] [--no-check] [--no-follow]
       qanuvi-master plan follow [<project>]
       qanuvi-master plan list
       qanuvi-master plan stop [<project>] [--now]";

fn list_of(v: Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// The value after a flag (`--steps 2-6`).
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_string()
}

/// The project a plan file is in (the deepest one when projects nest) and the file's path inside it, with `/`.
pub fn owning_project<'a>(list: &'a [Value], file: &str) -> Result<(&'a Value, String), String> {
    let f = norm(file);
    let fl = f.to_lowercase();
    let windows = cfg!(windows);
    list.iter()
        .filter_map(|p| {
            let root = norm(p["path"].as_str().unwrap_or(""));
            if root.is_empty() {
                return None;
            }
            let inside = if windows {
                fl.starts_with(&format!("{}/", root.to_lowercase()))
            } else {
                f.starts_with(&format!("{root}/"))
            };
            inside.then(|| (p, f[root.len() + 1..].to_string(), root.len()))
        })
        .max_by_key(|(_, _, n)| *n)
        .map(|(p, rel, _)| (p, rel))
        .ok_or_else(|| {
            format!(
                "{file} is not in any of this master's projects. Add its folder first: qanuvi-master projects add <folder>"
            )
        })
}

/// The kept start a terminal run copies: the project's own, else the newest of another project (its rules dropped).
fn kept_start(dir: &Path, project_id: &str) -> Result<Value, String> {
    let at = dir.join("run-starts");
    let read = |f: &PathBuf| {
        std::fs::read_to_string(f)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .filter(Value::is_object)
    };
    if let Some(v) = read(&at.join(format!("{project_id}.json"))) {
        return Ok(v);
    }
    let newest = std::fs::read_dir(&at)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .and_then(|e| read(&e.path()));
    match newest {
        Some(mut v) => {
            v["rules"] = json!({});
            Ok(v)
        }
        None => Err("This master has not run a plan yet, so a terminal does not know which agent, models and \
             permission mode to use. Start one plan run from a Qanuvi window showing this machine once; \
             later runs can start here."
            .into()),
    }
}

/// A kept start made into this run's: the new plan file, all open steps (or `steps`), no resume, `budget` if given.
pub fn start_from(
    mut kept: Value,
    project_name: &str,
    file: &str,
    steps: Option<&str>,
    budget: Option<f64>,
) -> Value {
    kept["projectName"] = json!(project_name);
    kept["file"] = json!(file);
    kept["resume"] = Value::Null;
    if !kept["options"].is_object() {
        kept["options"] = json!({});
    }
    let o = &mut kept["options"];
    o["fromLine"] = json!(0);
    o["onlyOne"] = json!(false);
    match steps.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => o["ranges"] = json!(s),
        None => {
            if let Some(m) = o.as_object_mut() {
                m.remove("ranges");
            }
        }
    }
    if let Some(b) = budget {
        o["budgetUsd"] = json!(b);
    }
    kept
}

fn mode_name(mode: &str) -> &str {
    match mode {
        "default" => "Manual",
        "acceptEdits" => "Edit automatically",
        "plan" => "Plan",
        "auto" => "Auto",
        m => m,
    }
}

fn money(usd: f64) -> String {
    // + 0.0: an empty sum is -0.0, which would print as $-0.00.
    format!("${:.2}", usd + 0.0)
}

/// One line on the setup a run uses.
pub fn setup_line(start: &Value) -> String {
    let o = &start["options"];
    let budget = o["budgetUsd"].as_f64().unwrap_or(0.0);
    format!(
        "Agent {}, {} mode, budget {}{}.",
        start["providerName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("as Qanuvi picks"),
        mode_name(o["mode"].as_str().unwrap_or("default")),
        if budget > 0.0 {
            money(budget)
        } else {
            "none".into()
        },
        o["ranges"]
            .as_str()
            .map(|r| format!(", steps {r}"))
            .unwrap_or_default()
    )
}

fn step_name(step: &Value) -> String {
    let title = step["title"].as_str().unwrap_or("").trim();
    match step["number"].as_str() {
        Some(n) if !title.is_empty() => format!("Step {n} · {title}"),
        Some(n) => format!("Step {n}"),
        None => title.to_string(),
    }
}

fn step_cost(step: &Value) -> f64 {
    step["attempts"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t["costUsd"].as_f64()).sum())
        .unwrap_or(0.0)
}

fn run_cost(run: &Value) -> f64 {
    run["steps"]
        .as_array()
        .map(|s| s.iter().map(step_cost).sum())
        .unwrap_or(0.0)
}

/// The lines a change of the run's view makes, from the view shown before (`None`: the first one).
pub fn view_lines(before: Option<&Value>, now: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let run = &now["run"];
    let empty = Vec::new();
    let old_steps = before
        .and_then(|b| b["run"]["steps"].as_array())
        .unwrap_or(&empty);
    for (i, st) in run["steps"].as_array().unwrap_or(&empty).iter().enumerate() {
        let old = old_steps.get(i);
        let status = st["status"].as_str().unwrap_or("");
        let was = old.and_then(|o| o["status"].as_str()).unwrap_or("waiting");
        let tries = st["attempts"].as_array().map_or(0, Vec::len);
        let tried = old
            .and_then(|o| o["attempts"].as_array())
            .map_or(0, Vec::len);
        let last = st["attempts"].as_array().and_then(|a| a.last());
        // A try that did not finish while the step goes on (a done or stopped one is told by the status line).
        let unfinished =
            last.is_some_and(|t| matches!(t["outcome"].as_str(), Some("open" | "error")));
        if status == "running" && tries > tried && was == "running" && unfinished {
            out.push(format!(
                "  {} tried on {}: {}; it tries again.",
                step_name(st),
                last.and_then(|t| t["model"].as_str()).unwrap_or("?"),
                last.and_then(|t| t["note"].as_str())
                    .map(|n| n.trim().trim_end_matches('.'))
                    .filter(|n| !n.is_empty())
                    .unwrap_or("not finished")
            ));
        }
        if status == was {
            continue;
        }
        let note = last
            .and_then(|t| t["note"].as_str())
            .filter(|n| !n.trim().is_empty())
            .map(|n| format!(": {}", n.trim()))
            .unwrap_or_default();
        out.push(match status {
            "running" => format!(
                "> {} (on {})",
                step_name(st),
                now["currentModel"]
                    .as_str()
                    .filter(|m| !m.is_empty())
                    .unwrap_or("its model")
            ),
            "done" => format!("  done: {} ({})", step_name(st), money(step_cost(st))),
            "failed" => format!("  FAILED: {}{note}", step_name(st)),
            "blocked" => format!("  blocked: {}{note}", step_name(st)),
            "held" => format!(
                "  {} waits for your answer. See: qanuvi-master inbox list",
                step_name(st)
            ),
            "set-aside" => format!("  set aside: {} (waits for a held step)", step_name(st)),
            "skipped" => format!("  skipped: {}", step_name(st)),
            "waiting" if was == "running" => format!("  back to waiting: {}", step_name(st)),
            _ => continue,
        });
    }
    let old_notes: Vec<&str> = before
        .and_then(|b| b["notices"].as_array())
        .map(|n| n.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    for n in now["notices"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(Value::as_str)
    {
        if !old_notes.contains(&n) {
            out.push(format!("  note: {n}"));
        }
    }
    if let Some(until) = now["waitingUntil"].as_i64()
        && before.and_then(|b| b["waitingUntil"].as_i64()) != Some(until)
    {
        let mins = ((until - now_ms()).max(0) + 59_999) / 60_000;
        out.push(format!(
            "  The agent's usage limit is reached; the run waits about {mins} min for it to reset."
        ));
    }
    let asks = now["waitingAnswers"].as_u64().unwrap_or(0);
    if asks
        > before
            .and_then(|b| b["waitingAnswers"].as_u64())
            .unwrap_or(0)
    {
        out.push(format!(
            "  {asks} {} for you in the Inbox: qanuvi-master inbox list",
            if asks == 1 {
                "question waits"
            } else {
                "questions wait"
            }
        ));
    }
    let was_on = before.is_some_and(|b| b["running"] == true || b["finishing"] == true);
    let on = now["running"] == true || now["finishing"] == true;
    if was_on && !on {
        out.push(end_line(now));
    }
    out
}

/// The run's last line: how it ended and what it cost.
pub fn end_line(view: &Value) -> String {
    let run = &view["run"];
    let cost = money(run_cost(run));
    let msg = run["message"].as_str().unwrap_or("").trim();
    let tail = if msg.is_empty() {
        String::new()
    } else {
        format!(" {msg}")
    };
    if let Some(e) = view["error"].as_str().filter(|e| !e.is_empty()) {
        return format!("The run ended with an error: {e}");
    }
    match run["status"].as_str() {
        Some("done") => format!("The run is done ({cost}).{tail}"),
        Some("needs-you") => {
            format!("The run stopped: it needs you ({cost}).{tail} See: qanuvi-master inbox list")
        }
        Some("paused") => format!("The run is paused ({cost}).{tail}"),
        Some("failed") => format!("The run failed ({cost}).{tail}"),
        _ => match run["budgetReached"].as_f64() {
            Some(b) => format!("The run stopped: it reached its budget of {}.", money(b)),
            None => format!("The run stopped ({cost}).{tail}"),
        },
    }
}

/// A path inside the project as the plan names it (from its root, with `/`); any other text as it is.
fn inside(root: &str, what: &str) -> String {
    let r = norm(root);
    let w = what.replace('\\', "/");
    let same = if cfg!(windows) {
        w.to_lowercase()
            .starts_with(&format!("{}/", r.to_lowercase()))
    } else {
        w.starts_with(&format!("{r}/"))
    };
    if !r.is_empty() && same {
        w[r.len() + 1..].to_string()
    } else {
        what.to_string()
    }
}

/// What the step's agent says and does, short: its text and the tools it uses (paths from the project's `root`).
pub fn agent_lines(event: &Value, root: &str) -> Vec<String> {
    if event["kind"] != "line" {
        return Vec::new();
    }
    let Ok(msg) = serde_json::from_str::<Value>(event["line"].as_str().unwrap_or("")) else {
        return Vec::new();
    };
    if msg["type"] != "assistant" {
        return Vec::new();
    }
    let mut out = Vec::new();
    for b in msg["message"]["content"].as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => {
                for l in b["text"].as_str().unwrap_or("").lines() {
                    let l = l.trim_end();
                    if !l.trim().is_empty() {
                        out.push(format!("    | {}", cut(l, 160)));
                    }
                }
            }
            Some("tool_use") => {
                let i = &b["input"];
                let what = ["file_path", "path", "command", "pattern", "url"]
                    .iter()
                    .find_map(|k| i[*k].as_str())
                    .unwrap_or("");
                let what = inside(root, what);
                out.push(format!(
                    "    . {} {}",
                    b["name"].as_str().unwrap_or("tool"),
                    cut(what.lines().next().unwrap_or(""), 120)
                ));
            }
            _ => {}
        }
    }
    out
}

fn cut(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Sends one command that streams the run's news (`onEvent`) and prints it until the run ends or the terminal leaves.
/// `start`: the command's answer is not a view (plan_run_start), so the view is asked for once it answered.
fn follow(dir: &Path, cmd: &str, mut args: Value, start: bool, root: &str) -> Result<(), String> {
    let (stream, token) = ensure_service(dir)?;
    let mut w = stream.try_clone().map_err(|e| e.to_string())?;
    let pid = args["projectId"].clone();
    args["onEvent"] = json!("run");
    writeln!(w, "{}", json!({"token": token})).map_err(|e| e.to_string())?;
    writeln!(w, "{}", json!({"id": 1, "cmd": cmd, "args": args})).map_err(|e| e.to_string())?;
    let mut shown: Option<Value> = None;
    let mut out = std::io::stdout();
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let view = if msg["id"] == 1 || msg["id"] == 2 {
            if let Some(e) = msg.get("err") {
                return Err(e.as_str().unwrap_or("Error").to_string());
            }
            if start && msg["id"] == 1 {
                writeln!(
                    w,
                    "{}",
                    json!({"id": 2, "cmd": "plan_run_state", "args": {"projectId": pid}})
                )
                .map_err(|e| e.to_string())?;
                continue;
            }
            msg["ok"].clone()
        } else if msg["stream"] == "run" && msg["event"]["kind"] == "view" {
            msg["event"]["view"].clone()
        } else {
            if msg["stream"] == "run" && msg["event"]["kind"] == "agent" {
                for l in agent_lines(&msg["event"]["event"], root) {
                    let _ = writeln!(out, "{l}");
                }
                let _ = out.flush();
            }
            continue;
        };
        if shown.is_none() && view["running"] != true && view["finishing"] != true {
            println!(
                "{}",
                match view["run"].is_object() {
                    true => format!("No run is going on. The last one: {}", end_line(&view)),
                    false => "No run is going on, and this project has none before.".to_string(),
                }
            );
            break;
        }
        for l in view_lines(shown.as_ref(), &view) {
            let _ = writeln!(out, "{l}");
        }
        let _ = out.flush();
        let on = view["running"] == true || view["finishing"] == true;
        shown = Some(view);
        if !on {
            break;
        }
    }
    let _ = w.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn project_of(dir: &Path, which: Option<String>) -> Result<Value, String> {
    let list = list_of(call(dir, "projects_list", json!({}))?);
    if let Some(w) = which.filter(|w| !w.trim().is_empty()) {
        return find_project(&list, &w).cloned();
    }
    let active = list_of(call(dir, "plan_run_active", json!({}))?);
    let on: Vec<&Value> = list
        .iter()
        .filter(|p| active.iter().any(|a| *a == p["id"]))
        .collect();
    match on.as_slice() {
        [p] => Ok((*p).clone()),
        [] => Err("No plan run is going on.".into()),
        many => Err(format!(
            "Runs go on in {} projects ({}); name one.",
            many.len(),
            many.iter()
                .filter_map(|p| p["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The lines of `plan list`: each project's run going on or its last one.
pub fn list_lines(rows: &[(Value, Option<Value>, bool)], now: i64) -> Vec<String> {
    let mut out = Vec::new();
    for (p, last, on) in rows {
        let Some(view) = last else { continue };
        let run = &view["run"];
        let steps = run["steps"].as_array().cloned().unwrap_or_default();
        let done = steps.iter().filter(|s| s["status"] == "done").count();
        let at = steps.iter().find(|s| s["status"] == "running");
        let state = if *on {
            match at {
                Some(s) => format!("running {}", step_name(s)),
                None => "running".to_string(),
            }
        } else {
            let when = run["endedAt"]
                .as_f64()
                .or(run["startedAt"].as_f64())
                .map(|t| format!(", {}", ago(t as i64, now)))
                .unwrap_or_default();
            format!("{}{when}", run["status"].as_str().unwrap_or("stopped"))
        };
        out.push(format!(
            "{}  {}  {state}  ({done} of {} steps done, {})",
            p["name"].as_str().unwrap_or(""),
            run["file"].as_str().unwrap_or(""),
            steps.len(),
            money(run_cost(run))
        ));
    }
    if out.is_empty() {
        out.push("No plan runs yet. Start one: qanuvi-master plan run <plan file>".into());
    }
    out
}

/// `qanuvi-master plan …`; `args` are the words after `plan`.
pub fn plan_cli(dir: &Path, args: &[String]) -> Result<(), String> {
    let w = words(args);
    let rest = || w.get(1..).map(|r| r.join(" "));
    match w.first().copied() {
        Some("run") => {
            let typed = w.get(1).ok_or(USAGE)?;
            let file = std::path::absolute(Path::new(typed))
                .map_err(|e| format!("{typed} cannot be found: {e}"))?;
            if !file.is_file() {
                return Err(format!("There is no file {}.", file.display()));
            }
            let budget = match flag(args, "--budget") {
                Some(b) => Some(
                    b.trim_start_matches('$')
                        .parse::<f64>()
                        .ok()
                        .filter(|b| b.is_finite() && *b >= 0.0)
                        .ok_or_else(|| {
                            format!("\"{b}\" is not an amount of dollars, for example 2.50.")
                        })?,
                ),
                None => None,
            };
            let list = list_of(call(dir, "projects_list", json!({}))?);
            let (p, rel) = owning_project(&list, &file.to_string_lossy())?;
            let name = p["name"].as_str().unwrap_or("").to_string();
            let id = p["id"].as_str().unwrap_or("").to_string();
            if list_of(call(dir, "plan_run_active", json!({}))?)
                .iter()
                .any(|a| *a == id.as_str())
            {
                return Err(format!(
                    "A plan run is going on in {name}. Follow it: qanuvi-master plan follow \"{name}\"  Stop it: qanuvi-master plan stop \"{name}\""
                ));
            }
            let start = start_from(
                kept_start(dir, &id)?,
                &name,
                &rel,
                flag(args, "--steps"),
                budget,
            );
            // y9-1: the check before the run, as the window's Run panel makes it (--no-check leaves it out).
            let mut start = start;
            let check = !args.iter().any(|a| a == "--no-check");
            start["checkFirst"] = json!(check);
            let args_out = json!({"projectId": id, "start": start});
            println!("Plan run of {rel} in {name}. {}", setup_line(&start));
            if check {
                println!(
                    "First a read-only check of what the run needs (a few minutes: it reads the files the steps change); Qanuvi takes its \
                     recommended answers, and a step that truly needs you waits for your answer in the Inbox."
                );
            }
            if args.iter().any(|a| a == "--no-follow") {
                call(dir, "plan_run_start", args_out)?;
                println!("Started. Follow it: qanuvi-master plan follow \"{name}\"");
                return Ok(());
            }
            println!(
                "Ctrl+C leaves this terminal; the run goes on. Stop it: qanuvi-master plan stop \"{name}\""
            );
            follow(
                dir,
                "plan_run_start",
                args_out,
                true,
                p["path"].as_str().unwrap_or(""),
            )
        }
        Some("follow") => {
            let p = project_of(dir, rest())?;
            println!(
                "Following the run in {}. Ctrl+C leaves; the run goes on.",
                p["name"].as_str().unwrap_or("")
            );
            follow(
                dir,
                "plan_run_watch",
                json!({"projectId": p["id"]}),
                false,
                p["path"].as_str().unwrap_or(""),
            )
        }
        Some("list") | None => {
            let list = list_of(call(dir, "projects_list", json!({}))?);
            let active = list_of(call(dir, "plan_run_active", json!({}))?);
            let mut rows = Vec::new();
            for p in list {
                let on = active.iter().any(|a| *a == p["id"]);
                let v = call(dir, "plan_run_state", json!({"projectId": p["id"]}))?;
                let last = v["run"].is_object().then_some(v);
                rows.push((p, last, on));
            }
            // Runs going on first.
            rows.sort_by_key(|r| !r.2);
            list_lines(&rows, now_ms())
                .iter()
                .for_each(|l| println!("{l}"));
            Ok(())
        }
        Some("stop") => {
            let p = project_of(dir, rest())?;
            let name = p["name"].as_str().unwrap_or("");
            let active = list_of(call(dir, "plan_run_active", json!({}))?);
            if !active.iter().any(|a| *a == p["id"]) {
                println!("No plan run is going on in {name}.");
                return Ok(());
            }
            call(dir, "plan_run_stop", json!({"projectId": p["id"]}))?;
            if args.iter().any(|a| a == "--now") {
                // The second stop is at once (Controls::stop).
                call(dir, "plan_run_stop", json!({"projectId": p["id"]}))?;
                println!("Stopping the run in {name} now.");
            } else {
                println!(
                    "Stopping the run in {name}: the step's agent finishes the edit in hand first. At once: qanuvi-master plan stop \"{name}\" --now"
                );
            }
            Ok(())
        }
        Some(_) => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<Value> {
        vec![
            json!({"id": "a", "name": "Shop", "path": "E:\\Work\\shop"}),
            json!({"id": "b", "name": "Theme", "path": "E:\\Work\\shop\\theme"}),
            json!({"id": "c", "name": "Api", "path": "/srv/api"}),
        ]
    }

    #[test]
    fn a_plan_file_belongs_to_its_deepest_project() {
        let l = projects();
        let (p, rel) = owning_project(&l, "/srv/api/docs/PLAN.md").unwrap();
        assert_eq!(
            (p["id"].as_str(), rel.as_str()),
            (Some("c"), "docs/PLAN.md")
        );
        assert!(
            owning_project(&l, "/srv/apis/PLAN.md")
                .unwrap_err()
                .contains("projects add")
        );
        if cfg!(windows) {
            let (p, rel) = owning_project(&l, "e:\\work\\shop\\theme\\PLAN.md").unwrap();
            assert_eq!((p["id"].as_str(), rel.as_str()), (Some("b"), "PLAN.md"));
            let (p, _) = owning_project(&l, "E:\\Work\\shop\\PLAN-F.md").unwrap();
            assert_eq!(p["id"], "a");
        }
    }

    #[test]
    fn a_kept_start_becomes_this_runs() {
        let kept = json!({
            "projectName": "Old", "file": "OLD.md", "providerName": "Test Agent",
            "options": {"fromLine": 40, "onlyOne": true, "ranges": "3", "mode": "acceptEdits", "budgetUsd": 1.0},
            "resume": {"sessionId": "s"}
        });
        let s = start_from(kept.clone(), "Shop", "docs/PLAN.md", None, None);
        assert_eq!(s["file"], "docs/PLAN.md");
        assert_eq!(s["projectName"], "Shop");
        assert!(s["resume"].is_null());
        assert_eq!(s["options"]["fromLine"], 0);
        assert_eq!(s["options"]["onlyOne"], false);
        assert!(s["options"].get("ranges").is_none());
        assert_eq!(
            setup_line(&s),
            "Agent Test Agent, Edit automatically mode, budget $1.00."
        );
        let s = start_from(kept, "Shop", "P.md", Some(" 2-6 "), Some(0.0));
        assert_eq!(s["options"]["ranges"], "2-6");
        assert_eq!(
            setup_line(&s),
            "Agent Test Agent, Edit automatically mode, budget none, steps 2-6."
        );
        assert_eq!(
            start_from(json!({}), "X", "P.md", None, None)["options"]["fromLine"],
            0
        );
    }

    fn view(steps: Value, running: bool) -> Value {
        json!({"running": running, "currentModel": "model-a", "notices": [], "waitingAnswers": 0,
               "run": {"file": "PLAN.md", "status": if running { "running" } else { "done" }, "steps": steps}})
    }

    #[test]
    fn view_changes_read_as_lines() {
        let a = view(
            json!([{"number": "1", "title": "Add rows", "status": "running", "attempts": []},
                            {"number": "2", "title": "Docs", "status": "waiting", "attempts": []}]),
            true,
        );
        assert_eq!(view_lines(None, &a), ["> Step 1 · Add rows (on model-a)"]);
        let b = view(
            json!([{"number": "1", "title": "Add rows", "status": "done",
                             "attempts": [{"model": "m", "costUsd": 0.25, "note": ""}]},
                            {"number": "2", "title": "Docs", "status": "held", "attempts": []}]),
            true,
        );
        let l = view_lines(Some(&a), &b);
        assert_eq!(l[0], "  done: Step 1 · Add rows ($0.25)");
        assert!(l[1].contains("Step 2 · Docs waits for your answer"));
        let mut c = b.clone();
        c["running"] = json!(false);
        c["run"]["status"] = json!("done");
        c["notices"] = json!(["Checks passed."]);
        let l = view_lines(Some(&b), &c);
        assert_eq!(l, ["  note: Checks passed.", "The run is done ($0.25)."]);
        assert!(view_lines(Some(&c), &c).is_empty());
    }

    #[test]
    fn only_a_try_that_did_not_finish_says_it_tries_again() {
        let step = |tries: Value| json!([{"number": "1", "title": "A", "status": "running", "attempts": tries}]);
        let a = view(step(json!([])), true);
        let done = view(
            step(json!([{"model": "m", "outcome": "done", "note": ""}])),
            true,
        );
        assert!(view_lines(Some(&a), &done).is_empty());
        let open = view(
            step(json!([{"model": "m", "outcome": "open", "note": "tests fail."}])),
            true,
        );
        assert_eq!(
            view_lines(Some(&a), &open),
            ["  Step 1 · A tried on m: tests fail; it tries again."]
        );
    }

    #[test]
    fn the_end_says_why_it_stopped() {
        let mut v = view(json!([]), false);
        v["run"]["status"] = json!("stopped");
        v["run"]["budgetReached"] = json!(1.0);
        assert_eq!(
            end_line(&v),
            "The run stopped: it reached its budget of $1.00."
        );
        v["error"] = json!("no agent");
        assert_eq!(end_line(&v), "The run ended with an error: no agent");
    }

    #[test]
    fn agent_lines_show_text_and_tools() {
        let line = json!({"type": "assistant", "message": {"content": [
            {"type": "text", "text": "Reading the schema.\n\nThen the test."},
            {"type": "tool_use", "name": "Edit", "input": {"file_path": "/srv/api/src/db.rs"}},
            {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test\nmore"}}]}});
        let l = agent_lines(
            &json!({"kind": "line", "line": line.to_string()}),
            "/srv/api",
        );
        assert_eq!(
            l,
            [
                "    | Reading the schema.",
                "    | Then the test.",
                "    . Edit src/db.rs",
                "    . Bash cargo test"
            ]
        );
        assert!(agent_lines(&json!({"kind": "exit", "code": 0}), "").is_empty());
        assert!(
            agent_lines(&json!({"kind": "line", "line": "{\"type\":\"user\"}"}), "").is_empty()
        );
    }

    #[test]
    fn the_list_shows_runs_going_on_and_last_ones() {
        let on = view(
            json!([{"number": "1", "title": "A", "status": "done", "attempts": [{"costUsd": 0.5}]},
                             {"number": "2", "title": "B", "status": "running", "attempts": []}]),
            true,
        );
        let mut old = view(
            json!([{"title": "A", "status": "done", "attempts": []}]),
            false,
        );
        old["run"]["endedAt"] = json!(0.0);
        let rows = vec![
            (json!({"name": "Shop"}), Some(on), true),
            (json!({"name": "Api"}), Some(old), false),
            (json!({"name": "Empty"}), None, false),
        ];
        let l = list_lines(&rows, 2 * 3_600_000);
        assert_eq!(
            l,
            [
                "Shop  PLAN.md  running Step 2 · B  (1 of 2 steps done, $0.50)",
                "Api  PLAN.md  done, 2 h ago  (1 of 1 steps done, $0.00)",
            ]
        );
        assert!(list_lines(&[], 0)[0].contains("No plan runs yet"));
    }

    #[test]
    fn words_skip_the_plan_flags() {
        let a: Vec<String> = [
            "run",
            "PLAN.md",
            "--steps",
            "2-6",
            "--budget",
            "3",
            "--data",
            "/d",
            "--no-follow",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(words(&a), ["run", "PLAN.md"]);
        assert_eq!(flag(&a, "--steps"), Some("2-6"));
    }
}
