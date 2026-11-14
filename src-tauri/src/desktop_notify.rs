//! Desktop notifications, the window part (24u part 4, session B): shows what notify.rs decided and brings Qanuvi to
//! the front with the Inbox open when one is clicked.
//!
//! Windows: an installed Qanuvi sends toasts under its own app id (the Start-menu shortcut's) with a protocol launch
//! "qanuvi://inbox/<id>", so a click on the banner, on the copy in the notification centre or with Qanuvi closed all
//! come in as that link (a running Qanuvi gets it from the single-instance plugin). A test build has no shortcut: it
//! sends under PowerShell's app id and reads the click in the process, which works while the banner shows.
//! Linux and macOS: notify-rust, a thread waits for the click. A banner timing out is not an answer; the Inbox keeps
//! every item.

use crate::inbox::InboxItem;
use crate::notify::{self, NotifySettings, Shown};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// The event a window gets to open its Inbox (at one item, or at the top).
pub const OPEN_EVENT: &str = "qanuvi-inbox-open";

#[derive(Default)]
pub struct Notifier {
    settings: Mutex<NotifySettings>,
    /// Items gathered for one notification, and whether the gathering has started.
    gathered: Mutex<(Vec<InboxItem>, bool)>,
    /// A click that started Qanuvi: kept until the window is ready to ask for it.
    at_start: Mutex<Option<Option<String>>>,
    /// Shown toasts, kept so their click handlers live as long as the banner (test builds).
    #[cfg(windows)]
    toasts: Mutex<std::collections::VecDeque<windows::UI::Notifications::ToastNotification>>,
}

impl Notifier {
    pub fn set(&self, s: NotifySettings) {
        *self.settings.lock().unwrap() = s;
    }

    /// The click that started Qanuvi, once.
    pub fn take_start(&self) -> Option<Option<String>> {
        self.at_start.lock().unwrap().take()
    }
}

/// An installed Qanuvi (not a test build out of `target/`): its own app id and the qanuvi:// link work. Only Windows
/// and macOS ask; on Linux it was unused, and clippy's warning failed every Linux CI build from 07d8e38 on.
#[cfg(any(windows, target_os = "macos"))]
fn installed() -> bool {
    !crate::data_location::from_source()
}

/// A new Inbox item: told on the desktop when the owner's settings want it.
pub fn new_item(app: &AppHandle, item: &InboxItem) {
    let Some(n) = app.try_state::<Notifier>() else {
        return;
    };
    let s = n.settings.lock().unwrap().clone();
    if !notify::wanted(&s, item) || notify::quiet_now(&s, notify::local_minute()) {
        return;
    }
    if !s.bundle {
        for one in notify::compose(std::slice::from_ref(item), false) {
            show(app, &one);
        }
        return;
    }
    let mut g = n.gathered.lock().unwrap();
    g.0.push(item.clone());
    if g.1 {
        return;
    }
    g.1 = true;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(notify::BUNDLE_MS));
        let Some(n) = app.try_state::<Notifier>() else {
            return;
        };
        let items = {
            let mut g = n.gathered.lock().unwrap();
            g.1 = false;
            std::mem::take(&mut g.0)
        };
        for one in notify::compose(&items, true) {
            show(&app, &one);
        }
    });
}

/// A test notification from Settings (quiet hours do not apply to it).
pub fn test(app: &AppHandle) -> Result<(), String> {
    let one = Shown {
        title: "Qanuvi notifications are on".into(),
        text: "Questions from your plan runs show here. Click to open the Inbox.".into(),
        id: None,
    };
    show_now(app, &one)
}

fn show(app: &AppHandle, one: &Shown) {
    if let Err(e) = show_now(app, one) {
        eprintln!("Desktop notification failed: {e}");
    }
}

/// Brings a Qanuvi window to the front and opens its Inbox (at the item, when there is one).
pub fn open(app: &AppHandle, id: Option<String>) {
    let windows = app.webview_windows();
    let win = windows.get("main").or_else(|| windows.values().next());
    let Some(win) = win else { return };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
    let _ = app.emit_to(win.label(), OPEN_EVENT, serde_json::json!({ "id": id }));
}

/// The links among a start's arguments (a click on a notification starts Qanuvi with "qanuvi://inbox/<id>").
fn links(args: &[String]) -> Option<Option<String>> {
    args.iter().find_map(|a| notify::parse_link(a))
}

/// Qanuvi was started with a link: kept for the window, which asks once it is ready.
pub fn started_with(app: &AppHandle, args: &[String]) {
    if let (Some(n), Some(target)) = (app.try_state::<Notifier>(), links(args)) {
        *n.at_start.lock().unwrap() = Some(target);
    }
}

/// Qanuvi was started again while running (single instance): a link opens the Inbox, anything else just brings the
/// window to the front.
pub fn second_start(app: &AppHandle, args: &[String]) {
    match links(args) {
        Some(target) => open(app, target),
        None => {
            let windows = app.webview_windows();
            if let Some(win) = windows.get("main").or_else(|| windows.values().next()) {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
}

/// A link that reached Qanuvi another way (macOS hands links to the running app).
#[cfg(target_os = "macos")]
pub fn opened_url(app: &AppHandle, url: &str) {
    if let Some(target) = notify::parse_link(url) {
        open(app, target);
    }
}

#[cfg(windows)]
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// PowerShell's app id: Windows shows toasts sent under it from a program that has no Start-menu shortcut.
#[cfg(windows)]
const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

#[cfg(windows)]
fn show_now(app: &AppHandle, one: &Shown) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    let own = installed();
    let url = notify::link(one.id.as_deref());
    // Installed: the click starts the qanuvi:// link (from the banner, the notification centre or with Qanuvi
    // closed). Test build: the click is read here while the banner shows. "long" keeps the banner about 25 s.
    let launch = if own {
        format!(r#" activationType="protocol" launch="{}""#, xml_text(&url))
    } else {
        format!(r#" launch="{}""#, xml_text(&url))
    };
    let text = if one.text.is_empty() {
        String::new()
    } else {
        format!("<text>{}</text>", xml_text(&one.text))
    };
    let xml = format!(
        r#"<toast duration="long"{launch}><visual><binding template="ToastGeneric"><text>{}</text>{text}</binding></visual></toast>"#,
        xml_text(&one.title)
    );
    let err = |e: windows::core::Error| e.message().to_string();
    let doc = XmlDocument::new().map_err(err)?;
    doc.LoadXml(&HSTRING::from(xml)).map_err(err)?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(err)?;
    if !own {
        let app = app.clone();
        let id = one.id.clone();
        toast
            .Activated(&TypedEventHandler::new(move |_, _| {
                open(&app, id.clone());
                Ok(())
            }))
            .map_err(err)?;
    }
    let app_id = if own {
        app.config().identifier.as_str()
    } else {
        POWERSHELL_APP_ID
    };
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|n| n.Show(&toast))
        .map_err(err)?;
    if let Some(n) = app.try_state::<Notifier>() {
        let mut kept = n.toasts.lock().unwrap();
        kept.push_back(toast);
        while kept.len() > 20 {
            kept.pop_front();
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn show_now(app: &AppHandle, one: &Shown) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Shown as Qanuvi when installed (its bundle id); a test build has no bundle, so the system's default stays.
        static ONCE: std::sync::Once = std::sync::Once::new();
        if installed() {
            let id = app.config().identifier.clone();
            ONCE.call_once(|| {
                let _ = notify_rust::set_application(&id);
            });
        }
    }
    let mut n = notify_rust::Notification::new();
    n.summary(&one.title).body(&one.text).appname("Qanuvi");
    // Linux: "default" is the click on the notification itself (no button is shown for it).
    #[cfg(target_os = "linux")]
    n.action("default", "Open");
    let handle = n.show().map_err(|e| e.to_string())?;
    let app = app.clone();
    let id = one.id.clone();
    // Waits until the notification is clicked or closed (macOS sends it here); never on the caller's thread.
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                open(&app, id);
            }
        });
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn show_now(_app: &AppHandle, _one: &Shown) -> Result<(), String> {
    Err("Desktop notifications are not available on this system.".into())
}
