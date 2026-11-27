//! The owner's messages in a step's chat (24z R2c part 2, owner agreed 2026-11-15). A message the owner types into a
//! chat the runner opened goes to the runner first, not straight into the agent's task. The decider (a free model)
//! reads it by its meaning, in any language, never by word lists; with no model a message is a question only by its
//! question mark, else a change, so the owner is asked. It sorts it:
//!
//! - a note (a correction or a fact for this step): passed to the agent, the work goes on;
//! - a question: the agent answers it briefly in the chat, then goes on;
//! - a change of plan or scope: the agent finishes the edit in hand and waits; the runner asks in words (chat and
//!   Inbox) when to do it: now, after this step, or as a new step. With no answer within the wait (a run setting,
//!   10 minutes by default) the safe choice is taken: the step goes on as planned and the change becomes a new step;
//! - stop or pause words: as the buttons.
//!
//! What each message was taken for and how it was handled goes into the step's log and the run's decisions.

use super::planner::free_labels;
use super::tasklist::parse_plan;
use serde_json::{Value, json};

/// How long a change question waits for the owner before the safe choice is taken.
pub const WAIT_DEFAULT_MS: i64 = 600_000;

/// What the decider asks the free model.
pub const SORT_SYSTEM: &str =
    "You sort an owner's chat message to a coding agent. Answer with JSON only.";

/// Who sorted a message when no model did.
pub const WORD_RULES: &str = "Qanuvi (no model)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Note,
    /// A standing rule for the whole plan ("always follow WordPress core style"): kept in the plan's memory.
    Rule,
    Question,
    Change,
    Stop,
    Pause,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Note => "note",
            Kind::Rule => "rule",
            Kind::Question => "question",
            Kind::Change => "change",
            Kind::Stop => "stop",
            Kind::Pause => "pause",
        }
    }

    fn from_word(w: &str) -> Option<Kind> {
        Some(match w.trim().to_lowercase().as_str() {
            "note" | "correction" | "fact" => Kind::Note,
            "rule" | "standing rule" => Kind::Rule,
            "question" => Kind::Question,
            "change" | "scope" | "plan" => Kind::Change,
            "stop" => Kind::Stop,
            "pause" => Kind::Pause,
            _ => return None,
        })
    }
}

/// When the owner wants a change done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// In this step, now.
    Now,
    /// In the same session, once the step is done.
    After,
    /// As its own step in the plan, after this one.
    NewStep,
}

impl When {
    pub fn id(self) -> &'static str {
        match self {
            When::Now => "now",
            When::After => "after",
            When::NewStep => "new",
        }
    }

    pub fn from_id(id: &str) -> Option<When> {
        Some(match id {
            "now" => When::Now,
            "after" => When::After,
            "new" => When::NewStep,
            _ => return None,
        })
    }
}

/// How one message was sorted.
#[derive(Clone, Debug, PartialEq)]
pub struct Sorted {
    pub kind: Kind,
    /// Who sorted it (the free model's name, or Qanuvi with no model).
    pub by: String,
    /// For a change: a short title for it as a plan step.
    pub title: String,
    /// The message answers the waiting change question (now, after this step, a new step), read by the model.
    pub when: Option<When>,
    /// The owner asked to keep it (or it is a lasting fact about the project): it goes into the project's memory.
    pub keep: bool,
}

/// One message as the step's log and the run's decisions keep it.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub at: i64,
    pub text: String,
    pub kind: Kind,
    pub by: String,
    /// What was done with it, in plain words.
    pub handled: String,
}

/// A change the owner wants as its own step: the run loop writes it into the plan after the step at work.
#[derive(Clone, Debug, PartialEq)]
pub struct NewStep {
    pub title: String,
    pub text: String,
}

/// With no model, nothing is read by its words (a word list knows one language only): a message ending in a
/// question mark (any script's) is a question, anything else is taken as a change, so the owner is asked in words
/// what they meant and when; nothing is lost.
pub fn guess(text: &str) -> Kind {
    if text.trim_end().ends_with(['?', '؟', '？', ';']) {
        return Kind::Question;
    }
    Kind::Change
}

/// A short title for a change as a plan step, from the message itself: a long message ends at a word (the step's
/// "Asked by the owner" line keeps all of it).
pub fn title_of(text: &str) -> String {
    tidy_title(&text.chars().take(400).collect::<String>())
}

/// A step title made clean for the plan: no label the owner or the decider put in front ("Change:", "New step:"),
/// no "as a new step" asking how to take it, no ellipsis, at most 60 characters ending at a word, a capital first.
pub fn tidy_title(title: &str) -> String {
    const LEADS: [&str; 6] = ["change", "new step", "add step", "step", "todo", "to do"];
    let mut one = title.split_whitespace().collect::<Vec<_>>().join(" ");
    loop {
        let lower = one.to_lowercase();
        if lower.len() != one.len() {
            break; // lower-casing changed byte lengths: positions would not match
        }
        let lead = LEADS.iter().find_map(|l| {
            let rest = lower.strip_prefix(l)?;
            let rest_t = rest.trim_start();
            let sep = rest_t
                .chars()
                .next()
                .filter(|c| [':', '-', '–'].contains(c))?;
            Some(one.len() - rest_t.len() + sep.len_utf8())
        });
        match lead {
            Some(at) => one = one[at..].trim_start().to_string(),
            None => break,
        }
    }
    let lower = one.to_lowercase();
    // Positions in the lower-cased text are only good for the title when lower-casing kept the byte lengths.
    let tails: &[&str] = if lower.len() == one.len() {
        &[
            ", as a new step",
            " as a new step",
            ", as its own step",
            " as its own step",
        ]
    } else {
        &[]
    };
    for tail in tails {
        if let Some(at) = lower.rfind(tail) {
            let after = lower[at + tail.len()..]
                .trim_matches(|c: char| c.is_whitespace() || ".!?".contains(c));
            if after.is_empty() {
                one.truncate(at);
                break;
            }
        }
    }
    let one = one
        .trim_end_matches(['.', '!', '?', '…', ' ', ',', ';', ':'])
        .to_string();
    let mut t: String = one.chars().take(60).collect();
    if one.chars().count() > 60 {
        if let Some(cut) = t.rfind(' ') {
            t.truncate(cut);
        }
        t = t.trim_end_matches([',', ';', ':', '-', ' ']).to_string();
    }
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "The owner's change".into(),
    }
}

/// The decider's question. `waiting`: the owner's earlier change that Qanuvi asked about ("when should it be done?");
/// the message may be their answer, in any language or spelling.
pub fn sort_prompt(step: &str, text: &str, waiting: Option<&str>) -> String {
    let when = match waiting {
        Some(change) => format!(
            "\n\nQanuvi is waiting for the owner's answer to: when should their change \"{change}\" be done: now in \
             this step, after this step in the same session, or as a new step of its own? Read the message by what it \
             means (any language, spelling or wording, however short), never word by word. If it answers that \
             question, give \"when\": \"now\", \"after\" or \"new\" (\"later\" or \"not now\" means \"new\"); if it is \
             something else, give \"when\": null and sort it as below."
        ),
        None => String::new(),
    };
    format!(
        "The owner typed a message into the chat of a running plan step. Sort it.\n\nStep: {step}\nMessage: \
         \"{text}\"{when}\n\nKinds:\n- note: a correction or a fact for this step; the agent takes it into account and \
         goes on. Information the owner gives (addresses, where and how to test, how something is set up, what should \
         work, keys or logins already in place) is a note, even when it names what should work.\n- rule: a standing rule for the whole plan, not only this step (how all the work should be done, e.g. \
         \"always follow the core code style\", \"check the other plugins before adding helpers\"); every later step \
         gets it.\n- question: the owner asks something; the agent answers briefly and goes on.\n- change: the owner asks for \
         work this step does not cover, or a change of the plan or its scope.\n- stop: the owner wants the run \
         stopped.\n- pause: the owner wants the run paused.\n\nRead the message by what it means, in any language. \
         When unsure between note and change, answer note: the agent reads a note and can still act on it. When unsure \
         between note and rule, answer note. \"keep\": true when the owner asks to keep it (in memory, for later, \
         remember) or it is a lasting fact about the project (a test address, how the project is set up); it then \
         goes into the project's memory too. Answer with JSON only: {{\"kind\": \"note|rule|question|change|stop|pause\", \
         \"title\": \"for a change: a short title for it as a plan step; with keep: a short title for the fact\", \
         \"when\": \"now|after|new\" or null, \"keep\": true or false}}"
    )
}

/// The decider's answer: the kind, a change's title, and the answer to a waiting change question when the message is
/// one. None when it is not clear (then Qanuvi with no model sorts it).
pub fn parse_sort(answer: &str) -> Option<(Kind, String, Option<When>)> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let v: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let when = v
        .get("when")
        .and_then(Value::as_str)
        .and_then(When::from_id);
    let kind = match v
        .get("kind")
        .and_then(Value::as_str)
        .and_then(Kind::from_word)
    {
        Some(k) => k,
        // An answer to the waiting question needs no kind of its own.
        None if when.is_some() => Kind::Note,
        None => return None,
    };
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    Some((kind, title, when))
}

/// Whether the decider's answer says to keep the message in the project's memory.
pub fn parse_keep(answer: &str) -> bool {
    let (Some(start), Some(end)) = (answer.find('{'), answer.rfind('}')) else {
        return false;
    };
    answer
        .get(start..=end)
        .and_then(|j| serde_json::from_str::<Value>(j).ok())
        .and_then(|v| v.get("keep").and_then(Value::as_bool))
        .unwrap_or(false)
}

// ---------- What the agent is told ----------

pub fn note_message(text: &str) -> String {
    format!(
        "(Qanuvi: a note from the owner for this step: \"{text}\". Take it into account and go on with the step.)"
    )
}

pub fn rule_message(text: &str) -> String {
    format!(
        "(Qanuvi: a standing rule from the owner for the whole plan: \"{text}\". Follow it from now on, in this step \
         and after; Qanuvi keeps it in the plan's FACTS.md for every later step.)"
    )
}

/// The rule's line in the plan's FACTS.md.
pub fn rule_fact(text: &str) -> String {
    format!("- Owner rule: {text}\n")
}

pub fn question_message(text: &str) -> String {
    format!(
        "(Qanuvi: the owner asks: \"{text}\". Answer briefly in the chat, in a few sentences, then go on with the step \
         from where you were.)"
    )
}

pub fn change_hold_message(text: &str) -> String {
    format!(
        "(Qanuvi: the owner asks for a change: \"{text}\". Do not start it yet: go on with this step's own work as \
         before; Qanuvi asks the owner when the change should be done and tells you the answer. If the step is done \
         before that, answer \"Waiting for the owner\" and stop.)"
    )
}

pub fn when_message(when: When, text: &str) -> String {
    match when {
        When::Now => format!(
            "(Qanuvi: the owner wants the change now, in this step: \"{text}\". Do it, then finish the step; say in \
             the step's log what you changed for it.)"
        ),
        When::After => format!(
            "(Qanuvi: go on with the step as planned. When the step is done, also make the owner's change in this same \
             session: \"{text}\". Say in the step's log what you changed for it.)"
        ),
        When::NewStep => format!(
            "(Qanuvi: go on with the step as planned and do not make the owner's change \"{text}\" now: it becomes its \
             own step in the plan, after this one.)"
        ),
    }
}

// ---------- What the owner is told (in the step's chat) ----------

fn minutes(ms: i64) -> String {
    let m = (ms as f64 / 60_000.0).round().max(1.0) as i64;
    if m == 1 {
        "1 minute".into()
    } else {
        format!("{m} minutes")
    }
}

pub fn ask_text(text: &str, wait_ms: i64) -> String {
    format!(
        "Your message asks for a change: \"{text}\". The agent finishes the edit in hand and waits. When should the \
         change be done? Answer here in words: \"now\" (in this step), \"after this step\" (in the same session, once \
         the step is done) or \"new step\" (its own step in the plan). The question is in the Inbox too. With no answer \
         in {}, the step goes on as planned and the change becomes a new step.",
        minutes(wait_ms)
    )
}

pub fn when_text(when: When) -> &'static str {
    match when {
        When::Now => "Your change goes into this step now.",
        When::After => "Your change comes after this step, in the same session.",
        When::NewStep => {
            "Your change becomes a new step in the plan, after this one; this step goes on as planned."
        }
    }
}

pub fn default_text(wait_ms: i64) -> String {
    format!(
        "No answer in {}: this step goes on as planned and your change becomes a new step in the plan, after this \
         one.",
        minutes(wait_ms)
    )
}

pub fn handled_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Note => "passed to the agent as a note for this step",
        Kind::Rule => "kept as a rule for the whole plan (FACTS.md), every later step gets it",
        Kind::Question => "the agent answers it in the chat, then goes on",
        Kind::Stop => "taken as Stop: the agent finishes the edit in hand, then the run stops",
        Kind::Pause => "taken as Pause: the agent finishes the edit in hand and writes where it is",
        Kind::Change => "asked when to do it",
    }
}

pub fn chosen_text(when: When, by_default: bool, wait_ms: i64) -> String {
    let what = match when {
        When::Now => "done in this step",
        When::After => "done after this step, in the same session",
        When::NewStep => "added as a new step after this one",
    };
    if by_default {
        format!("no answer in {}, so {what}", minutes(wait_ms))
    } else {
        format!("the owner chose: {what}")
    }
}

/// A line the step's chat shows as Qanuvi's (a notice, not the agent's or the owner's words).
pub fn chat_line(text: &str) -> String {
    json!({"type": "system", "subtype": "qanuvi_runner", "text": text}).to_string()
}

/// The Inbox form of a change question.
pub fn form() -> Value {
    json!({"choices": [
        {"id": "now", "label": "Now, in this step"},
        {"id": "after", "label": "After this step", "hint": "In the same session, once the step is done"},
        {"id": "new", "label": "As a new step", "hint": "Its own step in the plan, after this one"},
    ]})
}

/// The step's log entry for one message.
pub fn log_entry(r: &Record, time: &str) -> String {
    format!(
        "\n- Owner message ({time}): \"{}\". Taken as a {} (by {}); {}.\n",
        r.text,
        r.kind.word(),
        r.by,
        r.handled
    )
}

/// Writes the owner's change into the plan as its own step, right after the step `number`/`title` (with a free
/// number and a Needs line on that step). Answers the new text and the new step's number.
pub fn add_step(
    text: &str,
    archive: &str,
    number: Option<&str>,
    title: &str,
    new: &NewStep,
) -> Result<(String, String), String> {
    let plan = parse_plan(text);
    let found = plan
        .steps
        .iter()
        .find(|s| s.number.as_deref() == number && s.title == title)
        .ok_or_else(|| "The step is no longer in the task list.".to_string())?;
    let label = free_labels(text, archive, number, 1)
        .into_iter()
        .next()
        .ok_or("no free step number")?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.split(eol).map(String::from).collect();
    let after = number
        .map(|n| format!("during step {n}"))
        .unwrap_or_else(|| format!("during \"{title}\""));
    let mut insert = vec![
        format!("- [ ] **{label}. {}**", tidy_title(&new.title)),
        format!("  - Asked by the owner {after}: \"{}\"", new.text),
    ];
    if let Some(n) = number {
        insert.push(format!("  - Needs: {n}"));
    }
    let at = found.end.min(lines.len());
    lines.splice(at..at, insert);
    Ok((lines.join(eol), label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_model_nothing_is_read_by_its_words() {
        // A question by its mark only, in any script; anything else is a change, so the owner is asked.
        assert_eq!(guess("why sqlite?"), Kind::Question);
        assert_eq!(guess("pourquoi sqlite ?"), Kind::Question);
        assert_eq!(guess("لماذا؟"), Kind::Question);
        assert_eq!(guess("為什麼？"), Kind::Question);
        assert_eq!(guess("also add a CSV export"), Kind::Change);
        assert_eq!(guess("the price helper is in lib/money.php"), Kind::Change);
        assert_eq!(guess("stop"), Kind::Change);
        assert_eq!(guess("plus tard"), Kind::Change);
    }

    #[test]
    fn reads_the_deciders_answer_with_when_in_any_language() {
        assert_eq!(
            parse_sort("Sure: {\"kind\": \"change\", \"title\": \"CSV export\"}"),
            Some((Kind::Change, "CSV export".into(), None))
        );
        assert_eq!(
            parse_sort("{\"kind\":\"question\", \"when\": null}"),
            Some((Kind::Question, String::new(), None))
        );
        // "plus tard" answering the waiting question: the model says new.
        assert_eq!(
            parse_sort("{\"kind\": \"note\", \"when\": \"new\"}"),
            Some((Kind::Note, String::new(), Some(When::NewStep)))
        );
        assert_eq!(
            parse_sort("{\"when\": \"now\"}"),
            Some((Kind::Note, String::new(), Some(When::Now)))
        );
        assert_eq!(parse_sort("{\"kind\": \"maybe\"}"), None);
        assert_eq!(parse_sort("no idea"), None);
        // The owner asked to keep it: it goes into the project memory too.
        assert!(parse_keep("{\"kind\": \"note\", \"keep\": true}"));
        assert!(!parse_keep("{\"kind\": \"note\"}"));
        // A change waiting for the owner never stops the step's own work.
        assert!(change_hold_message("x").contains("go on with this step's own work"));
        let p = sort_prompt("1. Export", "plus tard", Some("also add a CSV export"));
        assert!(
            p.contains("when should their change \"also add a CSV export\" be done"),
            "{p}"
        );
        assert!(p.contains("any language"), "{p}");
        assert!(!sort_prompt("1. Export", "x", None).contains("Qanuvi is waiting"));
    }

    #[test]
    fn a_title_from_the_message_is_short_and_starts_with_a_capital() {
        assert_eq!(title_of("also add a CSV export."), "Also add a CSV export");
        let long = title_of(&"word ".repeat(30));
        assert!(
            long.chars().count() <= 60 && long.ends_with("word"),
            "{long}"
        );
    }

    #[test]
    fn a_step_title_has_no_label_in_front_no_asking_how_and_no_ellipsis() {
        assert_eq!(tidy_title("Change: add a CSV export…"), "Add a CSV export");
        assert_eq!(
            tidy_title("change - new step: log the totals"),
            "Log the totals"
        );
        assert_eq!(
            tidy_title("add a CSV export as a new step."),
            "Add a CSV export"
        );
        assert_eq!(tidy_title("Changes to the README"), "Changes to the README");
        assert_eq!(tidy_title("Step 2 tests"), "Step 2 tests");
        assert_eq!(tidy_title("Change: "), "The owner's change");
        assert_eq!(
            title_of("Change: also export JSON, as a new step"),
            "Also export JSON"
        );
    }

    #[test]
    fn the_new_step_goes_right_after_the_step_with_a_free_number_and_needs_it() {
        let plan = "# Plan\n\n- [ ] **1. Export**\n  - Writes orders\n- [ ] **2. Later**\n";
        let (next, label) = add_step(
            plan,
            "",
            Some("1"),
            "Export",
            &NewStep {
                title: "CSV export".into(),
                text: "also add a CSV export".into(),
            },
        )
        .unwrap();
        assert_eq!(label, "1a");
        assert_eq!(
            next,
            "# Plan\n\n- [ ] **1. Export**\n  - Writes orders\n- [ ] **1a. CSV export**\n  - Asked by the owner \
             during step 1: \"also add a CSV export\"\n  - Needs: 1\n- [ ] **2. Later**\n"
        );
        let steps = parse_plan(&next).steps;
        assert_eq!(steps[1].number.as_deref(), Some("1a"));
        assert_eq!(steps[1].needs, vec!["1".to_string()]);
        // Windows line endings stay.
        let (crlf, _) = add_step(
            &plan.replace('\n', "\r\n"),
            "",
            Some("1"),
            "Export",
            &NewStep {
                title: "X".into(),
                text: "x".into(),
            },
        )
        .unwrap();
        assert!(crlf.contains("**1a. X**\r\n"));
        assert!(
            add_step(
                plan,
                "",
                Some("9"),
                "Gone",
                &NewStep {
                    title: "X".into(),
                    text: "x".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn the_form_offers_now_after_and_new_step() {
        let ids: Vec<String> = form()["choices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, ["now", "after", "new"]);
        for id in &ids {
            assert_eq!(When::from_id(id).map(When::id), Some(id.as_str()));
        }
    }
}
