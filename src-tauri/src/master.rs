//! Qanuvi without a window (Milestone 16): a master on a Linux server or VPS that the desktop app controls over SSH.
//!
//! `qanuvi-master serve` runs in the background and keeps the work going (agent sessions continue when the desktop
//! disconnects; their output is stored). It listens only on 127.0.0.1, behind a token kept in a file only its user can
//! read. `qanuvi-master --stdio` is what the desktop app starts through SSH: it starts the service when needed and
//! passes messages between SSH and the service. So there is no open port, and SSH's own sign-in protects the way in.
//!
//! Messages are JSON, one per line. The desktop sends `{"id": 1, "cmd": "git_status", "args": {"projectId": …}}` with
//! the same command names and arguments as the window's own calls; the master answers `{"id": 1, "ok": …}` or
//! `{"id": 1, "err": "…"}`, and sends `{"stream": "s1", "event": …}` for a running terminal or agent session.

use crate::agent::{self, Agents};
use crate::sink::Sink;
use crate::store::Db;
use crate::terminal::Terminals;
use crate::{checks, files, git, index, paths, projects, runs, terminal};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a running master shares with its connections.
pub struct Master {
    db: Db,
    terminals: Terminals,
    agents: Agents,
    indexes: index::Indexes,
    /// Remote access through a relay (Milestone 20).
    pub relay: Arc<crate::relay_client::RelayMaster>,
    /// This master itself, for the relay link (set once it is shared).
    me: std::sync::OnceLock<std::sync::Weak<Master>>,
}

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn send_line(out: &Out, v: &Value) -> bool {
    let mut w = out.lock().unwrap_or_else(|e| e.into_inner());
    writeln!(w, "{v}").and_then(|_| w.flush()).is_ok()
}

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").to_string()
}

fn opt(args: &Value, k: &str) -> Option<String> {
    args[k].as_str().map(String::from)
}

fn b(args: &Value, k: &str) -> bool {
    args[k].as_bool().unwrap_or(false)
}

fn n(args: &Value, k: &str) -> u64 {
    args[k].as_u64().unwrap_or(0)
}

fn to<T: serde::Serialize>(r: Result<T, String>) -> Result<Value, String> {
    r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
}

/// Makes the sink for a stream name (terminal output, agent events).
type Streams<'a> = &'a dyn Fn(&str) -> Sink<Value>;

impl Master {
    pub fn open(dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        paths::set_data_dir(dir.to_path_buf());
        let db = Db::open(&dir.join("qanuvi.db")).map_err(|e| e.to_string())?;
        agent::mark_interrupted(&db);
        runs::mark_interrupted(&db);
        Ok(Self {
            db,
            terminals: Terminals::default(),
            agents: Agents::default(),
            indexes: index::Indexes::default(),
            relay: crate::relay_client::RelayMaster::open(dir),
            me: std::sync::OnceLock::new(),
        })
    }

    /// Shares the master and starts its relay link (when remote access is on).
    pub fn shared(self) -> Arc<Self> {
        let me = Arc::new(self);
        let _ = me.me.set(Arc::downgrade(&me));
        me.relay.start(me.clone());
        me
    }

    fn root(&self, args: &Value) -> Result<PathBuf, String> {
        Ok(PathBuf::from(
            projects::get(&self.db, &s(args, "projectId"))?.path,
        ))
    }

    /// Runs one command. `stream` makes a sink for commands that stream events (terminals, agent sessions);
    /// `opened_terminals` collects the terminals this connection opened, to close them when it ends.
    pub fn dispatch(
        &self,
        cmd: &str,
        args: &Value,
        stream: Streams,
        opened_terminals: &Mutex<HashSet<String>>,
    ) -> Result<Value, String> {
        let db = &self.db;
        match cmd {
            "hello" => Ok(json!({
                "name": "Qanuvi master",
                "version": env!("CARGO_PKG_VERSION"),
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "host": hostname(),
                "dataDir": paths::data_dir().to_string_lossy(),
            })),
            // Remote access through a relay (Milestone 20), managed from this computer's own window.
            "relay_status" => to(Ok(self.relay.status())),
            "relay_activity" => to(Ok(self.relay.activity())),
            "relay_configure" => {
                let me = self
                    .me
                    .get()
                    .and_then(|w| w.upgrade())
                    .ok_or("The master is not ready.")?;
                to(self.relay.configure(
                    &s(args, "relay"),
                    &s(args, "name"),
                    b(args, "enabled"),
                    me,
                ))
            }
            "relay_pair_start" => to(self.relay.pair_start()),
            "relay_device_remove" => {
                self.relay.remove_device(&s(args, "pairing"));
                Ok(Value::Null)
            }
            "relay_stop" => {
                self.relay.stop_sessions();
                Ok(Value::Null)
            }
            // Projects (folders on this master).
            "projects_list" => to(projects::list(db)),
            "projects_add" => to(projects::add(db, &s(args, "path"))),
            "projects_remove" => {
                let id = s(args, "id");
                self.terminals.kill_project(&id);
                self.agents.stop_project(&id);
                to(projects::remove(db, &id))
            }
            "projects_update" => {
                let settings: projects::ProjectSettings =
                    serde_json::from_value(args["settings"].clone()).map_err(|e| e.to_string())?;
                to(projects::update(
                    db,
                    &s(args, "id"),
                    &s(args, "name"),
                    &settings,
                ))
            }
            "projects_touch" => to(projects::touch(db, &s(args, "id"))),
            "projects_redetect" => to(projects::redetect(db, &s(args, "id"))),
            // Files.
            "files_list_dir" => to(files::list_dir(&self.root(args)?, &s(args, "path"))),
            "files_list_all" => {
                let (list, truncated) = files::list_files(&self.root(args)?)?;
                Ok(json!({"files": list, "truncated": truncated}))
            }
            "files_read" => to(files::read_file(&self.root(args)?, &s(args, "path"))),
            "files_create" => to(files::create(
                &self.root(args)?,
                &s(args, "path"),
                b(args, "folder"),
            )),
            "files_rename" => to(files::rename(
                &self.root(args)?,
                &s(args, "from"),
                &s(args, "to"),
            )),
            "files_trash" => to(files::trash(&self.root(args)?, &s(args, "path"))),
            "files_write" => to(files::write_file(
                &self.root(args)?,
                &s(args, "path"),
                &s(args, "text"),
                opt(args, "expectedHash").as_deref(),
            )),
            "files_search" => to(files::search(
                &self.root(args)?,
                &s(args, "query"),
                b(args, "regex"),
                b(args, "caseSensitive"),
                2000,
            )),
            "index_find" => {
                let root = self.root(args)?;
                self.indexes.with(&root, |idx| {
                    Value::Array(
                        idx.find(&s(args, "query"), None, 80)
                            .into_iter()
                            .map(|f| {
                                json!({"path": f.path, "name": f.symbol.name, "kind": f.symbol.kind,
                                       "line": f.symbol.line, "container": f.symbol.container})
                            })
                            .collect(),
                    )
                })
            }
            "index_outline" => {
                let root = self.root(args)?;
                let path = s(args, "path");
                to(self.indexes.with(&root, |idx| {
                    idx.outline(&path).map(|s| s.to_vec()).unwrap_or_default()
                }))
            }
            "checks_run" => {
                let timeout = Duration::from_secs(n(args, "timeoutSecs").clamp(5, 1800));
                to(checks::run(&self.root(args)?, &s(args, "command"), timeout))
            }
            // Terminals: closed when the connection that opened them ends.
            "term_shells" => to(Ok(terminal::shells())),
            "term_spawn" => {
                let root = self.root(args)?;
                let sink = stream(&s(args, "onEvent"));
                let term = Sink::new(move |e: terminal::TermEvent| {
                    sink.send(serde_json::to_value(e).unwrap_or(Value::Null))
                        .is_ok()
                });
                let cwd = files::folder_in(&root, opt(args, "dir").as_deref())?;
                let id = self.terminals.spawn(
                    &s(args, "projectId"),
                    &cwd,
                    &s(args, "shell"),
                    n(args, "cols").max(10) as u16,
                    n(args, "rows").max(4) as u16,
                    term,
                )?;
                opened_terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id.clone());
                Ok(json!(id))
            }
            "term_write" => to(self.terminals.write(&s(args, "id"), &s(args, "data"))),
            "term_resize" => to(self.terminals.resize(
                &s(args, "id"),
                n(args, "cols") as u16,
                n(args, "rows") as u16,
            )),
            "term_kill" => {
                self.terminals.kill(&s(args, "id"));
                Ok(Value::Null)
            }
            // Git.
            "git_status" => to(git::status(&self.root(args)?)),
            "git_diff" => to(git::diff(&self.root(args)?, &s(args, "path"))),
            "git_stage" => to(git::stage(&self.root(args)?, &s(args, "path"))),
            "git_revert" => to(git::revert(&self.root(args)?, &s(args, "path"))),
            "git_versions" => to(git::versions(&self.root(args)?, &s(args, "path"))),
            "git_set_index" => to(git::set_index(
                &self.root(args)?,
                &s(args, "path"),
                &s(args, "text"),
            )),
            "git_unstage" => to(git::unstage(&self.root(args)?, &s(args, "path"))),
            "git_stage_all" => to(git::stage_all(&self.root(args)?)),
            "git_commit" => to(git::commit(
                &self.root(args)?,
                &s(args, "message"),
                b(args, "amend"),
                b(args, "all"),
            )),
            "git_sync" => to(git::sync(&self.root(args)?, &s(args, "action"))),
            "git_branches" => to(git::branches(&self.root(args)?)),
            "git_switch" => to(git::switch(
                &self.root(args)?,
                &s(args, "name"),
                b(args, "create"),
            )),
            "git_stashes" => to(git::stashes(&self.root(args)?)),
            "git_stash" => to(git::stash(
                &self.root(args)?,
                &s(args, "action"),
                n(args, "index") as u32,
                &s(args, "message"),
            )),
            "git_log" => to(git::log(
                &self.root(args)?,
                opt(args, "path").as_deref(),
                n(args, "max").max(1) as u32,
            )),
            "git_show" => to(git::show(
                &self.root(args)?,
                &s(args, "sha"),
                opt(args, "path").as_deref(),
            )),
            "git_blame" => to(git::blame(
                &self.root(args)?,
                &s(args, "path"),
                n(args, "line") as u32,
            )),
            // Agent sessions: they keep running when the connection ends; their output is stored.
            "agent_info" => to(Ok(agent::info())),
            "agent_sessions" => to(agent::list_sessions(db, &s(args, "projectId"))),
            "agent_file_history" => to(agent::file_history(
                db,
                &s(args, "projectId"),
                &s(args, "path"),
            )),
            "agent_session_events" => to(agent::session_events(db, &s(args, "sessionId"))),
            "agent_start" => {
                let project = projects::get(db, &s(args, "projectId"))?;
                let sink = stream(&s(args, "onEvent"));
                let events = Sink::new(move |e: agent::AgentEvent| {
                    sink.send(serde_json::to_value(e).unwrap_or(Value::Null))
                        .is_ok()
                });
                to(self.agents.start(
                    db,
                    &project,
                    opt(args, "sessionId").as_deref(),
                    &s(args, "prompt"),
                    &s(args, "model"),
                    &s(args, "mode"),
                    opt(args, "baseUrl").as_deref(),
                    opt(args, "effort").as_deref(),
                    b(args, "lean"),
                    b(args, "indexTools"),
                    b(args, "browserTools"),
                    opt(args, "context").as_deref(),
                    &args
                        .get("files")
                        .filter(|v| !v.is_null())
                        .map(|v| serde_json::from_value::<Vec<agent::Attachment>>(v.clone()))
                        .transpose()
                        .map_err(|e| e.to_string())?
                        .unwrap_or_default(),
                    opt(args, "agent").as_deref().unwrap_or("claude-code"),
                    args.get("setup")
                        .filter(|v| !v.is_null())
                        .map(|v| crate::brain::with_keys(&v.to_string(), crate::brain::env_key))
                        .transpose()?
                        .as_deref(),
                    events,
                ))
            }
            "agent_respond" => to(self.agents.respond(
                db,
                &s(args, "sessionId"),
                &s(args, "requestId"),
                b(args, "allow"),
                &s(args, "remember"),
            )),
            "agent_grants" => to(Ok::<_, String>(self.agents.grants(&s(args, "sessionId")))),
            "agent_forget_grants" => {
                self.agents.forget_grants(&s(args, "sessionId"));
                to(Ok::<_, String>(()))
            }
            "agent_stop" => {
                agent::mark_stopped(db, &s(args, "sessionId"));
                self.agents.stop(&s(args, "sessionId"));
                Ok(Value::Null)
            }
            "agent_rename" => to(agent::rename(db, &s(args, "sessionId"), &s(args, "title"))),
            "agent_delete" => {
                self.agents.stop(&s(args, "sessionId"));
                to(agent::delete(db, &s(args, "sessionId")))
            }
            "agent_usage_since" => to(agent::usage_since(db, args["since"].as_i64().unwrap_or(0))),
            "browser_agent_view" => to(Ok(crate::preview::agent_view(&agent::browser_state_file(
                &s(args, "sessionId"),
            )?))),
            // Plan runs.
            "runs_save" => {
                projects::get(db, &s(args, "projectId"))?;
                to(runs::save(
                    db,
                    &s(args, "projectId"),
                    &s(args, "id"),
                    &s(args, "file"),
                    &s(args, "status"),
                    &s(args, "data"),
                ))
            }
            "runs_list" => to(runs::list(db, &s(args, "projectId"))),
            "runs_list_all" => to(runs::list_all(db, n(args, "limit").max(1) as usize)),
            _ => Err(format!("This master does not know the command {cmd}.")),
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .unwrap_or_default()
}

/// Answers one connection's messages until it ends, then closes the terminals it opened. Each command runs on its
/// own thread, so a long one (a test run, a push) does not hold up the others.
pub fn handle_connection(master: Arc<Master>, input: impl BufRead, out: Out) {
    let terminals: Arc<Mutex<HashSet<String>>> = Arc::default();
    let mut running = Vec::new();
    for line in input.lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let (master, out, terminals) = (master.clone(), out.clone(), terminals.clone());
        running.push(std::thread::spawn(move || {
            let id = msg["id"].clone();
            let cmd = msg["cmd"].as_str().unwrap_or("");
            let stream_out = out.clone();
            let stream = move |name: &str| {
                let (o, name) = (stream_out.clone(), name.to_string());
                Sink::new(move |event: Value| {
                    send_line(&o, &json!({"stream": name, "event": event}))
                })
            };
            let reply = match master.dispatch(cmd, &msg["args"], &stream, &terminals) {
                Ok(v) => json!({"id": id, "ok": v}),
                Err(e) => json!({"id": id, "err": e}),
            };
            send_line(&out, &reply);
        }));
        running.retain(|t| !t.is_finished());
    }
    // Commands still running finish first (agent sessions themselves go on in the background).
    for t in running {
        let _ = t.join();
    }
    for id in terminals.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        master.terminals.kill(id);
    }
}

/// Where the background service notes its address and token (readable by its user only).
fn service_file(dir: &Path) -> PathBuf {
    dir.join("master.json")
}

#[cfg(unix)]
fn private(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn private(_file: &Path) {}

/// Runs the background service until it is stopped.
pub fn serve(dir: &Path) -> Result<(), String> {
    let master = Master::open(dir)?.shared();
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let file = service_file(dir);
    let tmp = file.with_extension("tmp");
    // The file is made private before the token is written into it.
    std::fs::write(&tmp, "").map_err(|e| e.to_string())?;
    private(&tmp);
    std::fs::write(
        &tmp,
        json!({"port": port, "token": token, "pid": std::process::id()}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    for stream in listener.incoming().flatten() {
        let (master, token) = (master.clone(), token.clone());
        std::thread::spawn(move || {
            let Ok(write_half) = stream.try_clone() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut first = String::new();
            // The first line must carry the token; anything else is closed at once.
            if reader.read_line(&mut first).is_err() {
                return;
            }
            let hello: Value = serde_json::from_str(&first).unwrap_or(Value::Null);
            if hello["token"].as_str() != Some(token.as_str()) {
                return;
            }
            if hello["stop"] == true {
                // Agent sessions and terminals end with the service.
                master.agents.stop_all();
                master.terminals.kill_all();
                std::process::exit(0);
            }
            let out: Out = Arc::new(Mutex::new(Box::new(write_half)));
            handle_connection(master, reader, out);
        });
    }
    Ok(())
}

/// Connects to the running service (with its token), if there is one.
fn connect(dir: &Path) -> Option<(TcpStream, String)> {
    let info: Value =
        serde_json::from_str(&std::fs::read_to_string(service_file(dir)).ok()?).ok()?;
    let port = info["port"].as_u64()? as u16;
    let token = info["token"].as_str()?.to_string();
    let s =
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2)).ok()?;
    Some((s, token))
}

pub fn is_running(dir: &Path) -> bool {
    connect(dir).is_some()
}

/// Starts the background service, detached from this process and its terminal.
fn start_service(dir: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve")
        .arg("--data")
        .arg(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    cmd.spawn()
        .map_err(|e| format!("The master service could not start: {e}"))?;
    Ok(())
}

/// Connects to the service, starting it first when it is not running.
pub fn ensure_service(dir: &Path) -> Result<(TcpStream, String), String> {
    if let Some(c) = connect(dir) {
        return Ok(c);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(service_file(dir));
    start_service(dir)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Some(c) = connect(dir) {
            return Ok(c);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("The master service did not start in 10 seconds.".into())
}

/// `qanuvi-master --stdio`: passes messages between standard input/output (the SSH connection) and the service.
pub fn stdio_bridge(dir: &Path) -> Result<(), String> {
    let (stream, token) = ensure_service(dir)?;
    let mut to_service = stream.try_clone().map_err(|e| e.to_string())?;
    writeln!(to_service, "{}", json!({"token": token})).map_err(|e| e.to_string())?;
    let from_service = stream;
    let down = std::thread::spawn(move || {
        let mut out = std::io::stdout();
        let _ = std::io::copy(&mut BufReader::new(from_service), &mut out);
    });
    let mut input = std::io::stdin().lock();
    let _ = std::io::copy(&mut input, &mut to_service);
    // The desktop went away: close our side so the service ends this connection (and its terminals).
    let _ = to_service.shutdown(std::net::Shutdown::Both);
    let _ = down.join();
    Ok(())
}

/// Stops the running service, if any.
pub fn stop_service(dir: &Path) -> bool {
    if let Some((mut s, token)) = connect(dir) {
        let _ = writeln!(s, "{}", json!({"token": token, "stop": true}));
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_file(service_file(dir));
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(m: &Master, cmd: &str, args: Value) -> Result<Value, String> {
        let streams = |_: &str| Sink::new(|_: Value| true);
        m.dispatch(cmd, &args, &streams, &Mutex::new(HashSet::new()))
    }

    #[test]
    fn answers_the_same_commands_as_the_window() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("cart.php"),
            "<?php\nfunction total() {}\n",
        )
        .unwrap();
        let m = Master::open(data.path()).unwrap();
        assert_eq!(
            call(&m, "hello", json!({})).unwrap()["name"],
            "Qanuvi master"
        );
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        let list = call(&m, "projects_list", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        let entries = call(&m, "files_list_dir", json!({"projectId": id, "path": ""})).unwrap();
        assert_eq!(entries[0]["name"], "cart.php");
        let file = call(
            &m,
            "files_read",
            json!({"projectId": id, "path": "cart.php"}),
        )
        .unwrap();
        let hash = file["hash"].as_str().unwrap();
        call(
            &m,
            "files_write",
            json!({"projectId": id, "path": "cart.php", "text": "<?php\nfunction sum() {}\n", "expectedHash": hash}),
        )
        .unwrap();
        let found = call(&m, "index_find", json!({"projectId": id, "query": "sum"})).unwrap();
        assert_eq!(found[0]["line"], 2);
        let st = call(&m, "git_status", json!({"projectId": id})).unwrap();
        assert_eq!(st["is_repo"], false);
        assert!(call(&m, "files_read", json!({"projectId": "nope", "path": "x"})).is_err());
        assert!(
            call(&m, "no_such_command", json!({}))
                .unwrap_err()
                .contains("does not know")
        );
    }

    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_connection_answers_each_message_and_streams_events() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        let p = projects::add(&m.db, &project.path().to_string_lossy()).unwrap();
        let shell = terminal::shells()[0].id.clone();
        let lines = [
            json!({"id": 1, "cmd": "term_spawn", "args": {"projectId": p.id, "shell": shell, "cols": 80, "rows": 24, "onEvent": "t1"}}),
            json!({"id": 2, "cmd": "nope", "args": {}}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let captured: Arc<Mutex<Vec<u8>>> = Arc::default();
        let out: Out = Arc::new(Mutex::new(Box::new(Captured(captured.clone()))));
        handle_connection(m.clone(), std::io::Cursor::new(lines.into_bytes()), out);
        let text = String::from_utf8_lossy(&captured.lock().unwrap()).to_string();
        let replies: Vec<Value> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let one = replies.iter().find(|r| r["id"] == 1).expect("answer 1");
        assert!(one["ok"].is_string(), "the terminal's id: {one}");
        let two = replies.iter().find(|r| r["id"] == 2).expect("answer 2");
        assert!(two["err"].as_str().unwrap().contains("nope"));
        // The terminal ends with the connection.
        assert!(m.terminals.write(one["ok"].as_str().unwrap(), "x").is_err());
    }

    #[test]
    fn the_service_starts_answers_through_its_token_and_stops() {
        // The service is started from the test binary itself only when it is the real program; here the parts are
        // tested directly: a private service file, a refused connection without the token.
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().to_path_buf();
        let d2 = dir.clone();
        std::thread::spawn(move || {
            let _ = serve(&d2);
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !is_running(&dir) {
            assert!(Instant::now() < deadline, "the service did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Without the token: closed.
        let info: Value =
            serde_json::from_str(&std::fs::read_to_string(service_file(&dir)).unwrap()).unwrap();
        let port = info["port"].as_u64().unwrap() as u16;
        let mut bad = TcpStream::connect(("127.0.0.1", port)).unwrap();
        writeln!(bad, "{}", json!({"token": "wrong"})).unwrap();
        bad.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut answer = String::new();
        let n = BufReader::new(bad).read_line(&mut answer).unwrap_or(0);
        assert_eq!(n, 0, "a wrong token gets no answer");
        // With the token: answers.
        let (s, token) = connect(&dir).unwrap();
        let mut w = s.try_clone().unwrap();
        writeln!(w, "{}", json!({"token": token})).unwrap();
        writeln!(w, "{}", json!({"id": 7, "cmd": "hello", "args": {}})).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["ok"]["name"], "Qanuvi master");
    }
}
