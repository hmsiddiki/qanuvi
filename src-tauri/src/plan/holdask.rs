//! A held plan step as an Inbox question (24u part 4), ported from `src/lib/plan/holdask.ts`: its key, its form and
//! what its answer runs with.

use super::setaside::HeldKind;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerChoice {
    Go,
    Skip,
    Decide,
    /// n1: end the run once what can still run is done; the step stays open for the next run.
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxAnswer {
    pub choice: AnswerChoice,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
}

/// One question per plan step: a later run holding the same step asks the same question, not a new one.
pub fn hold_key(project_id: &str, file: &str, step: &str) -> String {
    format!("hold:{project_id}:{file}:{step}")
}

/// The hold asks for a login, password or key: such answers are typed in the run panel only. p2 holds name the
/// project secret as a variable (DB_PASSWORD, SHOP_API_KEY), whose underscores join the words, so those count too.
pub fn asks_secret(why: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?-u:\b)(password|passphrase|log ?in|sign[- ]?in|credential|token|api[- ]?key|secret|ssh key)s?(?-u:\b)").unwrap()
    });
    static VAR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?-u:\b)[A-Z0-9_]*_?(PASSWORD|PASSWD|PASS|PWD|PASSPHRASE|TOKEN|SECRET|KEY|CREDENTIALS?)(_[A-Z0-9_]*)?(?-u:\b)").unwrap()
    });
    RE.is_match(why) || VAR.is_match(why)
}

/// "Let Qanuvi decide" as the held step is told it.
pub const DECIDE_TEXT: &str = "Decide this yourself: choose what fits the plan and the project best, take the safer way when unsure, and say in your summary what you chose and why.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormChoice {
    pub id: AnswerChoice,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldForm {
    pub choices: Vec<FormChoice>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub secret: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    /// n1: the question laid out (what happened, why the owner, what is wanted, a snippet to copy).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub card: Option<super::ownerask::Card>,
}

/// A hold's reason as the start of the Inbox text: "it needs ..." reads "It needs ..." there (f3).
pub fn sentence_start(why: &str) -> String {
    let mut chars = why.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The answer form of a held step, worded like the run panel's.
pub fn hold_form(kind: HeldKind, why: &str) -> HoldForm {
    // A question the check left open is never a login: the check asks for those apart (its secrets), so words like
    // "the login already on this computer" in a question do not lock it out of the Inbox and the terminal.
    let secret = !why.starts_with(super::preflight::OPEN_QUESTION) && asks_secret(why);
    let hold = kind == HeldKind::Hold;
    let choice = |id, label: &str, hint: Option<&str>| FormChoice {
        id,
        label: label.into(),
        hint: hint.map(Into::into),
    };
    let mut choices = vec![choice(
        AnswerChoice::Go,
        if hold { "Go ahead" } else { "Try again" },
        None,
    )];
    if hold && !secret {
        choices.push(choice(
            AnswerChoice::Decide,
            "Let Qanuvi decide",
            Some("It picks the safer way and says what it chose"),
        ));
    }
    if !hold {
        choices[0].hint = Some(
            "Run the step again as it is (when you changed something, say what in your answer)"
                .into(),
        );
    }
    choices.push(choice(
        AnswerChoice::Skip,
        "Skip this step",
        Some("Leave it, and the steps waiting for it"),
    ));
    if !hold {
        choices.push(choice(
            AnswerChoice::End,
            "End the run",
            Some("The run finishes what it still can, then ends; this step stays open for the next run"),
        ));
    }
    HoldForm {
        choices,
        secret: secret.then_some(true),
        text: (!secret).then(|| {
            if hold {
                "Your answer (optional)".to_string()
            } else {
                super::ownerask::ANSWER_HINT.to_string()
            }
        }),
        card: None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldReply {
    pub choice: AnswerChoice,
    pub text: String,
}

/// What a held step runs with for an answer: "decide" goes ahead with the deciding note.
pub fn hold_reply(answer: &InboxAnswer) -> HoldReply {
    let text = answer.text.as_deref().unwrap_or("").trim().to_string();
    match answer.choice {
        AnswerChoice::Skip => HoldReply {
            choice: AnswerChoice::Skip,
            text,
        },
        AnswerChoice::End => HoldReply {
            choice: AnswerChoice::End,
            text,
        },
        AnswerChoice::Decide => HoldReply {
            choice: AnswerChoice::Go,
            text: [DECIDE_TEXT.to_string(), text]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
        },
        AnswerChoice::Go => HoldReply {
            choice: AnswerChoice::Go,
            text,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(f: &HoldForm) -> Vec<AnswerChoice> {
        f.choices.iter().map(|c| c.id).collect()
    }

    #[test]
    fn keys_one_question_per_plan_step() {
        assert_eq!(
            hold_key("p1", "docs/PLAN.md", "1c"),
            "hold:p1:docs/PLAN.md:1c"
        );
    }

    #[test]
    fn offers_go_decide_and_skip_for_a_hold() {
        let f = hold_form(HeldKind::Hold, "it pushes to the live server");
        assert_eq!(
            ids(&f),
            [AnswerChoice::Go, AnswerChoice::Decide, AnswerChoice::Skip]
        );
        assert_eq!(f.choices[0].label, "Go ahead");
        assert!(f.text.is_some() && f.secret.is_none());
    }

    #[test]
    fn offers_try_again_and_skip_for_a_failed_step() {
        let f = hold_form(HeldKind::Failed, "the tests kept failing");
        let labels: Vec<&str> = f.choices.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Try again", "Skip this step", "End the run"]);
        // n1: a real answer box, not "what to do differently".
        assert_eq!(f.text.as_deref(), Some(super::super::ownerask::ANSWER_HINT));
    }

    #[test]
    fn never_asks_for_a_login_in_the_inbox() {
        assert!(asks_secret("it needs the FTP password"));
        assert!(asks_secret("needs an API key for the mail service"));
        assert!(asks_secret("sign in to the admin page"));
        assert!(!asks_secret("it changes the date format"));
        let q = hold_form(
            HeldKind::Hold,
            "a question was left open (May the run use the Claude login already on this computer?)",
        );
        assert_eq!(
            q.secret, None,
            "a question from the check is answered anywhere"
        );
        assert!(q.choices.iter().any(|c| c.id == AnswerChoice::Decide));
        assert_eq!(
            hold_form(HeldKind::Hold, "it needs the FTP password").secret,
            Some(true)
        );
        // p2 holds name the secret as a variable.
        assert!(asks_secret(
            "it needs The test key (SHOP_API_KEY). Before the run you wrote \"x\""
        ));
        assert!(asks_secret("it needs Where it signs (DB_PASSWORD)"));
        assert!(!asks_secret(
            "it needs Where the test database is (DB_HOST)"
        ));
        assert!(!asks_secret("the key point is the date (KEYBOARD_LAYOUT)"));
        let f = hold_form(HeldKind::Hold, "it needs the database login");
        assert_eq!(f.secret, Some(true));
        assert!(f.text.is_none());
        assert_eq!(ids(&f), [AnswerChoice::Go, AnswerChoice::Skip]);
    }

    #[test]
    fn turns_an_answer_into_what_the_step_runs_with() {
        let a = |choice, text: Option<&str>| InboxAnswer {
            choice,
            text: text.map(Into::into),
        };
        assert_eq!(
            hold_reply(&a(AnswerChoice::Go, Some(" yes "))),
            HoldReply {
                choice: AnswerChoice::Go,
                text: "yes".into()
            }
        );
        assert_eq!(
            hold_reply(&a(AnswerChoice::Skip, None)),
            HoldReply {
                choice: AnswerChoice::Skip,
                text: String::new()
            }
        );
        assert_eq!(hold_reply(&a(AnswerChoice::Decide, None)).text, DECIDE_TEXT);
        assert_eq!(
            hold_reply(&a(AnswerChoice::Decide, Some("keep it small"))).text,
            format!("{DECIDE_TEXT}\nkeep it small")
        );
    }
}
