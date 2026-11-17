// A master's server list from its own terminal (migration M3b): `qanuvi-master servers list | add | remove` and
// `qanuvi-master pair`. Each is a thin client of the running service (started when needed), using the same
// servers_* and remote_* commands a window sends, so a server added here is connected by the service and shows in any
// window showing this master. Names follow the window's rules (machineNames.ts): at most 40 characters, none seen
// twice (letter case aside), never "This PC".

use super::{Value, ensure_service, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

const NAME_MAX: usize = 40;
const RESERVED: [&str; 2] = ["this pc", "this computer"];
/// The window's colours (machineNames.ts COLORS); a new machine gets the first one no other has.
const COLORS: [&str; 8] = [
    "#3b82f6", "#a855f7", "#14b8a6", "#f97316", "#ec4899", "#84cc16", "#06b6d4", "#eab308",
];

fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// What is wrong with a typed name, or None.
pub fn name_problem(name: &str, others: &[String]) -> Option<String> {
    let n = name.trim();
    if n.chars()
        .any(|c| c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{feff}'))
    {
        return Some("The name has a character that cannot be seen; type it again.".into());
    }
    let len = n.chars().count();
    if len > NAME_MAX {
        return Some(format!(
            "The name has {len} characters; at most {NAME_MAX}."
        ));
    }
    if RESERVED.contains(&key(n).as_str()) {
        return Some(format!(
            "\"{n}\" is what Qanuvi calls the window's own computer; choose another name."
        ));
    }
    others
        .iter()
        .find(|o| key(o) == key(n))
        .map(|o| format!("Another machine is already named \"{o}\"."))
}

/// A name from what a machine says about itself, numbered when taken ("basevm378 (2)").
pub fn unique_name(from: &str, others: &[String]) -> String {
    let base: String = from
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(NAME_MAX)
        .collect::<String>()
        .trim()
        .to_string();
    if base.is_empty() {
        return base;
    }
    let free =
        |n: &str| !RESERVED.contains(&key(n).as_str()) && !others.iter().any(|o| key(o) == key(n));
    if free(&base) {
        return base;
    }
    (2..)
        .map(|i| {
            let tail = format!(" ({i})");
            let head: String = base.chars().take(NAME_MAX - tail.len()).collect();
            format!("{}{tail}", head.trim())
        })
        .find(|n| free(n))
        .unwrap_or_default()
}

/// A server's name as shown: as typed, or its address.
fn shown(server: &Value) -> String {
    let name = server["name"].as_str().unwrap_or("");
    if !name.is_empty() {
        return name.to_string();
    }
    if server["relay"].is_object() {
        "A computer".into()
    } else {
        server["target"].as_str().unwrap_or("").to_string()
    }
}

/// One command to the running service, as a window sends it.
pub fn call(dir: &Path, cmd: &str, args: Value) -> Result<Value, String> {
    let (stream, token) = ensure_service(dir)?;
    let mut w = stream.try_clone().map_err(|e| e.to_string())?;
    writeln!(w, "{}", json!({"token": token})).map_err(|e| e.to_string())?;
    writeln!(w, "{}", json!({"id": 1, "cmd": cmd, "args": args})).map_err(|e| e.to_string())?;
    for line in BufReader::new(stream).lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if msg["id"] == 1 {
            let _ = w.shutdown(std::net::Shutdown::Both);
            return match msg.get("err") {
                Some(e) => Err(e.as_str().unwrap_or("Error").to_string()),
                None => Ok(msg["ok"].clone()),
            };
        }
    }
    Err("The master service ended before it answered.".into())
}

fn servers(dir: &Path) -> Result<Vec<Value>, String> {
    Ok(call(dir, "servers_list", json!({}))?["servers"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// The value after a flag (`--name VPS`).
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// The words that are not flags or flag values.
fn words(args: &[String]) -> Vec<&str> {
    const VALUED: [&str; 7] = [
        "--data",
        "--site",
        "--name",
        "--program",
        "--key",
        "--relay",
        "--code",
    ];
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if VALUED.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            out.push(a.as_str());
        }
    }
    out
}

/// After the first hello: the lasting id, the computer name when none was given, a colour. A machine already in the
/// list (the same lasting id) is taken out again. Answers what to print.
fn settle(dir: &Path, server: &mut Value, info: &Value) -> Result<String, String> {
    let list = servers(dir)?;
    let id = server["id"].as_str().unwrap_or("").to_string();
    let others: Vec<&Value> = list.iter().filter(|s| s["id"] != id.as_str()).collect();
    if let Some(mid) = info["machineId"].as_str()
        && let Some(twin) = others.iter().find(|s| s["machineId"].as_str() == Some(mid))
    {
        call(dir, "servers_remove", json!({"id": id}))?;
        return Err(format!(
            "This is the same computer as {}. It was not added again.",
            shown(twin)
        ));
    }
    if let Some(mid) = info["machineId"].as_str() {
        server["machineId"] = json!(mid);
    }
    let names: Vec<String> = others.iter().map(|s| shown(s)).collect();
    if server["name"].as_str().unwrap_or("").is_empty()
        && let Some(host) = info["host"].as_str()
    {
        server["name"] = json!(unique_name(host, &names));
    }
    let used: Vec<&str> = others.iter().filter_map(|s| s["color"].as_str()).collect();
    let color = COLORS
        .iter()
        .find(|c| !used.contains(c))
        .unwrap_or(&COLORS[used.len() % COLORS.len()]);
    server["color"] = json!(color);
    call(dir, "servers_save", json!({"server": server}))?;
    Ok(format!(
        "{} is connected ({} · {}/{} · master {}).",
        shown(server),
        info["host"].as_str().unwrap_or("server"),
        info["os"].as_str().unwrap_or("?"),
        info["arch"].as_str().unwrap_or("?"),
        info["version"].as_str().unwrap_or("?"),
    ))
}

/// Saves a new server, connects to it and says how that went.
fn add_and_connect(dir: &Path, mut server: Value) -> Result<(), String> {
    call(dir, "servers_save", json!({"server": server}))?;
    match call(dir, "remote_connect", server.clone()) {
        Ok(info) => println!("{}", settle(dir, &mut server, &info)?),
        Err(e) => println!(
            "Added {}, but it did not connect: {e}\nThe master tries it again every minute.",
            shown(&server)
        ),
    }
    Ok(())
}

const USAGE: &str = "Usage: qanuvi-master servers list
       qanuvi-master servers add <ssh address> [--name <name>] [--program <path>] [--key <ssh key file>] [--no-auto]
       qanuvi-master servers remove <name or id>
       qanuvi-master pair --relay <address> --code <pairing code> [--name <this machine's name there>]";

/// `qanuvi-master servers …`; `args` are the words after `servers`.
pub fn servers_cli(dir: &Path, args: &[String]) -> Result<(), String> {
    let w = words(args);
    match w.first().copied() {
        Some("list") | None => {
            let list = servers(dir)?;
            if list.is_empty() {
                println!("No servers yet. Add one: qanuvi-master servers add <ssh address>");
                return Ok(());
            }
            let states = call(dir, "remote_states", json!({}))?;
            for s in &list {
                let id = s["id"].as_str().unwrap_or("");
                let st = &states[id];
                let how = if s["relay"].is_object() {
                    "through the relay".to_string()
                } else {
                    s["target"].as_str().unwrap_or("").to_string()
                };
                let state = match st["state"].as_str() {
                    Some("on") => "connected".to_string(),
                    Some("error") => format!("away: {}", st["error"].as_str().unwrap_or("")),
                    _ => "not connected".to_string(),
                };
                println!("{}  ({how})  {state}  [{id}]", shown(s));
            }
            Ok(())
        }
        Some("add") => {
            let target = w.get(1).ok_or(USAGE)?;
            crate::remote::check_target(target)?;
            let name = flag(args, "--name").unwrap_or("").trim().to_string();
            let names: Vec<String> = servers(dir)?.iter().map(shown).collect();
            if !name.is_empty()
                && let Some(p) = name_problem(&name, &names)
            {
                return Err(p);
            }
            let program = flag(args, "--program").unwrap_or("qanuvi-master");
            crate::remote::check_command(program)?;
            let identity = flag(args, "--key").unwrap_or("");
            if !identity.is_empty() {
                crate::remote::key_file(identity)?;
            }
            add_and_connect(
                dir,
                json!({
                    "id": uuid::Uuid::new_v4().to_string(),
                    "name": name,
                    "target": target,
                    "program": program,
                    "identity": identity,
                    "autoConnect": !args.iter().any(|a| a == "--no-auto"),
                }),
            )
        }
        Some("remove") => {
            let which = w.get(1).ok_or(USAGE)?;
            let list = servers(dir)?;
            let s = list
                .iter()
                .find(|s| s["id"] == *which)
                .or_else(|| list.iter().find(|s| key(&shown(s)) == key(which)))
                .ok_or_else(|| {
                    format!("No server named \"{which}\". See: qanuvi-master servers list")
                })?;
            if let Some(p) = s["relay"]["pairing"].as_str() {
                call(dir, "remote_unpair", json!({"pairing": p}))?;
            }
            call(dir, "servers_remove", json!({"id": s["id"]}))?;
            println!("Removed {}. Nothing on it was changed.", shown(s));
            Ok(())
        }
        Some(_) => Err(USAGE.into()),
    }
}

/// `qanuvi-master pair --relay <address> --code <code>`: pairs this master with a computer showing a pairing code.
pub fn pair_cli(dir: &Path, args: &[String]) -> Result<(), String> {
    let relay = flag(args, "--relay").ok_or(USAGE)?;
    let code = flag(args, "--code").ok_or(USAGE)?;
    let name = flag(args, "--name").unwrap_or("");
    let p = call(
        dir,
        "remote_pair",
        json!({"relay": relay, "code": code, "name": name}),
    )?;
    let names: Vec<String> = servers(dir)?.iter().map(shown).collect();
    add_and_connect(
        dir,
        json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "name": unique_name(p["masterName"].as_str().unwrap_or(""), &names),
            "target": "relay",
            "program": "",
            "identity": "",
            "autoConnect": true,
            "relay": {"url": p["relay"], "masterId": p["masterId"], "pairing": p["pairing"]},
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_windows_rules() {
        let others = vec!["VPS".to_string(), "basevm378".to_string()];
        assert!(
            name_problem("vps", &others)
                .unwrap()
                .contains("already named \"VPS\"")
        );
        assert!(name_problem("This PC", &others).is_some());
        assert!(name_problem(&"x".repeat(41), &others).is_some());
        assert!(name_problem("a\u{200b}b", &others).is_some());
        assert_eq!(name_problem("Laptop", &others), None);
        assert_eq!(unique_name("basevm378", &others), "basevm378 (2)");
        assert_eq!(unique_name("  shop  ", &others), "shop");
        assert_eq!(unique_name("this pc", &[]), "this pc (2)");
        assert_eq!(unique_name(&"y".repeat(50), &[]).chars().count(), 40);
    }

    #[test]
    fn words_leave_out_flags_and_their_values() {
        let a: Vec<String> = [
            "add",
            "--name",
            "My VPS",
            "root@h",
            "--data",
            "/d",
            "--no-auto",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(words(&a), ["add", "root@h"]);
        assert_eq!(flag(&a, "--name"), Some("My VPS"));
    }
}
