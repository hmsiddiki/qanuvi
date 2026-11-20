//! The desktop app (Tauri): the window's own commands, registered in [`run`]. Since migration M5a the window is a
//! viewer: everything about This PC's work goes to This PC's master over the "this-computer" link (`remote_call`),
//! as for any paired master; what is left here is the screen's own (notifications, the inspector, coding tools, the
//! browser half of a sign-in, the data folder, the links to masters).

use crate::{agent, preview, projects};

use serde::Serialize;
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
    languages: crate::lsp::Servers,
    debuggers: crate::dap::Adapters,
    inspector: preview::Inspector,
    remotes: crate::remote::Remotes,
    accounts: crate::account::Accounts,
}

type Res<T> = Result<T, String>;

use crate::remote::THIS_PC;

/// One connect at a time to This PC's master, so two windows asking at once never start two links.
static THIS_PC_CONNECT: std::sync::Mutex<()> = std::sync::Mutex::new(());

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo::current()
}

/// u2: this screen's half of Qanuvi's use (the window, its WebView2 processes and link helpers), measured here
/// whatever master is shown, every master's service left out; the master's half comes from the master
/// (qanuvi_usage).
#[tauri::command]
async fn screen_usage(state: State<'_, AppState>) -> Res<Option<crate::usage::Usage>> {
    let links = state.remotes.link_pids();
    blocking(move || Ok(crate::usage::screen_usage(&links))).await
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

// Language servers (15i) and debug adapters (15i part 3) ------------------------------------------------------

/// The machine a language server or debugger call is about: a connected master, or This PC's own master (migration
/// M5c: the window starts no program itself; This PC's go through the same pipes as every machine's).
fn tools_host(app: &tauri::AppHandle, master: &Option<String>) -> Res<crate::lsp::On> {
    match master.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => crate::lsp::On::new(link_for(app, m)?),
        None => crate::lsp::On::this_pc(link_for(app, THIS_PC)?),
    }
}

#[tauri::command]
async fn lsp_statuses(
    app: tauri::AppHandle,
    master: Option<String>,
) -> Res<Vec<crate::lsp::Status>> {
    blocking(move || Ok(crate::lsp::statuses(&tools_host(&app, &master)?))).await
}

/// Installs a server; the window asked the user first.
#[tauri::command]
async fn lsp_install(
    app: tauri::AppHandle,
    id: String,
    master: Option<String>,
    on_line: Channel<String>,
) -> Res<crate::lsp::Status> {
    let spec = crate::lsp::spec(&id)?;
    // The install's output lines while it runs; nobody listens to them once it has answered.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let d = done.clone();
    let live = crate::sink::Sink::new(move |l: String| {
        !d.load(std::sync::atomic::Ordering::SeqCst) && on_line.send(l).is_ok()
    });
    let r =
        blocking(move || crate::lsp::install(&tools_host(&app, &master)?, spec, Some(live))).await;
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    r
}

/// `project_id` is the master's own id for the project (This PC's master's for a This PC project).
#[tauri::command]
async fn lsp_start(
    window: tauri::Window,
    app: tauri::AppHandle,
    project_id: String,
    id: String,
    master: Option<String>,
    on_event: Channel<crate::lsp::LspEvent>,
) -> Res<String> {
    let servers = app.state::<AppState>().languages.clone();
    let owner = window.label().to_string();
    blocking(move || {
        servers.start_on(
            tools_host(&app, &master)?,
            &project_id,
            &owner,
            &id,
            on_event.into(),
        )
    })
    .await
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

#[tauri::command]
async fn dap_statuses(
    app: tauri::AppHandle,
    master: Option<String>,
) -> Res<Vec<crate::dap::Status>> {
    blocking(move || Ok(crate::dap::statuses(&tools_host(&app, &master)?))).await
}

/// PHP on that machine and its Xdebug (with the Xdebug file chosen in that machine's Settings).
#[tauri::command]
async fn dap_php(
    app: tauri::AppHandle,
    xdebug_file: String,
    master: Option<String>,
) -> Res<crate::dap::PhpInfo> {
    blocking(move || {
        Ok(crate::dap::php_info(
            &tools_host(&app, &master)?,
            &xdebug_file,
        ))
    })
    .await
}

/// Installs an adapter on that machine; the window asked the user first.
#[tauri::command]
async fn dap_install(
    app: tauri::AppHandle,
    id: String,
    master: Option<String>,
) -> Res<crate::dap::Status> {
    let spec = crate::dap::spec(&id)?;
    blocking(move || crate::dap::install(&tools_host(&app, &master)?, spec)).await
}

#[tauri::command]
async fn dap_start(
    window: tauri::Window,
    app: tauri::AppHandle,
    project_id: String,
    id: String,
    master: Option<String>,
    on_event: Channel<crate::dap::DapEvent>,
) -> Res<String> {
    let adapters = app.state::<AppState>().debuggers.clone();
    let owner = window.label().to_string();
    blocking(move || {
        adapters.start_on(
            tools_host(&app, &master)?,
            &project_id,
            &owner,
            &id,
            on_event.into(),
        )
    })
    .await
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

// The preview pane's inspector and an agent's browser picture ---------------------------------------------------

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

// Signing in with a Qlopy site account (Milestone 19).

/// The window's part of any machine's browser sign-in: qanuvi.com's sign-in page opens in this computer's browser
/// (Google, Microsoft or GitHub there); this waits up to 5 minutes for its answer and gives back the one-time code.
#[tauri::command]
async fn account_wait(app: tauri::AppHandle, url: String, state: String) -> Res<String> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_opener::OpenerExt;
        if !url.starts_with("https://")
            && !url.starts_with("http://localhost")
            && !url.starts_with("http://127.0.0.1")
        {
            return Err("That is not a sign-in address.".to_string());
        }
        let st = app2.state::<AppState>();
        st.accounts.catch_code(&url, &state, |u| {
            app2.opener()
                .open_url(u, None::<&str>)
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

/// The memory disk's passphrase in the system's credential store (the test below; the master keeps it now).
#[cfg(test)]
fn keyring_entry(service: &str) -> Res<keyring::Entry> {
    keyring::Entry::new(service, "memory-disk").map_err(|e| e.to_string())
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
            let key = crate::pairing_keys::get(&r.pairing)
                .map_err(|_| "This device's pairing is missing; pair again.".to_string())?;
            crate::remote::Transport::Relay(crate::relay_client::Paired {
                relay: r.url,
                master_id: r.master_id,
                master_name: String::new(),
                pairing: r.pairing,
                key,
            })
        }
        // This computer's own master, on the app's own data (remote access, Milestone 20; This PC itself, M5a).
        "self" => this_pc_transport()?,
        t => crate::remote::Transport::Process(crate::remote::command_for(
            t,
            program.trim(),
            identity.as_deref(),
            &data.join("local-master"),
        )?),
    };
    let _ = &state;
    blocking(move || connect_link(&app, &id, transport)).await
}

/// The way to This PC's master: its program in bridge mode on the app's own data folder.
fn this_pc_transport() -> Res<crate::remote::Transport> {
    Ok(crate::remote::Transport::Process(
        crate::remote::command_for("self", "", None, &crate::paths::data_dir())?,
    ))
}

/// Opens a link and answers the master's hello; its end is told to the windows ("remote-closed").
fn connect_link(
    app: &tauri::AppHandle,
    id: &str,
    transport: crate::remote::Transport,
) -> Res<serde_json::Value> {
    let state = app.state::<AppState>();
    let app2 = app.clone();
    let on_end: crate::remote::OnEnd = std::sync::Arc::new(move |id: &str| {
        // M5b u1: This PC's ended link is let go, so the next call makes it again (its master was restarted by its
        // watcher after a crash, or by Settings). Kept, every call went on the dead link: "The connection to the
        // master has ended". Until one is made again no other can be: the entry is still the ended one.
        if id == THIS_PC {
            let _one = THIS_PC_CONNECT.lock().unwrap_or_else(|e| e.into_inner());
            app2.state::<AppState>().remotes.disconnect(THIS_PC);
        }
        let _ = tauri::Emitter::emit(&app2, "remote-closed", serde_json::json!({"id": id}));
    });
    if id == THIS_PC {
        let _one = THIS_PC_CONNECT.lock().unwrap_or_else(|e| e.into_inner());
        return state.remotes.connect(on_end, id, transport);
    }
    state.remotes.connect(on_end, id, transport)
}

/// The link a call goes on. This PC's is made when first needed and again after it ended (its master restarted).
fn link_for(app: &tauri::AppHandle, master: &str) -> Res<std::sync::Arc<dyn crate::pipes::Link>> {
    let state = app.state::<AppState>();
    match state.remotes.link(master) {
        Ok(l) => Ok(l),
        // M5c part 2: so is the way to one of This PC's servers, which goes through it.
        Err(_)
            if master == THIS_PC
                || (state.remotes.through_for(master).is_some()
                    && state.remotes.get(THIS_PC).is_err()) =>
        {
            connect_link(app, THIS_PC, this_pc_transport()?)?;
            state.remotes.link(master)
        }
        Err(e) => Err(e),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RelayTarget {
    url: String,
    master_id: String,
    pairing: String,
}

/// Whether this is a test build from the source tree (it keeps its own data; the title bar says so).
#[tauri::command]
fn data_test_build() -> bool {
    crate::data_location::from_source()
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

/// The desktop app's default backups folder (a master has none).
fn backups_default() -> std::path::PathBuf {
    crate::backups::default_dir(crate::data_location::from_source())
}

/// Starts Qanuvi again (to finish a move): the new instance waits until this one has closed.
#[tauri::command]
fn app_restart(app: tauri::AppHandle) -> Res<()> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // This PC's master has the data open: a move or a booked restore needs it closed. The new instance starts it again.
    crate::master::stop_service_because(&crate::paths::data_dir(), "restart");
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

/// A file from this computer for the next message (the composer's + menu).
#[tauri::command]
async fn attachment_file(path: String) -> Res<agent::Attachment> {
    blocking(move || crate::attachments::read_file(std::path::Path::new(&path))).await
}

/// A conversation exported as Markdown (24n), saved on this computer where the Save box pointed: like a download, it
/// stays with the screen the user sits at, whichever machine is shown. Only Markdown files.
#[tauri::command]
async fn export_save(path: String, text: String) -> Res<()> {
    blocking(move || {
        let p = std::path::PathBuf::from(&path);
        if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            return Err("An export is saved as a .md file.".into());
        }
        std::fs::write(&p, text).map_err(|e| format!("The export could not be saved: {e}"))
    })
    .await
}

/// Pairs this device with a computer showing a pairing code; the key goes to the credential store.
#[tauri::command]
async fn remote_pair(relay: String, code: String, name: String) -> Res<serde_json::Value> {
    blocking(move || {
        let p = crate::relay_client::pair(&relay, &code, &name)?;
        crate::pairing_keys::set(&p.pairing, &p.key)?;
        Ok(serde_json::json!({"relay": p.relay, "masterId": p.master_id, "masterName": p.master_name, "pairing": p.pairing}))
    })
    .await
}

/// Forgets a pairing on this device.
#[tauri::command]
fn remote_unpair(pairing: String) -> Res<()> {
    crate::pairing_keys::delete(&pairing)
}

/// Passes one of the window's calls to a master: one of This PC's, or one reached through them by its path
/// ("vps1/vps2", migration M3c). With `streams`, events of what it starts (terminal output, agent events) go to
/// `on_event`.
#[tauri::command]
async fn remote_call(
    app: tauri::AppHandle,
    master: String,
    cmd: String,
    args: serde_json::Value,
    streams: bool,
    on_event: Channel<serde_json::Value>,
) -> Res<serde_json::Value> {
    blocking(move || {
        let link = link_for(&app, &master)?;
        link.call(&cmd, args, streams.then(|| on_event.into()))
    })
    .await
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

/// A free local port that reaches `port` on a connected master's localhost (migration M1: the preview of a shown
/// master's dev server). It ends when that master is disconnected.
#[tauri::command]
async fn port_forward(app: tauri::AppHandle, master: String, port: u16) -> Res<u16> {
    // The link first (This PC's own is made again after its master restarted), then the port on it.
    blocking(move || {
        link_for(&app, &master)?;
        app.state::<AppState>().remotes.forward(&master, port)
    })
    .await
}

#[tauri::command]
fn port_unforward(state: State<AppState>, master: String, port: u16) {
    state.remotes.unforward(&master, port);
}

#[tauri::command]
fn inspector_close(state: State<AppState>) {
    state.inspector.close();
}

/// A change to This PC's Inbox, heard from its master: told to every window ("qanuvi-inbox"), so they all show the
/// same list, with the desktop notification for a new item (once).
fn inbox_heard(app: &tauri::AppHandle, change: serde_json::Value) {
    if change["change"] == "add"
        && change["new"] == true
        && let Ok(item) = serde_json::from_value::<crate::inbox::InboxItem>(change["item"].clone())
    {
        crate::desktop_notify::new_item(app, &item);
    }
    let _ = tauri::Emitter::emit(app, "qanuvi-inbox", change);
}

// Desktop notifications (24u part 4, session B): the window sends This PC's settings (24z3 D2: never a master's); a click that started Qanuvi
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

/// The window's own notice (M5a: the last window closed while This PC's work goes on, said once it is gone).
#[tauri::command]
fn notify_notice(app: tauri::AppHandle, title: String, text: String) -> Res<()> {
    crate::desktop_notify::notice(&app, &title, &text)
}

/// s4: the tray's Quit closes the windows; then This PC's work stops too, so the last window says nothing goes on.
static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn app_quitting() -> bool {
    QUITTING.load(std::sync::atomic::Ordering::SeqCst)
}

/// t1: started only to ask the tray's Quit question (`--quit-question <id>`): one small window, nothing else.
static QUESTION_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// t1: the tray's Quit question up now ({id, text}, or null), for the question window.
#[tauri::command]
async fn quit_question_text() -> Res<serde_json::Value> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::master::this_pc_call(
            &crate::paths::data_dir(),
            "app_quit_question",
            serde_json::json!({}),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

// t1: the answer to the tray's Quit question goes to This PC's master; the question window then ends.
#[tauri::command]
async fn quit_question_answer(app: tauri::AppHandle, id: u64, yes: bool) -> Res<()> {
    let sent = tauri::async_runtime::spawn_blocking(move || {
        crate::master::this_pc_call(
            &crate::paths::data_dir(),
            "app_quit_answer",
            serde_json::json!({"id": id, "yes": yes}),
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    if QUESTION_ONLY.load(std::sync::atomic::Ordering::SeqCst) {
        app.exit(0);
    }
    sent.map(|_| ())
}

/// t1: the app as the tray's Quit question alone, in a small window of Qanuvi's own look. No master starts here, no
/// single-instance lock is taken (a Qanuvi opened meanwhile opens as always), and it ends once it has answered.
fn run_quit_question(data: std::path::PathBuf, mut context: tauri::Context<tauri::Wry>) {
    QUESTION_ONLY.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::paths::set_data_dir(data);
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "quit-question",
                tauri::WebviewUrl::App("quit-question".into()),
            )
            .title("Quit Qanuvi")
            .inner_size(440.0, 196.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .decorations(false)
            .always_on_top(true)
            .center()
            .focused(true)
            .background_color(tauri::webview::Color(20, 20, 22, 255))
            .theme(Some(tauri::Theme::Dark))
            .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            quit_question_text,
            quit_question_answer
        ])
        .run(context)
        .expect("error while running Qanuvi's Quit question");
}

// A new item in a connected master's Inbox (24z R2d part 3): every window that follows the master asks, it is shown once.
// The window sends that master's Notifications settings with it (24z3 D2); an older window sends none: This PC's.
#[tauri::command]
fn notify_item(
    app: tauri::AppHandle,
    notifier: State<crate::desktop_notify::Notifier>,
    item: crate::inbox::InboxItem,
    settings: Option<crate::notify::NotifySettings>,
) {
    if notifier.first_time(&item.id) {
        crate::desktop_notify::new_item_by(&app, &item, settings);
    }
}

// This PC's settings (24z3 A3): the Settings page while This PC is shown.
// This PC's own server list (migration M3a), once only in the window's localStorage.
// Unsaved text of a machine the switcher hid (24z3 Part B2), kept here when the file is this computer's.
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
    // L1: WebKitGTK hangs on the system's WOFF fonts (a blank window); the window hides them from it.
    #[cfg(target_os = "linux")]
    // SAFETY: start-up, before the window or any other thread exists.
    unsafe {
        crate::linux_fonts::hide_web_fonts(&data)
    };
    let context = tauri::generate_context!();
    // t1: the tray's Quit question alone (no restore applied, no master started): the window's look and storage are
    // the app's own.
    if std::env::args().any(|a| a == crate::master::QUESTION_FLAG) {
        if (data != defaults.base || crate::data_location::from_source())
            && let Some(web) = defaults.webview_for(&data)
        {
            // SAFETY: start-up, before the window or any other thread exists (as for the app below).
            unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &web) };
        }
        return run_quit_question(data, context);
    }
    // A restore booked in Settings, Backups: applied now, before the database and the window open.
    if let Some(note) = crate::backups::apply_pending(
        &defaults.base,
        &data,
        defaults.webview_for(&data).as_deref(),
        Some(&backups_default()),
    ) {
        eprintln!("{note}");
    }
    // M4c: started at sign-in ("Start Qanuvi when Windows starts"; "...when I sign in" before M5b): This PC's
    // master starts on the app's data with its tray icon, and the app ends without a window.
    if std::env::args().any(|a| a == crate::sign_in_start::FLAG) {
        crate::master::mark_app_data(&data);
        if let Err(e) = crate::master::start_this_pc(&data) {
            eprintln!("This PC's master did not start: {e}");
        }
        return;
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
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            let dir = data.clone();
            crate::paths::set_data_dir(dir.clone());
            std::fs::create_dir_all(&dir)?;
            // M5a: the app no longer opens the database; This PC's master is the only one with it open.
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
                languages: crate::lsp::Servers::default(),
                debuggers: crate::dap::Adapters::default(),
                inspector: preview::Inspector::default(),
                remotes: {
                    // This PC starts every path its calls take (migration M3c: the loop guard).
                    let r = crate::remote::Remotes::default();
                    r.set_me(&crate::machine::lasting_id(&crate::paths::data_dir()));
                    // M5c part 2: This PC's servers are held by This PC's master; their calls go through it.
                    r.set_through(THIS_PC);
                    r
                },
                accounts: crate::account::Accounts::this_pc(&dir),
            });
            // This data folder is the app's own: a master service started on it is This PC.
            crate::master::mark_app_data(&dir);
            // M4a: This PC runs its own master on this data folder from launch on (remote access on or not). M4b: it
            // runs This PC's background jobs. M5a: it does all of This PC's work; the window reaches it on the
            // "this-computer" link, made on the first call.
            let master_dir = dir.clone();
            std::thread::spawn(move || {
                if let Err(e) = crate::master::start_this_pc(&master_dir) {
                    eprintln!("This PC's master did not start: {e}");
                }
            });
            // Every change to This PC's Inbox (made by any window, a run or the master's jobs) reaches all windows as
            // "qanuvi-inbox", and a new item raises the desktop notification once.
            let inbox_dir = dir.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                crate::master::follow_this_pc_inbox(
                    &inbox_dir,
                    || true,
                    move |change| inbox_heard(&handle, change),
                );
            });
            // M4c: the tray of This PC's master reaches the windows: "Open Qanuvi" shows one, "Quit Qanuvi" closes them
            // all (each as when closed by hand), and the master then ends.
            let app_dir = dir.clone();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                crate::master::follow_this_pc_app(
                    &app_dir,
                    || true,
                    move |ask| {
                        let windows = handle.webview_windows();
                        match ask["do"].as_str() {
                            Some("show") => {
                                if let Some(w) =
                                    windows.get("main").or_else(|| windows.values().next())
                                {
                                    let _ = w.unminimize();
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                            // t1: the tray's Quit question, in Qanuvi's own dialog in the window brought forward;
                            // with no window here the master opens a question window of its own.
                            Some("ask-quit") => {
                                if let Some(w) =
                                    windows.get("main").or_else(|| windows.values().next())
                                {
                                    let _ = w.unminimize();
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                    let _ = tauri::Emitter::emit_to(
                                        &handle,
                                        w.label(),
                                        "qanuvi-quit-question",
                                        &ask,
                                    );
                                } else {
                                    let id = ask["id"].clone();
                                    std::thread::spawn(move || {
                                        let _ = crate::master::this_pc_call(
                                            &crate::paths::data_dir(),
                                            "app_quit_answer",
                                            serde_json::json!({"id": id, "noWindow": true}),
                                        );
                                    });
                                }
                            }
                            Some("quit") => {
                                QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
                                // A window that cancelled its closing keeps everything running (master: 20 s).
                                std::thread::spawn(|| {
                                    std::thread::sleep(std::time::Duration::from_secs(30));
                                    QUITTING.store(false, std::sync::atomic::Ordering::SeqCst);
                                });
                                for w in windows.values() {
                                    let _ = w.close();
                                }
                            }
                            _ => {}
                        }
                    },
                );
            });
            // An entry that starts Qanuvi at sign-in follows the app when it moved.
            std::thread::spawn(crate::sign_in_start::refresh);
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
                // Nor its machine (24z3 step 4): a machine window closed by itself does not come back at the next
                // start (the window's own note may not leave before it is gone).
                let _ = tauri::Emitter::emit(
                    window.app_handle(),
                    "qanuvi-window-machine",
                    serde_json::json!({"label": window.label(), "machine": null}),
                );
                // A second window takes only its own coding tools with it; the last window ends the window's side.
                let others = window
                    .app_handle()
                    .webview_windows()
                    .keys()
                    .any(|l| l != window.label());
                if others {
                    state.languages.stop_owner(window.label());
                    state.debuggers.stop_owner(window.label());
                    return;
                }
                state.languages.stop_all();
                state.debuggers.stop_all();
                state.inspector.close();
                state.remotes.disconnect_all();
                // M4c: This PC's master stays with its tray icon, and its work goes on there (M5a: agents and plan
                // runs too; the window said so before it closed). Its tray's "Quit Qanuvi" stops it. The link's
                // terminals end with the link.
            }
        })
        .invoke_handler(tauri::generate_handler![
            notify_settings,
            notify_take_start,
            notify_test,
            notify_notice,
            app_quitting,
            quit_question_answer,
            notify_item,
            app_info,
            screen_usage,
            browser_agent_shot,
            inspector_show,
            inspector_scroll,
            inspector_pick,
            inspector_page_text,
            inspector_close,
            remote_connect,
            remote_call,
            remote_disconnect,
            port_forward,
            port_unforward,
            remote_list,
            remote_pair,
            attachment_file,
            export_save,
            data_test_build,
            data_location_move,
            data_location_cancel,
            data_location_remove_old,
            app_restart,
            remote_unpair,
            account_wait,
            account_cancel,
            startup_path,
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
        ])
        .run(context)
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
