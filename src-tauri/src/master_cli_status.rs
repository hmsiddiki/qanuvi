// A master from its own terminal, part two (migration M7a): `qanuvi-master status` says what the running master is
// and does (version, account, projects, servers, work, Inbox), and `qanuvi-master projects list | add | remove`
// keeps its project list. Both are thin clients of the running service over the same commands a window sends, so a
// project added here shows in any window showing this master.

use super::cli::call;
use super::{Value, json};
use std::path::Path;

const USAGE: &str = "Usage: qanuvi-master projects list
       qanuvi-master projects add <folder>
       qanuvi-master projects remove <name, folder or id>";

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What `status` prints for a running master, from the service's own answers.
#[allow(clippy::too_many_arguments)]
/// `account`: account_me's answer, or its error ("Not signed in." when none is kept).
pub fn status_lines(
    dir: &Path,
    hello: &Value,
    account: &Result<Value, String>,
    projects: usize,
    servers: &[Value],
    states: &Value,
    busy: &Value,
    inbox: &[Value],
) -> Vec<String> {
    let mut out = vec![
        format!(
            "Running: Qanuvi master {} on {} ({}/{}).",
            hello["version"].as_str().unwrap_or("?"),
            hello["host"].as_str().unwrap_or("this computer"),
            hello["os"].as_str().unwrap_or("?"),
            hello["arch"].as_str().unwrap_or("?"),
        ),
        format!("Data: {}", dir.display()),
    ];
    out.push(match account {
        Ok(a) => {
            let name = a["name"].as_str().unwrap_or("");
            let email = a["email"].as_str().unwrap_or("");
            let who = match (name.is_empty(), email.is_empty()) {
                (false, false) => format!("{name} <{email}>"),
                (false, true) => name.to_string(),
                _ => email.to_string(),
            };
            format!(
                "Account: {who} at {}.",
                a["site"].as_str().unwrap_or("the account site")
            )
        }
        Err(e) if e == "Not signed in." => {
            "Account: not signed in. Sign in: qanuvi-master login".to_string()
        }
        Err(e) => format!("Account: could not be checked ({e})."),
    });
    out.push(format!(
        "Projects: {}.",
        if projects == 0 {
            "none yet (qanuvi-master projects add <folder>)".to_string()
        } else {
            projects.to_string()
        }
    ));
    let connected = servers
        .iter()
        .filter(|s| states[s["id"].as_str().unwrap_or("")]["state"] == "on")
        .count();
    out.push(if servers.is_empty() {
        "Servers: none.".to_string()
    } else {
        format!(
            "Servers: {}, {connected} connected (qanuvi-master servers list).",
            servers.len()
        )
    });
    let agents = busy["agents"].as_u64().unwrap_or(0) as usize;
    let runs = busy["runs"].as_u64().unwrap_or(0) as usize;
    out.push(if agents + runs == 0 {
        "Working: nothing right now.".to_string()
    } else {
        format!(
            "Working: {}, {}.",
            plural(agents, "agent", "agents"),
            plural(runs, "plan run", "plan runs")
        )
    });
    let open: Vec<&Value> = inbox.iter().filter(|i| i["status"] == "open").collect();
    let asks = open.iter().filter(|i| i["type"] == "answer").count();
    out.push(if open.is_empty() {
        "Inbox: nothing open.".to_string()
    } else if asks == 0 {
        format!("Inbox: {} open.", plural(open.len(), "item", "items"))
    } else {
        format!(
            "Inbox: {} open, {} waiting for your answer.",
            plural(open.len(), "item", "items"),
            asks
        )
    });
    out
}

fn list_of(v: Value, key: Option<&str>) -> Vec<Value> {
    let v = match key {
        Some(k) => v[k].clone(),
        None => v,
    };
    v.as_array().cloned().unwrap_or_default()
}

/// `qanuvi-master status`. A master that is not running is not started to answer.
pub fn status_cli(dir: &Path) -> Result<(), String> {
    if !super::is_running(dir) {
        println!("Not running. Start it: qanuvi-master serve");
        return Ok(());
    }
    let hello = call(dir, "hello", json!({}))?;
    let account = call(dir, "account_me", json!({}));
    let projects = list_of(call(dir, "projects_list", json!({}))?, None).len();
    let servers = list_of(call(dir, "servers_list", json!({}))?, Some("servers"));
    let states = if servers.is_empty() {
        json!({})
    } else {
        call(dir, "remote_states", json!({}))?
    };
    let busy = call(dir, "master_busy", json!({}))?;
    let inbox = list_of(call(dir, "inbox_list", json!({}))?, None);
    let mut lines = status_lines(
        dir, &hello, &account, projects, &servers, &states, &busy, &inbox,
    );
    // M8a: what its own look on the update channel found (an older master has no such answer, and no line).
    if let Ok(update) = call(dir, "update_status", json!({})) {
        lines.insert(1, crate::master_update::status_line(&update));
    }
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// M8a: the line every terminal command starts with when a newer version is out for this machine. Only a running
/// master is asked (none is started for it), and a master that cannot answer says nothing.
pub fn update_notice(dir: &Path) {
    if !super::is_running(dir) {
        return;
    }
    if let Some(line) = call(dir, "update_status", json!({}))
        .ok()
        .and_then(|s| crate::master_update::notice(&s))
    {
        eprintln!("{line}");
    }
}

/// M8b: `qanuvi-master update`. A running master puts the update in itself (this follows it; Ctrl+C leaves, and the
/// master goes on); with none running, it is put in here. M8d: This PC's master puts the whole app in, through its
/// running service only (the installer replaces the app and the master, and starts Qanuvi again).
pub fn update_cli(dir: &Path) -> Result<(), String> {
    use crate::master_update as mu;
    let whole = super::on_app_data(dir);
    if !super::is_running(dir) {
        if whole {
            return Err(
                "This PC's Qanuvi is not running. Start Qanuvi, then run this again (its update goes in through it)."
                    .into(),
            );
        }
        return update_here();
    }
    let state = call(dir, "update_check", json!({}))?;
    let under_way = state["install"]["stage"].as_str().is_some_and(|s| {
        matches!(
            s,
            "downloading" | "checking" | "ready" | "waiting" | "restarting"
        )
    });
    if !under_way && mu::notice(&state).is_none() {
        println!("{}", mu::status_line(&state));
        return if state["error"].as_str().unwrap_or("").is_empty() {
            Ok(())
        } else {
            Err("Nothing was changed.".into())
        };
    }
    let started = call(dir, "update_install", json!({}))?;
    follow_update(dir, &started["install"], whole)
}

/// Prints the update's stages until the service runs the new version (or says why it failed). `whole`: This PC's
/// whole install (M8d), which keeps no `.prev`.
fn follow_update(dir: &Path, started: &Value, whole: bool) -> Result<(), String> {
    use crate::master_update as mu;
    use std::io::{IsTerminal, Write};
    use std::time::{Duration, Instant};
    let version = started["version"].as_str().unwrap_or("");
    // The service's own program (on Linux this terminal's path names the kept one once the swap is done).
    let prev = Path::new(started["program"].as_str().unwrap_or("qanuvi-master"));
    let prev = mu::prev_path(prev).display().to_string();
    let live = std::io::stdout().is_terminal();
    let (mut said, mut quarter, mut on_line) = (String::new(), 0u64, false);
    if let Some(line) = mu::install_line(started) {
        println!("{line}");
        said = line;
    }
    let mut gone_at: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(400));
        // M8d drive: This PC's install ends its service for the installer, which starts Qanuvi again. Following it
        // further would start the old service again (a call starts one when none runs) and keep the master's program
        // in use while the installer replaces it; this terminal is a program the installer waits for too.
        if whole && !super::is_running(dir) {
            if on_line {
                println!();
            }
            println!(
                "Qanuvi closes to put {version} in; the installer starts it again by itself. `qanuvi-master version` says {version} once it is done."
            );
            return Ok(());
        }
        let Ok(state) = call(dir, "update_status", json!({})) else {
            // The old service has ended: wait for the new one.
            let since = *gone_at.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(90) {
                return Err(format!(
                    "The service has not come back after the update to {version} yet; `qanuvi-master status` shows when it does."
                ));
            }
            continue;
        };
        let install = &state["install"];
        if state["current"] == version {
            if on_line {
                println!();
            }
            if whole {
                println!("Updated to {version}: Qanuvi and its background part run it.");
            } else {
                println!(
                    "Updated to {version}: the service runs it. The program before is kept as {prev}."
                );
            }
            return Ok(());
        }
        if install["stage"] == "failed" {
            if on_line {
                println!();
            }
            return Err(format!(
                "The update failed: {} Nothing was changed.",
                install["error"].as_str().unwrap_or("no reason given.")
            ));
        }
        if let Some(line) = mu::install_line(install)
            && line != said
        {
            if on_line {
                println!();
                on_line = false;
            }
            println!("{line}");
            said = line;
        }
        if install["stage"] == "downloading" {
            let (done, total) = (
                install["done"].as_u64().unwrap_or(0),
                install["total"].as_u64().unwrap_or(0),
            );
            if live {
                print!("\r  {}   ", mu::progress_text(done, total));
                let _ = std::io::stdout().flush();
                on_line = true;
            } else if total > 0 && done * 4 / total > quarter {
                quarter = done * 4 / total;
                println!("  {}", mu::progress_text(done, total));
            }
        }
    }
}

/// No service runs: the update is put in by this terminal (nothing works, so nothing waits).
fn update_here() -> Result<(), String> {
    use crate::master_update as mu;
    use std::io::{IsTerminal, Write};
    let state = mu::Updates::default().check(false);
    let found = match &state.available {
        Some(f) if !f.url.is_empty() => f.clone(),
        _ => {
            println!(
                "{}",
                mu::status_line(&serde_json::to_value(&state).unwrap_or(Value::Null))
            );
            return if state.error.is_empty() {
                Ok(())
            } else {
                Err("Nothing was changed.".into())
            };
        }
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    println!("Downloading {}.", found.version);
    let live = std::io::stdout().is_terminal();
    let new = mu::prepare(
        &found,
        &exe,
        &mu::trusted_key(),
        |done, total| {
            if live {
                print!("\r  {}   ", mu::progress_text(done, total));
                let _ = std::io::stdout().flush();
            }
        },
        || {
            if live {
                println!();
            }
            println!("Checking its signature, and that it starts on this machine.");
        },
    )
    .map_err(|e| format!("The update failed: {e} Nothing was changed."))?;
    mu::swap_in(&exe, &new).map_err(|e| format!("The update failed: {e} Nothing was changed."))?;
    println!(
        "Updated to {}. The program before is kept as {}. The service does not run; `qanuvi-master serve` starts it.",
        found.version,
        mu::prev_path(&exe).display()
    );
    Ok(())
}

/// The project a typed word means: its id, its folder, or its name (letter case aside; a name two projects share
/// asks for the folder or id instead).
pub fn find_project<'a>(list: &'a [Value], which: &str) -> Result<&'a Value, String> {
    let which = which.trim();
    let same_path = |p: &Value| {
        let path = p["path"].as_str().unwrap_or("");
        path.eq_ignore_ascii_case(which)
            || path.trim_end_matches(['/', '\\']) == which.trim_end_matches(['/', '\\'])
    };
    if let Some(p) = list
        .iter()
        .find(|p| p["id"] == which)
        .or_else(|| list.iter().find(|p| same_path(p)))
    {
        return Ok(p);
    }
    let named: Vec<&Value> = list
        .iter()
        .filter(|p| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.trim().eq_ignore_ascii_case(which))
        })
        .collect();
    match named.as_slice() {
        [p] => Ok(p),
        [] => Err(format!(
            "No project named \"{which}\". See: qanuvi-master projects list"
        )),
        _ => Err(format!(
            "{} projects are named \"{which}\"; give the folder or the id instead.",
            named.len()
        )),
    }
}

/// A typed folder made whole from where the terminal is (the service runs elsewhere, so a relative one would not
/// mean the same folder there).
fn whole(folder: &str) -> Result<String, String> {
    let p = Path::new(folder.trim());
    std::path::absolute(p)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| format!("{folder} cannot be found: {e}"))
}

/// `qanuvi-master projects …`; `args` are the words after `projects`.
pub fn projects_cli(dir: &Path, args: &[String]) -> Result<(), String> {
    let w = super::cli::words(args);
    match w.first().copied() {
        Some("list") | None => {
            let list = list_of(call(dir, "projects_list", json!({}))?, None);
            if list.is_empty() {
                println!("No projects yet. Add one: qanuvi-master projects add <folder>");
            }
            for p in &list {
                println!(
                    "{}  {}  ({})  [{}]",
                    p["name"].as_str().unwrap_or(""),
                    p["path"].as_str().unwrap_or(""),
                    p["kind"].as_str().unwrap_or("folder"),
                    p["id"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        Some("add") => {
            let folder = whole(w.get(1).ok_or(USAGE)?)?;
            let before: Vec<String> = list_of(call(dir, "projects_list", json!({}))?, None)
                .iter()
                .filter_map(|p| p["id"].as_str().map(String::from))
                .collect();
            let p = call(dir, "projects_add", json!({"path": folder}))?;
            let name = p["name"].as_str().unwrap_or("");
            let path = p["path"].as_str().unwrap_or("");
            if p["id"]
                .as_str()
                .is_some_and(|id| before.iter().any(|b| b == id))
            {
                println!("{name} ({path}) is already one of this master's projects.");
            } else {
                println!("Added {name} ({path}).");
            }
            Ok(())
        }
        Some("remove") => {
            let which = w[1..].join(" ");
            if which.trim().is_empty() {
                return Err(USAGE.into());
            }
            let list = list_of(call(dir, "projects_list", json!({}))?, None);
            let p = find_project(&list, &which)?;
            call(dir, "projects_remove", json!({"id": p["id"]}))?;
            println!(
                "Removed {} from this master's projects. Its files stay where they are.",
                p["name"].as_str().unwrap_or("")
            );
            Ok(())
        }
        Some(_) => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(
        account: Result<Value, String>,
        servers: Vec<Value>,
        busy: Value,
        inbox: Vec<Value>,
    ) -> Vec<String> {
        status_lines(
            Path::new("/d"),
            &json!({"version": "0.1.25", "host": "box", "os": "linux", "arch": "x86_64"}),
            &account,
            2,
            &servers,
            &json!({"a": {"state": "on"}, "b": {"state": "error"}}),
            &busy,
            &inbox,
        )
    }

    #[test]
    fn status_says_what_runs_and_waits() {
        let l = lines(
            Ok(json!({"name": "Test Owner", "email": "", "site": "https://example.test"})),
            vec![json!({"id": "a"}), json!({"id": "b"})],
            json!({"agents": 1, "runs": 0}),
            vec![
                json!({"status": "open", "type": "answer"}),
                json!({"status": "open", "type": "info"}),
                json!({"status": "answered", "type": "answer"}),
            ],
        );
        assert_eq!(l[0], "Running: Qanuvi master 0.1.25 on box (linux/x86_64).");
        assert_eq!(l[2], "Account: Test Owner at https://example.test.");
        assert_eq!(l[3], "Projects: 2.");
        assert_eq!(
            l[4],
            "Servers: 2, 1 connected (qanuvi-master servers list)."
        );
        assert_eq!(l[5], "Working: 1 agent, 0 plan runs.");
        assert_eq!(l[6], "Inbox: 2 items open, 1 waiting for your answer.");
    }

    #[test]
    fn status_when_signed_out_and_idle() {
        let l = lines(
            Err("Not signed in.".into()),
            vec![],
            json!({"agents": 0, "runs": 0}),
            vec![],
        );
        assert_eq!(l[2], "Account: not signed in. Sign in: qanuvi-master login");
        assert_eq!(l[4], "Servers: none.");
        assert_eq!(l[5], "Working: nothing right now.");
        assert_eq!(l[6], "Inbox: nothing open.");
        let off = lines(Err("offline".into()), vec![], json!({}), vec![]);
        assert_eq!(off[2], "Account: could not be checked (offline).");
    }

    #[test]
    fn projects_are_found_by_id_folder_or_one_name() {
        let list = vec![
            json!({"id": "1", "name": "shop", "path": "E:\\work\\shop"}),
            json!({"id": "2", "name": "site", "path": "/srv/site"}),
            json!({"id": "3", "name": "Site", "path": "/srv/site2"}),
        ];
        assert_eq!(find_project(&list, "1").unwrap()["id"], "1");
        assert_eq!(find_project(&list, "SHOP").unwrap()["id"], "1");
        assert_eq!(find_project(&list, "e:\\work\\shop").unwrap()["id"], "1");
        assert_eq!(find_project(&list, "/srv/site/").unwrap()["id"], "2");
        assert!(
            find_project(&list, "site")
                .unwrap_err()
                .contains("2 projects")
        );
        assert!(
            find_project(&list, "nope")
                .unwrap_err()
                .contains("No project")
        );
    }
}
