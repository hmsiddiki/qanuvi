// A master's Inbox from its own terminal (migration M7b): `qanuvi-master inbox list | show | answer`. A thin client
// of the running service with the inbox_* commands a window sends, so the first answer anywhere still counts and a
// question answered here closes in every window. A run reads its answers from this Inbox itself.
//
// Items are numbered in the list's own order (open questions first, then newest), and each also shows the start of
// its id; either one picks it. An answer by number says which question it answered, so a list changed in between
// cannot go unnoticed.

use super::cli::{call, words};
use super::{Value, json};
use std::path::Path;

const USAGE: &str = "Usage: qanuvi-master inbox list [--all]
       qanuvi-master inbox show <number or id>
       qanuvi-master inbox answer <number or id> <choice> [text]";

/// How many items `list` shows without --all.
const SHOWN: usize = 20;

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(at_ms: i64, now_ms: i64) -> String {
    let s = (now_ms - at_ms).max(0) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        _ => {
            let d = s / 86400;
            format!("{d} {} ago", if d == 1 { "day" } else { "days" })
        }
    }
}

/// The few words in front of an item: what it wants from the owner.
fn tag(item: &Value) -> &'static str {
    let review = item["source"]["kind"].as_str() == Some("review");
    match (item["type"].as_str(), item["status"].as_str()) {
        // Y6 (y9-5): the plan's review list; nothing waits on it.
        (Some("answer"), Some("open")) if review => "to look at",
        (Some("answer"), Some("open")) => "needs your answer",
        (Some("answer"), Some("answered")) => "answered",
        (Some("answer"), _) => "no longer asked",
        (Some("problem"), _) => "problem",
        _ => "note",
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The list's lines (`all`: every item, else the first SHOWN).
pub fn list_lines(items: &[Value], all: bool, now: i64) -> Vec<String> {
    if items.is_empty() {
        return vec!["The Inbox is empty.".into()];
    }
    let mut out: Vec<String> = items
        .iter()
        .enumerate()
        .take(if all { usize::MAX } else { SHOWN })
        .map(|(i, it)| {
            format!(
                "{:>3}. {}{}  [{}]  {}  [{}]",
                i + 1,
                if it["read"] == false { "* " } else { "" },
                it["title"].as_str().unwrap_or(""),
                tag(it),
                ago(it["at"].as_i64().unwrap_or(0), now),
                short(it["id"].as_str().unwrap_or(""))
            )
        })
        .collect();
    if !all && items.len() > SHOWN {
        out.push(format!(
            "     … and {} older (qanuvi-master inbox list --all).",
            items.len() - SHOWN
        ));
    }
    let asks = items
        .iter()
        .filter(|i| matches!(tag(i), "needs your answer" | "to look at"))
        .count();
    if asks > 0 {
        out.push(
            "See one: qanuvi-master inbox show <number>. Answer: qanuvi-master inbox answer <number> <choice> [text]"
                .into(),
        );
    }
    out
}

/// The item a typed word means: its number in the list, or the start of its id (at least 4 characters, one match).
pub fn pick<'a>(items: &'a [Value], which: &str) -> Result<(usize, &'a Value), String> {
    let which = which.trim().trim_end_matches('.');
    if let Ok(n) = which.parse::<usize>() {
        return items
            .get(n.wrapping_sub(1))
            .map(|it| (n, it))
            .ok_or_else(|| {
                format!(
                    "The Inbox has no item {n} (it has {}). See: qanuvi-master inbox list",
                    items.len()
                )
            });
    }
    if which.chars().count() < 4 {
        return Err(format!(
            "\"{which}\" is not a number or an id. See: qanuvi-master inbox list"
        ));
    }
    let found: Vec<(usize, &Value)> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| {
            it["id"]
                .as_str()
                .is_some_and(|id| id.to_lowercase().starts_with(&which.to_lowercase()))
        })
        .map(|(i, it)| (i + 1, it))
        .collect();
    match found.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!(
            "No Inbox item has the id {which}. See: qanuvi-master inbox list"
        )),
        _ => Err(format!(
            "{} items start with {which}; type more of the id.",
            found.len()
        )),
    }
}

fn choices(item: &Value) -> Vec<Value> {
    item["form"]["choices"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn label_of(item: &Value, choice: &str) -> String {
    choices(item)
        .iter()
        .find(|c| c["id"] == choice)
        .and_then(|c| c["label"].as_str())
        .unwrap_or(choice)
        .to_string()
}

/// One item in full, with its answer form when it is still asked.
pub fn show_lines(n: usize, item: &Value, now: i64) -> Vec<String> {
    let mut out = vec![
        format!(
            "{n}. {}  [{}]",
            item["title"].as_str().unwrap_or(""),
            tag(item)
        ),
        format!(
            "   {}, id {}",
            ago(item["at"].as_i64().unwrap_or(0), now),
            item["id"].as_str().unwrap_or("")
        ),
    ];
    let text = item["text"].as_str().unwrap_or("").trim();
    if !text.is_empty() {
        out.push(String::new());
        out.extend(text.lines().map(|l| format!("   {l}")));
    }
    match tag(item) {
        "needs your answer" | "to look at" => {
            out.push(String::new());
            if item["form"]["secret"] == true {
                out.push("   This asks for a login or password. Type it in the run panel of the plan in a Qanuvi".into());
                out.push(
                    "   window, so it is kept as a private detail; it is not answered here.".into(),
                );
                return out;
            }
            out.push("   Choices:".into());
            for c in choices(item) {
                let hint = c["hint"].as_str().unwrap_or("");
                out.push(format!(
                    "     {}  {}{}",
                    c["id"].as_str().unwrap_or(""),
                    c["label"].as_str().unwrap_or(""),
                    if hint.is_empty() {
                        String::new()
                    } else {
                        format!(" ({hint})")
                    }
                ));
            }
            if let Some(p) = item["form"]["text"].as_str() {
                out.push(format!("   A text may follow the choice: {p}"));
            }
            let first = choices(item)
                .first()
                .and_then(|c| c["id"].as_str().map(String::from))
                .unwrap_or_else(|| "<choice>".into());
            out.push(format!(
                "   Answer: qanuvi-master inbox answer {n} {first}{}",
                if item["form"]["text"].is_string() {
                    " [text]"
                } else {
                    ""
                }
            ));
        }
        "answered" => {
            let said = item["answer"]["text"].as_str().unwrap_or("").trim();
            let by = match item["answeredBy"].as_str() {
                Some("terminal") => " in a terminal",
                _ => "",
            };
            out.push(String::new());
            out.push(format!(
                "   Answered{by}: {}{}",
                label_of(item, item["answer"]["choice"].as_str().unwrap_or("")),
                if said.is_empty() {
                    String::new()
                } else {
                    format!(" · \"{said}\"")
                }
            ));
        }
        _ => {}
    }
    out
}

/// The answer a typed choice and text make, or why it cannot be given.
pub fn answer_of(item: &Value, choice: &str, text: &str) -> Result<Value, String> {
    match tag(item) {
        "needs your answer" | "to look at" => {}
        "answered" => return Err("This question is already answered.".into()),
        "no longer asked" => return Err("This question is no longer asked.".into()),
        _ => return Err("This item is a note, not a question; nothing to answer.".into()),
    }
    if item["form"]["secret"] == true {
        return Err("This asks for a login or password. Type it in the run panel of the plan in a Qanuvi window, so it is kept as a private detail.".into());
    }
    let all = choices(item);
    let c = all
        .iter()
        .find(|c| {
            c["id"]
                .as_str()
                .is_some_and(|id| id.eq_ignore_ascii_case(choice))
        })
        .or_else(|| {
            all.iter().find(|c| {
                c["label"]
                    .as_str()
                    .is_some_and(|l| l.eq_ignore_ascii_case(choice))
            })
        })
        .ok_or_else(|| {
            let ids: Vec<&str> = all.iter().filter_map(|c| c["id"].as_str()).collect();
            format!(
                "\"{choice}\" is not one of the choices: {}.",
                ids.join(", ")
            )
        })?;
    let text = text.trim();
    if !text.is_empty() && !item["form"]["text"].is_string() {
        return Err("This question takes a choice only, no text.".into());
    }
    Ok(json!({"choice": c["id"], "text": text}))
}

fn items(dir: &Path) -> Result<Vec<Value>, String> {
    Ok(call(dir, "inbox_list", json!({}))?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

/// `qanuvi-master inbox …`; `args` are the words after `inbox`.
pub fn inbox_cli(dir: &Path, args: &[String]) -> Result<(), String> {
    let w = words(args);
    let print = |lines: Vec<String>| lines.iter().for_each(|l| println!("{l}"));
    match w.first().copied() {
        Some("list") | None => {
            print(list_lines(
                &items(dir)?,
                args.iter().any(|a| a == "--all"),
                now_ms(),
            ));
            Ok(())
        }
        Some("show") => {
            let all = items(dir)?;
            let (n, item) = pick(&all, w.get(1).ok_or(USAGE)?)?;
            print(show_lines(n, item, now_ms()));
            Ok(())
        }
        Some("answer") => {
            let (Some(which), Some(choice)) = (w.get(1), w.get(2)) else {
                return Err(USAGE.into());
            };
            let all = items(dir)?;
            let (n, item) = pick(&all, which)?;
            let answer = answer_of(item, choice, &w[3..].join(" "))?;
            let done = call(
                dir,
                "inbox_answer",
                json!({"id": item["id"], "answer": answer, "by": "terminal"}),
            )?;
            if done.is_null() {
                return Err("Already answered somewhere else.".into());
            }
            println!(
                "Answered {n}. {}: {}.",
                item["title"].as_str().unwrap_or(""),
                label_of(item, answer["choice"].as_str().unwrap_or(""))
            );
            Ok(())
        }
        Some(_) => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask() -> Value {
        json!({
            "id": "abcd1234-0000", "at": 0, "type": "answer", "status": "open", "read": false,
            "title": "Step 2 waits for you", "text": "It deletes old rows.",
            "form": {"choices": [{"id": "go", "label": "Go ahead"}, {"id": "skip", "label": "Skip it", "hint": "and what waits for it"}],
                     "text": "Anything to tell it"}
        })
    }

    fn note(id: &str) -> Value {
        json!({"id": id, "at": 0, "type": "info", "status": "open", "read": true, "title": "Run finished"})
    }

    #[test]
    fn times_read_as_people_say_them() {
        assert_eq!(ago(0, 30_000), "just now");
        assert_eq!(ago(0, 5 * 60_000), "5 min ago");
        assert_eq!(ago(0, 3 * 3_600_000), "3 h ago");
        assert_eq!(ago(0, 86_400_000), "1 day ago");
        assert_eq!(ago(10, 0), "just now");
    }

    #[test]
    fn the_list_numbers_items_and_says_how_to_answer() {
        let l = list_lines(&[ask(), note("ffff0000-1")], false, 120_000);
        assert_eq!(
            l[0],
            "  1. * Step 2 waits for you  [needs your answer]  2 min ago  [abcd1234]"
        );
        assert_eq!(l[1], "  2. Run finished  [note]  2 min ago  [ffff0000]");
        assert!(l[2].contains("inbox answer <number>"));
        assert_eq!(list_lines(&[], false, 0), ["The Inbox is empty."]);
        let many: Vec<Value> = (0..25).map(|i| note(&format!("id{i:04}xx"))).collect();
        let l = list_lines(&many, false, 0);
        assert_eq!(l.len(), 21);
        assert!(l[20].contains("5 older"));
        assert_eq!(list_lines(&many, true, 0).len(), 25);
    }

    #[test]
    fn items_are_picked_by_number_or_id_start() {
        let all = [ask(), note("abcd9999-1"), note("ffff0000-1")];
        assert_eq!(pick(&all, "2").unwrap().0, 2);
        assert_eq!(pick(&all, "3.").unwrap().0, 3);
        assert!(pick(&all, "0").is_err());
        assert!(pick(&all, "4").unwrap_err().contains("has 3"));
        assert_eq!(pick(&all, "FFFF").unwrap().0, 3);
        assert!(pick(&all, "abcd").unwrap_err().contains("2 items"));
        assert_eq!(pick(&all, "abcd1").unwrap().0, 1);
        assert!(pick(&all, "ab").is_err());
    }

    #[test]
    fn answers_follow_the_form() {
        let q = ask();
        assert_eq!(
            answer_of(&q, "go", " be careful ").unwrap(),
            json!({"choice": "go", "text": "be careful"})
        );
        assert_eq!(answer_of(&q, "skip it", "").unwrap()["choice"], "skip");
        assert!(answer_of(&q, "maybe", "").unwrap_err().contains("go, skip"));
        let mut plain = ask();
        plain["form"].as_object_mut().unwrap().remove("text");
        assert!(
            answer_of(&plain, "go", "hi")
                .unwrap_err()
                .contains("choice only")
        );
        let mut secret = ask();
        secret["form"]["secret"] = json!(true);
        assert!(
            answer_of(&secret, "go", "")
                .unwrap_err()
                .contains("run panel")
        );
        let mut done = ask();
        done["status"] = json!("answered");
        assert!(answer_of(&done, "go", "").unwrap_err().contains("already"));
        assert!(
            answer_of(&note("x"), "go", "")
                .unwrap_err()
                .contains("note")
        );
    }

    /// A plan's list to look at is answered from a terminal as in the window: "All fine", or words for a new step.
    #[test]
    fn a_list_to_look_at_is_answered_and_shows_its_choices() {
        let list = json!({"id": "r1", "at": 0, "type": "answer", "status": "open", "read": false,
            "title": "proj: 2 things to look at from PLAN.md",
            "source": {"kind": "review", "entries": []},
            "form": {"choices": [{"id": "ok", "label": "All fine", "hint": "Closes this list"}],
                     "text": "Or say what to change"}});
        assert_eq!(tag(&list), "to look at");
        assert_eq!(answer_of(&list, "ok", "").unwrap()["choice"], "ok");
        assert_eq!(answer_of(&list, "All fine", "").unwrap()["choice"], "ok");
        let shown = show_lines(2, &list, 0).join("\n");
        assert!(
            shown.contains("All fine") && shown.contains("inbox answer 2 ok"),
            "{shown}"
        );
    }

    #[test]
    fn show_gives_the_form_or_the_answer() {
        let l = show_lines(1, &ask(), 0);
        assert!(
            l.iter()
                .any(|x| x == "     skip  Skip it (and what waits for it)")
        );
        assert!(
            l.iter()
                .any(|x| x == "   Answer: qanuvi-master inbox answer 1 go [text]")
        );
        let mut done = ask();
        done["status"] = json!("answered");
        done["answer"] = json!({"choice": "skip", "text": "later"});
        done["answeredBy"] = json!("terminal");
        let l = show_lines(1, &done, 0);
        assert_eq!(
            l.last().unwrap(),
            "   Answered in a terminal: Skip it · \"later\""
        );
    }
}
