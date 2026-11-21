//! n1: a blocked or unfinished plan step asks the owner a question that stands on its own, so it can be answered from
//! a phone without reading the chat: what happened, why only the owner can act, the exact change wanted (with a
//! copyable snippet), and what an answer does. The owner's typed answer is read by its meaning before anything runs
//! again (n2: never by a list of phrases): a question back is answered plainly and the need asked again; something
//! looking can settle is looked up read-only and offered for a yes; an unclear reply brings a plainer question, and
//! after one, guidance one small step at a time; a reply that settles the need is an instruction the step's next try
//! acts on. No question is asked the same way twice. The talk goes on until the need is met, the owner skips the
//! step, or the run ends.

use super::js;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

/// Text that can be copied as it is: a config rule, a command, a line of code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// What it is and where it goes ("Add to /etc/nginx/nginx.conf, inside the server block").
    pub label: String,
    pub text: String,
}

/// The question as the owner sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// n2: the plain answer to what the owner asked back, shown before the question.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reply: String,
    /// One plain sentence: what is wanted from the owner.
    pub headline: String,
    /// What the step tried and what went wrong.
    #[serde(default)]
    pub happened: String,
    /// Why the agent cannot do it itself.
    #[serde(default)]
    pub why_you: String,
    /// The exact change or information wanted.
    #[serde(default)]
    pub wanted: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<Snippet>,
    /// What Qanuvi can do itself when asked to, and what it would need for that.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub can_do: String,
}

/// One round of the talk about a step: what was asked (its headline) and what the owner said.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    pub asked: String,
    pub answer: String,
}

/// PM a: a lasting fact about the project in the owner's reply (a test site, how tests run, how to get a test
/// login): a short title and the fact itself.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectFact {
    pub title: String,
    pub text: String,
}

/// The project fact a reading call gave (`{"title": ..., "text": ...}`), when it has both.
pub fn fact_of(v: Option<&Value>) -> Option<ProjectFact> {
    let v = v?;
    let get = |k: &str, n: usize| {
        js::cut(v.get(k).and_then(Value::as_str).unwrap_or("").trim(), n).to_string()
    };
    let f = ProjectFact {
        title: get("title", 120),
        text: get("text", 600),
    };
    (!f.title.is_empty() && !f.text.is_empty()).then_some(f)
}

/// What the owner's typed answer means.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Something to act on: the owner's words, made exact for the step (`instruction`, may be empty), and what
    /// the reply says about the project that holds beyond this step (PM a), kept in This project memory.
    Act {
        instruction: String,
        fact: Option<ProjectFact>,
        /// wd1: a folder outside the project the owner allowed the step to write into (absolute, as asked).
        write_dir: Option<String>,
    },
    /// "I don't understand": the same need asked again, simpler and in more detail.
    Clarify(Card),
    /// The answer does not settle the need: a follow-up question.
    FollowUp(Card),
}

/// The words a text answer box gets: plain examples of what can be said.
pub const ANSWER_HINT: &str = "Your answer, in your own words: \"do it yourself\", \"done, try again\", \"I don't understand\"…";

/// The card as plain text: the Inbox item's text, a notification, `qanuvi-master inbox list`, and older windows.
pub fn card_text(c: &Card) -> String {
    let mut out = vec![];
    if !c.reply.trim().is_empty() {
        out.push(c.reply.trim().to_string());
    }
    out.push(c.headline.trim().to_string());
    let mut part = |label: &str, text: &str| {
        let t = text.trim();
        if !t.is_empty() {
            out.push(format!("{label}: {t}"));
        }
    };
    part("What happened", &c.happened);
    part("Why it needs you", &c.why_you);
    part("What is needed", &c.wanted);
    if let Some(s) = c.snippet.as_ref().filter(|s| !s.text.trim().is_empty()) {
        let label = if s.label.trim().is_empty() {
            "Copy this"
        } else {
            s.label.trim()
        };
        // A label that already ends in a stop or a colon gets no colon of its own.
        let colon = if label.ends_with(['.', ':']) { "" } else { ":" };
        out.push(format!("{label}{colon}\n{}", s.text.trim_end()));
    }
    if !c.can_do.trim().is_empty() {
        out.push(c.can_do.trim().to_string());
    }
    out.join("\n\n")
}

/// What the explaining call reads about the step.
pub struct StepFacts<'a> {
    pub step: &'a str,
    pub details: &'a [String],
    /// Why it was held: the agent's "Blocked:" line, or the run's reason.
    pub why: &'a str,
    /// The agent's last message in its last try.
    pub last_message: &'a str,
    /// Files the step changed so far.
    pub changed: &'a [String],
    pub talk: &'a [Exchange],
}

const CARD_SHAPE: &str = r#"{"headline": "...", "happened": "...", "whyYou": "...", "wanted": "...", "snippet": {"label": "...", "text": "..."} or null, "canDo": "..."}"#;

const CARD_RULES: &str = "The owner may read this on a phone, without the chat, and may not be a programmer. Write plain, \
short sentences; no step numbers, no tool names, no talk about agents or prompts.\n\
- headline: one sentence saying what is wanted from the owner.\n\
- happened: what the step tried and what went wrong (two sentences at most).\n\
- whyYou: why it cannot be done without the owner (no access, a decision only they can make, a password, a server outside the project…).\n\
- wanted: the exact change or information wanted, with file names, places and values when known.\n\
- snippet: the exact text to copy (a config rule, a command, code) and where it goes; null when there is none. Never invent paths or values that are not in what you read; say what is unknown instead.\n\
- canDo: what Qanuvi can do itself if the owner says so, and what it would need for that (for example the file's place); empty when nothing.\n\
Examples use made-up neutral names (for example \"Example Shop Ltd\"), never the owner's own name or a name taken from a folder path, user name or account.\n\
Never put passwords, keys or tokens in any field.";

fn step_block(f: &StepFacts) -> String {
    let mut out = vec![format!("The step: {}", f.step)];
    for d in f.details.iter().take(12) {
        out.push(format!("  {}", js::cut(d.trim(), 300)));
    }
    out.push(format!("Why it stopped: {}", f.why.trim()));
    if !f.changed.is_empty() {
        out.push(format!("Files it changed so far: {}", f.changed.join(", ")));
    }
    let last = f.last_message.trim();
    if !last.is_empty() {
        // The end of the message holds the conclusion.
        let len = js::len(last);
        let tail = if len > 4000 {
            format!("…{}", js::from_unit(last, len - 4000))
        } else {
            last.to_string()
        };
        out.push(format!("The agent's last message:\n<<<\n{tail}\n>>>"));
    }
    if !f.talk.is_empty() {
        out.push("Asked and answered so far:".into());
        for e in f.talk.iter().rev().take(6).rev() {
            out.push(format!("- Asked: {}\n  Owner: {}", e.asked, e.answer));
        }
    }
    out.join("\n")
}

/// The prompt for the card of a step that stopped for the owner.
pub fn explain_prompt(f: &StepFacts) -> String {
    [
        "A step of a coding plan stopped because only the owner can unblock it. Write the question the owner gets."
            .to_string(),
        String::new(),
        step_block(f),
        String::new(),
        CARD_RULES.into(),
        String::new(),
        format!("Answer with JSON only: {CARD_SHAPE}"),
    ]
    .join("\n")
}

/// n2: the prompt that reads the owner's typed reply to `card` by its meaning (any wording, spelling or language;
/// never a list of phrases). After a reply that settled nothing, it guides instead of asking the same way again.
pub fn reading_prompt(f: &StepFacts, card: &Card, answer: &str) -> String {
    let round = f.talk.len() + 1;
    let mut out = vec![
        "A step of a coding plan stopped and the owner was asked the question below. Read their reply by what it \
         means (any wording, spelling or language, however short), never word by word."
            .to_string(),
        String::new(),
        step_block(f),
        String::new(),
        format!("The question:\n<<<\n{}\n>>>", card_text(card)),
        format!(
            "The owner's reply (reply {round} about this step):\n<<<\n{}\n>>>",
            answer.trim()
        ),
        String::new(),
        "Decide what the reply is:".into(),
        "- \"act\": it settles the need: a value, \"it is done now\", \"do it yourself\", \"do it another way\", or a \
         yes to something you offered. Give \"instruction\": what the step must do now, in one or two exact sentences."
            .into(),
        "- \"question\": the owner asks something back (what a word means, why, where). Give \"reply\": the plain \
         answer to what they asked, one or two sentences with an example; and \"card\": the need asked again in the \
         light of that answer."
            .into(),
        "- \"tried\": they ask you to find or check something that looking can settle (a value in a config file, the \
         docs, a setting on this computer). Do it now, read-only and quick (a few looks at most), and give \"card\": \
         what you found and where, asked as a yes question (\"I found … in …. Shall I use it?\"). Found nothing: say \
         where you looked and ask for exactly what is missing. A change to make is \"act\", not this."
            .into(),
        "- \"unclear\": you cannot tell what they mean, or they say they do not follow. Give \"card\": the need asked \
         again more plainly (explain any technical word, say step by step what to do and where)."
            .into(),
        "- \"followup\": the reply gives part of the need or something else. Give \"card\": a follow-up question \
         that asks exactly for what is still missing, and says what their reply gave."
            .into(),
        "Not now, later, or not known yet (in any language) is never \"act\": answer \"followup\" with a card that \
         says the step keeps waiting until they give it, and what exactly is still needed."
            .into(),
    ];
    if round > 1 {
        out.extend([
            String::new(),
            "The owner is still stuck: this is not the first reply. Guide them instead of asking the whole thing \
             again: ask one small question only, say exactly where to find the thing (which program, window, menu or \
             file, for example \"open Wnmp and look at the number next to MySQL\"), give an example answer, and offer \
             to find it yourself."
                .into(),
        ]);
    }
    out.extend([
        "Never repeat the wording of a question already asked (the question above and the ones under \"Asked and \
         answered so far\"); say it another way."
            .into(),
        String::new(),
        CARD_RULES.into(),
        String::new(),
        PROJECT_FACT_RULE.into(),
        ALLOW_WRITE_RULE.into(),
        format!(
            "Answer with JSON only: {{\"intent\": \"act\" | \"question\" | \"tried\" | \"unclear\" | \"followup\", \"instruction\": \"...\", \"reply\": \"...\", \"card\": {CARD_SHAPE} or null, \"projectFact\": {{\"title\": \"...\", \"text\": \"...\"}} or null, \"allowWrite\": \"...\" or null}}"
        ),
    ]);
    out.join("\n")
}

/// wd1: an answer that allows writing outside the project becomes a run rule, so the step's next try is not refused
/// by the same guard again (the owner said yes, the guard did not know, and the step asked the same thing twice).
const ALLOW_WRITE_RULE: &str = "\"allowWrite\" (with \"act\" only): when the step was stopped because it could not write \
     outside the project and the reply clearly allows a folder for that, the folder as an absolute path (the one the \
     question named; if the reply names it slightly differently, the question's); null otherwise, and null when the \
     reply only says to use another way or a folder inside the project.";

/// PM a: what the reading call keeps for the project besides the step.
const PROJECT_FACT_RULE: &str = "\"projectFact\" (with \"act\" only): when the reply says something about this project \
     that holds beyond this step and plan (which site or address to test on, how the tests run, how to get a test \
     login, where something is), a short title and the fact in one plain sentence; never a password, key or other \
     secret value (say where it is kept instead). null when the reply is only about this step.";

/// n2: a new question never repeats an earlier one word for word (the question being answered and the talk's): it is
/// asked another way, from what is needed, else plainly marked as put another way.
pub fn not_repeated(mut card: Card, asked_now: &str, talk: &[Exchange]) -> Card {
    let same = |a: &str| a.trim().eq_ignore_ascii_case(card.headline.trim());
    if !same(asked_now) && !talk.iter().any(|e| same(&e.asked)) {
        return card;
    }
    let wanted = card.wanted.trim().to_string();
    card.headline = if wanted.is_empty() || same(&wanted) {
        format!("Put another way: {}", card.headline.trim())
    } else {
        card.wanted = String::new();
        wanted
    };
    card
}

fn json_in(text: &str) -> Option<Value> {
    let (start, end) = (text.find('{')?, text.rfind('}')?);
    (end > start)
        .then(|| serde_json::from_str(&text[start..=end]).ok())
        .flatten()
}

fn card_of(v: &Value) -> Option<Card> {
    let s = |k: &str, max: usize| {
        js::cut(v.get(k).and_then(Value::as_str).unwrap_or("").trim(), max).to_string()
    };
    let headline = s("headline", 300);
    if headline.is_empty() {
        return None;
    }
    let snippet = v.get("snippet").and_then(|n| {
        let text = n
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_end();
        (!text.trim().is_empty()).then(|| Snippet {
            label: js::cut(
                n.get("label").and_then(Value::as_str).unwrap_or("").trim(),
                200,
            )
            .to_string(),
            text: js::cut(text, 3000).to_string(),
        })
    });
    Some(Card {
        reply: s("reply", 600),
        headline,
        happened: s("happened", 600),
        why_you: s("whyYou", 400),
        wanted: s("wanted", 800),
        snippet,
        can_do: s("canDo", 400),
    })
}

/// The card from the explaining call's answer; None when it gave none.
pub fn parse_card(text: &str) -> Option<Card> {
    card_of(&json_in(text)?)
}

/// What the reading call made of the answer; None when it gave nothing usable.
pub fn parse_reading(text: &str) -> Option<Reading> {
    let v = json_in(text)?;
    // n2: the plain answer to a question back goes on the card, before the question.
    let reply = js::cut(
        v.get("reply").and_then(Value::as_str).unwrap_or("").trim(),
        600,
    )
    .to_string();
    let card = v.get("card").and_then(card_of).map(|mut c| {
        if !reply.is_empty() {
            c.reply = reply;
        }
        c
    });
    match v.get("intent").and_then(Value::as_str)?.trim() {
        "act" => Some(Reading::Act {
            instruction: js::cut(
                v.get("instruction")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim(),
                600,
            )
            .to_string(),
            fact: fact_of(v.get("projectFact")),
            write_dir: v
                .get("allowWrite")
                .and_then(Value::as_str)
                .and_then(crate::run_rules::writable_dir),
        }),
        "clarify" | "question" | "unclear" => card.map(Reading::Clarify),
        "followup" | "tried" => card.map(Reading::FollowUp),
        _ => None,
    }
}

/// The first fenced block of a message, and the line before it as its label.
fn first_block(text: &str) -> Option<Snippet> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)(?:^|\n)([^\n]*)\n```[^\n]*\n(.*?)\n```").unwrap());
    let c = RE.captures(text)?;
    let label = c[1]
        .trim()
        .trim_end_matches(':')
        .trim_start_matches(['-', '*', ' '])
        .to_string();
    Some(Snippet {
        label: js::cut(&label, 200).to_string(),
        text: js::cut(&c[2], 3000).to_string(),
    })
}

fn sentences(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if js::len(&flat) > max {
        format!("{} …", js::cut(&flat, max))
    } else {
        flat
    }
}

/// The card when no model could write one: still the whole matter, from the agent's own words.
pub fn fallback_card(f: &StepFacts) -> Card {
    let why = f.why.trim();
    let why = if why.is_empty() || why == "it needs you" {
        "the agent could not finish it without you"
    } else {
        why
    };
    let without_blocks = Regex::new(r"(?s)```.*?```")
        .map(|re| re.replace_all(f.last_message, " ").to_string())
        .unwrap_or_default();
    Card {
        headline: format!("A step needs you: {}", f.step.trim()),
        happened: sentences(why, 400),
        why_you: String::new(),
        wanted: sentences(&without_blocks, 900),
        snippet: first_block(f.last_message),
        can_do: String::new(),
        reply: String::new(),
    }
}

/// What the next try is told: the talk so far and how to act on the owner's last answer.
pub fn next_try_note(why: &str, talk: &[Exchange], answer: &str, instruction: &str) -> String {
    let mut out = vec![format!(
        "Earlier this step stopped for the owner ({}). They were asked and answered:",
        why.trim()
    )];
    for e in talk.iter().rev().take(6).rev() {
        out.push(format!("- Asked: {}\n  Owner: \"{}\"", e.asked, e.answer));
    }
    if !talk.iter().any(|e| e.answer == answer) {
        out.push(format!("- Owner: \"{answer}\""));
    }
    if !instruction.trim().is_empty() {
        out.push(format!(
            "What their answer means for this try: {}",
            instruction.trim()
        ));
    }
    out.push(
        "Act on the answer. If they ask you to make the change yourself, make it now; if something is missing to do \
         that (where a file is, access you do not have), mark the step \"- [!]\" again and ask for exactly that one \
         thing. If they say it is done, check that it is in place, then go on. Do not repeat what failed before \
         without a new reason; if the answer does not settle what you need, mark the step \"- [!]\" and ask a clear \
         follow-up question instead of guessing. If their words ask something back or say they do not follow, do \
         not do the work: answer what they asked in plain words with an example, then mark the step \"- [!]\" and \
         ask again more simply, never in the same words as before."
            .into(),
    );
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(last: &'a str, talk: &'a [Exchange]) -> StepFacts<'a> {
        StepFacts {
            step: "20. Serve the new upload folder",
            details: &[],
            why: "needs an nginx rule on the server",
            last_message: last,
            changed: &[],
            talk,
        }
    }

    const NGINX: &str = "The uploads return 404 because nginx does not serve /files/. Add this to nginx.conf:\n\
```nginx\nlocation /files/ {\n    alias /srv/app/files/;\n}\n```\nThen reload nginx.";

    #[test]
    fn reads_a_card_and_writes_it_as_plain_text() {
        let c = parse_card(
            r#"Here: {"headline": "Add one rule to the web server", "happened": "Uploads give 404.", "whyYou": "The server's nginx.conf is outside the project.", "wanted": "Add the rule below and reload nginx.", "snippet": {"label": "Add to nginx.conf, inside the server block", "text": "location /files/ {\n    alias /srv/app/files/;\n}"}, "canDo": "Say \"do it yourself\" and tell me where nginx.conf is."}"#,
        )
        .unwrap();
        assert_eq!(c.headline, "Add one rule to the web server");
        assert!(
            c.snippet
                .as_ref()
                .unwrap()
                .text
                .starts_with("location /files/")
        );
        let t = card_text(&c);
        assert!(
            t.starts_with("Add one rule to the web server\n\nWhat happened: Uploads give 404.")
        );
        assert!(t.contains("Add to nginx.conf, inside the server block:\nlocation /files/ {"));
        assert!(t.ends_with("tell me where nginx.conf is."));
        assert_eq!(parse_card("no json here"), None);
        assert_eq!(parse_card(r#"{"headline": ""}"#), None);
    }

    #[test]
    fn falls_back_to_the_agents_own_words_and_snippet() {
        let c = fallback_card(&facts(NGINX, &[]));
        assert_eq!(c.happened, "needs an nginx rule on the server");
        assert!(c.wanted.contains("return 404"));
        assert!(
            !c.wanted.contains("alias"),
            "the snippet is apart: {}",
            c.wanted
        );
        let s = c.snippet.unwrap();
        assert_eq!(
            s.label,
            "The uploads return 404 because nginx does not serve /files/. Add this to nginx.conf"
        );
        assert!(s.text.contains("alias /srv/app/files/;"));
        // Nothing but "it needs you": never that alone.
        let bare = fallback_card(&StepFacts {
            why: "it needs you",
            last_message: "",
            ..facts("", &[])
        });
        assert_eq!(bare.happened, "the agent could not finish it without you");
    }

    #[test]
    fn reads_what_an_answer_means() {
        assert_eq!(
            parse_reading(
                r#"{"intent": "act", "instruction": "Add the rule to /etc/nginx/nginx.conf yourself and reload nginx."}"#
            ),
            Some(Reading::Act {
                instruction: "Add the rule to /etc/nginx/nginx.conf yourself and reload nginx."
                    .into(),
                fact: None,
                write_dir: None,
            })
        );
        // wd1: a folder the owner allowed comes with it; a system folder never does.
        let r = parse_reading(
            r#"{"intent": "act", "instruction": "Save the script there.", "allowWrite": "E:/tmp/qna-scripts/"}"#,
        );
        assert!(
            matches!(&r, Some(Reading::Act { write_dir: Some(d), .. }) if d.eq_ignore_ascii_case("E:/tmp/qna-scripts")),
            "{r:?}"
        );
        let r = parse_reading(r#"{"intent": "act", "allowWrite": "/etc"}"#);
        assert!(
            matches!(
                r,
                Some(Reading::Act {
                    write_dir: None,
                    ..
                })
            ),
            "{r:?}"
        );
        // PM a: what the reply says about the project beyond this step comes with it.
        let r = parse_reading(
            r#"{"intent": "act", "instruction": "Test on the test site.", "projectFact": {"title": "Test site", "text": "Plan steps test on https://example.test/shop/"}}"#,
        );
        assert!(matches!(r, Some(Reading::Act { fact: Some(f), .. }) if f.title == "Test site"));
        let r = parse_reading(
            r#"{"intent": "act", "projectFact": {"title": "Test site", "text": ""}}"#,
        );
        assert!(matches!(r, Some(Reading::Act { fact: None, .. })));
        let r = parse_reading(
            r#"{"intent": "clarify", "card": {"headline": "The web server needs one more rule"}}"#,
        );
        assert!(
            matches!(r, Some(Reading::Clarify(c)) if c.headline == "The web server needs one more rule")
        );
        let r = parse_reading(
            r#"{"intent": "followup", "card": {"headline": "Where is nginx.conf on the server?"}}"#,
        );
        assert!(matches!(r, Some(Reading::FollowUp(_))));
        // A clarify without a card is no reading (the fallback then answers).
        assert_eq!(
            parse_reading(r#"{"intent": "clarify", "card": null}"#),
            None
        );
        assert_eq!(parse_reading("sure"), None);
    }

    #[test]
    fn reads_a_question_back_a_tried_lookup_and_an_unclear_reply() {
        let r = parse_reading(
            r#"{"intent": "question", "reply": "A port is a door number, like 3306.", "card": {"headline": "Which number does MySQL use?"}}"#,
        );
        let Some(Reading::Clarify(c)) = r else {
            panic!("{r:?}")
        };
        assert_eq!(c.reply, "A port is a door number, like 3306.");
        assert!(card_text(&c).starts_with("A port is a door number, like 3306.\n\nWhich number"));
        assert!(matches!(
            parse_reading(r#"{"intent": "tried", "card": {"headline": "I found 3307. Use it?"}}"#),
            Some(Reading::FollowUp(_))
        ));
        assert!(matches!(
            parse_reading(r#"{"intent": "unclear", "card": {"headline": "Plainer"}}"#),
            Some(Reading::Clarify(_))
        ));
    }

    #[test]
    fn guides_once_the_owner_is_still_stuck() {
        let talk = [Exchange {
            asked: "Which port?".into(),
            answer: "what is a port".into(),
        }];
        let card = fallback_card(&facts(NGINX, &[]));
        assert!(!reading_prompt(&facts(NGINX, &[]), &card, "huh").contains("still stuck"));
        let p = reading_prompt(&facts(NGINX, &talk), &card, "huh");
        assert!(p.contains("still stuck") && p.contains("one small question"));
        assert!(p.contains("reply 2 about this step"));
        assert!(p.contains("read-only"));
    }

    #[test]
    fn never_asks_the_same_way_twice() {
        let talk = [Exchange {
            asked: "Which port does MySQL use?".into(),
            answer: "?".into(),
        }];
        let card = |headline: &str, wanted: &str| Card {
            headline: headline.into(),
            wanted: wanted.into(),
            ..Default::default()
        };
        let fresh = not_repeated(card("Which number is next to MySQL?", ""), "Now", &talk);
        assert_eq!(fresh.headline, "Which number is next to MySQL?");
        let again = not_repeated(
            card(
                "which port does MySQL use? ",
                "Open Wnmp and look next to MySQL.",
            ),
            "Now",
            &talk,
        );
        assert_eq!(again.headline, "Open Wnmp and look next to MySQL.");
        assert_eq!(again.wanted, "");
        let bare = not_repeated(card("Now", ""), "Now", &[]);
        assert_eq!(bare.headline, "Put another way: Now");
    }

    #[test]
    fn tells_the_next_try_the_talk_and_how_to_act() {
        let talk = [
            Exchange {
                asked: "Add one rule to the web server".into(),
                answer: "I don't understand".into(),
            },
            Exchange {
                asked: "The web server needs one more rule".into(),
                answer: "insert it yourself".into(),
            },
        ];
        let n = next_try_note(
            "needs an nginx rule",
            &talk,
            "insert it yourself",
            "Add the rule to nginx.conf yourself.",
        );
        assert!(n.contains("Owner: \"I don't understand\""));
        assert_eq!(n.matches("insert it yourself").count(), 1);
        assert!(n.contains(
            "What their answer means for this try: Add the rule to nginx.conf yourself."
        ));
        assert!(n.contains("ask for exactly that one thing"));
    }

    #[test]
    fn prompts_carry_the_step_the_message_and_the_talk() {
        let talk = [Exchange {
            asked: "Add one rule".into(),
            answer: "?".into(),
        }];
        let p = explain_prompt(&facts(NGINX, &talk));
        assert!(
            p.contains("alias /srv/app/files/;")
                && p.contains("Owner: ?")
                && p.contains("Answer with JSON only")
        );
        let card = fallback_card(&facts(NGINX, &[]));
        let r = reading_prompt(&facts(NGINX, &[]), &card, " do it yourself ");
        assert!(
            r.contains("The owner's reply (reply 1 about this step):\n<<<\ndo it yourself\n>>>")
        );
    }
}
