//! Desktop notifications from This PC's master while no window follows it (migration M5a): a blocked step's question
//! or a budget stop never waits unseen. The same notifications as the app's (notify.rs decides and words them); a
//! click opens Qanuvi at the item: an installed Qanuvi through its qanuvi:// link, a test build through `open`.

use crate::notify::Shown;

/// The desktop app's identifier (tauri.conf.json), under which an installed Qanuvi's notifications show.
pub const APP_ID: &str = "dev.qanuvi.desktop";

#[cfg(windows)]
static KEPT: std::sync::Mutex<
    std::collections::VecDeque<windows::UI::Notifications::ToastNotification>,
> = std::sync::Mutex::new(std::collections::VecDeque::new());

/// Shows one notification. `installed`: an installed Qanuvi (its own app id and link); `open` is called with the item
/// when a test build's notification is clicked.
#[cfg(windows)]
pub fn show(
    one: &Shown,
    installed: bool,
    open: impl Fn(Option<String>) + Send + Sync + 'static,
) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    let err = |e: windows::core::Error| e.message().to_string();
    let doc = XmlDocument::new().map_err(err)?;
    doc.LoadXml(&HSTRING::from(crate::notify::toast_xml(one, installed)))
        .map_err(err)?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(err)?;
    if !installed {
        let id = one.id.clone();
        toast
            .Activated(&TypedEventHandler::new(move |_, _| {
                open(id.clone());
                Ok(())
            }))
            .map_err(err)?;
    }
    let app_id = if installed {
        APP_ID
    } else {
        crate::desktop_notify::POWERSHELL_APP_ID
    };
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|n| n.Show(&toast))
        .map_err(err)?;
    // Kept so a test build's click handler lives as long as the banner.
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    kept.push_back(toast);
    while kept.len() > 20 {
        kept.pop_front();
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn show(
    one: &Shown,
    _installed: bool,
    open: impl Fn(Option<String>) + Send + Sync + 'static,
) -> Result<(), String> {
    let mut n = notify_rust::Notification::new();
    n.summary(&one.title).body(&one.text).appname("Qanuvi");
    #[cfg(target_os = "linux")]
    n.action("default", "Open");
    let handle = n.show().map_err(|e| e.to_string())?;
    let id = one.id.clone();
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                open(id);
            }
        });
    });
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn show(
    _one: &Shown,
    _installed: bool,
    _open: impl Fn(Option<String>) + Send + Sync + 'static,
) -> Result<(), String> {
    Err("Desktop notifications are not available on this system.".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_app_id_is_the_desktop_apps() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], super::APP_ID);
    }
}
