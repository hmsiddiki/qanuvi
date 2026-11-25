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
#[path = "master_cli_inbox.rs"]
mod cli_inbox;
#[path = "master_cli_plan.rs"]
mod cli_plan;
#[path = "master_cli_status.rs"]
mod cli_status;
#[path = "master_inbox.rs"]
mod inbox_cmds;
#[path = "master_quit.rs"]
mod quit_question;
pub use quit_question::QUESTION_FLAG;
#[path = "master_runs.rs"]
mod plan_runs;
#[path = "master_servers.rs"]
mod server_cmds;
#[path = "master_watch.rs"]
pub mod watch;
pub use cli::{pair_cli, servers_cli};
pub use cli_inbox::inbox_cli;
pub use cli_plan::plan_cli;
pub use cli_status::{projects_cli, status_cli, update_cli, update_notice};
pub use watch::Watch;

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
    /// Windows following this master's project list: told when a project is added, removed or changed by anyone
    /// (its own terminal, another window), so an open window's list follows.
    projects_watchers: inbox_cmds::InboxWatchers,
    /// The desktop app's links to This PC's master (migration M4c): the tray's Open and Quit reach the app on them.
    app_links: inbox_cmds::InboxWatchers,
    /// The tray's Quit question while it is up (t1): asked in Qanuvi's own dialog, answered on `app_quit_answer`.
    quit_question: quit_question::QuitQuestion,
    remote_errors: Mutex<std::collections::HashMap<String, String>>,
    /// Servers disconnected on purpose (finding z2): not connected again every minute until a connect asks for them
    /// (their "connect by itself" setting stays as it is; the next start of the master tries them again).
    held_off: Mutex<std::collections::HashSet<String>>,
    /// Servers whose connection ended by itself and are being tried again quickly (nb3), one series each.
    retrying: Mutex<std::collections::HashSet<String>>,
    /// Remote access through a relay (Milestone 20).
    pub relay: Arc<crate::relay_client::RelayMaster>,
    /// This master itself, for the relay link (set once it is shared).
    me: std::sync::OnceLock<std::sync::Weak<Master>>,
    /// M5b: the plan runs the last master left going (their projects), and why it ended; resumed on their own
    /// after a computer restart when Qanuvi starts with Windows.
    cut_off: Mutex<(String, Vec<String>)>,
    /// M8a: what its own look on the update channel found.
    updates: Arc<crate::master_update::Updates>,
    /// LM1: the Preview's live pictures of agents' test browsers being watched.
    lives: Arc<crate::live::Lives>,
}

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn send_line(out: &Out, v: &Value) -> bool {
    // sl1: the line is made first and written once. Written straight into the connection, JSON goes out piece by
    // piece (each escaped newline or quote its own write): a 4,000-line file took ~190 ms on every road.
    let mut line = v.to_string();
    line.push('\n');
    let mut w = out.lock().unwrap_or_else(|e| e.into_inner());
    w.write_all(line.as_bytes()).and_then(|_| w.flush()).is_ok()
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
        // M5b: a master that left no note ended by a shutdown (the computer started again after its last state
        // note) or some other way; one that ended cleanly removed its state note.
        let state_at = std::fs::read_to_string(watch::state_file(dir))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v["at"].as_u64());
        let _ = std::fs::remove_file(watch::state_file(dir));
        let why = why
            .map(|w| w.trim().to_string())
            .filter(|w| !w.is_empty())
            .or_else(|| watch::unclean_why(state_at, watch::boot_time()).map(String::from));
        let cut_off = runs::mark_interrupted_because(&db, why.as_deref());
        // The watcher's crash report goes into the Inbox (the window shows it, and tells it).
        if let Ok(t) = std::fs::read_to_string(watch::report_file(dir)) {
            let _ = std::fs::remove_file(watch::report_file(dir));
            if let Ok(r) = serde_json::from_str::<Value>(&t) {
                let _ = crate::inbox::add(
                    &db,
                    &crate::inbox::NewItem {
                        kind_of: "problem".into(),
                        kind: "master-crash".into(),
                        title: r["title"]
                            .as_str()
                            .unwrap_or("Qanuvi's background part crashed")
                            .into(),
                        text: r["text"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(6000)
                            .collect(),
                        key: None,
                        open: None,
                        form: None,
                        source: None,
                    },
                );
            }
        }
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
            projects_watchers: inbox_cmds::InboxWatchers::default(),
            app_links: inbox_cmds::InboxWatchers::default(),
            quit_question: Default::default(),
            remote_errors: Mutex::default(),
            held_off: Mutex::default(),
            retrying: Mutex::default(),
            relay: {
                // 24rl R3: what is learned about each relay's own port is kept with this master's data.
                crate::relay_client::keep_roads_in(dir);
                crate::relay_client::RelayMaster::open(dir)
            },
            me: std::sync::OnceLock::new(),
            cut_off: Mutex::new((why.unwrap_or_default(), cut_off)),
            updates: Arc::default(),
            lives: Arc::default(),
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
            // t1: the tray's Quit question, read by the app's question window and answered by the app's dialog.
            "app_quit_question" => Ok(self.quit_question_now()),
            "app_quit_answer" => Ok(self.quit_question_answer(args)),
            // "Start Qanuvi when Windows starts" (M4c, renamed in M5b): only This PC's master starts at sign-in.
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
            // 24rl F1/F2: the free relay list (signed, from the update channel), each one tested from this machine.
            "free_relays" => to(Ok(crate::free_relays::list_tested(&paths::data_dir()))),
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
            // The list, and a note on the stream whenever it changes (a project added from this master's terminal
            // reaches an open window).
            "projects_watch" => {
                if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
                    self.projects_watchers.add(stream(&name));
                }
                to(projects::list(db))
            }
            "projects_add" => {
                let added = projects::add(db, &s(args, "path"));
                if added.is_ok() {
                    self.projects_watchers.send(&json!({"change": "add"}));
                }
                to(added)
            }
            // The window's folder picker for this machine (there is no system picker for a server's folders).
            "dirs_list" => to(projects::list_folders(&s(args, "path"))),
            "projects_remove" => {
                let id = s(args, "id");
                self.terminals.kill_project(&id);
                self.agents.stop_project(&id);
                let _ = crate::drafts::drop_project(db, &id);
                let gone = projects::remove(db, &id);
                if gone.is_ok() {
                    self.projects_watchers.send(&json!({"change": "remove"}));
                }
                to(gone)
            }
            "projects_update" => {
                let settings: projects::ProjectSettings =
                    serde_json::from_value(args["settings"].clone()).map_err(|e| e.to_string())?;
                let updated = projects::update(db, &s(args, "id"), &s(args, "name"), &settings);
                if updated.is_ok() {
                    self.projects_watchers.send(&json!({"change": "update"}));
                }
                to(updated)
            }
            "projects_touch" => to(projects::touch(db, &s(args, "id"))),
            "projects_redetect" => {
                let found = projects::redetect(db, &s(args, "id"));
                if found.is_ok() {
                    self.projects_watchers.send(&json!({"change": "update"}));
                }
                to(found)
            }
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
            "secrets_copy" => {
                let id = s(args, "projectId");
                let from = s(args, "fromProjectId");
                projects::get(db, &id)?;
                projects::get(db, &from)?;
                crate::secrets::copy(&id, &s(args, "name"), &from, &s(args, "fromName"))?;
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
            // Terminals: kept for TERM_GRACE when the connection that opened them ends (c2), then closed.
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
                self.terminals.hold(&id, holder_of(opened_terminals));
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
                // c2: it is this connection's now, kept again for a while if this one ends too.
                opened_terminals
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(s(args, "id"));
                self.terminals
                    .hold(&s(args, "id"), holder_of(opened_terminals));
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
            // ba1: on the app's own data these are This PC's backups, in the app's folder with its window storage,
            // where the daily job puts them (backup_place); the master's own folder showed other backups.
            "backups_info" => {
                let d = &self.dir;
                let (_, default) = backup_place(d, self.app_data);
                to(Ok(crate::backups::info(d, d, Some(&default))))
            }
            "backups_set" => to(crate::backups::set(
                &self.dir,
                &s(args, "folder"),
                args["off"].as_bool().unwrap_or(false),
            )),
            "backups_now" => {
                let (web, default) = backup_place(&self.dir, self.app_data);
                to(crate::backups::now_backup(
                    &self.dir,
                    web.as_deref(),
                    Some(&default),
                ))
            }
            "backups_restore" => {
                let d = &self.dir;
                let (_, default) = backup_place(d, self.app_data);
                to(crate::backups::book_restore(
                    d,
                    d,
                    Some(&default),
                    Path::new(&s(args, "path")),
                ))
            }
            "backups_cancel_restore" => to(crate::backups::cancel_restore(&self.dir)),
            "backups_dismiss_note" => to(crate::backups::dismiss_note(&self.dir)),
            "backups_remove" => to(crate::backups::remove_one(
                &self.dir,
                Some(&backup_place(&self.dir, self.app_data).1),
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
            // M8a: what this master's own look on the update channel found, and a look now.
            "update_status" => to(Ok(self.updates.state())),
            "update_check" => to(Ok(self.updates.check(self.app_data))),
            // M8b: put the update in (download, signature, start test, wait for work, swap, restart from it). M8c: the
            // window's "Update to x" holds once it is checked ("Restart to update" asks again without hold).
            "update_install" => to(self.update_install(args["hold"].as_bool().unwrap_or(false))),
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
            // c1: Claude Code on this machine, installed and signed in from the window.
            "setup_claude" => to(Ok(crate::local_servers::claude_status())),
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
            "qanuvi_usage" => to(Ok(crate::usage::usage(watch::watched()))),
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
            // PM a: the owner's answer about the project, kept in This project at once (one entry per title).
            "memory_keep" => to(crate::memory::keep_owner_fact(
                db,
                &s(args, "project"),
                &s(args, "title"),
                &s(args, "body"),
            )),
            // PM c: what the memory knows that applies to a project, one line per entry (for the check).
            "memory_lines" => to(Ok(crate::memory::project_lines(
                db,
                &s(args, "project"),
                3000,
            ))),
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
                let removed = agent::clean_up_chats(
                    db,
                    args["days"].as_i64().unwrap_or(0),
                    &keep,
                    crate::store::now(),
                );
                // Images only the removed chats showed go with them (24n).
                let _ = agent::remove_unused_images(
                    db,
                    &self.dir.join("attachments"),
                    agent::UNUSED_IMAGE_AGE,
                );
                to(removed)
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
                    None,
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
                let done = agent::delete(db, &s(args, "sessionId"));
                // Its images go too when no other message shows them (24n).
                let _ = agent::remove_unused_images(
                    db,
                    &self.dir.join("attachments"),
                    agent::UNUSED_IMAGE_AGE,
                );
                to(done)
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
            "browser_agent_shot" => to(crate::preview::agent_shot(&agent::browser_state_file(
                &s(args, "sessionId"),
            )?)),
            // LM1: a live picture of the session's test browser, for the Preview while it is watched.
            "browser_live_start" => {
                let sink = stream(&s(args, "onEvent"));
                to(self
                    .lives
                    .start(&agent::browser_state_file(&s(args, "sessionId"))?, sink))
            }
            "browser_live_ack" => to(self.lives.ack(&s(args, "id"))),
            "browser_live_stop" => {
                self.lives.stop(&s(args, "id"));
                Ok(Value::Null)
            }
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
                let out = self.servers(c, args, stream)?;
                if c == "remote_call" {
                    self.note_forwarded(args, &out, opened_terminals);
                }
                Ok(out)
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
        if let Some((key, id)) = server_cmds::forwarded(id) {
            master.close_forwarded(key, id);
        } else if crate::pipes::is_pipe(id) {
            master.pipes.stop(id);
        } else if let Some(turn) = master.terminals.orphan(id, holder_of(&terminals)) {
            // ST1 flow 25 c2: the shell goes on for a while; a window that comes back takes it over (term_attach).
            let (me, id, grace) = (
                Arc::downgrade(&master),
                id.clone(),
                master.terminals.grace(),
            );
            std::thread::spawn(move || {
                std::thread::sleep(grace);
                if let Some(m) = me.upgrade() {
                    m.terminals.end_orphan(&id, turn);
                }
            });
        }
    }
}

/// A connection's identity while it lasts (c2): the address of its own set of opened terminals.
fn holder_of(opened: &Mutex<HashSet<String>>) -> usize {
    opened as *const _ as usize
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

/// The lock a running master holds on its data folder (s1).
fn lock_file(dir: &Path) -> PathBuf {
    dir.join("master.lock")
}

/// Takes the data folder's master lock; `None` when another master holds it.
fn take_lock(dir: &Path) -> Result<Option<std::fs::File>, String> {
    watch::take_lock_at(&lock_file(dir))
}

/// A watcher or a core holds this folder (M5b; it may still be starting and not yet have written its address).
fn held(dir: &Path) -> bool {
    watch::held(dir)
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
        // M5b: started once without its start-up list after crashes at start (its watcher's safe fix).
        if watch::skip_startup() {
            eprintln!("Started without the start-up list (after crashes at start).");
        } else {
            self.startup
                .start_all(&crate::startup::items_from_settings(&settings));
        }
        // M5b: Qanuvi cleans up after itself: test browser profiles left by a process that ended without its own
        // cleanup, and (This PC) the updater's folders of updates already installed.
        std::thread::spawn({
            let app_data = self.app_data;
            move || {
                crate::browser::sweep_profiles(false);
                if app_data {
                    crate::browser::sweep_updater_folders();
                }
            }
        });
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
        // M8a: its own look for a newer Qanuvi, two minutes after start, then every hour, with no window needed.
        if crate::master_update::checks_by_itself() {
            let me = Arc::downgrade(self);
            std::thread::spawn(move || {
                std::thread::sleep(crate::master_update::FIRST_CHECK);
                loop {
                    let Some(m) = me.upgrade() else { return };
                    let state = m.updates.check(m.app_data);
                    // M8c: "Download updates by themselves" on this master: it puts the update in itself, still
                    // waiting for running work. M8d: This PC's whole install closes Qanuvi's windows, so while one is
                    // open its update is only downloaded and held ("Restart to update"); it goes in at a look with
                    // no window open.
                    if crate::master_update::auto_wanted(
                        &crate::settings::get_all(&m.db).unwrap_or_default(),
                        &state,
                    ) {
                        let hold = m.app_data && m.app_links.reach(&json!({"do": "wait"})) > 0;
                        if let Err(e) = m.update_install(hold) {
                            eprintln!("The update did not start by itself: {e}");
                        }
                    }
                    drop(m);
                    std::thread::sleep(crate::master_update::CHECK_EVERY);
                }
            });
        }
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
        // app's own data too (M5c part 2): This PC's master holds This PC's server connections, window or not.
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
        self.end_with(why, 0)
    }

    /// Ends this master with an exit code its watcher reads (M8d: [`watch::INSTALL_CODE`]).
    fn end_with(&self, why: &str, code: i32) -> ! {
        if runs::stop_note(why).is_some() {
            let _ = std::fs::write(stop_note_file(&self.dir), why);
        }
        self.agents.stop_all();
        self.terminals.kill_all();
        self.startup.stop_all();
        self.remotes.disconnect_all();
        let _ = std::fs::remove_file(service_file(&self.dir));
        // A clean end: no state note left to read as a crash or a shutdown (M5b), no browser profile left behind.
        let _ = std::fs::remove_file(watch::state_file(&self.dir));
        crate::browser::sweep_profiles(true);
        std::process::exit(code);
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
        start_app(item);
    }

    /// M4c, the tray's "Quit Qanuvi": the app's windows close first, as when closed by hand, then this master ends.
    /// A window still open after 20 seconds (its closing was cancelled) keeps everything running.
    pub fn quit_all(&self) {
        // M5a: work going on here stops with it; said first, and nothing stops without a yes.
        let (agents, runs) = self.work();
        // s2: one thing is "it", more are "they".
        let they = if agents.max(0) as usize + runs == 1 {
            "It"
        } else {
            "They"
        };
        // s10 (owner, 2026-11-25): with plan runs going, "Go on next time" quits too, and the runs start again by
        // themselves at the next start (resume_cut_off).
        let mut why = "quit";
        if let Some(what) = work_text((agents, runs)) {
            let text = if runs > 0 {
                format!(
                    "Quitting Qanuvi stops {what} on this computer.\n\nGo on next time: the plan {} by itself when \
                     Qanuvi starts again. Quit: {} for you to choose Resume.",
                    if runs == 1 {
                        "run goes on"
                    } else {
                        "runs go on"
                    },
                    if runs == 1 { "it waits" } else { "they wait" },
                )
            } else {
                format!(
                    "Quitting Qanuvi stops {what} on this computer. {they} can be resumed afterwards.\n\nQuit anyway?"
                )
            };
            match self.ask_quit(&text, runs > 0) {
                quit_question::QuitChoice::Keep => return,
                quit_question::QuitChoice::Quit => {}
                quit_question::QuitChoice::Later => why = "quit-later",
            }
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
        self.end_because(why)
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
        self.restart_from(dir, None)
    }

    /// M8b: puts the update the last look found in. M8d: This PC's master installs the whole app instead (the app's
    /// signed installer, kept in its data folder), and ends for it once nothing works.
    fn update_install(&self, hold: bool) -> Result<crate::master_update::UpdateState, String> {
        let exe = crate::data_location::current_exe().map_err(|e| e.to_string())?;
        let target = if self.app_data {
            crate::master_update::Target::Installer {
                folder: self.dir.join("updates"),
                program: exe,
            }
        } else {
            crate::master_update::Target::Program(exe)
        };
        let me = self.me.get().cloned().ok_or("The master is not ready.")?;
        const KEY: &str = "master-update-waits";
        let (busy_me, wait_me, waited_me, dir) =
            (me.clone(), me.clone(), me.clone(), self.dir.clone());
        let hooks = crate::master_update::InstallHooks {
            busy: Box::new(move || {
                let m = busy_me.upgrade()?;
                work_text((crate::agent::working_count(&m.db), m.runs.active().len()))
            }),
            waits: Box::new(move |version, what| {
                let Some(m) = wait_me.upgrade() else { return };
                let item = crate::inbox::NewItem {
                    kind_of: "info".into(),
                    kind: "update".into(),
                    title: format!("Update to {version} waits for running work"),
                    text: format!(
                        "{} working. The update goes in as soon as it ends; nothing is cut off.",
                        crate::master_update::is_or_are(what)
                    ),
                    key: Some(KEY.into()),
                    open: None,
                    form: None,
                    source: None,
                };
                if let Ok((added, new)) = crate::inbox::add_new(&m.db, &item) {
                    (m.inbox_teller())(&json!({"change": "add", "item": added, "new": new}));
                }
            }),
            waited: Box::new(move || {
                let Some(m) = waited_me.upgrade() else { return };
                if crate::inbox::resolve(&m.db, KEY).unwrap_or(false) {
                    (m.inbox_teller())(&json!({"change": "remove", "item": null}));
                }
            }),
            restart: Box::new(move |path| match me.upgrade() {
                Some(m) if m.app_data => m.install_whole_app(path),
                Some(m) => m.restart_from(&dir, Some(path)),
                None => Err("The master ended before the update went in.".into()),
            }),
        };
        self.updates.install(target, hooks, hold)
    }

    /// M8d: puts This PC's whole install in from the checked `installer`. The app's windows close first, as for the
    /// tray's Quit (their unsaved text is kept as drafts); then a helper waits for the app and this master to end and
    /// runs the installer quietly, which replaces both and starts Qanuvi again (in the background when no window was
    /// open). A window that does not close stops the update; nothing changes then.
    ///
    /// L2: on Linux the helper puts the app's tarball into the user's home folder instead ([`linux_install_script`]).
    fn install_whole_app(&self, installer: &Path) -> Result<(), String> {
        if !cfg!(any(windows, target_os = "linux")) {
            return Err(
                "This PC's whole install updates by itself on Windows and Linux only so far."
                    .into(),
            );
        }
        let had_window = self.app_links.reach(&json!({"do": "wait"})) > 0;
        if had_window && self.app_links.reach(&json!({"do": "quit"})) > 0 {
            let deadline = Instant::now() + Duration::from_secs(20);
            while self.app_links.reach(&json!({"do": "wait"})) > 0 {
                if Instant::now() >= deadline {
                    return Err("A Qanuvi window did not close, so the update did not go in. Close it, then choose Restart to update again.".into());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        let master = crate::data_location::current_exe().map_err(|e| e.to_string())?;
        let app = crate::sign_in_start::app_exe()?;
        let script = if cfg!(windows) {
            installer_script(&[&master, &app], installer, !had_window)
        } else {
            let (home, own) = linux_install_home()?;
            linux_install_script(&master, &app, installer, !had_window, &home, own)
        };
        // A watched core's children end with its watcher (its job object), so the watcher starts the helper.
        if watch::watched() {
            std::fs::write(watch::install_file(&self.dir), &script).map_err(|e| e.to_string())?;
            self.end_with("update", watch::INSTALL_CODE)
        }
        run_helper(&script)?;
        self.end_because("update")
    }

    /// A restart, or (M8b, `updated`: the program now in place) the end of an update: the service is started again
    /// from that program, as a watched core's watcher would otherwise start the old one it runs from.
    fn restart_from(&self, dir: &Path, updated: Option<&Path>) -> Result<(), String> {
        let me = self
            .me
            .get()
            .and_then(|w| w.upgrade())
            .ok_or("The master is not ready.")?;
        std::fs::write(restart_mark(dir), b"").map_err(|e| e.to_string())?;
        let _ = std::fs::write(
            stop_note_file(dir),
            if updated.is_some() {
                "update"
            } else {
                "restart"
            },
        );
        // M5b: a core its watcher started asks the watcher to start it again (its exit code); else a new service is
        // started that waits for this one to end.
        let watched = watch::watched();
        let started = match updated {
            Some(exe) if !watched => spawn_service(exe, dir, Some(std::process::id())),
            _ if !watched => start_service(dir, Some(std::process::id())),
            _ => Ok(()),
        };
        if let Err(e) = started {
            let _ = std::fs::remove_file(restart_mark(dir));
            return Err(e);
        }
        let dir = dir.to_path_buf();
        let updated = updated.is_some();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            me.agents.stop_all();
            me.terminals.kill_all();
            me.startup.stop_all();
            me.remotes.disconnect_all();
            let _ = std::fs::remove_file(service_file(&dir));
            let _ = std::fs::remove_file(watch::state_file(&dir));
            std::process::exit(match (watched, updated) {
                (true, true) => watch::UPDATED_CODE,
                (true, false) => watch::RESTART_CODE,
                _ => 0,
            });
        });
        Ok(())
    }
}

/// Starts the app beside this master (for a notification's item, its Inbox opens there).
fn start_app(item: Option<String>) {
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

/// M8d: the PowerShell line that waits (a minute at most) for every process running one of `programs` to end, then
/// starts `installer` quietly as an update that starts Qanuvi again (`background`: without a window).
pub fn installer_script(programs: &[&Path], installer: &Path, background: bool) -> String {
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "''"));
    let list: Vec<String> = programs.iter().map(|p| q(p)).collect();
    let mut args = vec!["'/S'", "'/UPDATE'", "'/R'"];
    if background {
        args.extend(["'/ARGS'", "'--background'"]);
    }
    format!(
        "$p = @({}); Get-Process | Where-Object {{ $p -contains $_.Path }} | Wait-Process -Timeout 60 -ErrorAction SilentlyContinue; Start-Process -FilePath {} -ArgumentList @({})",
        list.join(", "),
        q(installer),
        args.join(", ")
    )
}

/// L2: where Linux puts the app: the user's home folder (`own`: the user's real one, where a system-wide copy from the
/// .deb is moved out of the way), or `QANUVI_INSTALL_HOME` for a test.
fn linux_install_home() -> Result<(PathBuf, bool), String> {
    match std::env::var_os("QANUVI_INSTALL_HOME").filter(|h| !h.is_empty()) {
        Some(h) => Ok((PathBuf::from(h), false)),
        None => std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| (PathBuf::from(h), true))
            .ok_or_else(|| "The home folder is not known, so the update cannot go in.".into()),
    }
}

/// The Linux helper (L2), as `sh -c` runs it; the @NAMES@ are filled in by [`linux_install_script`].
const LINUX_INSTALL: &str = r#"n=0
while [ $n -lt 120 ]; do
  busy=
  for e in /proc/[0-9]*/exe; do
    case "$(readlink "$e" 2>/dev/null)" in @PROGRAMS@) busy=1; break ;; esac
  done
  [ -z "$busy" ] && break
  n=$((n + 1)); sleep 0.5
done
lib=@HOME@/.local/lib/qanuvi
rm -rf "$lib.new"
if ! { mkdir -p "$lib.new" && tar -xzf @TARBALL@ -C "$lib.new" && [ -x "$lib.new/qanuvi" ] && [ -x "$lib.new/qanuvi-master" ]; }; then
  rm -rf "$lib.new"
  @OLD_APP@ @FLAG@ </dev/null >/dev/null 2>&1 &
  exit 1
fi
rm -rf "$lib.prev"
[ -d "$lib" ] && mv "$lib" "$lib.prev"
mv "$lib.new" "$lib"
apps=@HOME@/.local/share/applications
icons=@HOME@/.local/share/icons/hicolor/128x128/apps
mkdir -p @HOME@/.local/bin "$apps" "$icons"
ln -sfn "$lib/qanuvi" @HOME@/.local/bin/qanuvi
[ -f "$lib/qanuvi.png" ] && cp -f "$lib/qanuvi.png" "$icons/qanuvi.png"
printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Qanuvi' 'Comment=Qanuvi' "Exec=\"$lib/qanuvi\" %u" 'Icon=qanuvi' 'Terminal=false' 'Categories=Development;' 'MimeType=x-scheme-handler/qanuvi;' 'StartupWMClass=qanuvi' > "$apps/Qanuvi.desktop"
update-desktop-database "$apps" >/dev/null 2>&1
xdg-mime default Qanuvi.desktop x-scheme-handler/qanuvi >/dev/null 2>&1
"$lib/qanuvi" @FLAG@ </dev/null >/dev/null 2>&1 &
asked=@HOME@/.local/lib/qanuvi-system-copy-asked
if @OWN@ && [ ! -e "$asked" ] && dpkg-query -W -f='${Status}' qanuvi 2>/dev/null | grep -q 'ok installed'; then
  touch "$asked"
  sleep 5
  pkexec dpkg -r qanuvi >/dev/null 2>&1
fi
"#;

/// L2: the shell lines that wait (a minute at most) for the app and its master (`master`, `app`: the programs they run
/// from) to end, unpack the checked `tarball` into `home`/.local/lib/qanuvi (the one before kept as `.prev`), link
/// ~/.local/bin/qanuvi, write the app's menu entry and `qanuvi://` link handler, and start Qanuvi again from there
/// (`background`: without a window). A tarball that does not unpack changes nothing and starts the old app again.
/// `own` (the user's real home): the first time, a system-wide copy from the .deb is removed (the system asks for the
/// password once; refused, it stays and the home-folder copy is the one the menu and the sign-in start use).
pub fn linux_install_script(
    master: &Path,
    app: &Path,
    tarball: &Path,
    background: bool,
    home: &Path,
    own: bool,
) -> String {
    let q = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', r"'\''"));
    LINUX_INSTALL
        .replace("@PROGRAMS@", &format!("{}|{}", q(master), q(app)))
        .replace("@HOME@", &q(home))
        .replace("@TARBALL@", &q(tarball))
        .replace("@OLD_APP@", &q(app))
        .replace("@FLAG@", if background { "--background" } else { "" })
        .replace("@OWN@", if own { "true" } else { "false" })
}

/// M8d: starts the helper that runs the installer (its `installer_script` line); it outlives this master.
fn run_helper(script: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        let args: Vec<std::ffi::OsString> = [
            "powershell.exe",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ]
        .iter()
        .map(|a| a.into())
        .collect();
        // M8d drive: PowerShell started with DETACHED_PROCESS ends at once without running its command; with a hidden
        // console of its own (CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP) it runs, and outlives this master.
        spawn_alone_with(&args, 0x0800_0000 | 0x0000_0200)
            .map_err(|e| e.replace("The master service", "The update's installer"))
    }
    #[cfg(not(windows))]
    {
        // L2: `sh` in a process group of its own, so it outlives this master and the app it waits for.
        use std::os::unix::process::CommandExt;
        std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("The update's helper could not start: {e}"))
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

/// Asks before the tray's Quit stops work (M5a). Yes quits; anything else keeps it all running. Since t1 only when
/// Qanuvi's own question window cannot start (the app's program is missing).
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

/// No box to ask with here when Qanuvi's own question window cannot start: the tray's Quit goes on.
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

/// h3: the data folder of a `qanuvi-master` started without --data. On a computer with the desktop app it is the app's
/// own folder (where the user moved it, if so), so "qanuvi-master status" there answers about This PC instead of
/// starting a second, empty master (which also held the next update back). A server without the app, or a computer
/// whose own master folder is in use already, keeps the master default.
pub fn default_data_dir() -> PathBuf {
    let d = crate::data_location::Defaults::real();
    pick_default_dir(&d.base, &crate::paths::default_master_dir())
}

fn pick_default_dir(app_base: &Path, master_default: &Path) -> PathBuf {
    let app = crate::data_location::read(app_base)
        .dir
        .unwrap_or_else(|| app_base.to_path_buf());
    if on_app_data(&app) && !master_default.join("qanuvi.db").is_file() {
        app
    } else {
        master_default.to_path_buf()
    }
}

/// What shows This PC's master's tray icon and answers it until the master ends (migration M4c). Only the desktop
/// build's `qanuvi-master` program has one (it links the tray; the library and its tests do not). M5b: the tray
/// belongs to the watcher, so it stays while a crashed core starts again.
pub type Tray = fn(Arc<Watch>) -> !;

/// `qanuvi-master serve` (M5b): the watcher of this data folder's core, until the core ends cleanly. `tray`: shown
/// when this master is This PC's. One watcher per data folder; a second one leaves it to the first.
pub fn serve(dir: &Path, tray: Option<Tray>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let Some(_lock) = watch::take_lock_at(&watch::watcher_lock(dir))? else {
        return Ok(());
    };
    let w = Arc::new(Watch::new(dir));
    // The icon needs the main thread, so the watching goes on in another one; its end ends the program.
    if let Some(tray) = tray.filter(|_| w.app_data()) {
        let w2 = w.clone();
        std::thread::spawn(move || {
            if let Err(e) = w2.run() {
                eprintln!("{e}");
            }
            std::process::exit(0);
        });
        tray(w);
    }
    w.run()
}

/// What this core does now, noted for its watcher (read after a crash) and for the next core (its age tells a
/// shutdown from a crash). A clean end removes it.
fn note_state(m: &Master) {
    let settings = crate::settings::get_all(&m.db).unwrap_or_default();
    let startup = if watch::skip_startup() {
        0
    } else {
        crate::startup::items_from_settings(&settings).len()
    };
    let (agents, runs) = m.work();
    let v = json!({
        "pid": std::process::id(),
        "at": watch::now_secs(),
        "agents": agents,
        "runs": runs,
        "startup": startup,
    });
    let _ = std::fs::write(watch::state_file(&m.dir), v.to_string());
}

/// `qanuvi-master core`: the master that does the work, until it is stopped (started by its watcher, M5b).
pub fn serve_core(dir: &Path) -> Result<(), String> {
    // One master per data folder (s1): the app's launch and its first This PC call could each start one. The lock
    // is held until this process ends; a second master finds it taken and leaves the folder to the first.
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let Some(_lock) = take_lock(dir)? else {
        return Ok(());
    };
    // Its standard error is the watcher's log pipe: children it starts get none of it (as in k1).
    #[cfg(windows)]
    keep_own_stdio_from_children();
    let master = Master::open(dir)?.shared();
    note_state(&master);
    master.start_own();
    let me = Arc::downgrade(&master);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(20));
            let Some(m) = me.upgrade() else { return };
            note_state(&m);
        }
    });
    master.resume_cut_off();
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
            // M5b: the watcher's tray asks the core (the app's links and the work are here).
            match hello["tray"].as_str() {
                Some("open") => return master.open_app(),
                Some("quit") => return master.quit_all(),
                _ => {}
            }
            // A test build's own crash, to drive the watcher live (never in an installed copy).
            if hello["crash"] == true && crate::data_location::from_source() {
                eprintln!("Crashing on purpose (test build).");
                std::process::abort();
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
    let exe = crate::data_location::current_exe().map_err(|e| e.to_string())?;
    spawn_service(&exe, dir, after)
}

/// The master program next to the running one (the desktop app ships `qanuvi-master` beside `qanuvi`).
pub fn master_exe_beside_me() -> Result<PathBuf, String> {
    let exe = crate::data_location::current_exe().map_err(|e| e.to_string())?;
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
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if held(dir) {
        return Ok(());
    }
    let exe = master_exe_beside_me()?;
    let _ = std::fs::remove_file(service_file(dir));
    spawn_service(&exe, dir, None)
}

fn spawn_service(exe: &Path, dir: &Path, after: Option<u32>) -> Result<(), String> {
    // s5: on Windows the service outlives whoever starts it, so it gets none of their handles. Started with every
    // inheritable handle (as Command does), it kept the app's links to This PC's master open after the app was gone
    // (the app -> bridge -> a second master's service), and the tray's Quit waited for a window that no longer was.
    #[cfg(windows)]
    {
        let mut args = vec![
            exe.as_os_str().to_owned(),
            "serve".into(),
            "--data".into(),
            dir.as_os_str().to_owned(),
        ];
        if let Some(pid) = after {
            args.push(format!("--after-pid={pid}").into());
        }
        keep_own_stdio_from_children();
        spawn_alone(&args)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("serve").arg("--data").arg(dir);
        // L1: the window's font setting stays with the window; the master and its terminals get the user's.
        #[cfg(target_os = "linux")]
        crate::linux_fonts::give_back(&mut cmd);
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
        cmd.spawn()
            .map_err(|e| format!("The master service could not start: {e}"))?;
        Ok(())
    }
}

/// One argument of a Windows command line, quoted the way the program's own argument parser reads it back.
#[cfg(windows)]
fn quote_arg(a: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let s: Vec<u16> = a.encode_wide().collect();
    let plain = !s.is_empty()
        && !s
            .iter()
            .any(|&c| c == ' ' as u16 || c == '\t' as u16 || c == '"' as u16);
    if plain {
        return s;
    }
    let mut out = vec!['"' as u16];
    let mut slashes = 0;
    for &c in &s {
        if c == '\\' as u16 {
            slashes += 1;
            continue;
        }
        // Backslashes before a quote are doubled, and the quote escaped.
        let n = if c == '"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        out.extend(std::iter::repeat_n('\\' as u16, n));
        slashes = 0;
        out.push(c);
    }
    // Backslashes before the closing quote are doubled.
    out.extend(std::iter::repeat_n('\\' as u16, slashes * 2));
    out.push('"' as u16);
    out
}

/// Starts a detached program that inherits no handles at all (s5), with no console window.
#[cfg(windows)]
fn spawn_alone(args: &[std::ffi::OsString]) -> Result<(), String> {
    // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
    spawn_alone_with(args, 0x0000_0008 | 0x0000_0200 | 0x0800_0000)
}

/// Starts a program that inherits no handles, with these creation flags.
#[cfg(windows)]
fn spawn_alone_with(args: &[std::ffi::OsString], flags: u32) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    let mut line: Vec<u16> = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            line.push(' ' as u16);
        }
        line.extend(quote_arg(a));
    }
    line.push(0);
    let si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            flags,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        )
    };
    if ok == 0 {
        return Err(format!(
            "The master service could not start: {}",
            std::io::Error::last_os_error()
        ));
    }
    unsafe {
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
    }
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
    // M5b: its watcher stopped at the crash limit and waits for the owner: said at once, nothing started.
    if held(dir)
        && let Some(text) = watch::stopped_text(dir)
    {
        return Err(text);
    }
    // A master that holds the folder is starting: wait for its address instead of starting another (s1).
    if !held(dir) {
        let _ = std::fs::remove_file(service_file(dir));
        start_service(dir, None)?;
    }
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
            return Err(watch::stopped_text(dir)
                .unwrap_or_else(|| "This PC's master is not running.".into()));
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

    /// h3: a plain "qanuvi-master status" on a computer with the app talks to the app's master, not a new empty one.
    #[test]
    fn a_master_started_without_a_folder_uses_the_apps_data_when_the_app_is_here() {
        let t = tempfile::tempdir().unwrap();
        let (app, own) = (t.path().join("dev.qanuvi.desktop"), t.path().join("qanuvi"));
        // A server without the app: its own default.
        assert_eq!(pick_default_dir(&app, &own), own);
        // The app has run here: its folder.
        std::fs::create_dir_all(&app).unwrap();
        mark_app_data(&app);
        assert_eq!(pick_default_dir(&app, &own), app);
        // The app's data moved elsewhere: there.
        let moved = t.path().join("D-drive/qanuvi");
        std::fs::create_dir_all(&moved).unwrap();
        mark_app_data(&moved);
        let loc = crate::data_location::Location {
            dir: Some(moved.clone()),
            ..Default::default()
        };
        crate::data_location::write(&app, &loc).unwrap();
        assert_eq!(pick_default_dir(&app, &own), moved);
        // A master of its own was set up on this computer before: it stays that one.
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("qanuvi.db"), "").unwrap();
        assert_eq!(pick_default_dir(&app, &own), own);
    }

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

    // M8d: the helper waits for the app and This PC's master to end, then runs the installer quietly as an update.
    #[test]
    fn the_installer_helper_waits_for_both_programs_then_runs_the_update_quietly() {
        let master = Path::new(r"C:\Users\O'Neil\AppData\Local\Qanuvi\qanuvi-master.exe");
        let app = Path::new(r"C:\Users\O'Neil\AppData\Local\Qanuvi\qanuvi.exe");
        let setup = Path::new(r"C:\Data\updates\Qanuvi_0.9.0_setup.exe");
        let s = installer_script(&[master, app], setup, false);
        assert!(
            s.starts_with(
                r"$p = @('C:\Users\O''Neil\AppData\Local\Qanuvi\qanuvi-master.exe', 'C:\Users\O''Neil\AppData\Local\Qanuvi\qanuvi.exe');"
            ),
            "{s}"
        );
        assert!(s.contains("Wait-Process -Timeout 60"), "{s}");
        assert!(
            s.ends_with(
                r"Start-Process -FilePath 'C:\Data\updates\Qanuvi_0.9.0_setup.exe' -ArgumentList @('/S', '/UPDATE', '/R')"
            ),
            "{s}"
        );
        // No window was open: Qanuvi comes back in the background.
        assert!(
            installer_script(&[master], setup, true)
                .ends_with("@('/S', '/UPDATE', '/R', '/ARGS', '--background')")
        );
    }

    // L2: the Linux helper waits for both programs, quoted, and puts the app into the home folder.
    #[test]
    fn the_linux_helper_waits_for_both_programs_and_installs_into_the_home_folder() {
        let master = Path::new("/usr/bin/qanuvi-master");
        let app = Path::new("/usr/bin/qanuvi");
        let tar = Path::new("/home/o'neil/.local/share/q/updates/Qanuvi_0.9.0_linux.tar.gz");
        let home = Path::new("/home/o'neil");
        let s = linux_install_script(master, app, tar, false, home, true);
        assert!(
            s.contains("in '/usr/bin/qanuvi-master'|'/usr/bin/qanuvi') busy=1"),
            "{s}"
        );
        assert!(
            s.contains("lib='/home/o'\\''neil'/.local/lib/qanuvi\n"),
            "{s}"
        );
        assert!(
            s.contains("tar -xzf '/home/o'\\''neil/.local/share/"),
            "{s}"
        );
        assert!(s.contains("dpkg-query -W -f='${Status}' qanuvi"), "{s}");
        assert!(!s.contains('@'), "every name is filled in: {s}");
        assert!(s.contains("if true && "), "{s}");
        // No window was open: Qanuvi comes back in the background; a test home never removes the system copy.
        let b = linux_install_script(master, app, tar, true, home, false);
        assert!(b.contains("\"$lib/qanuvi\" --background </dev/null"), "{b}");
        assert!(b.contains("if false && "), "{b}");
    }

    // L2, run for real on Linux: the tarball goes in, the one before is kept, the menu entry and link are written, the
    // new app is started; a broken tarball changes nothing and starts the old app again.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_linux_helper_puts_the_tarball_in_and_starts_the_new_app() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        let started = h.join("started.txt");
        let program = |file: &Path, says: &str| {
            let text = format!("#!/bin/sh\necho \"{says} $*\" >> '{}'\n", started.display());
            std::fs::write(file, text).unwrap();
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let build = h.join("build");
        std::fs::create_dir_all(&build).unwrap();
        program(&build.join("qanuvi"), "new app");
        program(&build.join("qanuvi-master"), "new master");
        std::fs::write(build.join("qanuvi.png"), "png").unwrap();
        let tar = h.join("Qanuvi_0.9.0_linux.tar.gz");
        let packed = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&tar)
            .arg("-C")
            .arg(&build)
            .args(["qanuvi", "qanuvi-master", "qanuvi.png"])
            .status()
            .unwrap();
        assert!(packed.success());
        let lib = h.join(".local/lib/qanuvi");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("qanuvi"), "old").unwrap();
        let old_app = h.join("old-app");
        program(&old_app, "old app");
        let run = || {
            let master = Path::new("/nowhere/qanuvi-master");
            let script = linux_install_script(master, &old_app, &tar, true, h, false);
            std::process::Command::new("sh")
                .arg("-c")
                .arg(&script)
                .status()
                .unwrap();
            // The started app runs in the background.
            std::thread::sleep(Duration::from_millis(500));
        };
        run();
        let prev = h.join(".local/lib/qanuvi.prev/qanuvi");
        assert_eq!(std::fs::read_to_string(prev).unwrap(), "old");
        assert!(lib.join("qanuvi-master").is_file());
        let entry =
            std::fs::read_to_string(h.join(".local/share/applications/Qanuvi.desktop")).unwrap();
        let exec = format!("Exec=\"{}/qanuvi\" %u", lib.display());
        assert!(entry.contains(&exec), "{entry}");
        assert!(
            entry.contains("MimeType=x-scheme-handler/qanuvi;"),
            "{entry}"
        );
        let link = std::fs::read_link(h.join(".local/bin/qanuvi")).unwrap();
        assert_eq!(link, lib.join("qanuvi"));
        assert!(
            h.join(".local/share/icons/hicolor/128x128/apps/qanuvi.png")
                .is_file()
        );
        assert_eq!(
            std::fs::read_to_string(&started).unwrap(),
            "new app --background\n"
        );
        // A broken tarball: the install stays as it is and the old app starts again.
        std::fs::write(&tar, "not a tarball").unwrap();
        std::fs::remove_file(&started).unwrap();
        run();
        assert!(lib.join("qanuvi-master").is_file());
        assert!(!h.join(".local/lib/qanuvi.new").exists());
        assert_eq!(
            std::fs::read_to_string(&started).unwrap(),
            "old app --background\n"
        );
    }

    // s5: the service starts with no inherited handles, from a command line its own parser reads back as given.
    #[cfg(windows)]
    #[test]
    fn the_service_command_line_reads_back_as_given() {
        let q = |s: &str| String::from_utf16(&quote_arg(std::ffi::OsStr::new(s))).unwrap();
        assert_eq!(q("serve"), "serve");
        assert_eq!(
            q(r"C:\Program Files\Qanuvi\qanuvi-master.exe"),
            r#""C:\Program Files\Qanuvi\qanuvi-master.exe""#
        );
        assert_eq!(q(r"C:\My Data\"), r#""C:\My Data\\""#);
        assert_eq!(q(r#"a "b""#), r#""a \"b\"""#);
        assert_eq!(q(""), r#""""#);
    }

    // s1: the app's launch and its first This PC call each started a master on the same folder; only one may run.
    #[test]
    fn a_second_master_on_the_same_folder_leaves_it_to_the_first() {
        let data = tempfile::tempdir().unwrap();
        let first = take_lock(data.path()).unwrap();
        assert!(first.is_some());
        assert!(held(data.path()));
        std::fs::write(service_file(data.path()), "first").unwrap();
        // The second ends at once, without opening the data or writing its own address.
        serve_core(data.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(service_file(data.path())).unwrap(),
            "first"
        );
        assert!(!data.path().join("qanuvi.db").exists());
        // start_this_pc leaves a starting master alone (no second spawn, its address kept).
        start_this_pc(data.path()).unwrap();
        assert!(service_file(data.path()).exists());
        drop(first);
        assert!(!held(data.path()));
    }

    // M5c part 2: a terminal, pipe or port opened on a server through this master is noted with the connection that
    // asked (by the server's path and its id there), so it is closed there when that connection ends; anything else
    // passed on is not noted.
    #[test]
    fn what_a_passed_on_call_opened_is_noted_with_the_asking_connection() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        let opened = Mutex::new(HashSet::new());
        let call = |cmd: &str| json!({"master": "vps", "via": ["box"], "cmd": cmd, "args": {}});
        m.note_forwarded(&call("term_spawn"), &json!("t-1"), &opened);
        m.note_forwarded(&call("pipe_start"), &json!("pipe-2"), &opened);
        m.note_forwarded(&call("projects_list"), &json!("x"), &opened);
        m.note_forwarded(&call("term_spawn"), &json!(null), &opened);
        let mut got: Vec<(String, String)> = opened
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| server_cmds::forwarded(e).map(|(k, i)| (k.to_string(), i.to_string())))
            .collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("vps/box".to_string(), "pipe-2".to_string()),
                ("vps/box".to_string(), "t-1".to_string())
            ]
        );
        assert!(server_cmds::forwarded("t-1").is_none());
        // Closing one whose way is gone, or along a connection still there, is no harm.
        m.close_forwarded("vps/box", "t-1");
        m.remotes
            .add_idle("vps", json!({"machineId": "v", "forwards": true}));
        m.close_forwarded("vps/box", "pipe-2");
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

    // Finding z2: a server disconnected on purpose is not connected again every minute, until a connect asks for it.
    #[test]
    fn a_server_disconnected_on_purpose_stays_off_until_connected_again() {
        let data = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap();
        // A relay server without its pairing key fails at once, so each try leaves its reason.
        let paired = json!({"id": "p1", "name": "", "target": "relay", "autoConnect": true,
            "relay": {"url": "wss://relay.example", "masterId": "m", "pairing": "abc123"}});
        call(&m, "servers_save", json!({"server": paired.clone()})).unwrap();
        let state =
            |m: &Master| call(m, "remote_states", json!({})).unwrap()["p1"]["state"].clone();
        m.connect_away_servers();
        assert_eq!(state(&m), "error");
        // A restart's disconnect (no held): the minute's retry tries it again.
        call(&m, "remote_disconnect", json!({"id": "p1"})).unwrap();
        m.connect_away_servers();
        assert_eq!(state(&m), "error");
        // On purpose: left alone, and its "connect by itself" setting is unchanged.
        call(&m, "remote_disconnect", json!({"id": "p1", "held": true})).unwrap();
        m.connect_away_servers();
        assert_ne!(state(&m), "error");
        let kept = call(&m, "servers_list", json!({})).unwrap();
        assert_eq!(kept["servers"][0]["autoConnect"], true);
        // Connected again on purpose: the hold goes, and the retry tries it again.
        assert!(call(&m, "remote_connect", paired).is_err());
        call(&m, "remote_disconnect", json!({"id": "p1"})).unwrap();
        m.connect_away_servers();
        assert_eq!(state(&m), "error");
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
        for cmd in [
            "secrets_names",
            "secrets_set",
            "secrets_remove",
            "secrets_copy",
        ] {
            let args = json!({"projectId": "not-a-project", "name": "DB_PASSWORD", "value": "x",
                "fromProjectId": "not-a-project", "fromName": "DB_PASSWORD"});
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
        // PM a + c: an owner's answer about a project is kept once per title and read back as lines.
        let mut kept = Value::Null;
        for body in [
            "https://shop.example.test/",
            "https://shop.example.test/new/",
        ] {
            kept = call(
                &m,
                "memory_keep",
                json!({"project": "p9", "title": "Test site", "body": body}),
            )
            .unwrap();
        }
        assert_eq!(
            call(&m, "memory_lines", json!({"project": "p9"})).unwrap(),
            json!([
                "[fact] Test site: https://shop.example.test/new/",
                "[fact] Master memory test fact: kept on the master"
            ]),
            "the project's own first, then the account's"
        );
        call(&m, "memory_delete", json!({"id": kept["id"]})).unwrap();
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
        // M5c part 2: This PC's list keeps the window's own test master and remote access ("local", "self"), and
        // its start-up connect never tries them (the window reaches them itself); its other servers it does.
        let local = json!({"id": "l1", "name": "", "target": "local", "program": "", "identity": "",
            "autoConnect": true});
        call(&m, "servers_save", json!({"server": local})).unwrap();
        let vps = json!({"id": "v1", "name": "", "target": "relay", "autoConnect": true,
            "relay": {"url": "wss://relay.example", "masterId": "m", "pairing": "m5c-part2-missing"}});
        call(&m, "servers_save", json!({"server": vps})).unwrap();
        m.connect_away_servers();
        let states = call(&m, "remote_states", json!({})).unwrap();
        assert_eq!(states["l1"]["state"], "off", "{states}");
        assert_eq!(states["v1"]["state"], "error", "{states}");
        // Its backups are the app's: the app's folder (not the master's own), as the app made them before M4b.
        let (_, folder) = backup_place(data.path(), true);
        assert_eq!(
            folder,
            crate::backups::default_dir(crate::data_location::from_source())
        );
        // ba1: Settings → Backups shows and lists that same folder (it showed the master's own).
        let i = call(&m, "backups_info", json!({})).unwrap();
        assert_eq!(i["defaultDir"], folder.to_string_lossy().as_ref(), "{i}");
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

    #[test]
    fn m4c_this_pcs_master_reaches_the_apps_windows() {
        // M5b: the tray is the watcher's (it starts the core as its own program, so it is driven live, not here); the
        // core on the app's data answers.
        let data = tempfile::tempdir().unwrap();
        mark_app_data(data.path());
        let dir = data.path().to_path_buf();
        std::thread::spawn(move || {
            let _ = serve_core(&dir);
        });
        let hello = this_pc_call(data.path(), "hello", json!({})).unwrap();
        assert_eq!(hello["name"], "Qanuvi master");
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
        m.terminals.set_grace(Duration::from_millis(300));
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
        // c2: the terminal outlives the connection for its grace, then ends.
        let term = one["ok"].as_str().unwrap();
        assert!(m.terminals.write(term, "x").is_ok());
        let deadline = Instant::now() + Duration::from_secs(10);
        while m.terminals.write(term, "x").is_ok() {
            assert!(
                Instant::now() < deadline,
                "the orphaned terminal did not end"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn an_old_connection_noticed_gone_late_leaves_a_taken_over_terminal_alone() {
        // ST1 flow 25 c2 (driven): the master noticed the laptop's old connection gone 80 s after the Wi-Fi drop,
        // when the window had already taken the shell over on a new one; that late end took its output away.
        let project = tempfile::tempdir().unwrap();
        let terms = terminal::Terminals::default();
        let (tx, rx) = std::sync::mpsc::channel::<terminal::TermEvent>();
        let sink = Sink::new(move |e| tx.send(e).is_ok());
        let shell = terminal::shells()[0].id.clone();
        let id = terms
            .spawn("p", project.path(), &shell, 80, 24, sink)
            .unwrap();
        let (old, new) = (1usize, 2usize);
        terms.hold(&id, old);
        // The window comes back on the new connection and takes it over.
        let (tx2, rx2) = std::sync::mpsc::channel::<terminal::TermEvent>();
        terms
            .attach(&id, Sink::new(move |e| tx2.send(e).is_ok()))
            .unwrap();
        terms.hold(&id, new);
        // The old connection's end comes now: nothing changes.
        assert_eq!(terms.orphan(&id, old), None);
        while rx2.try_recv().is_ok() {}
        terms.write(&id, "echo late-end-check\r\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = String::new();
        while !seen.contains("late-end-check") {
            assert!(
                Instant::now() < deadline,
                "the new connection got no output: {seen}"
            );
            if let Ok(terminal::TermEvent::Data { data }) =
                rx2.recv_timeout(Duration::from_millis(200))
            {
                seen.push_str(&data);
            }
        }
        // The holder's own end still orphans it.
        assert!(terms.orphan(&id, new).is_some());
        drop(rx);
        terms.kill(&id);
    }

    #[test]
    fn a_terminal_taken_over_by_a_new_connection_outlives_the_old_ones_grace() {
        // ST1 flow 25 c2: a Wi-Fi drop ends the connection; the window comes back on a new one and takes the shell.
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        m.terminals.set_grace(Duration::from_millis(600));
        let p = projects::add(&m.db, &project.path().to_string_lossy()).unwrap();
        let shell = terminal::shells()[0].id.clone();
        let spawn = json!({"id": 1, "cmd": "term_spawn", "args": {"projectId": p.id, "shell": shell, "cols": 80, "rows": 24, "onEvent": "t1"}});
        let answer = |line: Value| {
            let captured: Arc<Mutex<Vec<u8>>> = Arc::default();
            let out: Out = Arc::new(Mutex::new(Box::new(Captured(captured.clone()))));
            handle_connection(
                m.clone(),
                std::io::Cursor::new(line.to_string().into_bytes()),
                out,
            );
            let text = String::from_utf8_lossy(&captured.lock().unwrap()).to_string();
            text.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .find(|r| r["id"] == 1)
                .unwrap()
        };
        let first_ended = Instant::now();
        let term = answer(spawn)["ok"].as_str().unwrap().to_string();
        // Halfway through the first grace a second connection takes it over, then ends as well: its own grace starts.
        std::thread::sleep(Duration::from_millis(300));
        let taken =
            answer(json!({"id": 1, "cmd": "term_attach", "args": {"id": term, "onEvent": "t2"}}));
        assert!(taken.get("err").is_none(), "{taken}");
        // The first grace is over now; that one no longer ends it.
        std::thread::sleep(Duration::from_millis(750).saturating_sub(first_ended.elapsed()));
        assert!(
            m.terminals.write(&term, "x").is_ok(),
            "the first grace ended a taken-over terminal"
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while m.terminals.write(&term, "x").is_ok() {
            assert!(Instant::now() < deadline, "the second grace did not end it");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            answer(json!({"id": 1, "cmd": "term_attach", "args": {"id": term, "onEvent": "t3"}}))["err"]
                .is_string()
        );
    }

    #[test]
    fn the_service_starts_answers_through_its_token_and_stops() {
        // The service is started from the test binary itself only when it is the real program; here the parts are
        // tested directly: a private service file, a refused connection without the token.
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().to_path_buf();
        let d2 = dir.clone();
        std::thread::spawn(move || {
            let _ = serve_core(&d2);
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
        let start = json!({"projectId": id, "onEvent": "r1", "start": {"file": "PLAN.md", "projectName": "Shop", "provider": "qanuvi"}});
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

    /// M5b: a master that left no stop note but a state note from before the computer last started was cut off by
    /// a shutdown; its runs say so (stoppedBy for the window's offer) and are kept for a resume. The watcher's crash
    /// report goes into the Inbox once.
    #[test]
    fn m5b_a_shutdown_and_a_crash_report_are_told_at_the_next_start() {
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
            m.keep_start(&pid, &json!({"file": "T.md"}));
            assert!(
                data.path()
                    .join("run-starts")
                    .join(format!("{pid}.json"))
                    .exists()
            );
            pid
        };
        // Its last state note is older than the computer's start.
        std::fs::write(watch::state_file(data.path()), json!({"at": 1}).to_string()).unwrap();
        std::fs::write(
            watch::report_file(data.path()),
            json!({"title": "Qanuvi's background part crashed 3 times", "text": "Report"})
                .to_string(),
        )
        .unwrap();
        let m = Master::open(data.path()).unwrap();
        assert!(!watch::state_file(data.path()).exists(), "read once");
        let r = runs::list(&m.db, &pid).unwrap().remove(0);
        let d: Value = serde_json::from_str(&r.data).unwrap();
        assert_eq!(d["stoppedBy"], "shutdown");
        assert_eq!(d["message"], runs::stop_note("shutdown").unwrap());
        assert_eq!(
            *m.cut_off.lock().unwrap(),
            ("shutdown".to_string(), vec![pid.clone()])
        );
        let inbox = crate::inbox::list(&m.db).unwrap();
        assert!(inbox.iter().any(|i| i.title.contains("crashed 3 times")));
        assert!(!watch::report_file(data.path()).exists());
        // A resume that cannot start says so in the Inbox, naming the cut-off run's plan (a start that is not valid
        // here, so no run starts in a test).
        m.keep_start(&pid, &json!({"file": 5}));
        m.resume_one_because(&pid, "shutdown");
        let inbox = crate::inbox::list(&m.db).unwrap();
        let note = inbox
            .iter()
            .find(|i| i.title.starts_with("Plan run not resumed"))
            .unwrap();
        assert!(
            note.text.contains("The run of T.md was cut off"),
            "{}",
            note.text
        );
        assert!(note.text.contains(". Open the plan and choose Resume."));
        // A clean end leaves no state note: the next start finds nothing to tell.
        drop(m);
        let m = Master::open(data.path()).unwrap();
        assert_eq!(m.cut_off.lock().unwrap().0, "");
    }

    /// s10: Quit's "Go on next time" leaves the stop note "quit-later"; the next start keeps the runs for a resume
    /// of their own (whatever "Start Qanuvi when Windows starts" says), and a resume that cannot start says why.
    #[test]
    fn go_on_next_time_keeps_the_runs_for_the_next_start() {
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
            pid
        };
        std::fs::write(stop_note_file(data.path()), "quit-later").unwrap();
        let m = Master::open(data.path()).unwrap();
        let r = runs::list(&m.db, &pid).unwrap().remove(0);
        let d: Value = serde_json::from_str(&r.data).unwrap();
        assert_eq!(d["stoppedBy"], "quit-later");
        assert_eq!(d["message"], runs::stop_note("quit-later").unwrap());
        assert_eq!(
            *m.cut_off.lock().unwrap(),
            ("quit-later".to_string(), vec![pid.clone()])
        );
        m.keep_start(&pid, &json!({"file": 5}));
        m.resume_one_because(&pid, "quit-later");
        let inbox = crate::inbox::list(&m.db).unwrap();
        let note = inbox
            .iter()
            .find(|i| i.title.starts_with("Plan run not resumed"))
            .unwrap();
        assert!(
            note.text
                .contains("The run of T.md was stopped when Qanuvi was quit"),
            "{}",
            note.text
        );
    }

    /// A project added from the master's own terminal (`qanuvi-master projects add`), removed or changed reaches the
    /// windows following its list, so an open window shows it without switching machines.
    #[test]
    fn project_list_changes_reach_the_windows_following_it() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let m = Master::open(data.path()).unwrap().shared();
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
            .dispatch("projects_watch", &json!({"onEvent": "p1"}), &streams, &open)
            .unwrap();
        assert_eq!(first, json!([]));
        let p = call(
            &m,
            "projects_add",
            json!({"path": project.path().to_string_lossy()}),
        )
        .unwrap();
        let id = p["id"].as_str().unwrap().to_string();
        call(&m, "projects_redetect", json!({"id": id})).unwrap();
        call(&m, "projects_remove", json!({"id": id})).unwrap();
        let changes: Vec<Value> = heard
            .lock()
            .unwrap()
            .iter()
            .map(|v| v["change"].clone())
            .collect();
        assert_eq!(changes, [json!("add"), json!("update"), json!("remove")]);
        // A failed add tells nobody.
        assert!(
            call(
                &m,
                "projects_add",
                json!({"path": "/no/such/folder/at/all"})
            )
            .is_err()
        );
        assert_eq!(heard.lock().unwrap().len(), 3);
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
        // t1: the tray Quit question in the main window passes the answer on to the master (app_quit_answer).
        "quit_question_answer",
        // Popup notifications, the app itself, this screen's files and data folder.
        "notify_settings",
        "notify_take_start",
        "notify_test",
        "notify_notice",
        "app_quitting",
        "notify_item",
        // The window's part of a browser sign-in: this computer's browser and its loopback answer (M2).
        "account_wait",
        "account_cancel",
        "app_info",
        "screen_usage",
        "app_restart",
        "startup_path",
        "data_location_move",
        "data_location_cancel",
        "data_location_remove_old",
        "data_test_build",
        "attachment_file",
        "export_save",
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
        "clipboard_image",
        // The app's own lifecycle (M4c): it stops This PC's master before an update installs.
    ];

    /// The command names of the app's `generate_handler!` list.
    fn window_commands() -> Vec<String> {
        let src = include_str!("desktop.rs");
        // The main window's list is the last one (the small quit-question window, t1, has its own before it).
        let list = &src[src.rfind("generate_handler![").expect("the handler list")..];
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
