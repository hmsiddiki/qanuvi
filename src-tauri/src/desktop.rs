//! The desktop app (Tauri): the commands its window calls, registered in [`run`]. The core modules are in lib.rs.

use crate::{
    agent, agent_config, checks, files, git, index, paths, preview, projects, runs, store, terminal,
};

use serde::Serialize;
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::{Manager, State};

/// Basic facts about the running app, shown in the interface and used by later milestones (updates, diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: "Qanuvi",
            version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }
}

pub struct AppState {
    db: store::Db,
    terminals: terminal::Terminals,
    agents: agent::Agents,
    indexes: index::Indexes,
    inspector: preview::Inspector,
    remotes: crate::remote::Remotes,
    accounts: crate::account::Accounts,
}

type Res<T> = Result<T, String>;

fn project_root(state: &AppState, id: &str) -> Res<PathBuf> {
    Ok(PathBuf::from(projects::get(&state.db, id)?.path))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo::current()
}

/// The folder given on the command line (`qanuvi path/to/project` or `qanuvi .`), if it is a folder.
pub fn startup_folder(args: impl Iterator<Item = String>, cwd: &std::path::Path) -> Option<String> {
    args.skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| cwd.join(a))
        .find(|p| p.is_dir())
        .and_then(|p| p.canonicalize().ok())
        .map(|p| projects::display_path(&p))
}

#[tauri::command]
fn startup_path() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    startup_folder(std::env::args(), &cwd)
}

// Projects ------------------------------------------------------------------------------------------------------

#[tauri::command]
fn projects_list(state: State<AppState>) -> Res<Vec<projects::Project>> {
    projects::list(&state.db)
}

#[tauri::command]
fn projects_add(state: State<AppState>, path: String) -> Res<projects::Project> {
    projects::add(&state.db, &path)
}

#[tauri::command]
fn projects_remove(state: State<AppState>, id: String) -> Res<()> {
    state.terminals.kill_project(&id);
    state.agents.stop_project(&id);
    projects::remove(&state.db, &id)
}

#[tauri::command]
fn projects_update(
    state: State<AppState>,
    id: String,
    name: String,
    settings: projects::ProjectSettings,
) -> Res<projects::Project> {
    projects::update(&state.db, &id, &name, &settings)
}

#[tauri::command]
fn projects_touch(state: State<AppState>, id: String) -> Res<()> {
    projects::touch(&state.db, &id)
}

#[tauri::command]
fn projects_redetect(state: State<AppState>, id: String) -> Res<projects::Project> {
    projects::redetect(&state.db, &id)
}

// Files -----------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn files_list_dir(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Res<Vec<files::Entry>> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::list_dir(&root, &path)).await
}

#[derive(Serialize)]
struct FileList {
    files: Vec<String>,
    truncated: bool,
}

#[tauri::command]
async fn files_list_all(state: State<'_, AppState>, project_id: String) -> Res<FileList> {
    let root = project_root(&state, &project_id)?;
    blocking(move || {
        files::list_files(&root).map(|(files, truncated)| FileList { files, truncated })
    })
    .await
}

#[tauri::command]
async fn files_read(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Res<files::FileContent> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::read_file(&root, &path)).await
}

#[tauri::command]
async fn files_write(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    text: String,
    expected_hash: Option<String>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::write_file(&root, &path, &text, expected_hash.as_deref())).await
}

#[tauri::command]
async fn files_search(
    state: State<'_, AppState>,
    project_id: String,
    query: String,
    regex: bool,
    case_sensitive: bool,
) -> Res<files::SearchResult> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::search(&root, &query, regex, case_sensitive, 2000)).await
}

#[derive(Serialize)]
struct SymbolHit {
    path: String,
    name: String,
    kind: &'static str,
    line: usize,
    container: Option<String>,
}

/// "Go to symbol" across the project.
#[tauri::command]
async fn index_find(
    state: State<'_, AppState>,
    project_id: String,
    query: String,
) -> Res<Vec<SymbolHit>> {
    let root = project_root(&state, &project_id)?;
    state.indexes.with(&root, |idx| {
        idx.find(&query, None, 80)
            .into_iter()
            .map(|f| SymbolHit {
                path: f.path,
                name: f.symbol.name,
                kind: f.symbol.kind,
                line: f.symbol.line,
                container: f.symbol.container,
            })
            .collect()
    })
}

/// "Go to symbol in file".
#[tauri::command]
async fn index_outline(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Res<Vec<index::Symbol>> {
    let root = project_root(&state, &project_id)?;
    state.indexes.with(&root, |idx| {
        idx.outline(&path).map(|s| s.to_vec()).unwrap_or_default()
    })
}

/// Runs a project's test command for a plan step (at most `timeout_secs`, capped at 30 minutes).
#[tauri::command]
async fn checks_run(
    state: State<'_, AppState>,
    project_id: String,
    command: String,
    timeout_secs: u64,
) -> Res<checks::CheckResult> {
    let root = project_root(&state, &project_id)?;
    let timeout = std::time::Duration::from_secs(timeout_secs.clamp(5, 1800));
    blocking(move || checks::run(&root, &command, timeout)).await
}

// Terminals -------------------------------------------------------------------------------------------------------

#[tauri::command]
fn term_shells() -> Vec<terminal::Shell> {
    terminal::shells()
}

#[tauri::command]
fn term_spawn(
    window: tauri::Window,
    state: State<AppState>,
    project_id: String,
    shell: String,
    cols: u16,
    rows: u16,
    on_event: Channel<terminal::TermEvent>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    let id = state
        .terminals
        .spawn(&project_id, &root, &shell, cols, rows, on_event.into())?;
    state.terminals.set_owner(&id, window.label());
    Ok(id)
}

#[tauri::command]
fn term_write(state: State<AppState>, id: String, data: String) -> Res<()> {
    state.terminals.write(&id, &data)
}

#[tauri::command]
fn term_resize(state: State<AppState>, id: String, cols: u16, rows: u16) -> Res<()> {
    state.terminals.resize(&id, cols, rows)
}

#[tauri::command]
fn term_kill(state: State<AppState>, id: String) {
    state.terminals.kill(&id);
}

// Git -------------------------------------------------------------------------------------------------------------

#[tauri::command]
async fn git_status(state: State<'_, AppState>, project_id: String) -> Res<git::Status> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::status(&root)).await
}

#[tauri::command]
async fn git_diff(state: State<'_, AppState>, project_id: String, path: String) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::diff(&root, &path)).await
}

#[tauri::command]
async fn git_stage(state: State<'_, AppState>, project_id: String, path: String) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::stage(&root, &path)).await
}

#[tauri::command]
async fn git_revert(state: State<'_, AppState>, project_id: String, path: String) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::revert(&root, &path)).await
}

#[tauri::command]
async fn git_versions(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Res<git::Versions> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::versions(&root, &path)).await
}

#[tauri::command]
async fn git_set_index(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    text: String,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::set_index(&root, &path, &text)).await
}

#[tauri::command]
async fn git_unstage(state: State<'_, AppState>, project_id: String, path: String) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::unstage(&root, &path)).await
}

#[tauri::command]
async fn git_stage_all(state: State<'_, AppState>, project_id: String) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::stage_all(&root)).await
}

#[tauri::command]
async fn git_commit(
    state: State<'_, AppState>,
    project_id: String,
    message: String,
    amend: bool,
    all: bool,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::commit(&root, &message, amend, all)).await
}

#[tauri::command]
async fn git_sync(state: State<'_, AppState>, project_id: String, action: String) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::sync(&root, &action)).await
}

#[tauri::command]
async fn git_branches(state: State<'_, AppState>, project_id: String) -> Res<Vec<git::Branch>> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::branches(&root)).await
}

#[tauri::command]
async fn git_switch(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
    create: bool,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::switch(&root, &name, create)).await
}

#[tauri::command]
async fn git_stashes(state: State<'_, AppState>, project_id: String) -> Res<Vec<git::Stash>> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::stashes(&root)).await
}

#[tauri::command]
async fn git_stash(
    state: State<'_, AppState>,
    project_id: String,
    action: String,
    index: u32,
    message: String,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::stash(&root, &action, index, &message)).await
}

#[tauri::command]
async fn git_log(
    state: State<'_, AppState>,
    project_id: String,
    path: Option<String>,
    max: u32,
) -> Res<Vec<git::Commit>> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::log(&root, path.as_deref(), max)).await
}

#[tauri::command]
async fn git_show(
    state: State<'_, AppState>,
    project_id: String,
    sha: String,
    path: Option<String>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::show(&root, &sha, path.as_deref())).await
}

#[tauri::command]
async fn git_blame(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    line: u32,
) -> Res<git::Blame> {
    let root = project_root(&state, &project_id)?;
    blocking(move || git::blame(&root, &path, line)).await
}

// Agents ----------------------------------------------------------------------------------------------------------

#[tauri::command]
fn agent_info() -> agent::AgentInfo {
    agent::info()
}

#[tauri::command]
fn agent_sessions(state: State<AppState>, project_id: String) -> Res<Vec<agent::Session>> {
    agent::list_sessions(&state.db, &project_id)
}

#[tauri::command]
fn agent_session_events(state: State<AppState>, session_id: String) -> Res<Vec<String>> {
    agent::session_events(&state.db, &session_id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn agent_start(
    state: State<AppState>,
    project_id: String,
    session_id: Option<String>,
    prompt: String,
    model: String,
    mode: String,
    base_url: Option<String>,
    effort: Option<String>,
    lean: Option<bool>,
    index_tools: Option<bool>,
    browser_tools: Option<bool>,
    context: Option<String>,
    files: Option<Vec<agent::Attachment>>,
    agent: Option<String>,
    setup: Option<serde_json::Value>,
    on_event: Channel<agent::AgentEvent>,
) -> Res<agent::Session> {
    let project = projects::get(&state.db, &project_id)?;
    let setup = setup
        .filter(|v| !v.is_null())
        .map(|v| crate::brain::with_keys(&v.to_string(), model_key))
        .transpose()?;
    state.agents.start(
        &state.db,
        &project,
        session_id.as_deref(),
        &prompt,
        &model,
        &mode,
        base_url.as_deref(),
        effort.as_deref(),
        lean.unwrap_or(false),
        index_tools.unwrap_or(false),
        browser_tools.unwrap_or(false),
        context.as_deref(),
        &files.unwrap_or_default(),
        agent.as_deref().unwrap_or("claude-code"),
        setup.as_deref(),
        on_event.into(),
    )
}

/// The page the agent's browser tools are on, for the preview tab to follow.
#[tauri::command]
fn browser_agent_view(session_id: String) -> Res<Option<serde_json::Value>> {
    Ok(preview::agent_view(&agent::browser_state_file(
        &session_id,
    )?))
}

#[tauri::command]
fn browser_agent_shot(session_id: String) -> Res<String> {
    preview::agent_shot(&agent::browser_state_file(&session_id)?)
}

#[tauri::command]
async fn inspector_show(
    state: State<'_, AppState>,
    url: String,
    width: u32,
    height: u32,
) -> Res<serde_json::Value> {
    let i = state.inspector.clone();
    blocking(move || i.show(&url, width, height)).await
}

#[tauri::command]
async fn inspector_scroll(state: State<'_, AppState>, dy: f64) -> Res<serde_json::Value> {
    let i = state.inspector.clone();
    blocking(move || i.scroll(dy)).await
}

/// The element at a point of the inspector's picture, with the note to hand the agent.
#[tauri::command]
async fn inspector_pick(state: State<'_, AppState>, x: f64, y: f64) -> Res<serde_json::Value> {
    let i = state.inspector.clone();
    blocking(move || {
        let mut e = i.pick(x, y)?;
        e["note"] = serde_json::Value::String(preview::element_note(&e));
        Ok(e)
    })
    .await
}

#[tauri::command]
async fn inspector_page_text(state: State<'_, AppState>, url: String) -> Res<String> {
    let i = state.inspector.clone();
    blocking(move || i.page_text(&url)).await
}

// Memory (Milestone 17). The argument is `project` (not `projectId`): the memory stays on this PC, also for a
// server's projects.

#[tauri::command]
fn memory_list(
    state: State<AppState>,
    project: Option<String>,
    suggested: bool,
) -> Res<Vec<crate::memory::Entry>> {
    crate::memory::list(&state.db, project.as_deref(), suggested)
}

#[tauri::command]
fn memory_save(state: State<AppState>, entry: crate::memory::Entry) -> Res<crate::memory::Entry> {
    crate::memory::save(&state.db, &entry)
}

#[tauri::command]
fn memory_delete(state: State<AppState>, id: String) -> Res<()> {
    crate::memory::delete(&state.db, &id)
}

/// What an agent gets from the memory with a request.
#[tauri::command]
fn memory_slice(
    state: State<AppState>,
    project: Option<String>,
    query: String,
    budget: usize,
    count: Option<bool>,
) -> Res<crate::memory::Slice> {
    crate::memory::slice(
        &state.db,
        project.as_deref(),
        &query,
        budget.clamp(200, 20000),
        count.unwrap_or(false),
    )
}

#[tauri::command]
fn memory_search(
    state: State<AppState>,
    project: Option<String>,
    query: String,
) -> Res<Vec<crate::memory::Entry>> {
    crate::memory::search(&state.db, project.as_deref(), &query, 50)
}

// Signing in with a Qlopy site account (Milestone 19).

/// Signs in to the central Qanuvi account: qanuvi.com's sign-in page opens in the browser (Google, Microsoft or
/// GitHub there); this waits up to 5 minutes for it.
#[tauri::command]
async fn account_sign_in(app: tauri::AppHandle) -> Res<crate::account::Account> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_opener::OpenerExt;
        let (site, client) = crate::account::home();
        let state = app2.state::<AppState>();
        state.accounts.sign_in(&site, &client, |url| {
            app2.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| format!("The browser could not be opened: {e}"))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn account_cancel(state: State<AppState>) {
    state.accounts.cancel();
}

/// Who is signed in (after a restart too, from the kept sign-in).
#[tauri::command]
async fn account_me(app: tauri::AppHandle) -> Res<crate::account::Account> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (site, client) = crate::account::home();
        app2.state::<AppState>().accounts.me_at(&site, &client)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn account_sign_out(app: tauri::AppHandle) -> Res<()> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (site, client) = crate::account::home();
        app2.state::<AppState>().accounts.sign_out(&site, &client)
    })
    .await
    .map_err(|e| e.to_string())?
}

// Memory disk (Milestone 18). The passphrase lives in the system's credential store (Windows Credential Manager,
// macOS Keychain, the Secret Service on Linux), never in a file.

fn passphrase_entry() -> Res<keyring::Entry> {
    keyring_entry("Qanuvi")
}

fn keyring_entry(service: &str) -> Res<keyring::Entry> {
    keyring::Entry::new(service, "memory-disk").map_err(|e| e.to_string())
}

fn passphrase() -> Res<String> {
    passphrase_entry()?
        .get_password()
        .map_err(|_| "Set the memory disk's passphrase first.".to_string())
}

#[tauri::command]
fn memory_disk_device() -> Res<crate::memory_disk::Device> {
    let dir = crate::paths::data_dir();
    Ok(crate::memory_disk::device(&dir))
}

#[tauri::command]
fn memory_disk_passphrase_set(passphrase: String) -> Res<()> {
    if passphrase.chars().count() < 8 {
        return Err("The passphrase needs at least 8 characters.".into());
    }
    passphrase_entry()?
        .set_password(&passphrase)
        .map_err(|e| format!("The passphrase could not be saved: {e}"))
}

#[tauri::command]
fn memory_disk_passphrase_saved() -> bool {
    passphrase().is_ok()
}

#[tauri::command]
fn memory_disk_passphrase_forget() -> Res<()> {
    match passphrase_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn memory_disk_sync(
    state: State<'_, AppState>,
    disk: crate::memory_disk::Disk,
) -> Res<crate::memory_disk::Report> {
    let pass = passphrase()?;
    let me = crate::memory_disk::device(&crate::paths::data_dir());
    let db = state.db.clone();
    blocking(move || crate::memory_disk::sync(&db, &disk, &pass, &me)).await
}

#[tauri::command]
async fn memory_disk_versions(
    disk: crate::memory_disk::Disk,
) -> Res<Vec<crate::memory_disk::Version>> {
    let me = crate::memory_disk::device(&crate::paths::data_dir());
    blocking(move || crate::memory_disk::versions(&disk, &me)).await
}

#[tauri::command]
async fn memory_disk_restore(
    state: State<'_, AppState>,
    disk: crate::memory_disk::Disk,
    name: String,
) -> Res<usize> {
    let pass = passphrase()?;
    let db = state.db.clone();
    blocking(move || crate::memory_disk::restore(&db, &disk, &pass, &name)).await
}

// Masters without a window (Milestone 16) -------------------------------------------------------------------------

/// Connects to a master ("local" for the master program on this PC) and answers its hello.
#[tauri::command]
async fn remote_connect(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    target: String,
    program: String,
    identity: Option<String>,
    relay: Option<RelayTarget>,
) -> Res<serde_json::Value> {
    let data = crate::paths::data_dir();
    let transport = match target.trim() {
        // A paired computer through the relay; the pairing key comes from the credential store.
        "relay" => {
            let r = relay.ok_or("No relay details for this computer.")?;
            let key = relay_key(&r.pairing)?
                .get_password()
                .map_err(|_| "This device's pairing is missing; pair again.".to_string())?;
            crate::remote::Transport::Relay(crate::relay_client::Paired {
                relay: r.url,
                master_id: r.master_id,
                master_name: String::new(),
                pairing: r.pairing,
                key,
            })
        }
        // This computer's own master, on the app's own data (remote access, Milestone 20).
        "self" => {
            crate::remote::Transport::Process(crate::remote::command_for("self", "", None, &data)?)
        }
        t => crate::remote::Transport::Process(crate::remote::command_for(
            t,
            program.trim(),
            identity.as_deref(),
            &data.join("local-master"),
        )?),
    };
    let _ = &state;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        state.remotes.connect(app2.clone(), &id, transport)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayTarget {
    url: String,
    master_id: String,
    pairing: String,
}

/// Where a model provider's key is kept (Milestone 24).
fn model_key_entry(endpoint: &str) -> Res<keyring::Entry> {
    keyring::Entry::new("Qanuvi", &format!("models:{endpoint}")).map_err(|e| e.to_string())
}

/// A provider's key: the credential store, or an environment variable (QANUVI_KEY_<PROVIDER>).
fn model_key(endpoint: &str) -> Option<String> {
    model_key_entry(endpoint)
        .ok()
        .and_then(|e| e.get_password().ok())
        .or_else(|| crate::brain::env_key(endpoint))
}

/// Keeps a provider's key in the credential store (an empty key forgets it).
#[tauri::command]
fn model_key_set(endpoint: String, key: String) -> Res<()> {
    let entry = model_key_entry(&endpoint)?;
    if key.trim().is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        };
    }
    entry
        .set_password(key.trim())
        .map_err(|e| format!("The key could not be kept: {e}"))
}

/// Whether a provider has a key (the key itself never goes to the window).
#[tauri::command]
fn model_key_has(endpoint: String) -> bool {
    model_key(&endpoint).is_some()
}

/// The models a provider offers (OpenRouter: free ones that can use tools, unless `all`).
#[tauri::command]
async fn models_list(
    endpoint: crate::llm::Endpoint,
    all: Option<bool>,
) -> Res<Vec<crate::llm::ModelInfo>> {
    blocking(move || {
        let mut ep = endpoint;
        ep.key = model_key(&ep.id);
        crate::llm::list_models(&ep, all.unwrap_or(false))
    })
    .await
}

/// Tries a model with one tiny message (and one tool, as the agent needs tools): Ok with the seconds it took, or why
/// it did not answer. Routers list models whose providers are not connected; this finds out.
#[tauri::command]
async fn model_probe(endpoint: crate::llm::Endpoint, model: String) -> Res<f64> {
    blocking(move || {
        let mut ep = endpoint;
        ep.key = model_key(&ep.id);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let tools = serde_json::json!([{"type": "function", "function": {
                "name": "Read", "description": "Read a file.",
                "parameters": {"type": "object", "properties": {"file_path": {"type": "string"}}, "required": ["file_path"]}
            }}]);
            let messages = [serde_json::json!({"role": "user", "content": "Reply with the single word OK."})];
            let r = crate::llm::chat(&ep, &model, &messages, &tools, &["Read"], &mut |_| {})
                .map_err(|f| f.reason)
                .and_then(|r| {
                    if r.text.trim().is_empty() && r.calls.is_empty() {
                        Err("an empty answer".to_string())
                    } else {
                        Ok(start.elapsed().as_secs_f64())
                    }
                });
            let _ = tx.send(r);
        });
        rx.recv_timeout(std::time::Duration::from_secs(45))
            .unwrap_or_else(|_| Err("no answer within 45 seconds".into()))
    })
    .await
}

/// Checks a provider's key (OpenRouter: also the free requests left today).
#[tauri::command]
async fn model_key_check(endpoint: crate::llm::Endpoint) -> Res<String> {
    blocking(move || {
        let mut ep = endpoint;
        ep.key = model_key(&ep.id);
        crate::llm::check_key(&ep)
    })
    .await
}

/// This computer's memory and graphics card, for suggesting local models that fit.
#[tauri::command]
async fn machine_info() -> Res<crate::machine::Machine> {
    blocking(|| Ok(crate::machine::detect())).await
}

/// Whether a local provider's program (Ollama, OmniRoute) is installed on this computer.
#[tauri::command]
fn local_server_status(
    endpoint: crate::llm::Endpoint,
    start: Option<String>,
) -> Option<crate::local_servers::LocalServer> {
    crate::local_servers::status(&endpoint, start.as_deref())
}

/// Connects Qanuvi to the OmniRoute on this computer: asks it for a key named "Qanuvi" and keeps the key in the
/// credential store. The key never reaches the window.
#[tauri::command]
async fn omniroute_connect(endpoint: crate::llm::Endpoint) -> Res<()> {
    blocking(move || {
        let key = crate::local_servers::omniroute_new_key(&endpoint)?;
        model_key_set(endpoint.id.clone(), key)
    })
    .await
}

/// The tools the setup wizard looks for (Ollama, Node.js, npm, OmniRoute, winget).
#[tauri::command]
async fn setup_tools() -> Res<crate::local_servers::Tools> {
    blocking(|| Ok(crate::local_servers::tools())).await
}

/// Installs one of them at the user's click (Windows asks for permission itself).
#[tauri::command]
async fn setup_install(tool: String) -> Res<()> {
    blocking(move || crate::local_servers::install(&tool)).await
}

/// Free memory and free graphics memory right now (GB), for the wizard's first page.
#[tauri::command]
async fn machine_room() -> Res<Option<(f64, f64)>> {
    blocking(|| Ok(crate::machine::room().map(|r| (r.free_ram_gb, r.free_vram_gb)))).await
}

/// Where Qanuvi keeps its data, a move booked for the next start, and a copy left by the last move.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DataLocationInfo {
    current: String,
    default_dir: String,
    pending: Option<String>,
    old: Option<String>,
    note: Option<String>,
}

#[tauri::command]
fn data_location_info() -> DataLocationInfo {
    let d = crate::data_location::Defaults::real();
    let loc = crate::data_location::read(&d.base);
    let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
    DataLocationInfo {
        current: s(&crate::paths::data_dir()),
        default_dir: s(&d.base),
        pending: loc.pending.as_deref().map(s),
        old: loc.old.as_deref().map(s),
        note: loc.note,
    }
}

/// Books a move to `folder` for the next start (checked: empty, writable, room on the drive). Answers the folder.
#[tauri::command]
async fn data_location_move(folder: String) -> Res<String> {
    blocking(move || {
        let d = crate::data_location::Defaults::real();
        crate::data_location::plan_move(
            &d,
            &crate::paths::data_dir(),
            std::path::Path::new(folder.trim()),
        )
        .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
fn data_location_cancel() -> Res<()> {
    crate::data_location::cancel_move(&crate::data_location::Defaults::real().base)
}

/// Removes the copy left by the last move (the window asked the user first).
#[tauri::command]
async fn data_location_remove_old() -> Res<()> {
    blocking(|| {
        crate::data_location::remove_old(
            &crate::data_location::Defaults::real().base,
            &crate::paths::data_dir(),
        )
    })
    .await
}

/// Starts Qanuvi again (to finish a move): the new instance waits until this one has closed.
#[tauri::command]
fn app_restart(app: tauri::AppHandle) -> Res<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // The same project folder argument again (not the flags, not an older --after-pid).
    let keep: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    std::process::Command::new(exe)
        .arg(format!("--after-pid={}", std::process::id()))
        .args(keep)
        .spawn()
        .map_err(|e| format!("Qanuvi could not start again: {e}"))?;
    app.exit(0);
    Ok(())
}

/// A short decision from Qanuvi core's free models (no tools), asked one after another: `setup` names them in order.
#[tauri::command]
async fn core_decide(setup: String, system: String, prompt: String) -> Res<(String, String)> {
    blocking(move || {
        let setup = crate::brain::with_keys(&setup, model_key)?;
        let setup: crate::brain::Setup = serde_json::from_str(&setup).map_err(|e| e.to_string())?;
        crate::brain::decide(&setup, &system, &prompt)
    })
    .await
}

/// A file from this computer for the next message (the composer's + menu).
#[tauri::command]
async fn attachment_file(path: String) -> Res<agent::Attachment> {
    blocking(move || crate::attachments::read_file(std::path::Path::new(&path))).await
}

/// A web page as text for the next message.
#[tauri::command]
async fn attachment_page(url: String) -> Res<agent::Attachment> {
    blocking(move || crate::attachments::read_page(&url)).await
}

/// Qanuvi's remote knowledge file (24c), its signature checked against the built-in key; empty `url`: the default.
#[tauri::command]
async fn knowledge_fetch(url: String) -> Res<String> {
    blocking(move || {
        let url = if url.trim().is_empty() {
            crate::knowledge::DEFAULT_URL.to_string()
        } else {
            url
        };
        crate::knowledge::fetch(&url, crate::knowledge::PUBLIC_KEY)
    })
    .await
}

/// OmniRoute's provider connections and their health.
#[tauri::command]
async fn omniroute_connections(
    endpoint: crate::llm::Endpoint,
) -> Res<Vec<crate::omniroute::Connection>> {
    blocking(move || crate::omniroute::connections(&endpoint)).await
}

/// OmniRoute's whole provider catalog (for users who choose their own).
#[tauri::command]
async fn omniroute_catalog(
    endpoint: crate::llm::Endpoint,
) -> Res<Vec<crate::omniroute::CatalogEntry>> {
    blocking(move || crate::omniroute::catalog(&endpoint)).await
}

/// Connects a provider in OmniRoute with an API key: one the user pasted, or (`saved`) the key Qanuvi keeps for one of
/// its own providers, which goes from the credential store to OmniRoute without passing through the window.
#[tauri::command]
async fn omniroute_add_key(
    endpoint: crate::llm::Endpoint,
    provider: String,
    key: Option<String>,
    saved: Option<String>,
) -> Res<()> {
    blocking(move || {
        let key = match (key.filter(|k| !k.trim().is_empty()), saved) {
            (Some(k), _) => k,
            (None, Some(from)) => {
                model_key(&from).ok_or("Qanuvi keeps no key for that provider.")?
            }
            (None, None) => return Err("No key.".into()),
        };
        crate::omniroute::add_key(&endpoint, &provider, &key)
    })
    .await
}

#[tauri::command]
async fn omniroute_test(endpoint: crate::llm::Endpoint, id: String) -> Res<()> {
    blocking(move || crate::omniroute::test(&endpoint, &id)).await
}

/// Renews a sign-in ("panel": open OmniRoute's own page for this provider instead).
#[tauri::command]
async fn omniroute_sign_in(endpoint: crate::llm::Endpoint, provider: String) -> Res<()> {
    blocking(move || crate::omniroute::sign_in(&endpoint, &provider)).await
}

#[tauri::command]
async fn omniroute_make_combo(
    endpoint: crate::llm::Endpoint,
    name: String,
    models: Vec<String>,
) -> Res<()> {
    blocking(move || crate::omniroute::make_combo(&endpoint, &name, &models)).await
}

/// Starts a local provider's server and waits until it answers.
#[tauri::command]
async fn local_server_start(endpoint: crate::llm::Endpoint, start: Option<String>) -> Res<()> {
    blocking(move || {
        let mut ep = endpoint;
        ep.key = model_key(&ep.id);
        crate::local_servers::start(&ep, start.as_deref())
    })
    .await
}

/// Downloads a model into Ollama; progress goes to `on_event`.
#[tauri::command]
async fn ollama_pull(
    endpoint: crate::llm::Endpoint,
    model: String,
    on_event: Channel<serde_json::Value>,
) -> Res<()> {
    blocking(move || {
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
        crate::llm::ollama_pull(&endpoint, &model, |status, done, total| {
            // A few updates a second are enough.
            if last.elapsed().as_millis() >= 250 || status == "success" {
                last = std::time::Instant::now();
                let _ = on_event
                    .send(serde_json::json!({"status": status, "done": done, "total": total}));
            }
        })
    })
    .await
}

/// What the brain has learned about each model (turns, broken tool calls, rests).
#[tauri::command]
fn brain_records() -> crate::brain::Records {
    crate::brain::Records::load(&crate::paths::data_dir())
}

fn relay_key(pairing: &str) -> Res<keyring::Entry> {
    keyring::Entry::new("Qanuvi", &format!("relay:{pairing}")).map_err(|e| e.to_string())
}

/// Pairs this device with a computer showing a pairing code; the key goes to the credential store.
#[tauri::command]
async fn remote_pair(relay: String, code: String, name: String) -> Res<serde_json::Value> {
    blocking(move || {
        let p = crate::relay_client::pair(&relay, &code, &name)?;
        relay_key(&p.pairing)?
            .set_password(&p.key)
            .map_err(|e| format!("The pairing could not be kept: {e}"))?;
        Ok(serde_json::json!({"relay": p.relay, "masterId": p.master_id, "masterName": p.master_name, "pairing": p.pairing}))
    })
    .await
}

/// Forgets a pairing on this device.
#[tauri::command]
fn remote_unpair(pairing: String) -> Res<()> {
    match relay_key(&pairing)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Passes one of the window's calls to a master. With `streams`, events of what it starts (terminal output, agent
/// events) go to `on_event`.
#[tauri::command]
async fn remote_call(
    state: State<'_, AppState>,
    master: String,
    cmd: String,
    args: serde_json::Value,
    streams: bool,
    on_event: Channel<serde_json::Value>,
) -> Res<serde_json::Value> {
    let conn = state.remotes.get(&master)?;
    blocking(move || conn.call(&cmd, args, streams.then_some(on_event))).await
}

/// The masters this app is connected to (a new window joins them).
#[tauri::command]
fn remote_list(state: State<AppState>) -> Vec<String> {
    state.remotes.connected()
}

#[tauri::command]
fn remote_disconnect(state: State<AppState>, id: String) {
    state.remotes.disconnect(&id);
}

#[tauri::command]
fn inspector_close(state: State<AppState>) {
    state.inspector.close();
}

fn optional_project(state: &AppState, project_id: &Option<String>) -> Res<Option<PathBuf>> {
    match project_id {
        Some(id) if !id.is_empty() => Ok(Some(project_root(state, id)?)),
        _ => Ok(None),
    }
}

fn home() -> Res<PathBuf> {
    agent_config::home_dir().ok_or_else(|| "Your home folder could not be found.".to_string())
}

#[tauri::command]
fn agent_config_list(
    state: State<AppState>,
    provider: String,
    project_id: Option<String>,
) -> Res<Vec<agent_config::ConfigFile>> {
    let project = optional_project(&state, &project_id)?;
    agent_config::list(&provider, &home()?, project.as_deref())
}

#[tauri::command]
fn agent_config_read(
    state: State<AppState>,
    provider: String,
    id: String,
    project_id: Option<String>,
) -> Res<files::FileContent> {
    let project = optional_project(&state, &project_id)?;
    agent_config::read(&provider, &id, &home()?, project.as_deref())
}

#[tauri::command]
fn agent_config_write(
    state: State<AppState>,
    provider: String,
    id: String,
    project_id: Option<String>,
    text: String,
    expected_hash: Option<String>,
) -> Res<String> {
    let project = optional_project(&state, &project_id)?;
    agent_config::write(
        &provider,
        &id,
        &home()?,
        project.as_deref(),
        &text,
        expected_hash.as_deref(),
    )
}

#[tauri::command]
fn runs_save(
    state: State<AppState>,
    project_id: String,
    id: String,
    file: String,
    status: String,
    data: String,
) -> Res<()> {
    projects::get(&state.db, &project_id)?;
    runs::save(&state.db, &project_id, &id, &file, &status, &data)
}

#[tauri::command]
fn runs_list_all(state: State<AppState>, limit: usize) -> Res<Vec<runs::RunRecord>> {
    runs::list_all(&state.db, limit)
}

#[tauri::command]
fn agent_usage_since(state: State<AppState>, since: i64) -> Res<agent::Usage> {
    agent::usage_since(&state.db, since)
}

#[tauri::command]
fn runs_list(state: State<AppState>, project_id: String) -> Res<Vec<runs::RunRecord>> {
    runs::list(&state.db, &project_id)
}

#[tauri::command]
fn agent_respond(
    state: State<AppState>,
    session_id: String,
    request_id: String,
    allow: bool,
    remember: Option<String>,
) -> Res<()> {
    state.agents.respond(
        &state.db,
        &session_id,
        &request_id,
        allow,
        remember.as_deref().unwrap_or(""),
    )
}

#[tauri::command]
fn agent_grants(state: State<AppState>, session_id: String) -> Vec<agent::Grant> {
    state.agents.grants(&session_id)
}

#[tauri::command]
fn agent_forget_grants(state: State<AppState>, session_id: String) {
    state.agents.forget_grants(&session_id);
}

#[tauri::command]
fn agent_stop(state: State<AppState>, session_id: String) -> Res<()> {
    agent::mark_stopped(&state.db, &session_id);
    state.agents.stop(&session_id);
    Ok(())
}

#[tauri::command]
fn agent_rename(state: State<AppState>, session_id: String, title: String) -> Res<()> {
    agent::rename(&state.db, &session_id, &title)
}

#[tauri::command]
fn agent_delete(state: State<AppState>, session_id: String) -> Res<()> {
    state.agents.stop(&session_id);
    agent::delete(&state.db, &session_id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Started by Qanuvi's own restart: the old instance closes first, so nothing it holds open is copied.
    if let Some(pid) = crate::data_location::after_pid(std::env::args()) {
        crate::data_location::wait_for_exit(pid, std::time::Duration::from_secs(30));
    }
    let defaults = crate::data_location::Defaults::real();
    let data = crate::data_location::resolve(&defaults);
    if data != defaults.base
        && let Some(web) = defaults.webview_for(&data)
    {
        // SAFETY: start-up, before the window or any other thread exists. WebView2 reads this when it creates the
        // window's storage, so the settings and layout live with the rest of the data.
        unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &web) };
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let dir = data.clone();
            paths::set_data_dir(dir.clone());
            std::fs::create_dir_all(&dir)?;
            let db = store::Db::open(&dir.join("qanuvi.db"))?;
            agent::mark_interrupted(&db);
            runs::mark_interrupted(&db);
            app.manage(AppState {
                db,
                terminals: terminal::Terminals::default(),
                agents: agent::Agents::default(),
                indexes: index::Indexes::default(),
                inspector: preview::Inspector::default(),
                remotes: crate::remote::Remotes::default(),
                accounts: crate::account::Accounts::default(),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event
                && let Some(state) = window.try_state::<AppState>()
            {
                // The other windows stop showing its project as open elsewhere.
                let _ = tauri::Emitter::emit(
                    window.app_handle(),
                    "qanuvi-window-project",
                    serde_json::json!({"label": window.label(), "projectId": null}),
                );
                // A second window takes only its own terminals with it; the last window ends everything.
                let others = window
                    .app_handle()
                    .webview_windows()
                    .keys()
                    .any(|l| l != window.label());
                if others {
                    state.terminals.kill_owner(window.label());
                    return;
                }
                state.terminals.kill_all();
                state.agents.stop_all();
                state.inspector.close();
                state.remotes.disconnect_all();
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            browser_agent_view,
            browser_agent_shot,
            inspector_show,
            inspector_scroll,
            inspector_pick,
            inspector_page_text,
            inspector_close,
            remote_connect,
            remote_call,
            remote_disconnect,
            remote_list,
            remote_pair,
            model_key_set,
            model_key_has,
            models_list,
            brain_records,
            model_key_check,
            machine_info,
            ollama_pull,
            local_server_status,
            local_server_start,
            omniroute_connect,
            omniroute_connections,
            core_decide,
            knowledge_fetch,
            attachment_file,
            attachment_page,
            data_location_info,
            data_location_move,
            data_location_cancel,
            data_location_remove_old,
            app_restart,
            setup_tools,
            setup_install,
            machine_room,
            omniroute_catalog,
            omniroute_add_key,
            omniroute_test,
            omniroute_sign_in,
            omniroute_make_combo,
            model_probe,
            remote_unpair,
            memory_list,
            memory_save,
            memory_delete,
            memory_slice,
            memory_search,
            memory_disk_device,
            account_cancel,
            account_sign_in,
            account_me,
            account_sign_out,
            memory_disk_passphrase_set,
            memory_disk_passphrase_saved,
            memory_disk_passphrase_forget,
            memory_disk_sync,
            memory_disk_versions,
            memory_disk_restore,
            startup_path,
            projects_list,
            projects_add,
            projects_remove,
            projects_update,
            projects_touch,
            projects_redetect,
            files_list_dir,
            files_list_all,
            files_read,
            files_write,
            files_search,
            index_find,
            checks_run,
            index_outline,
            term_shells,
            term_spawn,
            term_write,
            term_resize,
            term_kill,
            git_status,
            git_diff,
            git_stage,
            git_revert,
            git_versions,
            git_set_index,
            git_unstage,
            git_stage_all,
            git_commit,
            git_sync,
            git_branches,
            git_switch,
            git_stashes,
            git_stash,
            git_log,
            git_show,
            git_blame,
            agent_info,
            agent_sessions,
            agent_session_events,
            agent_start,
            agent_respond,
            runs_save,
            runs_list,
            runs_list_all,
            agent_usage_since,
            agent_config_list,
            agent_config_read,
            agent_config_write,
            agent_stop,
            agent_grants,
            agent_forget_grants,
            agent_rename,
            agent_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Qanuvi");
}

#[cfg(test)]
mod tests {
    /// Saves, reads and deletes a test credential in the system store. Runs only with QANUVI_TEST_KEYRING=1.
    #[test]
    fn the_passphrase_goes_to_the_system_credential_store_and_back() {
        if std::env::var("QANUVI_TEST_KEYRING").is_err() {
            return;
        }
        let e = super::keyring_entry("Qanuvi-test").unwrap();
        e.set_password("a test passphrase").unwrap();
        assert_eq!(
            super::keyring_entry("Qanuvi-test")
                .unwrap()
                .get_password()
                .unwrap(),
            "a test passphrase"
        );
        e.delete_credential().unwrap();
        assert!(
            super::keyring_entry("Qanuvi-test")
                .unwrap()
                .get_password()
                .is_err()
        );
    }

    use super::*;

    #[test]
    fn app_info_reports_the_package_version_and_platform() {
        let info = AppInfo::current();
        assert_eq!(info.name, "Qanuvi");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(info.os, std::env::consts::OS);
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn startup_folder_takes_the_first_existing_folder_argument() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("site")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let args = |list: &[&str]| {
            list.iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .into_iter()
        };
        let found =
            startup_folder(args(&["qanuvi", "--flag", "file.txt", "site"]), dir.path()).unwrap();
        assert!(found.ends_with("site"), "{found}");
        assert!(startup_folder(args(&["qanuvi"]), dir.path()).is_none());
        assert!(startup_folder(args(&["qanuvi", "missing"]), dir.path()).is_none());
        assert!(startup_folder(args(&["qanuvi", "."]), dir.path()).is_some());
    }

    #[test]
    fn app_info_serialises_for_the_interface() {
        let json = serde_json::to_value(AppInfo::current()).unwrap();
        for key in ["name", "version", "os", "arch"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
