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

#[path = "master_cli.rs"]
mod cli;
#[path = "master_inbox.rs"]
mod inbox_cmds;
#[path = "master_runs.rs"]
mod plan_runs;
#[path = "master_servers.rs"]
mod server_cmds;
pub use cli::{pair_cli, servers_cli};

/// What a running master shares with its connections.
pub struct Master {
    /// Its data folder (the one it was opened on; paths keeps the first one a process sets).
    dir: PathBuf,
    /// Its lasting id (kept in its data folder), answered in hello so a window knows the same machine again.
    machine_id: String,
    /// Opened on the desktop app's own data folder (remote access, Milestone 20): it is This PC, so it keeps This
    /// PC's key names and leaves the start-up list, watches, syncs and backups to the desktop core (24z3 D6).
    app_data: bool,
    db: Db,
    terminals: Terminals,
    /// Program pipes and port links lent to viewers (migration M1); each ends with its connection.
    pipes: crate::pipes::Pipes,
    agents: Agents,
    indexes: index::Indexes,
    /// Start-up commands this master started (24z3 D4: its own list, from its own settings).
    startup: crate::startup::Startup,
    /// Plan runs on this master, and the connections that follow them.
    runs: Arc<crate::plan::live::Runs>,
    watchers: Arc<plan_runs::Watchers>,
    /// The connections that follow this master's Inbox (24z R2d part 3).
    inbox_watchers: Arc<inbox_cmds::InboxWatchers>,
    /// The connections that follow what plans keep between runs (migration M5a: every window of This PC sees a
    /// change made in another, or by a run).
    plan_data_watchers: Arc<inbox_cmds::InboxWatchers>,
    /// Its Qanuvi account (migration M2), kept in its own store.
    accounts: crate::account::Accounts,
    /// The servers this master connects to itself (migration M3b), the followers of their comings and goings, and
    /// why each away one is away.
    remotes: crate::remote::Remotes,
    remote_watchers: inbox_cmds::InboxWatchers,
    /// The desktop app's links to This PC's master (migration M4c): the tray's Open and Quit reach the app on them.
    app_links: inbox_cmds::InboxWatchers,
    remote_errors: Mutex<std::collections::HashMap<String, String>>,
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

fn config_home() -> Result<PathBuf, String> {
    crate::agent_config::home_dir()
        .ok_or_else(|| "This machine's home folder could not be found.".to_string())
}

fn opt(args: &Value, k: &str) -> Option<String> {
    args[k].as_str().map(String::from)
}

fn list(args: &Value, k: &str) -> Vec<String> {
    args[k]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn b(args: &Value, k: &str) -> bool {
    args[k].as_bool().unwrap_or(false)
}

fn n(args: &Value, k: &str) -> u64 {
    args[k].as_u64().unwrap_or(0)
}

/// A model provider from the window's `endpoint` argument (its key is added here, never sent).
fn endpoint_arg(args: &Value) -> Result<crate::llm::Endpoint, String> {
    let mut ep: crate::llm::Endpoint =
        serde_json::from_value(args["endpoint"].clone()).map_err(|e| e.to_string())?;
    ep.key = None;
    Ok(ep)
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
        let app_data = on_app_data(dir);
        if !app_data {
            crate::model_keys::use_master_names();
        }
        // A restore booked in Settings, Backups (24z3 D3): applied now, before the database opens. On the app's own
        // data the app applies it at its own start (M4: the app stops this master before it restarts for it).
        if !app_data
            && let Some(note) =
                crate::backups::apply_pending(dir, dir, None, Some(&backups_default()))
        {
            eprintln!("{note}");
        }
        let db = Db::open(&dir.join("qanuvi.db")).map_err(|e| e.to_string())?;
        let machine_id = crate::machine::lasting_id(dir);
        // Sessions and plan runs left running by a master that ended (M5a: This PC's too, they are all here now). The
        // runs say why when the last master said (an update, Quit, a restart).
        agent::mark_interrupted(&db);
        let why = std::fs::read_to_string(stop_note_file(dir)).ok();
        let _ = std::fs::remove_file(stop_note_file(dir));
        runs::mark_interrupted_because(&db, why.as_deref().map(str::trim));
        let accounts = accounts_for(dir, &machine_id, app_data);
        let remotes = crate::remote::Remotes::default();
        remotes.set_me(&machine_id);
        Ok(Self {
            dir: dir.to_path_buf(),
            accounts,
            machine_id,
            app_data,
            db,
            terminals: Terminals::default(),
            pipes: crate::pipes::Pipes::default(),
            agents: Agents::default(),
            indexes: index::Indexes::default(),
            startup: crate::startup::Startup::default(),
            runs: Arc::default(),
            watchers: Arc::default(),
            inbox_watchers: Arc::default(),
            plan_data_watchers: Arc::default(),
            remotes,
            remote_watchers: inbox_cmds::InboxWatchers::default(),
            app_links: inbox_cmds::InboxWatchers::default(),
            remote_errors: Mutex::default(),
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

    /// The project a call names, when it names one (agent config files of a project or of the home folder only).
    fn optional_root(&self, args: &Value) -> Result<Option<PathBuf>, String> {
        if s(args, "projectId").is_empty() {
            Ok(None)
        } else {
            self.root(args).map(Some)
        }
    }

    /// Runs one command. `stream` makes a sink for commands that stream events (terminals, agent sessions);
    /// `opened_terminals` collects the terminals and pipes this connection opened, to close them when it ends.
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
                "machineId": self.machine_id,
                // It passes calls on along a path to its own servers (migration M3c).
                "forwards": true,
            })),
            // Migration M4c: the desktop app follows This PC's master, so the tray's Open and Quit reach its windows.
            "app_follow" => {
                if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
                    self.app_links.add(stream(&name));
                }
                Ok(json!({"tray": self.app_data}))
            }
            // "Start Qanuvi in the background when I sign in" (M4c): only This PC's master starts at sign-in.
            "sign_in_start" => to(if self.app_data {
                crate::sign_in_start::is_on()
            } else {
                Ok(false)
            }),
            "sign_in_start_set" => {
                if !self.app_data {
                    return Err("Only This PC's Qanuvi starts when you sign in.".into());
                }
                to(crate::sign_in_start::set(b(args, "on")))
            }
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
            // The window's folder picker for this machine (there is no system picker for a server's folders).
            "dirs_list" => to(projects::list_folders(&s(args, "path"))),
            "projects_remove" => {
                let id = s(args, "id");
                self.terminals.kill_project(&id);
                self.agents.stop_project(&id);
                let _ = crate::drafts::drop_project(db, &id);
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
            "snapshots_take" => to(crate::snapshots::take(
                &self.root(args)?,
                &list(args, "also"),
            )),
            "snapshots_changes" => {
                let skip = s(args, "skip");
                to(crate::snapshots::changes(
                    &self.root(args)?,
                    &s(args, "before"),
                    &list(args, "also"),
                    b(args, "git"),
                    &|p| !skip.is_empty() && p == skip,
                ))
            }
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
            // The window's search options (whole words, files to include and leave out) when it sends them.
            "files_search" => {
                let o = match args.get("options").filter(|o| o.is_object()) {
                    Some(o) => serde_json::from_value(o.clone()).map_err(|e| e.to_string())?,
                    None => files::SearchOptions {
                        regex: b(args, "regex"),
                        case_sensitive: b(args, "caseSensitive"),
                        ..Default::default()
                    },
                };
                to(files::search_with(
                    &self.root(args)?,
                    &s(args, "query"),
                    &o,
                    5000,
                ))
            }
            "files_replace" => {
                let o: files::SearchOptions =
                    serde_json::from_value(args["options"].clone()).map_err(|e| e.to_string())?;
                let paths = args["paths"].is_array().then(|| list(args, "paths"));
                to(files::replace_with(
                    &self.root(args)?,
                    &s(args, "query"),
                    &o,
                    &s(args, "replacement"),
                    paths.as_deref(),
                ))
            }
            // The Plan view: the task lists, files changed during a step, and the step logs.
            "files_markdown" => to(files::markdown_files(&self.root(args)?)),
            "files_changed_since" => {
                let since =
                    std::time::UNIX_EPOCH + std::time::Duration::from_millis(n(args, "sinceMs"));
                to(files::changed_since(&self.root(args)?, since, 200))
            }
            "files_log_append" => to(files::log_append(
                &self.root(args)?,
                &s(args, "path"),
                &s(args, "text"),
            )),
            // A project's secrets (24z3 A3): names only, never values. A server build reads them from its
            // environment (QANUVI_SECRET_<NAME>) and says where to set them; a desktop master uses its credential store.
            // The machine's own Qanuvi account (migration M2): a browser sign-in begins and finishes here; the window
            // in between only opens the page and passes the one-time code on.
            "account_begin" => {
                let (site, client) = crate::account::home();
                to(self.accounts.begin(&site, &client))
            }
            "account_finish" => to(self.accounts.finish(&s(args, "state"), &s(args, "code"))),
            "account_me" => {
                let (site, client) = crate::account::home();
                to(self.accounts.me_at(&site, &client))
            }
            "account_sign_out" => {
                let (site, client) = crate::account::home();
                to(self.accounts.sign_out(&site, &client))
            }
            "secrets_store" => Ok(json!(crate::secrets::kind())),
            "secrets_names" => {
                let id = s(args, "projectId");
                projects::get(db, &id)?;
                to(Ok(crate::secrets::names(&id)))
            }
            "secrets_set" => {
                let id = s(args, "projectId");
                projects::get(db, &id)?;
                crate::secrets::set(&id, &s(args, "name"), &s(args, "value"))?;
                Ok(Value::Null)
            }
            "secrets_remove" => {
                let id = s(args, "projectId");
                projects::get(db, &id)?;
                crate::secrets::remove(&id, &s(args, "name"))?;
                Ok(Value::Null)
            }
            // Models (24z3 D1): this master's own providers, keys and local servers; a key never goes to the window.
            "model_key_store" => Ok(json!(crate::model_keys::kind())),
            "model_key_has" => Ok(json!(crate::model_keys::has(&s(args, "endpoint")))),
            "model_key_set" => {
                crate::model_keys::set(&s(args, "endpoint"), &s(args, "key"))?;
                Ok(Value::Null)
            }
            "models_list" => to(crate::llm::list_models(
                &crate::model_keys::with_key(endpoint_arg(args)?),
                b(args, "all"),
            )),
            "model_probe" => to(crate::llm::probe(
                crate::model_keys::with_key(endpoint_arg(args)?),
                s(args, "model"),
            )),
            "model_key_check" => to(crate::llm::check_key(&crate::model_keys::with_key(
                endpoint_arg(args)?,
            ))),
            "local_server_status" => to(Ok(crate::local_servers::status(
                &endpoint_arg(args)?,
                opt(args, "start").as_deref(),
            ))),
            "local_server_start" => {
                crate::local_servers::start(
                    &crate::model_keys::with_key(endpoint_arg(args)?),
                    opt(args, "start").as_deref(),
                )?;
                Ok(Value::Null)
            }
            "ollama_pull" => {
                let sink = stream(&s(args, "onEvent"));
                let mut last = Instant::now() - Duration::from_secs(1);
                crate::llm::ollama_pull(
                    &endpoint_arg(args)?,
                    &s(args, "model"),
                    |status, done, total| {
                        // A few updates a second are enough.
                        if last.elapsed().as_millis() >= 250 || status == "success" {
                            last = Instant::now();
                            let _ =
                                sink.send(json!({"status": status, "done": done, "total": total}));
                        }
                    },
                )?;
                Ok(Value::Null)
            }
            "brain_records" => to(Ok(crate::brain::Records::load(&paths::data_dir()))),
            // This machine's settings (24z3 A3): the window's Settings page while this master is shown.
            "settings_get" => to(crate::settings::get_all(db)),
            "settings_set" => {
                crate::settings::set_many(db, crate::settings::values_arg(&args["values"])?)?;
                Ok(Value::Null)
            }
            // What a plan keeps between runs, in this master's database, where its runner reads it.
            "plan_data_list" => to(crate::plan_data::list(db)),
            // The list, and every change after it on the stream (M5a): another window's writes, a run's.
            "plan_data_watch" => {
                if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
                    self.plan_data_watchers.add(stream(&name));
                }
                to(crate::plan_data::list(db))
            }
            "plan_data_set" => {
                let id = s(args, "projectId");
                projects::get(db, &id)?;
                let kind: crate::plan_data::Kind =
                    serde_json::from_value(args["kind"].clone()).map_err(|e| e.to_string())?;
                let file = s(args, "file");
                let kept = crate::plan_data::set(db, &id, &file, kind, &args["value"])?;
                let value = kept.map(|e| e.value).unwrap_or(Value::Null);
                self.plan_data_watchers
                    .send(&json!({"project_id": id, "file": file, "kind": kind, "value": value}));
                Ok(Value::Null)
            }
            // A window's old copies (kept in its own storage before 24z R2a), moved in once.
            "plan_data_import" => {
                let entries: Vec<crate::plan_data::Entry> =
                    serde_json::from_value(args["entries"].clone()).map_err(|e| e.to_string())?;
                to(crate::plan_data::import(db, &entries))
            }
            // Unsaved text of this master's files while the window shows another machine (24z3 Part B2).
            "drafts_put" => {
                let draft: crate::drafts::Draft =
                    serde_json::from_value(args["draft"].clone()).map_err(|e| e.to_string())?;
                if !draft.project_id.is_empty() {
                    projects::get(db, &draft.project_id)?;
                }
                crate::drafts::put(db, &draft)?;
                Ok(Value::Null)
            }
            "drafts_list" => to(crate::drafts::list(db, &s(args, "projectId"))),
            "drafts_drop" => {
                crate::drafts::drop(db, &s(args, "projectId"), &s(args, "path"))?;
                Ok(Value::Null)
            }
            // Private details given during a plan's run, in this master's credential store (a server build has
            // none and says so).
            "plan_private" => to(Ok(crate::secrets::plan_private(
                &s(args, "projectId"),
                &s(args, "file"),
            ))),
            "plan_private_add" => to(crate::secrets::plan_private_add(
                &s(args, "projectId"),
                &s(args, "file"),
                &s(args, "text"),
            )),
            "plan_private_clear" => to(crate::secrets::plan_private_clear(
                &s(args, "projectId"),
                &s(args, "file"),
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
            // The window's switcher hid this master (24z3 Part B2): output is kept, then given back on return.
            "term_detach" => {
                self.terminals.detach(&s(args, "id"));
                Ok(Value::Null)
            }
            "term_attach" => {
                let sink = stream(&s(args, "onEvent"));
                let term = Sink::new(move |e: terminal::TermEvent| {
                    sink.send(serde_json::to_value(e).unwrap_or(Value::Null))
                        .is_ok()
                });
                self.terminals.attach(&s(args, "id"), term)?;
                Ok(Value::Null)
            }
            "term_kill" => {
                self.terminals.kill(&s(args, "id"));
                Ok(Value::Null)
            }
            // The two pipes lent to a viewer (migration M1): run a program here with its input and output carried
            // over the connection, or reach a port on this machine's localhost. Both end with the connection.
            "pipe_start" => {
                let root = self.optional_root(args)?;
                let cwd = match root {
                    Some(r) => files::folder_in(&r, opt(args, "dir").as_deref())?,
                    None => config_home()?,
                };
                let id = self.pipes.start(
                    &s(args, "program"),
                    &list(args, "args"),
                    &cwd,
                    stream(&s(args, "onEvent")),
                )?;
                opened_terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id.clone());
                Ok(json!(id))
            }
            "port_open" => {
                let port = u16::try_from(n(args, "port")).map_err(|_| "That is not a port.")?;
                let id = self.pipes.open_port(port, stream(&s(args, "onEvent")))?;
                opened_terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id.clone());
                Ok(json!(id))
            }
            // Generic questions about this machine, for a viewer's coding tools (M1); no tool is known here.
            "host_info" => Ok(crate::pipes::host::info()),
            "host_find" => Ok(json!(crate::pipes::host::find(&s(args, "program")))),
            "host_exists" => Ok(json!(Path::new(&s(args, "path")).exists())),
            "host_run" => {
                let cwd = opt(args, "cwd")
                    .filter(|c| !c.is_empty())
                    .map(PathBuf::from);
                let timeout = Duration::from_secs(n(args, "timeoutSecs").clamp(5, 1800));
                // Output lines as they come, when the caller listens (an install the user waits for).
                let live = opt(args, "onEvent")
                    .filter(|e| !e.is_empty())
                    .map(|e| stream(&e));
                crate::pipes::host::run(
                    &s(args, "program"),
                    &list(args, "args"),
                    cwd.as_deref(),
                    timeout,
                    live,
                )
            }
            "host_write_new" => {
                crate::pipes::host::write_new(&s(args, "path"), &s(args, "text"))?;
                Ok(Value::Null)
            }
            "host_read" => Ok(json!(crate::pipes::host::read_small(&s(args, "path"))?)),
            "host_fetch" => {
                crate::pipes::host::fetch(
                    &s(args, "url"),
                    &s(args, "archive"),
                    &s(args, "into"),
                    &s(args, "expect"),
                    &s(args, "version"),
                )?;
                Ok(Value::Null)
            }
            "pipe_send" | "port_send" => {
                self.pipes
                    .send(&s(args, "id"), n(args, "seq"), &s(args, "data"))?;
                Ok(Value::Null)
            }
            "pipe_stop" | "port_close" => {
                self.pipes.stop(&s(args, "id"));
                Ok(Value::Null)
            }
            // Backups and the data folder (24z3 D3): the master's own, on by default like the desktop app's (in
            // backups::master_default_dir: Documents on Windows/macOS, ~/qanuvi-backups on Linux); its pointer file is in its own data folder; the data folder itself is
            // set where it starts (--data or QANUVI_MASTER_DATA), so Settings only shows it.
            "backups_info" => {
                let d = &self.dir;
                to(Ok(crate::backups::info(d, d, Some(&backups_default()))))
            }
            "backups_set" => to(crate::backups::set(
                &self.dir,
                &s(args, "folder"),
                args["off"].as_bool().unwrap_or(false),
            )),
            "backups_now" => to(crate::backups::now_backup(
                &self.dir,
                None,
                Some(&backups_default()),
            )),
            "backups_restore" => {
                let d = &self.dir;
                to(crate::backups::book_restore(
                    d,
                    d,
                    Some(&backups_default()),
                    Path::new(&s(args, "path")),
                ))
            }
            "backups_cancel_restore" => to(crate::backups::cancel_restore(&self.dir)),
            "backups_dismiss_note" => to(crate::backups::dismiss_note(&self.dir)),
            "backups_remove" => to(crate::backups::remove_one(
                &self.dir,
                Some(&backups_default()),
                Path::new(&s(args, "path")),
            )),
            "data_location_info" => {
                let d = &self.dir;
                let loc = crate::data_location::read(d);
                Ok(json!({
                    "current": d.to_string_lossy(),
                    "defaultDir": paths::default_master_dir().to_string_lossy(),
                    "pending": null,
                    "old": null,
                    "note": loc.note,
                    "fixed": true,
                }))
            }
            // What a restart stops (the window says so before asking), and the restart itself.
            "master_busy" => Ok(json!({
                "agents": self.agents.open_count(),
                "runs": self.runs.active().len(),
            })),
            // How many agents work here now (the window's machine switcher shows it, 24z3 step 4).
            "agents_working" => Ok(json!(crate::agent::working_count(&self.db))),
            "master_restart" => {
                self.restart(&self.dir)?;
                Ok(Value::Null)
            }
            // This machine and its setup (24z3 D2): the guide, the setup wizard and the status bar while it is shown.
            "machine_info" => to(Ok(crate::machine::detect())),
            "machine_room" => to(Ok(
                crate::machine::room().map(|r| (r.free_ram_gb, r.free_vram_gb))
            )),
            "setup_tools" => to(Ok(crate::local_servers::tools())),
            "setup_install_plan" => to(crate::local_servers::install_plan(&s(args, "tool"))),
            "setup_install_start" => {
                let sink = stream(&s(args, "onEvent"));
                let term = Sink::new(move |e: terminal::TermEvent| {
                    sink.send(serde_json::to_value(e).unwrap_or(Value::Null))
                        .is_ok()
                });
                let id = crate::local_servers::install_start(
                    &self.terminals,
                    &s(args, "tool"),
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
            "qanuvi_usage" => to(Ok(crate::usage::usage())),
            // Start-up and Claude Code updates (24z3 D4): this master's own list, started with the master.
            "startup_start" => {
                let items: Vec<crate::startup::Item> =
                    serde_json::from_value(args["items"].clone()).map_err(|e| e.to_string())?;
                self.startup.start_all(&items);
                to(Ok(self.startup.statuses()))
            }
            "startup_status" => to(Ok(self.startup.statuses())),
            "startup_restart" => {
                let item: crate::startup::Item =
                    serde_json::from_value(args["item"].clone()).map_err(|e| e.to_string())?;
                self.startup.restart(&item);
                to(Ok(self.startup.statuses()))
            }
            "startup_stop" => {
                self.startup.stop(&s(args, "id"));
                to(Ok(self.startup.statuses()))
            }
            "startup_headroom" => to(Ok(crate::startup::headroom_path())),
            "claude_settings_base_url" => to(Ok(crate::claude_update::settings_base_url())),
            "claude_versions" => to(crate::claude_update::versions()),
            "claude_update" => to(crate::claude_update::update()),
            // Memory entries (24z3 D7): this master's own memory, as the desktop answers them for This PC.
            "memory_list" => to(crate::memory::list(
                db,
                opt(args, "project").as_deref(),
                b(args, "suggested"),
            )),
            "memory_save" => {
                let entry: crate::memory::Entry =
                    serde_json::from_value(args["entry"].clone()).map_err(|e| e.to_string())?;
                to(crate::memory::save(db, &entry))
            }
            "memory_delete" => {
                crate::memory::delete(db, &s(args, "id"))?;
                Ok(Value::Null)
            }
            "memory_slice" => to(crate::memory::slice(
                db,
                opt(args, "project").as_deref(),
                &s(args, "query"),
                (n(args, "budget") as usize).clamp(200, 20000),
                b(args, "count"),
            )),
            "memory_search" => to(crate::memory::search(
                db,
                opt(args, "project").as_deref(),
                &s(args, "query"),
                50,
            )),
            // The memory disk (24z3 D6): this master's own disk (from its settings), passphrase and sync.
            "memory_disk_device" => to(Ok(crate::memory_disk::device(&self.dir))),
            "memory_disk_status" => to(Ok(crate::memory_disk::status(db))),
            "memory_disk_passphrase_saved" => Ok(json!(crate::memory_disk::passphrase().is_ok())),
            "memory_disk_passphrase_set" => {
                crate::memory_disk::passphrase_set(&s(args, "passphrase"))?;
                Ok(Value::Null)
            }
            "memory_disk_passphrase_forget" => {
                crate::memory_disk::passphrase_forget()?;
                Ok(Value::Null)
            }
            "memory_disk_sync" => to(crate::memory_disk::sync_here(db, &self.dir)),
            "memory_disk_versions" => to(crate::memory_disk::versions_here(db, &self.dir)),
            "memory_disk_restore" => to(crate::memory_disk::restore_here(
                db,
                &self.dir,
                &s(args, "name"),
            )),
            // Agent setup (24z3 D6): the agents' own config files in this machine's home folder and its projects, a
            // running session's model or mode, and the chat clean-up of this machine's chats.
            "agent_config_list" => to(crate::agent_config::list(
                &s(args, "provider"),
                &config_home()?,
                self.optional_root(args)?.as_deref(),
            )),
            "agent_config_read" => to(crate::agent_config::read(
                &s(args, "provider"),
                &s(args, "id"),
                &config_home()?,
                self.optional_root(args)?.as_deref(),
            )),
            "agent_config_write" => to(crate::agent_config::write(
                &s(args, "provider"),
                &s(args, "id"),
                &config_home()?,
                self.optional_root(args)?.as_deref(),
                &s(args, "text"),
                opt(args, "expectedHash").as_deref(),
            )),
            "agent_configure" => {
                self.agents.configure(
                    db,
                    &s(args, "sessionId"),
                    opt(args, "model").as_deref(),
                    opt(args, "mode").as_deref(),
                )?;
                Ok(Value::Null)
            }
            "agent_clean_up" => {
                let keep: Vec<String> =
                    serde_json::from_value(args["keep"].clone()).unwrap_or_default();
                to(agent::clean_up_chats(
                    db,
                    args["days"].as_i64().unwrap_or(0),
                    &keep,
                    crate::store::now(),
                ))
            }
            // OmniRoute (24z3 D5): the one on this master's machine, with this master's keys.
            "omniroute_connect" => {
                crate::omniroute::connect(&endpoint_arg(args)?)?;
                Ok(Value::Null)
            }
            "omniroute_connections" => to(crate::omniroute::connections(&endpoint_arg(args)?)),
            "omniroute_catalog" => to(crate::omniroute::catalog(&endpoint_arg(args)?)),
            "omniroute_add_key" => {
                crate::omniroute::add_key_with(
                    &endpoint_arg(args)?,
                    &s(args, "provider"),
                    opt(args, "key"),
                    opt(args, "saved"),
                )?;
                Ok(Value::Null)
            }
            "omniroute_test" => {
                crate::omniroute::test(&endpoint_arg(args)?, &s(args, "id"))?;
                Ok(Value::Null)
            }
            "omniroute_remove" => {
                crate::omniroute::remove(&endpoint_arg(args)?, &s(args, "id"))?;
                Ok(Value::Null)
            }
            "omniroute_sign_in" => {
                crate::omniroute::sign_in(&endpoint_arg(args)?, &s(args, "provider"))?;
                Ok(Value::Null)
            }
            "omniroute_make_combo" => {
                let models: Vec<String> =
                    serde_json::from_value(args["models"].clone()).map_err(|e| e.to_string())?;
                crate::omniroute::make_combo(&endpoint_arg(args)?, &s(args, "name"), &models)?;
                Ok(Value::Null)
            }
            "omniroute_look" => to(Ok(self.omniroute_look())),
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
            "agent_session_times" => to(agent::session_event_times(db, &s(args, "sessionId"))),
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
                        .map(|v| crate::brain::with_keys(&v.to_string(), crate::model_keys::get))
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
            "agent_interrupt" => to(self.agents.interrupt(db, &s(args, "sessionId"))),
            "agent_rename" => to(agent::rename(db, &s(args, "sessionId"), &s(args, "title"))),
            "agent_link" => to(agent::link(db, &s(args, "sessionId"), &s(args, "from"))),
            "agent_handover_off" => to(agent::set_handover_off(
                db,
                &s(args, "sessionId"),
                b(args, "off"),
            )),
            "agent_saved" => to(agent::set_saved(
                db,
                &s(args, "sessionId"),
                b(args, "saved"),
            )),
            "agent_delete" => {
                self.agents.stop(&s(args, "sessionId"));
                to(agent::delete(db, &s(args, "sessionId")))
            }
            "agent_usage_since" => to(agent::usage_since(db, args["since"].as_i64().unwrap_or(0))),
            // What the chat attaches: a web page, an image kept with a message (migration M5a).
            "attachment_page" => to(crate::attachments::read_page(&s(args, "url"))),
            "attachment_image" => to(crate::attachments::stored_image(
                &self.dir.join("attachments"),
                &s(args, "file"),
            )),
            // The brain's one-off decision with this machine's keys, and the shared knowledge it reads (M5a).
            "core_decide" => {
                let setup = crate::brain::with_keys(&s(args, "setup"), crate::model_keys::get)?;
                let setup: crate::brain::Setup =
                    serde_json::from_str(&setup).map_err(|e| e.to_string())?;
                to(crate::brain::decide(
                    &setup,
                    &s(args, "system"),
                    &s(args, "prompt"),
                ))
            }
            "knowledge_fetch" => {
                let url = s(args, "url");
                let url = if url.trim().is_empty() {
                    crate::knowledge::DEFAULT_URL.to_string()
                } else {
                    url
                };
                to(crate::knowledge::fetch(&url, crate::knowledge::PUBLIC_KEY))
            }
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
            c if c.starts_with("plan_run_") => self.plan_run(c, args, stream),
            c if c.starts_with("inbox_") => self.inbox(c, args, stream),
            c if c.starts_with("servers_") || c.starts_with("remote_") => {
                self.servers(c, args, stream)
            }
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
    // The other side is gone: what this connection followed (the app's link, an Inbox, a run) hears so at once. A
    // socket whose peer closed can still take a first write on Windows, so a follower would seem to listen (M4c: the
    // tray's Open then started no app).
    *out.lock().unwrap_or_else(|e| e.into_inner()) = Box::new(Gone);
    for id in terminals.lock().unwrap_or_else(|e| e.into_inner()).iter() {
        if crate::pipes::is_pipe(id) {
            master.pipes.stop(id);
        } else {
            master.terminals.kill(id);
        }
    }
}

/// The writer of a connection that ended: every write fails.
struct Gone;

impl Write for Gone {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}

/// A master's default backups folder (a test build's has its own name, as in the desktop app).
fn backups_default() -> PathBuf {
    crate::backups::master_default_dir(crate::data_location::from_source())
}

/// Where the daily backups of a data folder go and the window storage they take along. On the desktop app's own
/// data (M4b) the app's: its backups folder and its WebView folder (settings and layout), so the app's Settings,
/// Backups lists and prunes the same ones as before. Elsewhere a master's own folder, with no window storage.
fn backup_place(dir: &Path, app_data: bool) -> (Option<PathBuf>, PathBuf) {
    if app_data {
        let from_source = crate::data_location::from_source();
        (
            crate::data_location::Defaults::real().webview_for(dir),
            crate::backups::default_dir(from_source),
        )
    } else {
        (None, backups_default())
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

/// Marks a restart under way (24z3 D3): a connection that finds no service then waits for the new one instead of
/// starting a second.
fn restart_mark(dir: &Path) -> PathBuf {
    dir.join("master-restarting")
}

/// Why this master was asked to end ("update", "quit", "restart"), read once by the next one (M5a).
fn stop_note_file(dir: &Path) -> PathBuf {
    dir.join("master-stopped-why")
}

/// A restart mark younger than half a minute.
fn restarting(dir: &Path) -> bool {
    std::fs::metadata(restart_mark(dir))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < Duration::from_secs(30))
}

impl Master {
    /// What this master does by itself once it runs (24z3 D4), with no window connected: starts its own start-up
    /// list, and keeps Claude Code current when its settings ask for that (20 seconds after start, then once a day;
    /// never while an agent is working). On the desktop app's own data (M4b) this master is This PC and does these
    /// jobs in the app's place; the app no longer runs them, so nothing runs twice.
    pub fn start_own(self: &Arc<Self>) {
        let settings = crate::settings::get_all(&self.db).unwrap_or_default();
        self.startup
            .start_all(&crate::startup::items_from_settings(&settings));
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            loop {
                let Some(m) = me.upgrade() else { return };
                crate::claude_update::auto_update(&m.db);
                drop(m);
                std::thread::sleep(Duration::from_secs(24 * 3600));
            }
        });
        // The OmniRoute watch (24z3 D5): a minute after start, then every 30 minutes, with no window needed.
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(60));
            loop {
                let Some(m) = me.upgrade() else { return };
                m.omniroute_look();
                drop(m);
                std::thread::sleep(crate::omniroute::WATCH_EVERY);
            }
        });
        // The memory disk (24z3 D6): 90 seconds after start, then every 10 minutes, with this master's own disk and
        // passphrase, shown or not.
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            loop {
                let Some(m) = me.upgrade() else { return };
                crate::memory_disk::sync_due(&m.db, &m.dir);
                drop(m);
                std::thread::sleep(crate::memory_disk::SYNC_EVERY);
            }
        });
        // Its own servers (migration M3b): the ones marked to connect at start-up, now and again while away. On the
        // app's own data the window still holds This PC's server connections until M5.
        if self.app_data {
            return;
        }
        let me = Arc::downgrade(self);
        std::thread::spawn(move || {
            loop {
                let Some(m) = me.upgrade() else { return };
                m.connect_away_servers();
                drop(m);
                std::thread::sleep(server_cmds::RECONNECT_EVERY);
            }
        });
    }

    /// Ends this master: its agent sessions, terminals, start-up commands and server connections end with it.
    pub fn end(&self) -> ! {
        self.end_because("")
    }

    /// Ends this master, keeping why for its runs to say after the next start (M5a: "Stopped by the update").
    pub fn end_because(&self, why: &str) -> ! {
        if runs::stop_note(why).is_some() {
            let _ = std::fs::write(stop_note_file(&self.dir), why);
        }
        self.agents.stop_all();
        self.terminals.kill_all();
        self.startup.stop_all();
        self.remotes.disconnect_all();
        let _ = std::fs::remove_file(service_file(&self.dir));
        std::process::exit(0);
    }

    /// M4c, the tray's "Open Qanuvi": a running app shows its window; with none, the app beside this master starts.
    pub fn open_app(&self) {
        self.open_app_at(None);
    }

    /// Opens Qanuvi; started for a notification's item (M5a), it opens its Inbox there.
    pub fn open_app_at(&self, item: Option<String>) {
        if self.app_links.reach(&json!({"do": "show"})) > 0 {
            return;
        }
        match crate::sign_in_start::app_exe() {
            Ok(exe) => {
                let mut c = std::process::Command::new(exe);
                if item.is_some() {
                    c.arg(crate::notify::link(item.as_deref()));
                }
                let started = c
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                if let Err(e) = started {
                    eprintln!("Qanuvi could not start: {e}");
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    }

    /// M4c, the tray's "Quit Qanuvi": the app's windows close first, as when closed by hand, then this master ends.
    /// A window still open after 20 seconds (its closing was cancelled) keeps everything running.
    pub fn quit_all(&self) {
        // M5a: work going on here stops with it; said first, and nothing stops without a yes.
        if let Some(what) = work_text(self.work())
            && !confirm_quit(&format!(
                "Quitting Qanuvi stops {what} on this computer. They can be resumed afterwards.\n\nQuit anyway?"
            ))
        {
            return;
        }
        if self.app_links.reach(&json!({"do": "quit"})) > 0 {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                std::thread::sleep(Duration::from_millis(300));
                if self.app_links.reach(&json!({"do": "wait"})) == 0 {
                    break;
                }
                if Instant::now() >= deadline {
                    return;
                }
            }
        }
        self.end_because("quit")
    }

    /// Looks at this master's OmniRoute once; its Inbox changes go to the connections that follow the Inbox.
    fn omniroute_look(&self) -> Option<Vec<crate::omniroute::Connection>> {
        let tell = self.inbox_teller();
        crate::omniroute::look(&self.db, &move |event: &str, value: Value| {
            if event == "qanuvi-inbox" {
                tell(&value);
            }
        })
    }

    /// What works on this master now: agent sessions working, and plan runs going (M5a: said before it all stops).
    pub fn work(&self) -> (i64, usize) {
        (agent::working_count(&self.db), self.runs.active().len())
    }

    /// Tells a change of this master's Inbox (made by its own work) to the connections that follow it; on This PC
    /// with no window following, a new item is told on the desktop by the master itself (M5a).
    pub(crate) fn inbox_teller(&self) -> Arc<dyn Fn(&Value) + Send + Sync> {
        let watchers = self.inbox_watchers.clone();
        let me = self.me.get().cloned();
        Arc::new(move |change: &Value| {
            watchers.send(change);
            #[cfg(feature = "desktop")]
            if let Some(m) = me.as_ref().and_then(std::sync::Weak::upgrade) {
                m.tell_when_alone(change);
            }
            #[cfg(not(feature = "desktop"))]
            let _ = &me;
        })
    }

    /// A new Inbox item while no window follows This PC's master: told on the desktop by This PC's Notifications
    /// settings, once; a click opens Qanuvi at it. With a window following, the app tells it (desktop.rs).
    #[cfg(feature = "desktop")]
    fn tell_when_alone(self: &Arc<Self>, change: &Value) {
        if !self.app_data || change["change"] != "add" || change["new"] != true {
            return;
        }
        let Ok(item) = serde_json::from_value::<crate::inbox::InboxItem>(change["item"].clone())
        else {
            return;
        };
        if self.app_links.reach(&json!({"do": "wait"})) > 0 {
            return;
        }
        let s: crate::notify::NotifySettings = crate::settings::get_all(&self.db)
            .ok()
            .and_then(|all| all.get("notifications").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        if !crate::notify::wanted(&s, &item)
            || crate::notify::quiet_now(&s, crate::notify::local_minute())
        {
            return;
        }
        for one in crate::notify::compose(std::slice::from_ref(&item), false) {
            let me = Arc::downgrade(self);
            let shown =
                crate::master_notify::show(&one, !crate::data_location::from_source(), move |id| {
                    if let Some(m) = me.upgrade() {
                        m.open_app_at(Some(id.unwrap_or_default()).filter(|i| !i.is_empty()));
                    }
                });
            if let Err(e) = shown {
                eprintln!("Desktop notification failed: {e}");
            }
        }
    }

    /// Starts a new service that waits for this one to end, then ends this one: its agent sessions and terminals
    /// stop, a booked restore is applied by the new one before its database opens. The answer goes out first.
    fn restart(&self, dir: &Path) -> Result<(), String> {
        let me = self
            .me
            .get()
            .and_then(|w| w.upgrade())
            .ok_or("The master is not ready.")?;
        std::fs::write(restart_mark(dir), b"").map_err(|e| e.to_string())?;
        let _ = std::fs::write(stop_note_file(dir), "restart");
        if let Err(e) = start_service(dir, Some(std::process::id())) {
            let _ = std::fs::remove_file(restart_mark(dir));
            return Err(e);
        }
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            me.agents.stop_all();
            me.terminals.kill_all();
            me.startup.stop_all();
            me.remotes.disconnect_all();
            let _ = std::fs::remove_file(service_file(&dir));
            std::process::exit(0);
        });
        Ok(())
    }
}

/// What would stop, in words ("2 agent sessions and 1 plan run"), or None when nothing works.
pub fn work_text((agents, runs): (i64, usize)) -> Option<String> {
    let part = |n: u64, one: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {one}s")),
    };
    let parts: Vec<String> = [
        part(agents.max(0) as u64, "agent session"),
        part(runs as u64, "plan run"),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(" and "))
}

/// Asks before the tray's Quit stops work (M5a). Yes quits; anything else keeps it all running.
#[cfg(all(windows, feature = "desktop"))]
fn confirm_quit(text: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MessageBoxW,
    };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("Quit Qanuvi"));
    // SAFETY: both strings are NUL-terminated UTF-16 that live for the call; no owner window.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    answer == IDYES
}

/// No box to ask with here: the tray's Quit goes on (Linux and macOS trays come with M5b's watcher).
#[cfg(not(all(windows, feature = "desktop")))]
fn confirm_quit(_text: &str) -> bool {
    true
}

/// The file the desktop app keeps in its own data folder (24z3 D6), so a master service opened on it knows it is This PC.
const APP_MARK: &str = "qanuvi-app.mark";

/// The account store of a master on a data folder: This PC's own names on the desktop app's data, else its own.
fn accounts_for(dir: &Path, machine_id: &str, app_data: bool) -> crate::account::Accounts {
    if app_data {
        crate::account::Accounts::this_pc(dir)
    } else {
        crate::account::Accounts::master(dir, machine_id)
    }
}

/// The account of the master on a data folder, for `qanuvi-master login` and `logout`.
pub fn accounts_at(dir: &Path) -> crate::account::Accounts {
    let _ = std::fs::create_dir_all(dir);
    accounts_for(dir, &crate::machine::lasting_id(dir), on_app_data(dir))
}

/// Called by the desktop app at start-up on its data folder. A master on it keeps This PC's names and account, and
/// leaves to the app what the app still holds until M5 (its running sessions and runs, a booked restore, This PC's
/// server connections); since M4b it runs This PC's background jobs.
pub fn mark_app_data(dir: &Path) {
    let file = dir.join(APP_MARK);
    if !file.is_file() {
        let _ = std::fs::write(
            file,
            "This folder is the Qanuvi desktop app's own data. A master service opened on it is This PC.\n",
        );
    }
}

fn on_app_data(dir: &Path) -> bool {
    dir.join(APP_MARK).is_file()
}

/// What shows This PC's master's tray icon and answers it until the master ends (migration M4c). Only the desktop
/// build's `qanuvi-master` program has one (it links the tray; the library and its tests do not).
pub type Tray = fn(Arc<Master>) -> !;

/// Runs the background service until it is stopped. `tray`: shown when this master is This PC's.
pub fn serve(dir: &Path, tray: Option<Tray>) -> Result<(), String> {
    let master = Master::open(dir)?.shared();
    master.start_own();
    // The daily backup (24z3 D3), as in the desktop app: checked 2 minutes after start, then every hour. On the app's
    // own data (M4b) these are This PC's backups: the app's folder, with its window storage, as the app made them.
    let backup_dir = dir.to_path_buf();
    let (backup_web, backup_default) = backup_place(dir, master.app_data);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(120));
        loop {
            if let Err(e) = crate::backups::run_due(
                &backup_dir,
                backup_web.as_deref(),
                Some(&backup_default),
                false,
            ) {
                eprintln!("Backup failed: {e}");
            }
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
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
    let _ = std::fs::remove_file(restart_mark(dir));
    // M4c: This PC's master shows its tray icon while it runs (never a hidden process); the icon needs the main
    // thread, so the connections are taken on another one.
    if let Some(tray) = tray.filter(|_| master.app_data) {
        let m = master.clone();
        std::thread::spawn(move || accept(listener, m, token));
        tray(master);
    }
    accept(listener, master, token);
    Ok(())
}

/// Takes the service's connections: each must open with its token.
fn accept(listener: TcpListener, master: Arc<Master>, token: String) {
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
                master.end_because(hello["why"].as_str().unwrap_or(""));
            }
            let out: Out = Arc::new(Mutex::new(Box::new(write_half)));
            handle_connection(master, reader, out);
        });
    }
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
/// `after`: a service that is ending (a restart); the new one waits for it before it opens the data.
fn start_service(dir: &Path, after: Option<u32>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    spawn_service(&exe, dir, after)
}

/// The master program next to the running one (the desktop app ships `qanuvi-master` beside `qanuvi`).
pub fn master_exe_beside_me() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let master = exe.with_file_name(if cfg!(windows) {
        "qanuvi-master.exe"
    } else {
        "qanuvi-master"
    });
    if master.is_file() {
        Ok(master)
    } else {
        Err(format!(
            "The master program was not found next to Qanuvi ({}).",
            master.display()
        ))
    }
}

/// M4a: This PC's master on the desktop app's own data, started at the app's launch unless it runs already (or a
/// restart of it is under way). Does not wait for it: the app's window does not call it yet (M5).
pub fn start_this_pc(dir: &Path) -> Result<(), String> {
    if is_running(dir) || restarting(dir) {
        return Ok(());
    }
    let exe = master_exe_beside_me()?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(service_file(dir));
    spawn_service(&exe, dir, None)
}

fn spawn_service(exe: &Path, dir: &Path, after: Option<u32>) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("serve").arg("--data").arg(dir);
    if let Some(pid) = after {
        cmd.arg(format!("--after-pid={pid}"));
    }
    cmd.stdin(std::process::Stdio::null())
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
        keep_own_stdio_from_children();
    }
    cmd.spawn()
        .map_err(|e| format!("The master service could not start: {e}"))?;
    Ok(())
}

/// Windows starts children with every inheritable handle, whatever their own stdio is. This process's
/// stdin/stdout/stderr (the SSH session, or the pipes of whoever ran `--stdio`) would then stay open in the
/// long-lived service, and the caller would never see the output end after this process exits (finding k1).
/// The handles stay usable here; only children no longer get copies.
#[cfg(windows)]
fn keep_own_stdio_from_children() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    for h in [
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ] {
        if !h.is_null() && h as isize != -1 {
            // A console handle or a closed one may refuse; nothing to keep from children then.
            unsafe { SetHandleInformation(h as _, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

/// Connects to the service, starting it first when it is not running.
pub fn ensure_service(dir: &Path) -> Result<(TcpStream, String), String> {
    if let Some(c) = connect(dir) {
        return Ok(c);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // A restart under way: its new service is coming (it waits for the old one to end first).
    if restarting(dir) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline && restarting(dir) {
            if let Some(c) = connect(dir) {
                return Ok(c);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        if let Some(c) = connect(dir) {
            return Ok(c);
        }
    }
    let _ = std::fs::remove_file(service_file(dir));
    start_service(dir, None)?;
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

/// Connects to This PC's master with its hello sent, waiting up to 10 seconds while it is starting (the app starts
/// it at launch; this never starts a second one).
fn this_pc_link(dir: &Path) -> Result<(TcpStream, BufReader<TcpStream>), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut s, token) = loop {
        if let Some(c) = connect(dir) {
            break c;
        }
        if Instant::now() >= deadline {
            return Err("This PC's master is not running.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    writeln!(s, "{}", json!({"token": token})).map_err(|e| e.to_string())?;
    let reader = BufReader::new(s.try_clone().map_err(|e| e.to_string())?);
    Ok((s, reader))
}

/// M4b: one call to This PC's master on the app's own data (its start-up list lives there now). The answer, or the
/// master's error. Until M5 only the jobs that moved go this way; the window calls the app's core for the rest.
pub fn this_pc_call(dir: &Path, cmd: &str, args: Value) -> Result<Value, String> {
    let (mut s, reader) = this_pc_link(dir)?;
    s.set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    writeln!(s, "{}", json!({"id": 1, "cmd": cmd, "args": args})).map_err(|e| e.to_string())?;
    for line in reader.lines() {
        let line = line.map_err(|e| format!("This PC's master did not answer: {e}"))?;
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if msg["id"] == 1 {
            let _ = s.shutdown(std::net::Shutdown::Both);
            return match msg.get("err") {
                Some(e) => Err(e
                    .as_str()
                    .unwrap_or("This PC's master refused the call.")
                    .into()),
                None => Ok(msg["ok"].clone()),
            };
        }
    }
    Err("This PC's master closed the connection.".into())
}

/// M4b: follows the Inbox of This PC's master (what its own jobs add, such as the OmniRoute watch's sign-ins to
/// renew) and hands each change to `on`, in the payload of the app's "qanuvi-inbox" event. Kept up while `go` says
/// so: when the master restarts or is not up yet it tries again every 3 seconds.
pub fn follow_this_pc_inbox(dir: &Path, go: impl Fn() -> bool, on: impl Fn(Value)) {
    follow_this_pc(dir, "inbox_watch", go, on)
}

/// M4c: the app's link to This PC's master; what the tray asks of the app's windows ({"do": "show"} or "quit")
/// goes to `on`. Kept up as `follow_this_pc_inbox` is.
pub fn follow_this_pc_app(dir: &Path, go: impl Fn() -> bool, on: impl Fn(Value)) {
    follow_this_pc(dir, "app_follow", go, on)
}

/// Keeps one streaming command to This PC's master open while `go` says so, handing its events to `on`.
fn follow_this_pc(dir: &Path, cmd: &str, go: impl Fn() -> bool, on: impl Fn(Value)) {
    while go() {
        if let Ok((mut s, reader)) = this_pc_link(dir)
            && writeln!(
                s,
                "{}",
                json!({"id": 1, "cmd": cmd, "args": {"onEvent": "follow"}})
            )
            .is_ok()
        {
            for line in reader.lines().map_while(Result::ok) {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if msg["stream"] == "follow" {
                    on(msg["event"].clone());
                }
                if !go() {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// Stops the running service, if any.
pub fn stop_service(dir: &Path) -> bool {
    stop_service_because(dir, "")
}

/// Stops the running service, saying why ("update", "quit", "restart"): its runs say so after the next start.
pub fn stop_service_because(dir: &Path, why: &str) -> bool {
    if let Some((mut s, token)) = connect(dir) {
        let _ = writeln!(s, "{}", json!({"token": token, "stop": true, "why": why}));
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
    fn keeps_unsaved_text_of_its_files_while_another_machine_is_shown() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        let draft = json!({"projectId": id, "path": "a.js", "text": "let a = 2;", "hash": "h1"});
        call(&m, "drafts_put", json!({"projectId": id, "draft": draft})).unwrap();
        let list = call(&m, "drafts_list", json!({"projectId": id})).unwrap();
        assert_eq!(list[0]["text"], "let a = 2;");
        assert_eq!(list[0]["hash"], "h1");
        // A project this master does not have is refused.
        let other = json!({"projectId": "nope", "path": "a.js", "text": "x", "hash": ""});
        assert!(
            call(
                &m,
                "drafts_put",
                json!({"projectId": "nope", "draft": other})
            )
            .is_err()
        );
        call(&m, "drafts_drop", json!({"projectId": id, "path": "a.js"})).unwrap();
        assert_eq!(
            call(&m, "drafts_list", json!({"projectId": id})).unwrap(),
            json!([])
        );
    }

    // Migration M3b: a master keeps its own server list and answers the window's servers_* and remote_* calls.
    #[test]
    fn keeps_its_own_server_list_and_answers_remote_calls() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let vps = json!({"id": "v1", "name": "VPS", "target": "root@203.0.113.5", "program": "qanuvi-master",
            "identity": "", "autoConnect": false});
        call(&m, "servers_save", json!({"server": vps})).unwrap();
        let local = json!({"id": "l1", "name": "", "target": "local", "program": "", "identity": "",
            "autoConnect": true});
        // Kept before the master refused it (an older build, or This PC's list copied in).
        crate::servers::save(&m.db, &local).unwrap();
        let kept = call(&m, "servers_list", json!({})).unwrap();
        assert_eq!(kept["servers"][0]["name"], "VPS");
        assert_eq!(kept["servers"].as_array().unwrap().len(), 2);
        // Nothing connected yet: every server is off, and calls to one say so.
        let states = call(&m, "remote_states", json!({})).unwrap();
        assert_eq!(states["v1"]["state"], "off");
        assert_eq!(call(&m, "remote_list", json!({})).unwrap(), json!([]));
        let e = call(
            &m,
            "remote_call",
            json!({"master": "v1", "cmd": "hello", "args": {}}),
        )
        .unwrap_err();
        assert!(e.contains("Not connected"), "{e}");
        // Migration M3c: a call that already passed this machine is a loop; a path longer than three machines below
        // the window is refused; both before anything is sent on.
        let hello = call(&m, "hello", json!({})).unwrap();
        assert_eq!(hello["forwards"], true);
        let me = hello["machineId"].clone();
        let e = call(
            &m,
            "remote_call",
            json!({"master": "v1", "seen": ["pc", me], "cmd": "hello", "args": {}}),
        )
        .unwrap_err();
        assert_eq!(e, crate::remote::a_loop());
        // Finding n3: a server whose hello is not in yet is connecting, never on with nothing known of it.
        m.remotes.add_idle("v1", Value::Null);
        assert_eq!(
            call(&m, "remote_states", json!({})).unwrap()["v1"],
            json!({"state": "connecting"})
        );
        m.remotes.disconnect("v1");
        let e = call(
            &m,
            "remote_call",
            json!({"master": "v1", "via": ["x", "y"], "seen": ["pc"], "cmd": "hello", "args": {}}),
        )
        .unwrap_err();
        assert_eq!(e, crate::remote::too_deep());
        let e = call(
            &m,
            "remote_call",
            json!({"master": "v1", "via": ["x"], "seen": ["pc"], "cmd": "hello", "args": {}}),
        )
        .unwrap_err();
        assert!(e.contains("Not connected"), "{e}");
        // A desktop's test master next to the app is not a machine a master reaches onward: not kept (finding s1),
        // and one kept before is refused with why.
        let e = call(
            &m,
            "servers_save",
            json!({"server": {"id": "l2", "target": "local"}}),
        )
        .unwrap_err();
        assert!(e.contains("works only in the app"), "{e}");
        let e = call(&m, "remote_connect", local.clone()).unwrap_err();
        assert!(e.contains("over SSH or through the relay"), "{e}");
        let states = call(&m, "remote_watch", json!({})).unwrap();
        assert_eq!(states["l1"]["state"], "error");
        // The start-up connect tries it too, and keeps it away with the same reason.
        m.connect_away_servers();
        assert_eq!(
            call(&m, "remote_states", json!({})).unwrap()["l1"]["state"],
            "error"
        );
        // A relay server without its pairing key here.
        let paired = json!({"id": "p1", "name": "", "target": "relay", "autoConnect": false,
            "relay": {"url": "wss://relay.example", "masterId": "m", "pairing": "abc123"}});
        let e = call(&m, "remote_connect", paired).unwrap_err();
        assert!(e.contains("pairing is missing"), "{e}");
        assert!(call(&m, "remote_unpair", json!({"pairing": "../x"})).is_err());
        // Removed: out of the list, and its reason with it.
        assert_eq!(
            call(&m, "servers_remove", json!({"id": "l1"})).unwrap(),
            json!(true)
        );
        assert_eq!(
            call(&m, "servers_remove", json!({"id": "l1"})).unwrap(),
            json!(false)
        );
        let states = call(&m, "remote_states", json!({})).unwrap();
        assert!(states.get("l1").is_none());
        call(&m, "remote_disconnect", json!({"id": "v1"})).unwrap();
        assert!(
            call(&m, "remote_nothing", json!({}))
                .unwrap_err()
                .contains("does not know the command")
        );
    }

    // Migration M3b: the terminal client reaches the running service with the window's own commands.
    #[test]
    fn the_terminal_lists_adds_and_removes_through_the_service() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().to_path_buf();
        let master = Master::open(&dir).unwrap().shared();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::write(
            service_file(&dir),
            json!({"port": port, "token": "t", "pid": std::process::id()}).to_string(),
        )
        .unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let master = master.clone();
                std::thread::spawn(move || {
                    let w = stream.try_clone().unwrap();
                    let mut r = BufReader::new(stream);
                    let mut first = String::new();
                    r.read_line(&mut first).unwrap();
                    handle_connection(master, r, Arc::new(Mutex::new(Box::new(w))));
                });
            }
        });
        let save = json!({"server": {"id": "a", "name": "Shop", "target": "deploy@shop", "autoConnect": false}});
        cli::call(&dir, "servers_save", save).unwrap();
        servers_cli(&dir, &["list".to_string()]).unwrap();
        let got = cli::call(&dir, "servers_list", json!({})).unwrap();
        assert_eq!(got["servers"][0]["name"], "Shop");
        // A name another machine has is refused before anything is kept.
        let e = servers_cli(
            &dir,
            &["add", "root@other", "--name", "shop"].map(String::from),
        )
        .unwrap_err();
        assert!(e.contains("already named"), "{e}");
        servers_cli(&dir, &["remove".to_string(), "SHOP".to_string()]).unwrap();
        let got = cli::call(&dir, "servers_list", json!({})).unwrap();
        assert_eq!(got["servers"], json!([]));
        assert!(servers_cli(&dir, &["remove".to_string(), "Shop".to_string()]).is_err());
    }

    // 24z3 step 3, f1: the window's "+" on a shown server picks a folder there through this master's folder list.
    #[test]
    fn lists_its_folders_for_the_windows_folder_picker() {
        let data = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join("shop")).unwrap();
        std::fs::write(folder.path().join("notes.txt"), "").unwrap();
        let m = Master::open(data.path()).unwrap();
        let found = call(
            &m,
            "dirs_list",
            json!({"path": folder.path().to_string_lossy()}),
        )
        .unwrap();
        assert_eq!(found["dirs"].as_array().unwrap().len(), 1);
        assert_eq!(found["dirs"][0]["name"], "shop");
        let shop = found["dirs"][0]["path"].as_str().unwrap().to_string();
        let p = call(&m, "projects_add", json!({"path": shop})).unwrap();
        assert_eq!(p["name"], "shop");
        assert!(call(&m, "dirs_list", json!({"path": ""})).unwrap()["path"].is_string());
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
        let hello = call(&m, "hello", json!({})).unwrap();
        assert_eq!(hello["name"], "Qanuvi master");
        // Its lasting id: the same after it opens again on the same data folder.
        let id = hello["machineId"].as_str().unwrap().to_string();
        assert_eq!(id, crate::machine::lasting_id(data.path()));
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

    // R2e part 2: the Plan view of a master project found no task list ("does not know the command files_markdown"),
    // and its plan data (holds, picks, run settings) went to this computer, where the master's runner never reads it.
    #[test]
    fn answers_the_plan_views_file_and_plan_data_commands() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("PLAN-M.md"),
            "## Step 1\n- [ ] 1a. Add cat\n",
        )
        .unwrap();
        std::fs::write(project.path().join("a.js"), "cat catalog\n").unwrap();
        let m = Master::open(data.path()).unwrap();
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();

        let md = call(&m, "files_markdown", json!({"projectId": id})).unwrap();
        assert!(
            md.as_array().unwrap().iter().any(|f| f == "PLAN-M.md"),
            "{md}"
        );
        let changed = call(
            &m,
            "files_changed_since",
            json!({"projectId": id, "sinceMs": 0}),
        )
        .unwrap();
        assert!(
            changed.as_array().unwrap().iter().any(|f| f == "a.js"),
            "{changed}"
        );
        call(
            &m,
            "files_log_append",
            json!({"projectId": id, "path": ".qanuvi/plan-logs/PLAN-M/1a.md", "text": "tried once\n"}),
        )
        .unwrap();
        let log =
            std::fs::read_to_string(project.path().join(".qanuvi/plan-logs/PLAN-M/1a.md")).unwrap();
        assert!(log.contains("tried once"));

        // Search with the window's options (whole words), then replace with them.
        let opts = json!({"regex": false, "caseSensitive": true, "wholeWord": true});
        let found = call(
            &m,
            "files_search",
            json!({"projectId": id, "query": "cat", "regex": false, "caseSensitive": true, "options": opts}),
        )
        .unwrap();
        assert_eq!(
            found["matches"].as_array().unwrap().len(),
            2,
            "whole word 'cat' once in a.js and once in PLAN-M.md: {found}"
        );
        let done = call(
            &m,
            "files_replace",
            json!({"projectId": id, "query": "cat", "options": opts, "replacement": "dog", "paths": ["a.js"]}),
        )
        .unwrap();
        assert_eq!(done["replacements"], 1);
        assert_eq!(
            std::fs::read_to_string(project.path().join("a.js")).unwrap(),
            "dog catalog\n"
        );

        // Plan data lives in the master's own database, where its runner reads it.
        let hold = json!({"holds": {"1a": "wait"}});
        call(
            &m,
            "plan_data_set",
            json!({"projectId": id, "file": "PLAN-M.md", "kind": "run", "value": hold}),
        )
        .unwrap();
        assert_eq!(
            crate::plan_data::get(&m.db, &id, "PLAN-M.md", crate::plan_data::Kind::Run).unwrap(),
            Some(hold.clone())
        );
        let all = call(&m, "plan_data_list", json!({})).unwrap();
        assert_eq!(all[0]["project_id"], id);
        assert_eq!(all[0]["value"], hold);
        call(
            &m,
            "plan_data_set",
            json!({"projectId": id, "file": "PLAN-M.md", "kind": "run", "value": null}),
        )
        .unwrap();
        assert_eq!(call(&m, "plan_data_list", json!({})).unwrap(), json!([]));
    }

    #[test]
    fn says_where_it_keeps_secrets_and_only_for_its_projects() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        assert_eq!(
            call(&m, "secrets_store", json!({})).unwrap(),
            json!(crate::secrets::kind())
        );
        for cmd in ["secrets_names", "secrets_set", "secrets_remove"] {
            let args = json!({"projectId": "not-a-project", "name": "DB_PASSWORD", "value": "x"});
            assert!(call(&m, cmd, args).is_err(), "{cmd}");
        }
    }

    #[test]
    fn answers_the_machine_and_its_setup() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let info = call(&m, "machine_info", json!({})).unwrap();
        assert!(info["ramGb"].as_f64().is_some(), "{info}");
        call(&m, "machine_room", json!({})).unwrap();
        assert!(call(&m, "setup_tools", json!({})).unwrap()["os"].is_string());
        call(&m, "qanuvi_usage", json!({})).unwrap();
        let e = call(&m, "setup_install_plan", json!({"tool": "rm"})).unwrap_err();
        assert_eq!(e, "Qanuvi does not install that.");
        let e = call(
            &m,
            "setup_install_start",
            json!({"tool": "rm", "onEvent": "t1"}),
        )
        .unwrap_err();
        assert!(!e.contains("does not know"), "{e}");
    }

    #[test]
    fn answers_its_own_backups_and_shows_its_data_folder() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let d = data.path();
        let loc = call(&m, "data_location_info", json!({})).unwrap();
        assert_eq!(loc["fixed"], true);
        assert_eq!(loc["current"], d.to_string_lossy().as_ref());

        // On by default, in its own default folder (never the desktop app's); nothing is written there here.
        let i = call(&m, "backups_info", json!({})).unwrap();
        let def = backups_default().to_string_lossy().into_owned();
        assert_eq!(
            (i["dir"].as_str(), i["defaultDir"].as_str()),
            (Some(def.as_str()), Some(def.as_str()))
        );
        assert_eq!(
            (i["off"].as_bool(), i["custom"].as_bool()),
            (Some(false), Some(false))
        );
        let expected = if cfg!(any(windows, target_os = "macos")) {
            "Qanuvi master backups"
        } else {
            "qanuvi-backups"
        };
        assert!(def.contains(expected), "{def}");
        assert_ne!(backups_default(), crate::backups::default_dir(false));
        assert_ne!(backups_default(), crate::backups::default_dir(true));

        let dest = tempfile::tempdir().unwrap();
        let folder = dest.path().join("b").to_string_lossy().into_owned();
        call(&m, "backups_set", json!({"folder": folder, "off": false})).unwrap();
        let made = call(&m, "backups_now", json!({})).unwrap();
        let i = call(&m, "backups_info", json!({})).unwrap();
        assert_eq!(i["backups"].as_array().unwrap().len(), 1, "{i}");
        call(&m, "backups_restore", json!({"path": made})).unwrap();
        assert_eq!(
            call(&m, "backups_info", json!({})).unwrap()["restore"],
            made
        );
        call(&m, "backups_cancel_restore", json!({})).unwrap();
        call(&m, "backups_remove", json!({"path": made})).unwrap();
        let busy = call(&m, "master_busy", json!({})).unwrap();
        assert_eq!(
            (busy["agents"].as_u64(), busy["runs"].as_u64()),
            (Some(0), Some(0))
        );
        assert_eq!(call(&m, "agents_working", json!({})).unwrap(), 0);
        call(&m, "backups_set", json!({"folder": "", "off": false})).unwrap();
    }

    #[test]
    fn answers_the_models_page_with_its_own_keys() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        assert_eq!(
            call(&m, "model_key_store", json!({})).unwrap(),
            json!(crate::model_keys::kind())
        );
        // A key in the master's environment counts (a name no other test uses).
        let name = crate::model_keys::env_name("d1master-env");
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var(&name, "k2") };
        assert_eq!(
            call(&m, "model_key_has", json!({"endpoint": "d1master-env"})).unwrap(),
            json!(true)
        );
        unsafe { std::env::remove_var(&name) };
        assert_eq!(
            call(&m, "model_key_has", json!({"endpoint": "d1master-env"})).unwrap(),
            json!(false)
        );
        assert_eq!(
            call(&m, "brain_records", json!({})).unwrap()["models"],
            json!({})
        );
        // Known commands: a bad endpoint is the endpoint's error, never "does not know the command".
        for cmd in [
            "models_list",
            "model_probe",
            "model_key_check",
            "local_server_status",
            "local_server_start",
            "ollama_pull",
        ] {
            let e = call(&m, cmd, json!({"endpoint": {"id": "x"}})).unwrap_err();
            assert!(!e.contains("does not know"), "{cmd}: {e}");
        }
        #[cfg(not(feature = "desktop"))]
        {
            let e = call(
                &m,
                "model_key_set",
                json!({"endpoint": "openrouter", "key": "k"}),
            )
            .unwrap_err();
            assert!(e.contains("QANUVI_KEY_OPENROUTER"), "{e}");
        }
    }

    #[test]
    fn answers_memory_disk_and_agent_setup_on_its_own_machine() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        assert!(!m.app_data);
        // Known commands (24z3 D6): any problem is the command's own, never "does not know the command".
        let args = json!({"provider": "claude", "id": "nothing", "sessionId": "none", "days": 0, "keep": []});
        for cmd in [
            "memory_disk_device",
            "memory_disk_status",
            "memory_disk_passphrase_saved",
            "memory_disk_passphrase_set",
            "memory_disk_sync",
            "memory_disk_versions",
            "memory_disk_restore",
            "agent_config_list",
            "agent_config_read",
            "agent_config_write",
            "agent_configure",
            "agent_clean_up",
        ] {
            if let Err(e) = call(&m, cmd, args.clone()) {
                assert!(!e.contains("does not know the command"), "{cmd}: {e}");
            }
        }
        // The master's own disk is the one its settings name: none yet, said by its sync and kept for the window.
        let e = call(&m, "memory_disk_sync", json!({})).unwrap_err();
        assert!(e.contains("Choose where"), "{e}");
        let st = call(&m, "memory_disk_status", json!({})).unwrap();
        assert_eq!(st["error"], json!(e));
        assert_eq!(
            call(&m, "memory_disk_versions", json!({})).unwrap(),
            json!([])
        );
        // Its device id is its own, the same when asked again.
        let a = call(&m, "memory_disk_device", json!({})).unwrap();
        assert_eq!(a, call(&m, "memory_disk_device", json!({})).unwrap());
        assert_eq!(
            call(&m, "agent_clean_up", json!({"days": 0, "keep": []})).unwrap(),
            json!(0)
        );
    }

    #[test]
    fn answers_memory_entries_from_its_own_memory() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        // 24z3 D7: list, save, search, slice and delete work on this master's own memory.
        assert_eq!(
            call(
                &m,
                "memory_list",
                json!({"project": null, "suggested": true})
            )
            .unwrap(),
            json!([])
        );
        let saved = call(
            &m,
            "memory_save",
            json!({"entry": {"kind": "fact", "title": "Master memory test fact", "body": "kept on the master"}}),
        )
        .unwrap();
        let id = saved["id"].as_str().unwrap().to_string();
        assert!(!id.is_empty());
        let listed = call(
            &m,
            "memory_list",
            json!({"project": null, "suggested": false}),
        )
        .unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);
        let found = call(
            &m,
            "memory_search",
            json!({"project": null, "query": "master memory"}),
        )
        .unwrap();
        assert_eq!(found[0]["id"], json!(id));
        let slice = call(
            &m,
            "memory_slice",
            json!({"project": null, "query": "master memory", "budget": 4000, "count": false}),
        )
        .unwrap();
        assert!(
            slice.to_string().contains("Master memory test fact"),
            "{slice}"
        );
        call(&m, "memory_delete", json!({"id": id})).unwrap();
        assert_eq!(
            call(
                &m,
                "memory_list",
                json!({"project": null, "suggested": true})
            )
            .unwrap(),
            json!([])
        );
    }

    #[test]
    fn a_master_on_the_apps_own_data_is_this_pc() {
        let data = tempfile::tempdir().unwrap();
        assert!(!on_app_data(data.path()));
        mark_app_data(data.path());
        mark_app_data(data.path());
        assert!(on_app_data(data.path()));
        let m = Master::open(data.path()).unwrap().shared();
        assert!(m.app_data);
        // M4b: This PC's start-up list is its master's to start now (the app no longer starts it).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let item = json!({"id": "proxy", "name": "proxy", "command": "no-such-program-xyz", "waitFor": addr, "enabled": true});
        call(&m, "settings_set", json!({"values": {"startup": [item]}})).unwrap();
        m.start_own();
        let st = call(&m, "startup_status", json!({})).unwrap();
        assert_eq!(st[0]["id"], "proxy", "{st}");
        m.startup.stop_all();
        // Its backups are the app's: the app's folder (not the master's own), as the app made them before M4b.
        let (_, folder) = backup_place(data.path(), true);
        assert_eq!(
            folder,
            crate::backups::default_dir(crate::data_location::from_source())
        );
        let (web, folder) = backup_place(data.path(), false);
        assert_eq!((web, folder), (None, backups_default()));
    }

    #[test]
    fn m4b_the_app_reaches_this_pcs_master_and_hears_its_inbox() {
        let data = tempfile::tempdir().unwrap();
        mark_app_data(data.path());
        // Not running: the call says so after its wait instead of starting a second master.
        let missing = tempfile::tempdir().unwrap();
        let started = Instant::now();
        assert!(this_pc_call(missing.path(), "hello", json!({})).is_err());
        assert!(started.elapsed() >= Duration::from_secs(9));
        let m = Master::open(data.path()).unwrap().shared();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = "t0";
        std::fs::write(
            service_file(data.path()),
            json!({"port": port, "token": token, "pid": std::process::id()}).to_string(),
        )
        .unwrap();
        let server = m.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let master = server.clone();
                std::thread::spawn(move || {
                    let write_half = stream.try_clone().unwrap();
                    let mut reader = BufReader::new(stream);
                    let mut first = String::new();
                    reader.read_line(&mut first).unwrap();
                    let hello: Value = serde_json::from_str(&first).unwrap();
                    assert_eq!(hello["token"], "t0");
                    handle_connection(master, reader, Arc::new(Mutex::new(Box::new(write_half))));
                });
            }
        });
        let st = this_pc_call(data.path(), "startup_status", json!({})).unwrap();
        assert_eq!(st, json!([]));
        assert!(this_pc_call(data.path(), "startup_start", json!({"items": "nonsense"})).is_err());
        // A change the master makes reaches the app's follower, in the app's event payload.
        let (tx, rx) = std::sync::mpsc::channel();
        let dir = data.path().to_path_buf();
        std::thread::spawn(move || {
            follow_this_pc_inbox(
                &dir,
                || true,
                move |v| {
                    let _ = tx.send(v);
                },
            )
        });
        let item = json!({"kind": "test", "title": "Test note"});
        let deadline = Instant::now() + Duration::from_secs(10);
        let got = loop {
            call(&m, "inbox_add", json!({"item": item})).unwrap();
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(300)) {
                break v;
            }
            assert!(Instant::now() < deadline, "no Inbox change heard");
        };
        assert_eq!(got["change"], "add");
    }

    static TRAY_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn test_tray(_: Arc<Master>) -> ! {
        TRAY_SHOWN.store(true, std::sync::atomic::Ordering::SeqCst);
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }

    #[test]
    fn m4c_this_pcs_master_shows_its_tray_and_reaches_the_apps_windows() {
        // A master on the app's data hands its thread to the tray and still answers on another one.
        let data = tempfile::tempdir().unwrap();
        mark_app_data(data.path());
        let dir = data.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = serve(&dir, Some(test_tray));
        });
        let hello = this_pc_call(data.path(), "hello", json!({})).unwrap();
        assert_eq!(hello["name"], "Qanuvi master");
        assert!(TRAY_SHOWN.load(std::sync::atomic::Ordering::SeqCst));
        // The app's link hears the tray's "Open Qanuvi" (a running app shows its window, no second app starts).
        let m = Master::open(data.path()).unwrap().shared();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let other = tempfile::tempdir().unwrap();
        mark_app_data(other.path());
        std::fs::write(
            service_file(other.path()),
            json!({"port": port, "token": "t1", "pid": std::process::id()}).to_string(),
        )
        .unwrap();
        let server = m.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let master = server.clone();
                std::thread::spawn(move || {
                    let write_half = stream.try_clone().unwrap();
                    let mut reader = BufReader::new(stream);
                    let mut first = String::new();
                    reader.read_line(&mut first).unwrap();
                    handle_connection(master, reader, Arc::new(Mutex::new(Box::new(write_half))));
                });
            }
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let link_dir = other.path().to_path_buf();
        std::thread::spawn(move || {
            follow_this_pc_app(
                &link_dir,
                || true,
                move |v| {
                    let _ = tx.send(v);
                },
            );
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while m.app_links.reach(&json!({"do": "wait"})) == 0 {
            assert!(Instant::now() < deadline, "the app's link did not come");
            std::thread::sleep(Duration::from_millis(100));
        }
        m.open_app();
        let mut asked = Vec::new();
        while let Ok(v) = rx.recv_timeout(Duration::from_secs(5)) {
            asked.push(v["do"].as_str().unwrap_or("").to_string());
            if asked.contains(&"show".to_string()) {
                break;
            }
        }
        assert!(asked.contains(&"show".to_string()), "{asked:?}");
        // A link whose connection ended counts no more (Open then starts the app), even before a write fails.
        let ended = Master::open(other.path()).unwrap().shared();
        let input = std::io::Cursor::new(
            format!(
                "{}\n",
                json!({"id": 1, "cmd": "app_follow", "args": {"onEvent": "follow"}})
            )
            .into_bytes(),
        );
        handle_connection(
            ended.clone(),
            input,
            Arc::new(Mutex::new(Box::new(std::io::sink()))),
        );
        assert_eq!(ended.app_links.reach(&json!({"do": "wait"})), 0);
        // Another master never starts at sign-in.
        let plain = tempfile::tempdir().unwrap();
        let pm = Master::open(plain.path()).unwrap().shared();
        assert_eq!(call(&pm, "sign_in_start", json!({})).unwrap(), json!(false));
        assert!(call(&pm, "sign_in_start_set", json!({"on": true})).is_err());
        assert_eq!(call(&pm, "app_follow", json!({})).unwrap()["tray"], false);
    }

    #[test]
    fn m5a_a_master_starting_stops_what_its_last_life_left_running() {
        let status = |dir: &Path| -> String {
            let db = Db::open(&dir.join("qanuvi.db")).unwrap();
            db.conn()
                .query_row("SELECT status FROM plan_runs WHERE id = 'r1'", [], |r| {
                    r.get(0)
                })
                .unwrap()
        };
        for app_data in [true, false] {
            let data = tempfile::tempdir().unwrap();
            let project = tempfile::tempdir().unwrap();
            {
                let db = Db::open(&data.path().join("qanuvi.db")).unwrap();
                let p = crate::projects::add(&db, &project.path().to_string_lossy()).unwrap();
                runs::save(&db, &p.id, "r1", "PLAN.md", "running", "{\"steps\":[]}").unwrap();
            }
            if app_data {
                mark_app_data(data.path());
            }
            let _m = Master::open(data.path()).unwrap();
            // M5a: This PC's runs are its master's too (no app core runs them any more), so either way what the last
            // life left running is stopped.
            assert_eq!(status(data.path()), "stopped", "app data: {app_data}");
        }
    }

    #[test]
    fn answers_omniroute_on_its_own_machine() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        // Known commands: a bad endpoint gets the endpoint's error, never "does not know the command".
        for cmd in [
            "omniroute_connect",
            "omniroute_connections",
            "omniroute_catalog",
            "omniroute_add_key",
            "omniroute_test",
            "omniroute_remove",
            "omniroute_sign_in",
            "omniroute_make_combo",
        ] {
            let e = call(&m, cmd, json!({"endpoint": {"id": "x"}})).unwrap_err();
            assert!(!e.contains("does not know"), "{cmd}: {e}");
        }
        // Its own settings say Qanuvi does not run OmniRoute there: nothing to watch.
        let mut off = serde_json::Map::new();
        off.insert("brain".into(), json!({"omniAssist": false}));
        crate::settings::set_many(&m.db, &off).unwrap();
        assert_eq!(call(&m, "omniroute_look", json!({})).unwrap(), Value::Null);
        #[cfg(not(feature = "desktop"))]
        {
            let ep =
                json!({"id": "omniroute", "kind": "openai", "url": "http://localhost:20128/v1"});
            let e = call(&m, "omniroute_connect", json!({"endpoint": ep})).unwrap_err();
            assert!(e.contains("QANUVI_KEY_OMNIROUTE"), "{e}");
            let e = call(
                &m,
                "omniroute_sign_in",
                json!({"endpoint": ep, "provider": "antigravity"}),
            )
            .unwrap_err();
            assert_eq!(e, "panel");
        }
    }

    #[test]
    fn starts_its_own_start_up_list_and_answers_its_commands() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        // Something already answers at this address: started with the master, it is left alone.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let item = |id: &str, enabled: bool| json!({"id": id, "name": id, "command": "no-such-program-xyz", "waitFor": addr, "enabled": enabled});
        call(
            &m,
            "settings_set",
            json!({"values": {"startup": [item("proxy", true), item("off", false)]}}),
        )
        .unwrap();
        m.start_own();
        let st = call(&m, "startup_status", json!({})).unwrap();
        assert_eq!(st.as_array().unwrap().len(), 1, "{st}");
        assert_eq!(st[0]["state"], "already running");
        // The window's buttons: restart, stop, start again.
        let st = call(&m, "startup_restart", json!({"item": item("proxy", true)})).unwrap();
        assert_eq!(st[0]["state"], "already running");
        let st = call(&m, "startup_stop", json!({"id": "proxy"})).unwrap();
        assert!(st.as_array().unwrap().is_empty());
        let st = call(&m, "startup_start", json!({"items": [item("proxy", true)]})).unwrap();
        assert_eq!(st[0]["id"], "proxy");
        assert!(call(&m, "startup_start", json!({"items": "nonsense"})).is_err());
        // Answered on the master, never "does not know the command".
        call(&m, "startup_headroom", json!({})).unwrap();
        call(&m, "claude_settings_base_url", json!({})).unwrap();
        m.startup.stop_all();
    }

    #[test]
    fn keeps_its_own_settings_and_refuses_the_windows() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        assert_eq!(call(&m, "settings_get", json!({})).unwrap(), json!({}));
        call(
            &m,
            "settings_set",
            json!({"values": {"approveMinutes": 20}}),
        )
        .unwrap();
        assert_eq!(
            call(&m, "settings_get", json!({})).unwrap(),
            json!({"approveMinutes": 20})
        );
        assert!(call(&m, "settings_set", json!({"values": {"masters": []}})).is_err());
        assert!(call(&m, "settings_set", json!({"values": [1]})).is_err());
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
            let _ = serve(&d2, None);
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

    #[test]
    fn runs_a_plan_on_the_master_and_streams_its_views_to_a_watching_connection() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        // Every step is done already: the run ends at once and starts no agent.
        std::fs::write(
            project.path().join("PLAN.md"),
            "# Plan\n\n- [x] **1. Done**\n",
        )
        .unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        let heard: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let h = heard.clone();
        let streams = move |name: &str| {
            let (h, name) = (h.clone(), name.to_string());
            Sink::new(move |v: Value| {
                h.lock().unwrap().push((name.clone(), v));
                true
            })
        };
        let start = json!({"projectId": id, "onEvent": "r1", "start": {"file": "PLAN.md", "projectName": "Shop"}});
        m.dispatch(
            "plan_run_start",
            &start,
            &streams,
            &Mutex::new(HashSet::new()),
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !call(&m, "plan_run_active", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
        {
            assert!(Instant::now() < until, "the run did not end");
            std::thread::sleep(Duration::from_millis(5));
        }
        let state = call(&m, "plan_run_state", json!({"projectId": id})).unwrap();
        assert_eq!(state["running"], false);
        assert_eq!(state["run"]["status"], "done");
        assert_eq!(state["run"]["message"], "No open steps to run.");
        let heard = heard.lock().unwrap();
        assert!(heard.iter().all(|(n, _)| n == "r1"));
        let last = &heard.last().expect("the views came on the stream").1;
        assert_eq!(last["kind"], "view");
        assert_eq!(last["view"]["run"]["status"], "done");
        // An unknown run command is said plainly, and so is a start that does not read.
        let e = call(&m, "plan_run_nothing", json!({"projectId": id})).unwrap_err();
        assert!(e.contains("does not know the command plan_run_nothing"));
        let bad = call(
            &m,
            "plan_run_start",
            json!({"projectId": id, "start": {"file": 5}}),
        )
        .unwrap_err();
        assert!(bad.contains("start is not valid"));
    }

    #[test]
    fn its_inbox_is_read_and_answered_from_the_desktop() {
        use crate::plan::env::RunEnv;
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        let heard: Arc<Mutex<Vec<Value>>> = Arc::default();
        let h = heard.clone();
        let streams = move |_: &str| {
            let h = h.clone();
            Sink::new(move |v: Value| {
                h.lock().unwrap().push(v);
                true
            })
        };
        let first = m
            .dispatch(
                "inbox_watch",
                &json!({"onEvent": "i1"}),
                &streams,
                &Mutex::new(HashSet::new()),
            )
            .unwrap();
        assert_eq!(first, json!([]));
        // A question a run on the master asks reaches the watching desktop, though it does not watch the run.
        let asked = m
            .run_env(&id)
            .unwrap()
            .inbox_add(&crate::inbox::NewItem {
                kind_of: "answer".into(),
                kind: "run".into(),
                title: "Git cannot see some files".into(),
                text: String::new(),
                key: Some(format!("changes-from:{id}")),
                open: None,
                form: Some(json!({"choices": [{"id": "qanuvi", "label": "Use snapshots"}]})),
                source: Some(json!({"kind": "changes-from", "projectId": id})),
            })
            .unwrap();
        {
            let heard = heard.lock().unwrap();
            assert_eq!(heard.len(), 1);
            assert_eq!(heard[0]["change"], "add");
            assert_eq!(heard[0]["item"]["id"], asked.id.as_str());
        }
        let list = call(&m, "inbox_list", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        // "All read" leaves the open question unread, as on the desktop.
        assert_eq!(
            call(&m, "inbox_mark_read", json!({"id": null})).unwrap(),
            false
        );
        // The desktop's answer closes it here, says so to the watchers and is applied to the project.
        let done = call(
            &m,
            "inbox_answer",
            json!({"id": asked.id, "answer": {"choice": "qanuvi"}, "by": "main"}),
        )
        .unwrap();
        assert_eq!(done["status"], "answered");
        assert_eq!(done["answeredBy"], "main");
        assert_eq!(
            crate::projects::get(&m.db, &id)
                .unwrap()
                .settings
                .changes_from,
            "qanuvi"
        );
        assert_eq!(heard.lock().unwrap().last().unwrap()["change"], "answer");
        // A second answer (another window, the phone) is too late.
        let late = call(
            &m,
            "inbox_answer",
            json!({"id": asked.id, "answer": {"choice": "qanuvi"}}),
        )
        .unwrap();
        assert!(late.is_null());
        assert!(
            call(&m, "inbox_answer", json!({}))
                .unwrap_err()
                .contains("Which question")
        );
        assert_eq!(
            call(&m, "inbox_remove", json!({"id": asked.id})).unwrap(),
            true
        );
        assert_eq!(call(&m, "inbox_list", json!({})).unwrap(), json!([]));
        assert_eq!(heard.lock().unwrap().last().unwrap()["change"], "remove");
        // A note the window writes about the master's own work lands in the master's Inbox; the same key replaces it.
        let note = json!({"item": {"kind": "drafts", "title": "Unsaved changes kept", "text": "a", "key": "draft:x"}});
        let first = call(&m, "inbox_add", note.clone()).unwrap();
        assert_eq!(first["kind"], "drafts");
        assert_eq!(heard.lock().unwrap().last().unwrap()["new"], true);
        call(&m, "inbox_add", note).unwrap();
        assert_eq!(heard.lock().unwrap().last().unwrap()["change"], "add");
        assert_eq!(
            call(&m, "inbox_list", json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(call(&m, "inbox_add", json!({"item": {}})).is_err());
    }

    // Migration M1a: the pipes a master lends to a viewer, through its own commands, as a viewer reaches them.
    struct TestLink {
        m: Arc<Master>,
        opened: Arc<Mutex<HashSet<String>>>,
    }

    impl crate::pipes::Link for TestLink {
        fn call(
            &self,
            cmd: &str,
            args: Value,
            events: Option<Sink<Value>>,
        ) -> Result<Value, String> {
            let ev = events.unwrap_or_else(|| Sink::new(|_| true));
            let streams = move |_: &str| ev.clone();
            self.m.dispatch(cmd, &args, &streams, &self.opened)
        }
        fn tell(&self, cmd: &str, args: Value) {
            // Each command on its own thread, as a connection answers them, so writes may arrive out of order.
            let (m, opened, cmd) = (self.m.clone(), self.opened.clone(), cmd.to_string());
            std::thread::spawn(move || {
                let streams = |_: &str| Sink::new(|_: Value| true);
                let _ = m.dispatch(&cmd, &args, &streams, &opened);
            });
        }
    }

    #[test]
    fn lends_a_program_pipe_and_a_port_and_ends_them_with_the_connection() {
        use base64::{Engine, engine::general_purpose::STANDARD as B64};
        use std::io::{Read, Write};
        let data = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        let opened: Arc<Mutex<HashSet<String>>> = Arc::default();
        let link = Arc::new(TestLink {
            m: m.clone(),
            opened: opened.clone(),
        });

        // A port: an echo server on this machine, reached through a forward on a free local port.
        let server = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in server.incoming().map_while(Result::ok) {
                std::thread::spawn(move || {
                    let mut r = s.try_clone().unwrap();
                    let mut w = s;
                    let _ = std::io::copy(&mut r, &mut w);
                });
            }
        });
        let fwd = crate::pipes::forward(link.clone(), port).unwrap();
        assert_ne!(fwd.local, port);
        let mut c = std::net::TcpStream::connect(("127.0.0.1", fwd.local)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let sent: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut w = c.try_clone().unwrap();
        let to_send = sent.clone();
        std::thread::spawn(move || w.write_all(&to_send).unwrap());
        let mut got = vec![0u8; sent.len()];
        c.read_exact(&mut got).unwrap();
        assert!(got == sent, "the bytes came back changed or out of order");
        assert_eq!(opened.lock().unwrap().len(), 1);
        drop(c);

        // A program: a stand-in that echoes what comes in.
        if let Some(node) = crate::local_servers::find("node") {
            let (tx, rx) = std::sync::mpsc::channel();
            let sink = Sink::new(move |e: Value| tx.send(e).is_ok());
            let streams = move |_: &str| sink.clone();
            let id = m
                .dispatch(
                    "pipe_start",
                    &json!({"program": node.to_string_lossy(), "args": ["-e", "process.stdin.pipe(process.stdout)"]}),
                    &streams,
                    &opened,
                )
                .unwrap();
            let id = id.as_str().unwrap();
            let none = |_: &str| Sink::new(|_: Value| true);
            m.dispatch(
                "pipe_send",
                &json!({"id": id, "seq": 0, "data": B64.encode("Content-Length: 2\r\n\r\n{}")}),
                &none,
                &opened,
            )
            .unwrap();
            let mut out = Vec::new();
            while out.len() < 23 {
                let e = rx.recv_timeout(Duration::from_secs(10)).unwrap();
                if e["kind"] == "out" {
                    out.extend(B64.decode(e["data"].as_str().unwrap()).unwrap());
                }
            }
            assert_eq!(out, b"Content-Length: 2\r\n\r\n{}");
            // The connection ends: every pipe it opened ends with it.
            for id in opened.lock().unwrap().drain() {
                m.pipes.stop(&id);
            }
            let exit = loop {
                let e = rx.recv_timeout(Duration::from_secs(10)).unwrap();
                if e["kind"] == "exit" {
                    break e;
                }
            };
            assert!(exit.get("code").is_some());
        }
        drop(fwd);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while m.pipes.running() > 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(m.pipes.running(), 0);
        let none = |_: &str| Sink::new(|_: Value| true);
        assert!(
            m.dispatch("pipe_start", &json!({"program": ""}), &none, &opened)
                .is_err()
        );
        assert!(
            m.dispatch("port_open", &json!({"port": 70000}), &none, &opened)
                .is_err()
        );
    }

    // Migration M1b: generic questions a viewer's coding tools ask about this machine.
    #[test]
    fn answers_generic_questions_about_its_machine() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let info = call(&m, "host_info", json!({})).unwrap();
        assert!(info["dataDir"].is_string() && info["sep"].is_string());
        assert_eq!(
            call(&m, "host_find", json!({"program": "no-such-program-qz"})).unwrap(),
            Value::Null
        );
        assert_eq!(
            call(
                &m,
                "host_exists",
                json!({"path": data.path().to_string_lossy()})
            )
            .unwrap(),
            json!(true)
        );
        if let Some(node) = crate::local_servers::find("node") {
            let r = call(
                &m,
                "host_run",
                json!({"program": node.to_string_lossy(), "args": ["-e", "console.error('nope'); process.exit(3)"]}),
            )
            .unwrap();
            assert_eq!(r["code"], 3);
            assert_eq!(r["err"], "nope");
            // A caller that listens gets the output lines through its stream (live install output).
            let got = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
            let g = got.clone();
            let streams = move |name: &str| {
                let g = g.clone();
                let name = name.to_string();
                Sink::new(move |e: Value| {
                    g.lock()
                        .unwrap()
                        .push((name.clone(), e["line"].as_str().unwrap_or("").into()));
                    true
                })
            };
            m.dispatch(
                "host_run",
                &json!({"program": node.to_string_lossy(), "onEvent": "s9",
                        "args": ["-e", "console.log('one'); console.error('two')"]}),
                &streams,
                &Mutex::new(HashSet::new()),
            )
            .unwrap();
            let mut lines = got.lock().unwrap().clone();
            lines.sort();
            assert_eq!(
                lines,
                [
                    ("s9".to_string(), "one".to_string()),
                    ("s9".into(), "two".into())
                ]
            );
        }
        let rel = format!("host-test-{}/a/package.json", uuid::Uuid::new_v4());
        call(&m, "host_write_new", json!({"path": rel, "text": "{}"})).unwrap();
        call(
            &m,
            "host_write_new",
            json!({"path": rel, "text": "changed"}),
        )
        .unwrap();
        let file = crate::paths::data_dir().join(&rel);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{}");
        // A small file reads back; a missing one is null (M1d: a debugger's version note).
        assert_eq!(
            call(&m, "host_read", json!({"path": rel})).unwrap(),
            json!("{}")
        );
        assert_eq!(
            call(&m, "host_read", json!({"path": format!("{rel}.none")})).unwrap(),
            Value::Null
        );
        let _ =
            std::fs::remove_dir_all(crate::paths::data_dir().join(rel.split('/').next().unwrap()));
        for bad in ["../x.txt", "", "/etc/x", "a/../../x"] {
            assert!(
                call(&m, "host_write_new", json!({"path": bad, "text": ""})).is_err(),
                "{bad}"
            );
            assert!(
                call(&m, "host_read", json!({"path": bad})).is_err(),
                "{bad}"
            );
            assert!(
                call(
                    &m,
                    "host_fetch",
                    json!({"url": "https://github.com/x.zip", "archive": "zip", "into": bad, "expect": "a.js"})
                )
                .is_err(),
                "{bad}"
            );
        }
        // Only https downloads.
        assert!(
            call(
                &m,
                "host_fetch",
                json!({"url": "http://example.com/x.zip", "archive": "zip", "into": "debug-adapters/t", "expect": "a.js"})
            )
            .is_err()
        );
    }

    /// M5a: what would stop is said in words, and a run stopped by an update (or Quit, a restart) says so after the
    /// next master starts; This PC's own master marks its leftovers too (they are its own now).
    #[test]
    fn m5a_a_stop_says_why_and_what_it_stops() {
        assert_eq!(work_text((0, 0)), None);
        assert_eq!(work_text((1, 0)).unwrap(), "1 agent session");
        assert_eq!(
            work_text((2, 1)).unwrap(),
            "2 agent sessions and 1 plan run"
        );
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        mark_app_data(data.path());
        let pid = {
            let m = Master::open(data.path()).unwrap();
            let p = call(
                &m,
                "projects_add",
                json!({"path": project.path().to_string_lossy()}),
            )
            .unwrap();
            let pid = p["id"].as_str().unwrap().to_string();
            runs::save(
                &m.db,
                &pid,
                "r1",
                "T.md",
                "running",
                r#"{"status":"running"}"#,
            )
            .unwrap();
            assert_eq!(m.work().1, 0, "a record is no live run");
            pid
        };
        std::fs::write(stop_note_file(data.path()), "update").unwrap();
        let m = Master::open(data.path()).unwrap();
        assert!(m.app_data);
        assert!(!stop_note_file(data.path()).exists(), "read once");
        let r = runs::list(&m.db, &pid).unwrap().remove(0);
        assert_eq!(r.status, "stopped");
        assert!(r.data.contains(runs::stop_note("update").unwrap()));
    }

    /// M5a: what the app's core did for This PC's windows, its master does: plan data told to every follower (a
    /// window's write and a run's), the one-time moves out of old window storage, and the chat's attachments.
    #[test]
    fn m5a_plan_data_is_followed_and_the_moved_commands_answer() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        let heard: Arc<Mutex<Vec<Value>>> = Arc::default();
        let h = heard.clone();
        let streams = move |_: &str| {
            let h = h.clone();
            Sink::new(move |v: Value| {
                h.lock().unwrap().push(v);
                true
            })
        };
        let open = Mutex::new(HashSet::new());
        let first = m
            .dispatch(
                "plan_data_watch",
                &json!({"onEvent": "d1"}),
                &streams,
                &open,
            )
            .unwrap();
        assert_eq!(first, json!([]));
        call(
            &m,
            "plan_data_set",
            json!({"projectId": id, "file": "PLAN.md", "kind": "notes", "value": {"a": 1}}),
        )
        .unwrap();
        assert_eq!(
            heard.lock().unwrap().last().unwrap(),
            &json!({"project_id": id, "file": "PLAN.md", "kind": "notes", "value": {"a": 1}})
        );
        // A run's plan data reaches the followers too, not only the run's watchers.
        let env = m.run_env(&id).unwrap();
        (env.notify)(
            "qanuvi-plan-data",
            json!({"project_id": id, "file": "PLAN.md", "kind": "state", "value": 2}),
        );
        assert_eq!(heard.lock().unwrap().last().unwrap()["kind"], "state");

        // The old window copies move in once.
        let n = call(
            &m,
            "plan_data_import",
            json!({"entries": [{"project_id": id, "file": "B.md", "kind": "picks", "value": [1]}]}),
        )
        .unwrap();
        assert_eq!(n, 1);
        let n = call(
            &m,
            "inbox_import",
            json!({"items": [{"id": "old-1", "at": 1, "kind": "info", "title": "An old note"}]}),
        )
        .unwrap();
        assert_eq!(n, 1);
        let list = call(&m, "inbox_list", json!({})).unwrap();
        assert!(
            list.as_array()
                .unwrap()
                .iter()
                .any(|i| i["title"] == "An old note")
        );

        // The chat's attachments and the brain's calls answer here (their own errors, never an unknown command).
        for (cmd, args) in [
            ("attachment_image", json!({"file": "missing.png"})),
            ("attachment_page", json!({"url": "not a page"})),
            (
                "core_decide",
                json!({"setup": "{}", "system": "", "prompt": ""}),
            ),
        ] {
            let e = call(&m, cmd, args).unwrap_err();
            assert!(!e.contains("does not know the command"), "{cmd}: {e}");
        }
    }

    // Migration M0 (docs/MIGRATION-VIEWER.md): every command the app answers is answered by the master too, or it
    // belongs to the interface. M5a/M6 removed the second core: the app answers only INTERFACE (and the links to
    // masters, which a master answers as well), and the window keeps every INTERFACE command on screen.

    /// Commands that stay with the interface (the visual container).
    const INTERFACE: &[&str] = &[
        // Popup notifications, the app itself, this screen's files and data folder.
        "notify_settings",
        "notify_take_start",
        "notify_test",
        "notify_notice",
        "notify_item",
        // The window's part of a browser sign-in: this computer's browser and its loopback answer (M2).
        "account_wait",
        "account_cancel",
        "app_info",
        "app_restart",
        "startup_path",
        "data_location_move",
        "data_location_cancel",
        "data_location_remove_old",
        "data_test_build",
        "attachment_file",
        // The viewer side of the master's port links (M1).
        "port_forward",
        "port_unforward",
        // Coding tools: the interface's, run on the shown machine through the master's pipes (M1, M5).
        "lsp_statuses",
        "lsp_install",
        "lsp_start",
        "lsp_send",
        "lsp_stop",
        "lsp_stop_window",
        "dap_statuses",
        "dap_php",
        "dap_install",
        "dap_start",
        "dap_connect",
        "dap_send",
        "dap_stop",
        "dap_stop_window",
        "inspector_show",
        "inspector_scroll",
        "inspector_pick",
        "inspector_page_text",
        "inspector_close",
        "browser_agent_shot",
        // The app's own lifecycle (M4c): it stops This PC's master before an update installs.
        "this_pc_master_stop",
    ];

    /// The command names of the app's `generate_handler!` list.
    fn window_commands() -> Vec<String> {
        let src = include_str!("desktop.rs");
        let list = &src[src.find("generate_handler![").expect("the handler list")..];
        let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
        list.lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .flat_map(|l| l.split(','))
            .map(|c| c.trim().rsplit("::").next().unwrap_or("").to_string())
            .filter(|c| !c.is_empty())
            .collect()
    }

    /// The commands one master match answers: the string patterns of its top-level arms, from `start` to its
    /// "does not know the command" fallback. Top level = the first arm's indent, so nested matches never count.
    fn arms(src: &str, start: &str, into: &mut HashSet<String>) {
        let from = src
            .find(start)
            .unwrap_or_else(|| panic!("{start} not found"));
        let to = from + src[from..].find("does not know the command").unwrap();
        let mut indent = None;
        for line in src[from..to].lines() {
            let t = line.trim_start();
            let pattern = t.strip_prefix("| ").unwrap_or(t);
            if !pattern.starts_with('"') {
                continue;
            }
            let ind = line.len() - t.len();
            if *indent.get_or_insert(ind) != ind {
                continue;
            }
            let pattern = pattern.split("=>").next().unwrap_or("");
            for name in pattern.split('|') {
                let name = name.trim().trim_matches('"');
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    into.insert(name.to_string());
                }
            }
        }
    }

    fn master_commands() -> HashSet<String> {
        let mut all = HashSet::new();
        arms(include_str!("master.rs"), "pub fn dispatch(", &mut all);
        arms(include_str!("master_inbox.rs"), "fn inbox(", &mut all);
        arms(include_str!("master_runs.rs"), "fn plan_run(", &mut all);
        arms(include_str!("master_servers.rs"), "fn servers(", &mut all);
        all
    }

    #[test]
    fn answers_every_window_command_or_it_is_the_interfaces() {
        let window = window_commands();
        let master = master_commands();
        assert!(
            window.len() > 30,
            "the handler list was not read ({})",
            window.len()
        );
        assert!(
            master.contains("hello")
                && master.contains("inbox_list")
                && master.contains("plan_run_start")
        );
        let interface: HashSet<&str> = INTERFACE.iter().copied().collect();
        assert_eq!(
            interface.len(),
            INTERFACE.len(),
            "an interface command is listed twice"
        );
        // The links to masters: the app's and a master's own (M3b), so both answer them.
        let links = |c: &str| c.starts_with("remote_");
        let missing: Vec<&String> = window
            .iter()
            .filter(|c| !master.contains(*c) && !interface.contains(c.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "the app answers these but the master does not: {missing:?}. Answer them in the master, or add them to \
             INTERFACE (the screen's own)."
        );
        // M6: no second core. Every command the app answers is the screen's own or a link.
        let core: Vec<&String> = window
            .iter()
            .filter(|c| !interface.contains(c.as_str()) && !links(c))
            .collect();
        assert!(
            core.is_empty(),
            "the app answers these, which are the master's: {core:?}. Remove them from the app."
        );
        let both: Vec<&&str> = INTERFACE.iter().filter(|c| master.contains(**c)).collect();
        assert!(
            both.is_empty(),
            "the master answers these, so they are not the interface's alone: {both:?}"
        );
        let stale: Vec<&str> = INTERFACE
            .iter()
            .copied()
            .filter(|c| !window.iter().any(|w| w == c))
            .collect();
        assert!(
            stale.is_empty(),
            "the app no longer has these, remove them from the list: {stale:?}"
        );
    }

    /// M5a: the window sends a call to This PC's master unless remote.ts keeps it on screen (WINDOW_ONLY). Every
    /// command only the app answers must be kept there, and nothing the app no longer answers.
    #[test]
    fn the_window_keeps_the_interface_commands_on_screen() {
        let src = include_str!("../../src/lib/remote.ts");
        let from = src
            .find("const WINDOW_ONLY")
            .expect("WINDOW_ONLY in remote.ts");
        let list = &src[from..from + src[from..].find("]);").expect("its end")];
        let kept: HashSet<String> = list
            .lines()
            .map(|l| l.split("//").next().unwrap_or("").trim())
            .filter_map(|l| l.strip_prefix('"').and_then(|l| l.split('"').next()))
            .map(str::to_string)
            .collect();
        let window: HashSet<String> = window_commands().into_iter().collect();
        let not_kept: Vec<&&str> = INTERFACE.iter().filter(|c| !kept.contains(**c)).collect();
        assert!(
            not_kept.is_empty(),
            "the window would send these to This PC's master, which does not answer them: {not_kept:?}"
        );
        let gone: Vec<&String> = kept.iter().filter(|c| !window.contains(*c)).collect();
        assert!(
            gone.is_empty(),
            "remote.ts keeps these on screen, but the app no longer answers them: {gone:?}"
        );
    }
}
