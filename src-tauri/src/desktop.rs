//! The desktop app (Tauri): the commands its window calls, registered in [`run`]. The core modules are in lib.rs.

use crate::{
    agent, agent_config, checks, files, git, inbox, index, paths, preview, projects, runs, store,
    terminal,
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
    languages: crate::lsp::Servers,
    debuggers: crate::dap::Adapters,
    agents: agent::Agents,
    indexes: index::Indexes,
    inspector: preview::Inspector,
    remotes: crate::remote::Remotes,
    accounts: crate::account::Accounts,
    startup: crate::startup::Startup,
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
    state.languages.stop_project(&id);
    state.debuggers.stop_project(&id);
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

/// Files modified since a moment (milliseconds since 1970): what a plan step changed.
#[tauri::command]
async fn files_changed_since(
    state: State<'_, AppState>,
    project_id: String,
    since_ms: u64,
) -> Res<Vec<String>> {
    let root = project_root(&state, &project_id)?;
    let since = std::time::UNIX_EPOCH + std::time::Duration::from_millis(since_ms);
    blocking(move || files::changed_since(&root, since, 200)).await
}

/// Qanuvi's own record of the project's files before a piece of work (git or not). `also`: folders followed even
/// where ignore rules leave them out (the plan's folder).
#[tauri::command]
async fn snapshots_take(
    state: State<'_, AppState>,
    project_id: String,
    also: Vec<String>,
) -> Res<crate::snapshots::Taken> {
    let root = project_root(&state, &project_id)?;
    blocking(move || crate::snapshots::take(&root, &also)).await
}

/// What changed since a snapshot, with each file's change; `git`: the user chose git's diffs where git sees the file.
/// `skip`: a path left out (the plan file itself).
#[tauri::command]
async fn snapshots_changes(
    state: State<'_, AppState>,
    project_id: String,
    before: String,
    also: Vec<String>,
    git: bool,
    skip: Option<String>,
) -> Res<crate::snapshots::Changes> {
    let root = project_root(&state, &project_id)?;
    blocking(move || {
        let skip = skip.unwrap_or_default();
        crate::snapshots::changes(&root, &before, &also, git, &|p| {
            !skip.is_empty() && p == skip
        })
    })
    .await
}

/// Adds to a plan step's log (under .qanuvi/plan-logs).
#[tauri::command]
async fn files_log_append(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    text: String,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::log_append(&root, &path, &text)).await
}

/// Every Markdown file in the project, ignore rules or not: where the Plan view finds task lists.
#[tauri::command]
async fn files_markdown(state: State<'_, AppState>, project_id: String) -> Res<Vec<String>> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::markdown_files(&root)).await
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

/// Renames or moves a file or folder inside the project.
#[tauri::command]
async fn files_rename(
    state: State<'_, AppState>,
    project_id: String,
    from: String,
    to: String,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::rename(&root, &from, &to)).await
}

/// Moves a file or folder to the Recycle Bin.
#[tauri::command]
async fn files_trash(state: State<'_, AppState>, project_id: String, path: String) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::trash(&root, &path)).await
}

#[tauri::command]
async fn files_create(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    folder: bool,
) -> Res<()> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::create(&root, &path, folder)).await
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
    options: Option<files::SearchOptions>,
) -> Res<files::SearchResult> {
    let root = project_root(&state, &project_id)?;
    let o = options.unwrap_or(files::SearchOptions {
        regex,
        case_sensitive,
        ..Default::default()
    });
    blocking(move || files::search_with(&root, &query, &o, 5000)).await
}

/// Replaces the matches of a search in the project's files (only in `paths` when given).
#[tauri::command]
async fn files_replace(
    state: State<'_, AppState>,
    project_id: String,
    query: String,
    options: files::SearchOptions,
    replacement: String,
    paths: Option<Vec<String>>,
) -> Res<files::ReplaceResult> {
    let root = project_root(&state, &project_id)?;
    blocking(move || files::replace_with(&root, &query, &options, &replacement, paths.as_deref()))
        .await
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
    let env = crate::secrets::env(&project_id);
    blocking(move || checks::run_with_env(&root, &command, timeout, &env)).await
}

/// The names of a project's secrets (never the values).
#[tauri::command]
fn secrets_names(project_id: String) -> Vec<String> {
    crate::secrets::names(&project_id)
}

#[tauri::command]
fn secrets_set(project_id: String, name: String, value: String) -> Res<()> {
    crate::secrets::set(&project_id, &name, &value)
}

/// Start with Qanuvi: starts the enabled commands that are not running yet, and answers how each one is.
#[tauri::command]
async fn startup_start(
    state: State<'_, AppState>,
    items: Vec<crate::startup::Item>,
) -> Res<Vec<crate::startup::Status>> {
    state.startup.start_all(&items);
    Ok(state.startup.statuses())
}

#[tauri::command]
fn startup_status(state: State<AppState>) -> Vec<crate::startup::Status> {
    state.startup.statuses()
}

#[tauri::command]
fn startup_restart(
    state: State<AppState>,
    item: crate::startup::Item,
) -> Vec<crate::startup::Status> {
    state.startup.restart(&item);
    state.startup.statuses()
}

#[tauri::command]
fn startup_stop(state: State<AppState>, id: String) -> Vec<crate::startup::Status> {
    state.startup.stop(&id);
    state.startup.statuses()
}

/// What Qanuvi uses in all: itself and every process it started.
#[tauri::command]
fn qanuvi_usage() -> Option<crate::usage::Usage> {
    crate::usage::usage()
}

/// Claude Code's installed and newest versions.
#[tauri::command]
async fn claude_versions() -> Res<crate::claude_update::Versions> {
    blocking(crate::claude_update::versions).await
}

/// Updates Claude Code through its own `claude update`.
#[tauri::command]
async fn claude_update() -> Res<String> {
    blocking(crate::claude_update::update).await
}

/// Where Headroom is installed, if it is (for the one-click setup).
#[tauri::command]
fn startup_headroom() -> Option<String> {
    crate::terminal::find_on_path("headroom").map(|p| p.to_string_lossy().into_owned())
}

/// The proxy address Claude Code's own user settings send it to (headroom wrap sets one), if any: then every Claude
/// Code, Qanuvi's too, goes through it already.
#[tauri::command]
fn claude_settings_base_url() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let text = std::fs::read_to_string(
        std::path::Path::new(&home)
            .join(".claude")
            .join("settings.json"),
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["env"]["ANTHROPIC_BASE_URL"].as_str().map(str::to_string)
}

/// A plan's private details (the plan run puts them into each step's request).
#[tauri::command]
fn plan_private(project_id: String, file: String) -> Vec<String> {
    crate::secrets::plan_private(&project_id, &file)
}

#[tauri::command]
fn plan_private_add(project_id: String, file: String, text: String) -> Res<()> {
    crate::secrets::plan_private_add(&project_id, &file, &text)
}

#[tauri::command]
fn plan_private_clear(project_id: String, file: String) -> Res<()> {
    crate::secrets::plan_private_clear(&project_id, &file)
}

#[tauri::command]
fn secrets_remove(project_id: String, name: String) -> Res<()> {
    crate::secrets::remove(&project_id, &name)
}

// Terminals -------------------------------------------------------------------------------------------------------

#[tauri::command]
fn term_shells() -> Vec<terminal::Shell> {
    terminal::shells()
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn term_spawn(
    window: tauri::Window,
    state: State<AppState>,
    project_id: String,
    shell: String,
    cols: u16,
    rows: u16,
    dir: Option<String>,
    on_event: Channel<terminal::TermEvent>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    let cwd = files::folder_in(&root, dir.as_deref())?;
    let id = state
        .terminals
        .spawn(&project_id, &cwd, &shell, cols, rows, on_event.into())?;
    state.terminals.set_owner(&id, window.label());
    Ok(id)
}

/// Stops the terminals of this window (before its interface reloads; they start again afterwards).
#[tauri::command]
fn term_kill_window(window: tauri::Window, state: State<AppState>) {
    state.terminals.kill_owner(window.label());
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

// Language servers (15i) ------------------------------------------------------------------------------------

#[tauri::command]
async fn lsp_statuses() -> Res<Vec<crate::lsp::Status>> {
    blocking(|| Ok(crate::lsp::statuses())).await
}

/// Installs a server; the window asked the user first.
#[tauri::command]
async fn lsp_install(id: String) -> Res<crate::lsp::Status> {
    let spec = crate::lsp::spec(&id)?;
    blocking(move || crate::lsp::install(spec)).await
}

#[tauri::command]
async fn lsp_start(
    window: tauri::Window,
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    on_event: Channel<crate::lsp::LspEvent>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    let servers = state.languages.clone();
    let owner = window.label().to_string();
    blocking(move || servers.start(&project_id, &owner, &root, &id, on_event.into())).await
}

#[tauri::command]
fn lsp_send(state: State<AppState>, key: String, text: String) -> Res<()> {
    state.languages.send(&key, &text)
}

#[tauri::command]
fn lsp_stop(state: State<AppState>, key: String) {
    state.languages.stop(&key);
}

/// Stops this window's servers (before its interface reloads).
#[tauri::command]
fn lsp_stop_window(window: tauri::Window, state: State<AppState>) {
    state.languages.stop_owner(window.label());
}

// Debug adapters (15i part 3) -----------------------------------------------------------------------------

#[tauri::command]
async fn dap_statuses() -> Res<Vec<crate::dap::Status>> {
    blocking(|| Ok(crate::dap::statuses())).await
}

/// PHP on this computer and its Xdebug (with the Xdebug file chosen in Settings).
#[tauri::command]
async fn dap_php(xdebug_file: String) -> Res<crate::dap::PhpInfo> {
    blocking(move || Ok(crate::dap::php_info(&xdebug_file))).await
}

/// Installs an adapter; the window asked the user first.
#[tauri::command]
async fn dap_install(id: String) -> Res<crate::dap::Status> {
    let spec = crate::dap::spec(&id)?;
    blocking(move || crate::dap::install(spec)).await
}

#[tauri::command]
async fn dap_start(
    window: tauri::Window,
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    on_event: Channel<crate::dap::DapEvent>,
) -> Res<String> {
    let root = project_root(&state, &project_id)?;
    let adapters = state.debuggers.clone();
    let owner = window.label().to_string();
    blocking(move || adapters.start(&project_id, &owner, &root, &id, on_event.into())).await
}

#[tauri::command]
async fn dap_connect(state: State<'_, AppState>, key: String) -> Res<u32> {
    let adapters = state.debuggers.clone();
    blocking(move || adapters.connect(&key)).await
}

#[tauri::command]
fn dap_send(state: State<AppState>, key: String, conn: u32, text: String) -> Res<()> {
    state.debuggers.send(&key, conn, &text)
}

#[tauri::command]
fn dap_stop(state: State<AppState>, key: String) {
    state.debuggers.stop(&key);
}

/// Stops this window's debuggers (before its interface reloads).
#[tauri::command]
fn dap_stop_window(window: tauri::Window, state: State<AppState>) {
    state.debuggers.stop_owner(window.label());
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

/// The agent sessions of a project that changed a file (the project panel's Timeline).
#[tauri::command]
fn agent_file_history(
    state: State<AppState>,
    project_id: String,
    path: String,
) -> Res<Vec<agent::FileTouch>> {
    agent::file_history(&state.db, &project_id, &path)
}

#[tauri::command]
fn agent_session_events(state: State<AppState>, session_id: String) -> Res<Vec<String>> {
    agent::session_events(&state.db, &session_id)
}

#[tauri::command]
fn agent_session_times(state: State<AppState>, session_id: String) -> Res<Vec<i64>> {
    agent::session_event_times(&state.db, &session_id)
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

/// Whether this is a test build from the source tree (it keeps its own data; the title bar says so).
#[tauri::command]
fn data_test_build() -> bool {
    crate::data_location::from_source()
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

/// Backups (1d): where they go, whether daily backups are on, the last one, the list, and a restore booked.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupsInfo {
    data: String,
    dir: String,
    default_dir: String,
    custom: bool,
    off: bool,
    last_at: u64,
    last_error: Option<String>,
    keep: usize,
    restore: Option<String>,
    backups: Vec<crate::backups::Entry>,
}

fn backups_webview() -> Option<std::path::PathBuf> {
    crate::data_location::Defaults::real().webview_for(&crate::paths::data_dir())
}

#[tauri::command]
async fn backups_info() -> Res<BackupsInfo> {
    blocking(|| {
        let data = crate::paths::data_dir();
        let test = crate::data_location::from_source();
        let c = crate::backups::read_config(&data);
        let dir = crate::backups::folder(&data, test);
        let loc = crate::data_location::read(&crate::data_location::Defaults::real().base);
        let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
        Ok(BackupsInfo {
            data: s(&data),
            dir: s(&dir),
            default_dir: s(&crate::backups::default_dir(test)),
            custom: c.dir.is_some(),
            off: c.off,
            last_at: c.last_at,
            last_error: c.last_error,
            keep: crate::backups::KEEP,
            restore: loc.restore.as_deref().map(s),
            backups: crate::backups::list(&dir),
        })
    })
    .await
}

/// Sets the backups folder (empty: the default one) and whether daily backups run.
#[tauri::command]
async fn backups_set(folder: String, off: bool) -> Res<()> {
    blocking(move || {
        let data = crate::paths::data_dir();
        let mut c = crate::backups::read_config(&data);
        let folder = folder.trim();
        c.dir = if folder.is_empty() {
            None
        } else {
            let p = std::path::PathBuf::from(folder);
            if !p.is_absolute() {
                return Err("Choose a full folder path.".into());
            }
            if p.starts_with(&data) {
                return Err("The backups folder cannot be inside Qanuvi's data folder.".into());
            }
            std::fs::create_dir_all(&p)
                .map_err(|e| format!("{} cannot be created: {e}", p.display()))?;
            Some(p)
        };
        c.off = off;
        crate::backups::write_config(&data, &c)
    })
    .await
}

/// Backs up now. Answers the new backup's folder.
#[tauri::command]
async fn backups_now() -> Res<String> {
    blocking(|| {
        let web = backups_webview();
        crate::backups::run_due(
            &crate::paths::data_dir(),
            web.as_deref(),
            crate::data_location::from_source(),
            true,
        )?
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "No backup was made.".to_string())
    })
    .await
}

/// Books a backup to be restored at the next start (the window asked the user first).
#[tauri::command]
async fn backups_restore(path: String) -> Res<()> {
    blocking(move || {
        let data = crate::paths::data_dir();
        crate::backups::plan_restore(
            &crate::data_location::Defaults::real().base,
            &crate::backups::folder(&data, crate::data_location::from_source()),
            std::path::Path::new(&path),
        )
    })
    .await
}

#[tauri::command]
fn backups_cancel_restore() -> Res<()> {
    crate::backups::cancel_restore(&crate::data_location::Defaults::real().base)
}

/// Removes one backup (the window asked the user first).
#[tauri::command]
async fn backups_remove(path: String) -> Res<()> {
    blocking(move || {
        let data = crate::paths::data_dir();
        crate::backups::remove(
            &crate::backups::folder(&data, crate::data_location::from_source()),
            std::path::Path::new(&path),
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

/// An image sent earlier in a conversation (by its stored name), for the window to show.
#[tauri::command]
async fn attachment_image(file: String) -> Res<String> {
    blocking(move || {
        crate::attachments::stored_image(&crate::paths::data_dir().join("attachments"), &file)
    })
    .await
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

/// Removes a connection from OmniRoute (asked for and confirmed in the window).
#[tauri::command]
async fn omniroute_remove(endpoint: crate::llm::Endpoint, id: String) -> Res<()> {
    blocking(move || crate::omniroute::remove(&endpoint, &id)).await
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

// The Inbox (24u part 4): every change is told to every window ("qanuvi-inbox"), so they all show the same list.
fn inbox_changed(app: &tauri::AppHandle, change: &str, item: Option<&inbox::InboxItem>) {
    let _ = tauri::Emitter::emit(
        app,
        "qanuvi-inbox",
        serde_json::json!({"change": change, "item": item}),
    );
}

// Desktop notifications (24u part 4, session B): the window sends the owner's settings; a click that started Qanuvi
// waits until the window asks for it.
#[tauri::command]
fn notify_settings(
    notifier: State<crate::desktop_notify::Notifier>,
    settings: crate::notify::NotifySettings,
) {
    notifier.set(settings);
}

#[tauri::command]
fn notify_take_start(
    notifier: State<crate::desktop_notify::Notifier>,
) -> Option<serde_json::Value> {
    notifier
        .take_start()
        .map(|id| serde_json::json!({ "id": id }))
}

#[tauri::command]
fn notify_test(app: tauri::AppHandle) -> Res<()> {
    crate::desktop_notify::test(&app)
}

#[tauri::command]
fn inbox_list(state: State<AppState>) -> Res<Vec<inbox::InboxItem>> {
    inbox::list(&state.db)
}

#[tauri::command]
fn inbox_add(
    app: tauri::AppHandle,
    state: State<AppState>,
    item: inbox::NewItem,
) -> Res<inbox::InboxItem> {
    let (added, new) = inbox::add_new(&state.db, &item)?;
    inbox_changed(&app, "add", Some(&added));
    if new {
        crate::desktop_notify::new_item(&app, &added);
    }
    Ok(added)
}

#[tauri::command]
fn inbox_answer(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<AppState>,
    id: Option<String>,
    key: Option<String>,
    answer: serde_json::Value,
) -> Res<Option<inbox::InboxItem>> {
    let by = window.label().to_string();
    let done = match (id, key) {
        (Some(id), _) => inbox::answer(&state.db, &id, &answer, &by)?,
        (None, Some(key)) => inbox::answer_key(&state.db, &key, &answer, &by)?,
        _ => return Err("Which question?".into()),
    };
    if let Some(item) = &done {
        inbox_changed(&app, "answer", Some(item));
    }
    Ok(done)
}

#[tauri::command]
fn inbox_close(app: tauri::AppHandle, state: State<AppState>, id: String) -> Res<bool> {
    let closed = inbox::close(&state.db, &id)?;
    if closed {
        inbox_changed(&app, "close", inbox::get(&state.db, &id)?.as_ref());
    }
    Ok(closed)
}

#[tauri::command]
fn inbox_resolve(app: tauri::AppHandle, state: State<AppState>, key: String) -> Res<bool> {
    let gone = inbox::resolve(&state.db, &key)?;
    if gone {
        inbox_changed(&app, "remove", None);
    }
    Ok(gone)
}

#[tauri::command]
fn inbox_mark_read(app: tauri::AppHandle, state: State<AppState>, id: Option<String>) -> Res<bool> {
    let changed = inbox::mark_read(&state.db, id.as_deref())?;
    if changed {
        inbox_changed(&app, "read", None);
    }
    Ok(changed)
}

#[tauri::command]
fn inbox_remove(app: tauri::AppHandle, state: State<AppState>, id: String) -> Res<bool> {
    let gone = inbox::remove(&state.db, &id)?;
    if gone {
        inbox_changed(&app, "remove", None);
    }
    Ok(gone)
}

#[tauri::command]
fn inbox_clear(app: tauri::AppHandle, state: State<AppState>) -> Res<bool> {
    let gone = inbox::clear(&state.db)?;
    if gone {
        inbox_changed(&app, "remove", None);
    }
    Ok(gone)
}

#[tauri::command]
fn inbox_import(
    app: tauri::AppHandle,
    state: State<AppState>,
    items: Vec<inbox::OldItem>,
) -> Res<usize> {
    let n = inbox::import(&state.db, &items)?;
    if n > 0 {
        inbox_changed(&app, "add", None);
    }
    Ok(n)
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

/// A running session's model or mode, changed now from the picker.
#[tauri::command]
fn agent_configure(
    state: State<AppState>,
    session_id: String,
    model: Option<String>,
    mode: Option<String>,
) -> Res<()> {
    state
        .agents
        .configure(&state.db, &session_id, model.as_deref(), mode.as_deref())
}

/// "Stop this command, read my message now": ends the agent's turn, keeps the session (24r).
#[tauri::command]
fn agent_interrupt(state: State<AppState>, session_id: String) -> Res<()> {
    state.agents.interrupt(&state.db, &session_id)
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
fn agent_link(state: State<AppState>, session_id: String, from: String) -> Res<()> {
    agent::link(&state.db, &session_id, &from)
}

#[tauri::command]
fn agent_handover_off(state: State<AppState>, session_id: String, off: bool) -> Res<()> {
    agent::set_handover_off(&state.db, &session_id, off)
}

#[tauri::command]
fn agent_saved(state: State<AppState>, session_id: String, saved: bool) -> Res<()> {
    agent::set_saved(&state.db, &session_id, saved)
}

#[tauri::command]
fn agent_delete(state: State<AppState>, session_id: String) -> Res<()> {
    state.agents.stop(&session_id);
    agent::delete(&state.db, &session_id)
}

/// Chat clean-up (24t): deletes chats not used for `days` days, keeping the chats in `keep` (open now).
#[tauri::command]
fn agent_clean_up(state: State<AppState>, days: i64, keep: Vec<String>) -> Res<usize> {
    agent::clean_up_chats(&state.db, days, &keep, store::now())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Started by Qanuvi's own restart: the old instance closes first, so nothing it holds open is copied.
    if let Some(pid) = crate::data_location::after_pid(std::env::args()) {
        crate::data_location::wait_for_exit(pid, std::time::Duration::from_secs(30));
    }
    let defaults = crate::data_location::Defaults::real();
    if crate::data_location::from_source()
        && let Some(note) = crate::data_location::seed_from_installed(
            &defaults,
            &crate::data_location::Defaults::installed(),
        )
    {
        eprintln!("{note}");
    }
    let data = crate::data_location::resolve(&defaults);
    // A restore booked in Settings, Backups: applied now, before the database and the window open.
    if let Some(note) = crate::backups::apply_pending(
        &defaults.base,
        &data,
        defaults.webview_for(&data).as_deref(),
        crate::data_location::from_source(),
    ) {
        eprintln!("{note}");
    }
    // A test build sets it always: WebView2's own default is the installed copy's folder (named after the app id),
    // which a test build must never share.
    if (data != defaults.base || crate::data_location::from_source())
        && let Some(web) = defaults.webview_for(&data)
    {
        // SAFETY: start-up, before the window or any other thread exists. WebView2 reads this when it creates the
        // window's storage, so the settings and layout live with the rest of the data.
        unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &web) };
    }
    // One Qanuvi at a time when installed: starting it again (or a notification's qanuvi:// link) reaches the
    // running one. Not for test builds, which run beside the installed copy. A restart waits for the old instance
    // above, so its lock is free by now.
    let mut builder = tauri::Builder::default();
    if !crate::data_location::from_source() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            crate::desktop_notify::second_start(app, &args);
        }));
    }
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let dir = data.clone();
            paths::set_data_dir(dir.clone());
            std::fs::create_dir_all(&dir)?;
            let db = store::Db::open(&dir.join("qanuvi.db"))?;
            agent::mark_interrupted(&db);
            runs::mark_interrupted(&db);
            app.manage(crate::desktop_notify::Notifier::default());
            let args: Vec<String> = std::env::args().collect();
            crate::desktop_notify::started_with(app.handle(), &args);
            // macOS hands a clicked qanuvi:// link to the running app instead of starting it with the link.
            #[cfg(target_os = "macos")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |e| {
                    for url in e.urls() {
                        crate::desktop_notify::opened_url(&handle, url.as_str());
                    }
                });
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    for url in urls {
                        crate::desktop_notify::opened_url(app.handle(), url.as_str());
                    }
                }
            }
            app.manage(AppState {
                db,
                terminals: terminal::Terminals::default(),
                languages: crate::lsp::Servers::default(),
                debuggers: crate::dap::Adapters::default(),
                agents: agent::Agents::default(),
                indexes: index::Indexes::default(),
                inspector: preview::Inspector::default(),
                remotes: crate::remote::Remotes::default(),
                accounts: crate::account::Accounts::default(),
                startup: crate::startup::Startup::default(),
            });
            // The daily backup (1d): checked 2 minutes after start (start-up is busy enough), then every hour; made
            // when the last good one is a day old.
            let backup_dir = dir.clone();
            let backup_web = crate::data_location::Defaults::real().webview_for(&dir);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(120));
                loop {
                    if let Err(e) = crate::backups::run_due(
                        &backup_dir,
                        backup_web.as_deref(),
                        crate::data_location::from_source(),
                        false,
                    ) {
                        eprintln!("Backup failed: {e}");
                    }
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            });
            // Agent processes of sessions idle for 10 minutes are closed (memory); a new message opens them again.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    if let Some(state) = handle.try_state::<AppState>() {
                        state.agents.close_idle(&state.db, 600);
                    }
                }
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
                    state.languages.stop_owner(window.label());
                    state.debuggers.stop_owner(window.label());
                    return;
                }
                state.terminals.kill_all();
                state.languages.stop_all();
                state.debuggers.stop_all();
                state.agents.stop_all();
                // Only what Qanuvi started at start-up (a proxy it found already running stays).
                state.startup.stop_all();
                state.inspector.close();
                state.remotes.disconnect_all();
            }
        })
        .invoke_handler(tauri::generate_handler![
            notify_settings,
            notify_take_start,
            notify_test,
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
            omniroute_remove,
            core_decide,
            knowledge_fetch,
            attachment_file,
            attachment_page,
            attachment_image,
            data_location_info,
            data_test_build,
            data_location_move,
            data_location_cancel,
            data_location_remove_old,
            backups_info,
            backups_set,
            backups_now,
            backups_restore,
            backups_cancel_restore,
            backups_remove,
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
            files_markdown,
            files_log_append,
            files_changed_since,
            snapshots_take,
            snapshots_changes,
            files_replace,
            files_read,
            files_write,
            files_create,
            files_rename,
            files_trash,
            files_search,
            index_find,
            checks_run,
            index_outline,
            term_shells,
            term_spawn,
            term_write,
            term_kill_window,
            term_resize,
            term_kill,
            lsp_statuses,
            lsp_install,
            lsp_start,
            lsp_send,
            lsp_stop,
            lsp_stop_window,
            dap_statuses,
            dap_php,
            dap_install,
            dap_start,
            dap_connect,
            dap_send,
            dap_stop,
            dap_stop_window,
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
            agent_session_times,
            agent_file_history,
            agent_start,
            agent_respond,
            runs_save,
            runs_list,
            runs_list_all,
            inbox_list,
            inbox_add,
            inbox_answer,
            inbox_close,
            inbox_resolve,
            inbox_mark_read,
            inbox_remove,
            inbox_clear,
            inbox_import,
            agent_usage_since,
            agent_config_list,
            agent_config_read,
            agent_config_write,
            agent_stop,
            agent_interrupt,
            agent_configure,
            secrets_names,
            secrets_set,
            secrets_remove,
            plan_private,
            startup_start,
            startup_status,
            startup_restart,
            startup_stop,
            startup_headroom,
            claude_versions,
            qanuvi_usage,
            claude_update,
            claude_settings_base_url,
            plan_private_add,
            plan_private_clear,
            agent_grants,
            agent_forget_grants,
            agent_rename,
            agent_link,
            agent_handover_off,
            agent_saved,
            agent_delete,
            agent_clean_up,
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
