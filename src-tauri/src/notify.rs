//! Desktop notifications of the Inbox (24u part 4, session B): which new items are told on the desktop, quiet
//! hours, and several items arriving together told as one. Only the decisions live here (the core has no window);
//! showing them and reading the click is the desktop app's part (desktop_notify.rs). Do Not Disturb stays the
//! system's choice, and the Inbox keeps every item whatever is shown.

use crate::inbox::InboxItem;
use serde::{Deserialize, Serialize};

/// The owner's choices (Settings > Notifications), sent by the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifySettings {
    /// Show new Inbox items as desktop notifications.
    pub desktop: bool,
    /// "questions": questions only; "problems": questions and problems; "all": also news (a run finished).
    pub what: String,
    /// Items arriving within a few seconds of each other are told in one notification.
    pub bundle: bool,
    /// Quiet hours: nothing is shown between `quiet_from` and `quiet_to` (minutes after midnight, local time).
    pub quiet: bool,
    pub quiet_from: u16,
    pub quiet_to: u16,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            desktop: true,
            what: "problems".into(),
            bundle: true,
            quiet: false,
            quiet_from: 22 * 60,
            quiet_to: 7 * 60,
        }
    }
}

/// How long items are gathered before one notification tells them all.
pub const BUNDLE_MS: u64 = 3000;

/// The address a click opens: one item, or the Inbox itself.
pub fn link(id: Option<&str>) -> String {
    match id {
        Some(id) => format!("qanuvi://inbox/{id}"),
        None => "qanuvi://inbox".into(),
    }
}

/// What a `qanuvi://` address asks for: `Some(Some(id))` one item, `Some(None)` the Inbox, `None` not ours.
pub fn parse_link(url: &str) -> Option<Option<String>> {
    let rest = url.trim().strip_prefix("qanuvi://")?;
    let rest = rest.trim_end_matches('/');
    let path = rest.strip_prefix("inbox")?;
    if path.is_empty() {
        return Some(None);
    }
    let id = path.strip_prefix('/')?;
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    Some(ok.then(|| id.to_string()))
}

/// Whether this new item is told on the desktop at all.
pub fn wanted(s: &NotifySettings, item: &InboxItem) -> bool {
    if !s.desktop || item.status != "open" || item.read {
        return false;
    }
    match item.kind_of.as_str() {
        "answer" => true,
        "problem" => s.what != "questions",
        _ => s.what == "all",
    }
}

/// Whether `minute` (after local midnight) falls in the quiet hours; a span past midnight (22:00-07:00) works.
pub fn quiet_now(s: &NotifySettings, minute: u16) -> bool {
    if !s.quiet || s.quiet_from == s.quiet_to {
        return false;
    }
    let (from, to) = (s.quiet_from % 1440, s.quiet_to % 1440);
    if from < to {
        minute >= from && minute < to
    } else {
        minute >= from || minute < to
    }
}

/// One notification as shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub title: String,
    pub text: String,
    /// The item a click opens; none opens the Inbox.
    pub id: Option<String>,
}

fn short(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn one(item: &InboxItem) -> Shown {
    let title = match item.kind_of.as_str() {
        "answer" => format!("Needs your answer: {}", item.title),
        "problem" => format!("Problem: {}", item.title),
        _ => item.title.clone(),
    };
    Shown {
        title: short(&title, 120),
        text: short(&item.text, 240),
        id: Some(item.id.clone()),
    }
}

/// The notifications for items gathered together: one each, or (bundled) one telling them all.
pub fn compose(items: &[InboxItem], bundle: bool) -> Vec<Shown> {
    if items.len() <= 1 || !bundle {
        return items.iter().map(one).collect();
    }
    let questions = items.iter().filter(|i| i.kind_of == "answer").count();
    let title = if questions > 0 {
        format!(
            "{} new in the Inbox ({questions} need{} your answer)",
            items.len(),
            if questions == 1 { "s" } else { "" }
        )
    } else {
        format!("{} new in the Inbox", items.len())
    };
    let mut lines: Vec<String> = items.iter().take(3).map(|i| short(&i.title, 70)).collect();
    if items.len() > 3 {
        lines.push(format!("and {} more", items.len() - 3));
    }
    vec![Shown {
        title,
        text: lines.join("\n"),
        id: None,
    }]
}

/// Minutes after local midnight, from the system's clock and time zone.
pub fn local_minute() -> u16 {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only fills the struct it is given.
        let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        t.wHour * 60 + t.wMinute
    }
    #[cfg(unix)]
    {
        // SAFETY: localtime_r writes only into `tm`; a null result leaves it zeroed (midnight).
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            (tm.tm_hour * 60 + tm.tm_min) as u16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind_of: &str, title: &str) -> InboxItem {
        InboxItem {
            id: format!("id-{title}"),
            at: 1,
            kind_of: kind_of.into(),
            kind: "run".into(),
            title: title.into(),
            text: "text".into(),
            read: false,
            key: None,
            open: None,
            form: None,
            source: None,
            status: "open".into(),
            answer: None,
            answered_by: None,
            answered_at: None,
        }
    }

    #[test]
    fn links_round_trip_and_strangers_are_refused() {
        let id = "0b8e3c1a-1f2d-4c5e-9a7b-123456789abc";
        assert_eq!(parse_link(&link(Some(id))), Some(Some(id.to_string())));
        assert_eq!(parse_link(&link(None)), Some(None));
        assert_eq!(parse_link("qanuvi://inbox/"), Some(None));
        assert_eq!(parse_link("qanuvi://inbox/a b"), Some(None));
        assert_eq!(parse_link("qanuvi://inbox/../x"), Some(None));
        assert_eq!(parse_link("qanuvi://inboxes"), None);
        assert_eq!(parse_link("qanuvi://other/1"), None);
        assert_eq!(parse_link("https://inbox/1"), None);
    }

    #[test]
    fn which_items_are_told() {
        let mut s = NotifySettings::default();
        assert!(wanted(&s, &item("answer", "q")));
        assert!(wanted(&s, &item("problem", "p")));
        assert!(!wanted(&s, &item("info", "i")));
        s.what = "questions".into();
        assert!(!wanted(&s, &item("problem", "p")));
        s.what = "all".into();
        assert!(wanted(&s, &item("info", "i")));
        let mut read = item("answer", "q");
        read.read = true;
        assert!(!wanted(&s, &read));
        s.desktop = false;
        assert!(!wanted(&s, &item("answer", "q")));
    }

    #[test]
    fn quiet_hours_across_midnight_and_within_a_day() {
        let mut s = NotifySettings {
            quiet: true,
            ..Default::default()
        };
        assert!(quiet_now(&s, 23 * 60));
        assert!(quiet_now(&s, 3 * 60));
        assert!(!quiet_now(&s, 7 * 60));
        assert!(!quiet_now(&s, 12 * 60));
        s.quiet_from = 12 * 60;
        s.quiet_to = 13 * 60;
        assert!(quiet_now(&s, 12 * 60 + 30));
        assert!(!quiet_now(&s, 13 * 60));
        s.quiet = false;
        assert!(!quiet_now(&s, 12 * 60 + 30));
    }

    #[test]
    fn items_together_are_told_once_when_bundled() {
        let items = vec![
            item("answer", "1a waits"),
            item("problem", "Sign-in"),
            item("info", "Run finished"),
            item("info", "More"),
        ];
        let one_each = compose(&items, false);
        assert_eq!(one_each.len(), 4);
        assert_eq!(one_each[0].title, "Needs your answer: 1a waits");
        assert_eq!(one_each[0].id.as_deref(), Some("id-1a waits"));
        let bundled = compose(&items, true);
        assert_eq!(bundled.len(), 1);
        assert_eq!(bundled[0].title, "4 new in the Inbox (1 needs your answer)");
        assert_eq!(
            bundled[0].text,
            "1a waits\nSign-in\nRun finished\nand 1 more"
        );
        assert_eq!(bundled[0].id, None);
        assert_eq!(
            compose(&items[..1], true)[0].id.as_deref(),
            Some("id-1a waits")
        );
    }
}
