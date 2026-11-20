//! Project memory from runs (PM, owner agreed 2026-11-21). A run's FACTS.md holds what its steps learned; the facts
//! about the project itself (how the tests run, which site to test on, how to get a test login) belong to the project,
//! not to one plan, so every later plan and check can use them. At a run's end they are offered as memory suggestions,
//! kept with one click from the Inbox (b); once per project, the notes of the plans from before this are read the same
//! way (e). The call also names the test command the run used, so a project without one can set it in one click.

use super::js::{self, json_in};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// At most this much of the plans' notes goes into one call.
pub const NOTES_CHARS: usize = 9000;
/// At most this many suggestions from one run.
pub const MAX_FACTS: usize = 8;

pub const SYSTEM: &str =
    "You pick lasting facts about a software project out of a plan's notes. Answer with JSON only.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Learned {
    /// Short title and the fact in a sentence.
    pub facts: Vec<(String, String)>,
    /// The command the run's steps used to run the project's tests ("" when none shows).
    pub test_command: String,
}

/// The call: the notes (this plan's, and on the first time every plan's), what the memory already has.
pub fn prompt(project: &str, notes: &str, memory: &[String], test_command: &str) -> String {
    let notes = if js::len(notes) > NOTES_CHARS {
        format!("{}\n… (cut)", js::cut(notes, NOTES_CHARS))
    } else {
        notes.to_string()
    };
    let mut out = vec![
        format!("Below are notes that plan runs in the project \"{project}\" kept for their later steps. Pick out the facts"),
        "about the project itself that any later plan or person working on it would need, not only this plan:".into(),
        "- how the tests or checks run (the exact command), and anything they need first;".into(),
        "- which site, address or database to test on, and how to start it;".into(),
        "- how to get a test login or test account (the recipe: which file, command or page; never a password, key".into(),
        "  or other secret value; say where it is kept instead);".into(),
        "- where things are and conventions the code follows, when they are clearly lasting.".into(),
        "Leave out: what a step did or still has to do, plan progress, decisions only about this plan's own work, and".into(),
        "anything the memory below already says.".into(),
        String::new(),
        "Already in the memory:".into(),
    ];
    if memory.is_empty() {
        out.push("- (nothing)".into());
    } else {
        out.extend(memory.iter().take(40).map(|m| format!("- {m}")));
    }
    out.push(format!(
        "The project's test command in Qanuvi: {}",
        if test_command.trim().is_empty() {
            "none set"
        } else {
            test_command.trim()
        }
    ));
    out.extend([
        String::new(),
        "The notes:".into(),
        "<<<".into(),
        notes,
        ">>>".into(),
        String::new(),
        format!(
            "Answer JSON only: {{\"facts\": [{{\"title\": \"a few words\", \"text\": \"the fact, one or two plain sentences\"}}], \"testCommand\": \"the command the notes say runs the project's tests, or empty\"}}. At most {MAX_FACTS} facts; an empty list when there is nothing new."
        ),
    ]);
    out.join("\n")
}

/// What the call found; None when its answer could not be read.
pub fn parse(answer: &str) -> Option<Learned> {
    let v = json_in(answer)?;
    let text = |x: Option<&Value>, n: usize| {
        js::cut(x.and_then(Value::as_str).unwrap_or("").trim(), n).to_string()
    };
    let facts = v
        .get("facts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|f| (text(f.get("title"), 120), text(f.get("text"), 600)))
                .filter(|(t, b)| !t.is_empty() && !b.is_empty())
                .take(MAX_FACTS)
                .collect()
        })
        .unwrap_or_default();
    let cmd = text(v.get("testCommand"), 200);
    Some(Learned {
        facts,
        // One line, no secret: a command, not a sentence about it.
        test_command: if cmd.contains('\n') || super::guidance::looks_secret(&cmd) {
            String::new()
        } else {
            cmd
        },
    })
}

/// The Inbox text listing what a run learned.
pub fn inbox_text(facts: &[(String, String)]) -> String {
    let mut lines = vec![
        "Facts the run learned about this project. Kept, every later plan, check and step gets them.".to_string(),
    ];
    lines.extend(facts.iter().map(|(t, b)| format!("- {t}: {b}")));
    lines.join("\n")
}

/// PM c: the project memory as a block for a step's work or a call that writes the owner's questions ("" when it
/// has nothing).
pub fn memory_block(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = vec![
        "What this project's memory in Qanuvi says (the owner's own facts about the project: use them; never ask the owner what they already answer, say \"from the project memory\" where you use one):".to_string(),
    ];
    out.extend(lines.iter().map(|l| format!("- {l}")));
    out.join("\n")
}

/// PM a: the first web address in the owner's answer (a test site fills Preview address when it is empty).
pub fn first_url(text: &str) -> Option<String> {
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"https?://[^\s"'<>()\[\]]+"#).unwrap());
    URL.find(text).map(|m| {
        m.as_str()
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_carries_the_notes_the_memory_and_the_test_command() {
        let p = prompt(
            "Shop",
            "- Tests: php tests/run.php --strict",
            &["[fact] Test site: https://example.test/".into()],
            "",
        );
        assert!(p.contains("\"Shop\""));
        assert!(p.contains("- [fact] Test site: https://example.test/"));
        assert!(p.contains("test command in Qanuvi: none set"));
        assert!(p.contains("<<<\n- Tests: php tests/run.php --strict\n>>>"));
        assert!(prompt("S", "", &[], "npm test").contains("- (nothing)"));
        let long = "x".repeat(NOTES_CHARS + 50);
        assert!(prompt("S", &long, &[], "").contains("… (cut)"));
    }

    #[test]
    fn reads_facts_and_the_test_command_and_drops_secrets() {
        let l = parse(
            r#"{"facts": [{"title": "Tests", "text": "php tests/run.php"}, {"title": "", "text": "x"}], "testCommand": "php tests/run.php"}"#,
        )
        .unwrap();
        assert_eq!(l.facts, vec![("Tests".into(), "php tests/run.php".into())]);
        assert_eq!(l.test_command, "php tests/run.php");
        let l = parse(r#"{"facts": [], "testCommand": "TOKEN=ghp_abcdefghijklmnop1234 npm test"}"#)
            .unwrap();
        assert_eq!(l.test_command, "");
        assert_eq!(parse("no json"), None);
    }

    #[test]
    fn the_memory_block_is_empty_without_entries() {
        assert_eq!(memory_block(&[]), "");
        let b = memory_block(&["[fact] Test site: https://shop.example.test/".into()]);
        assert!(b.starts_with("What this project's memory"));
        assert!(b.ends_with("\n- [fact] Test site: https://shop.example.test/"));
    }

    #[test]
    fn finds_the_first_address_in_an_answer() {
        assert_eq!(
            first_url("Test on https://shop.example.test/base/. Admin is in the seed file."),
            Some("https://shop.example.test/base/".into())
        );
        assert_eq!(
            first_url("(http://localhost:8080)"),
            Some("http://localhost:8080".into())
        );
        assert_eq!(first_url("on the test site"), None);
    }
}
