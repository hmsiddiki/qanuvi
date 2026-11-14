//! The owner's messages in a step's chat (24z R2c part 2, owner agreed 2026-11-15). A message the owner types into a
//! chat the runner opened goes to the runner first, not straight into the agent's task. The decider (a free model,
//! else word rules) sorts it:
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
pub const WORD_RULES: &str = "Qanuvi (word rules)";

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
    /// Who sorted it (the free model's name, or the word rules).
    pub by: String,
    /// For a change: a short title for it as a plan step.
    pub title: String,
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

fn words(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| *w != "please")
        .collect::<Vec<_>>()
        .join(" ")
}

/// A message that is only a stop or pause word needs no model ("stop", "pause the run", "hold on"). Anything longer
/// ("stop using sqlite") is sorted as usual.
pub fn quick(text: &str) -> Option<Kind> {
    match words(text).as_str() {
        "stop" | "stop now" | "stop it" | "stop the run" | "stop the step" | "stop here"
        | "abort" | "cancel" | "cancel the run" => Some(Kind::Stop),
        "pause" | "pause now" | "pause it" | "pause the run" | "pause the step" | "pause here"
        | "hold on" | "wait" => Some(Kind::Pause),
        _ => None,
    }
}

/// The word rules, when no model answers: a question by its form, a change by its words; when unsure, a change (the
/// owner is asked, nothing is lost).
pub fn guess(text: &str) -> Kind {
    if let Some(k) = quick(text) {
        return k;
    }
    let w = words(text);
    let first = w.split(' ').next().unwrap_or("");
    const ASKS: [&str; 16] = [
        "why", "what", "how", "when", "where", "which", "who", "is", "are", "can", "could", "does",
        "did", "should", "will", "would",
    ];
    if text.trim_end().ends_with('?') || ASKS.contains(&first) {
        return Kind::Question;
    }
    // Words that set how all the work goes, not this step only ("always ...", "from now on ...").
    const RULES: [&str; 9] = [
        "always ",
        "never ",
        "from now on",
        "in every step",
        "for every step",
        "in all steps",
        "for all steps",
        "each step ",
        "every time ",
    ];
    let spaced = format!(" {w} ");
    if first == "always"
        || first == "never"
        || RULES.iter().any(|r| spaced.contains(&format!(" {r}")))
    {
        return Kind::Rule;
    }
    const CHANGES: [&str; 12] = [
        "also ",
        "as well",
        "instead",
        "new step",
        "another step",
        "change the plan",
        "add ",
        "remove ",
        "skip ",
        "don't do",
        "do not do",
        "drop ",
    ];
    let padded = format!("{w} ");
    if CHANGES.iter().any(|c| padded.contains(c)) {
        return Kind::Change;
    }
    Kind::Note
}

/// A short title for a change as a plan step, from the message itself.
pub fn title_of(text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = one.trim_end_matches(['.', '!', '?']);
    let mut t: String = one.chars().take(60).collect();
    if one.chars().count() > 60 {
        if let Some(cut) = t.rfind(' ') {
            t.truncate(cut);
        }
        t.push('…');
    }
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "The owner's change".into(),
    }
}

/// The decider's question.
pub fn sort_prompt(step: &str, text: &str) -> String {
    format!(
        "The owner typed a message into the chat of a running plan step. Sort it.\n\nStep: {step}\nMessage: \
         \"{text}\"\n\nKinds:\n- note: a correction or a fact for this step; the agent takes it into account and goes \
         on.\n- rule: a standing rule for the whole plan, not only this step (how all the work should be done, e.g. \
         \"always follow the core code style\", \"check the other plugins before adding helpers\"); every later step \
         gets it.\n- question: the owner asks something; the agent answers briefly and goes on.\n- change: the owner wants \
         something this step does not cover, or a change of the plan or its scope.\n- stop: the owner wants the run \
         stopped.\n- pause: the owner wants the run paused.\n\nWhen unsure between note and change, answer change. \
         When unsure between note and rule, answer note. Answer with JSON only: {{\"kind\": \
         \"note|rule|question|change|stop|pause\", \"title\": \"for a change: a short \
         title for it as a plan step\"}}"
    )
}

/// The decider's answer; None when it is not clear (the word rules decide then).
pub fn parse_sort(answer: &str) -> Option<(Kind, String)> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let v: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let kind = Kind::from_word(v.get("kind")?.as_str()?)?;
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .map(|t| t.trim().to_string())
        .unwrap_or_default();
    Some((kind, title))
}

/// The owner answers a change question in words in the chat ("now", "after this step", "new step"); a longer message
/// is a new message, not an answer.
pub fn when_in_words(text: &str) -> Option<When> {
    let w = words(text);
    if w.split(' ').count() > 8 {
        return None;
    }
    let has = |p: &str| format!(" {w} ").contains(&format!(" {p} "));
    if has("new step") || has("own step") || has("separate") || has("later") || has("not now") {
        Some(When::NewStep)
    } else if has("after") || has("afterwards") || has("when done") {
        Some(When::After)
    } else if has("now") || has("right away") || has("in this step") || has("go ahead") {
        Some(When::Now)
    } else {
        None
    }
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
        "(Qanuvi: the owner asks for a change: \"{text}\". Do not start it yet. Finish only the edit in hand, then \
         answer \"Waiting for the owner\" and stop; Qanuvi asks the owner when it should be done.)"
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
        format!("- [ ] **{label}. {}**", new.title),
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
    fn only_bare_stop_and_pause_words_skip_the_decider() {
        assert_eq!(quick("Stop!"), Some(Kind::Stop));
        assert_eq!(quick("please pause the run"), Some(Kind::Pause));
        assert_eq!(quick("hold on"), Some(Kind::Pause));
        assert_eq!(quick("stop using sqlite here"), None);
        assert_eq!(quick("wait, the key is in .env"), None);
    }

    #[test]
    fn the_word_rules_sort_questions_changes_and_notes() {
        assert_eq!(guess("why sqlite?"), Kind::Question);
        assert_eq!(guess("Should this go in the helper"), Kind::Question);
        assert_eq!(guess("also add a CSV export"), Kind::Change);
        assert_eq!(guess("do the export in JSON instead"), Kind::Change);
        assert_eq!(guess("the price helper is in lib/money.php"), Kind::Note);
        assert_eq!(guess("stop"), Kind::Stop);
        assert_eq!(guess("always follow WordPress core style"), Kind::Rule);
        assert_eq!(
            guess("From now on check the other plugins before adding helpers"),
            Kind::Rule
        );
        assert_eq!(guess("never edit the vendor folder"), Kind::Rule);
        assert_eq!(guess("always add tests?"), Kind::Question);
    }

    #[test]
    fn reads_the_deciders_answer_or_leaves_it_to_the_word_rules() {
        assert_eq!(
            parse_sort("Sure: {\"kind\": \"change\", \"title\": \"CSV export\"}"),
            Some((Kind::Change, "CSV export".into()))
        );
        assert_eq!(
            parse_sort("{\"kind\":\"question\"}"),
            Some((Kind::Question, String::new()))
        );
        assert_eq!(parse_sort("{\"kind\": \"maybe\"}"), None);
        assert_eq!(parse_sort("no idea"), None);
    }

    #[test]
    fn reads_when_in_the_owners_words() {
        assert_eq!(when_in_words("now"), Some(When::Now));
        assert_eq!(when_in_words("Do it now please."), Some(When::Now));
        assert_eq!(when_in_words("after this step"), Some(When::After));
        assert_eq!(when_in_words("make it a new step"), Some(When::NewStep));
        assert_eq!(when_in_words("not now"), Some(When::NewStep));
        assert_eq!(when_in_words("later"), Some(When::NewStep));
        assert_eq!(
            when_in_words("now that I think of it the export should also write a header row"),
            None
        );
        assert_eq!(when_in_words("use the helper"), None);
    }

    #[test]
    fn a_title_from_the_message_is_short_and_starts_with_a_capital() {
        assert_eq!(title_of("also add a CSV export."), "Also add a CSV export");
        let long = title_of(&"word ".repeat(30));
        assert!(long.chars().count() <= 61 && long.ends_with('…'), "{long}");
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
