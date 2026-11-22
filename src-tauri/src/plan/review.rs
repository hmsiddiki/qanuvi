//! One review list (24y Y6): checks that need a person's eyes (how a page looks, a wording choice) go to one list in
//! the Inbox instead of holding the run. A step's check written in words ("Check: the page looks right") and the
//! "To look at:" lines its agent writes under it are put on the list once the step is done; the step is not held.
//!
//! The list is one Inbox item per plan (each run adds to it; what an earlier run put there and nobody answered yet
//! stays). "All fine" closes it; words typed instead become a new step in the plan, after the last step looked at,
//! so the next run makes the change.

use super::env::RunEnv;
use super::housekeeping::archive_path;
use super::ownermsg::{self, NewStep};
use super::runner::LookItem;
use crate::inbox::{InboxItem, NewItem};
use serde_json::{Value, json};

/// The Inbox key of a plan's review list.
pub fn key(project_id: &str, file: &str) -> String {
    format!("review:{project_id}:{file}")
}

/// The things a step's lines say to look at: "To look at: ..." or "Look at: ..." (list marks around them allowed).
pub fn looks_in(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines.iter().flat_map(|l| l.lines()) {
        let t = line.trim().trim_start_matches(['-', '*', '+']).trim();
        let lower = t.to_lowercase();
        let rest = ["to look at:", "look at:"]
            .iter()
            .find_map(|p| lower.starts_with(p).then(|| t[p.len()..].trim()));
        if let Some(what) = rest.filter(|w| !w.is_empty() && w.len() <= 300)
            && out.len() < 5
            && !out.iter().any(|o| o == what)
        {
            out.push(what.to_string());
        }
    }
    out
}

/// "Step 3: Cart page", or the title alone.
pub fn step_name(item: &LookItem) -> String {
    match &item.number {
        Some(n) => format!("Step {n}: {}", item.title),
        None => item.title.clone(),
    }
}

/// The entries an open review list holds now (from an earlier run, not answered yet).
pub fn open_entries(item: Option<&InboxItem>) -> Vec<LookItem> {
    item.filter(|i| i.status == "open")
        .and_then(|i| i.source.as_ref())
        .and_then(|s| s.get("entries").cloned())
        .and_then(|e| serde_json::from_value(e).ok())
        .unwrap_or_default()
}

/// The list's Inbox item: what to look at, step by step, with the project's Preview address when it has one.
pub fn inbox_item(
    project_id: &str,
    project_name: &str,
    file: &str,
    run_id: &str,
    preview: &str,
    entries: &[LookItem],
) -> NewItem {
    let plan = file.rsplit('/').next().unwrap_or(file);
    let project = if project_name.is_empty() {
        String::new()
    } else {
        format!("{project_name}: ")
    };
    let n = entries.len();
    let mut text =
        String::from("The run went on without waiting for these; look when you have time.\n");
    for e in entries {
        text.push_str(&format!("\n• {}: {}", step_name(e), e.what));
    }
    if !preview.trim().is_empty() {
        text.push_str(&format!("\n\nPreview: {}", preview.trim()));
    }
    NewItem {
        kind_of: "answer".into(),
        kind: "run".into(),
        title: format!(
            "{project}{n} {} to look at from {plan}",
            if n == 1 { "thing" } else { "things" }
        ),
        text,
        key: Some(key(project_id, file)),
        open: Some(json!({"label": "Open the plan", "view": "plan", "projectId": project_id})),
        form: Some(json!({
            "choices": [{"id": "ok", "label": "All fine", "hint": "Closes this list"}],
            "text": "Or say what needs changing (it becomes a step in the plan)"
        })),
        source: Some(json!({
            "kind": "review",
            "projectId": project_id,
            "file": file,
            "runId": run_id,
            "entries": entries,
        })),
    }
}

/// The owner answered the review list: words typed become a step in the plan, right after the last step looked at
/// (the next run makes the change). "All fine" needs nothing more. Answers the new step's number, if one was added.
pub fn on_answer(env: &dyn RunEnv, item: &InboxItem) -> Option<String> {
    let said = item
        .answer
        .as_ref()
        .and_then(|a| a.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())?
        .to_string();
    let src = item.source.as_ref()?;
    let file = src.get("file").and_then(Value::as_str)?;
    let entries: Vec<LookItem> = src
        .get("entries")
        .cloned()
        .and_then(|e| serde_json::from_value(e).ok())
        .unwrap_or_default();
    let now = env.read(file).ok()?;
    let archive = env
        .read(&archive_path(file))
        .map(|f| f.text)
        .unwrap_or_default();
    let new = NewStep {
        title: "Changes after your look".into(),
        text: said,
    };
    // After the last step looked at that is still in the plan.
    for e in entries.iter().rev() {
        if let Ok((next, label)) =
            ownermsg::add_step(&now.text, &archive, e.number.as_deref(), &e.title, &new)
            && env.write(file, &next, Some(&now.hash)).is_ok()
        {
            return Some(label);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_a_step_says_to_look_at() {
        let got = looks_in(&[
            "Add the cart page.",
            "  - To look at: the header on a phone-sized window\n- look at: the wording of the empty-cart text",
            "- To look at:",
            "Looking at: not this",
        ]);
        assert_eq!(
            got,
            [
                "the header on a phone-sized window",
                "the wording of the empty-cart text"
            ]
        );
    }

    #[test]
    fn the_list_names_every_step_and_the_preview() {
        let entries = vec![
            LookItem {
                number: Some("3".into()),
                title: "Cart page".into(),
                what: "the page looks right".into(),
            },
            LookItem {
                number: None,
                title: "Wording".into(),
                what: "the welcome text reads well".into(),
            },
        ];
        let item = inbox_item(
            "p1",
            "Shop",
            "docs/PLAN.md",
            "r1",
            "http://localhost/shop/",
            &entries,
        );
        assert_eq!(item.title, "Shop: 2 things to look at from PLAN.md");
        assert!(
            item.text
                .contains("• Step 3: Cart page: the page looks right")
        );
        assert!(item.text.contains("• Wording: the welcome text reads well"));
        assert!(item.text.ends_with("Preview: http://localhost/shop/"));
        assert_eq!(item.key.as_deref(), Some("review:p1:docs/PLAN.md"));
        let src = item.source.unwrap();
        assert_eq!(src["kind"], "review");
        assert_eq!(src["entries"][0]["what"], "the page looks right");
    }
}
