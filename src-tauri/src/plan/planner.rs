//! Planner and workers (Milestone 14), ported from `src/lib/plan/planner.ts`: a strong model turns a request into
//! task-list steps; Qanuvi checks them, numbers them and writes them into the file itself (the planner works
//! read-only). When a step fails even on the strongest model, the planner is asked once to rewrite it into smaller
//! steps.

use regex::{Captures, Regex};
use std::sync::LazyLock;

use super::js;
use super::routing::AgentModel;
use super::tasklist::{Step, parse_plan, split_lines};

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

fn ids(models: &[AgentModel]) -> String {
    models
        .iter()
        .map(|m| m.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn planner_prompt(request: &str, file: &str, models: &[AgentModel]) -> String {
    [
        "Plan the following work for this project. You are the planner: read the code as much as you need (read-only),".to_string(),
        "then answer with the plan only, as task-list steps inside one ```tasklist block. Do not change any file.".into(),
        String::new(),
        "Request:".into(),
        js::trim(request).to_string(),
        String::new(),
        "Format of each step (Qanuvi reads exactly this):".into(),
        "```tasklist".into(),
        "- [ ] **1. Short step title**".into(),
        "  - What to do, concretely: files, functions, behaviour. One point per line.".into(),
        "  - Validate: how to check it is done (a test, a command, what to look at).".into(),
        "  - Model: <model id>".into(),
        "```".into(),
        String::new(),
        "Rules:".into(),
        "- 2 to 10 steps, in order; each small enough for one focused session, and each leaves the project working.".into(),
        format!("- Choose a model per step from: {} (strongest first). Use the strongest for money, security and hard design; a mid model for normal coding; the smallest for docs and small edits.", ids(models)),
        format!("- Number the steps from 1; Qanuvi renumbers them to follow the steps already in {file}."),
        "- Put tests in the same step as the code they test.".into(),
    ]
    .join("\n")
}

pub fn replan_prompt(file: &str, step: &Step, failure: &str, models: &[AgentModel]) -> String {
    let number = step
        .number
        .as_deref()
        .filter(|n| !n.is_empty())
        .map(|n| format!("{n}. "))
        .unwrap_or_default();
    let note = if step.note.is_empty() {
        String::new()
    } else {
        format!(" {}", step.note)
    };
    let failure = js::trim(failure);
    let mut out = vec![
        format!(
            "A step in {file} was not finished, even by the strongest model. Rewrite it into 2 to 5 smaller steps that together do the same work, using what went wrong. Read the code as you need (read-only); answer with the steps only, inside one ```tasklist block, in the same format as the task list (with Validate and Model lines; models: {}).",
            ids(models)
        ),
        String::new(),
        "The step:".into(),
        format!("**{number}{}**{note}", step.title),
    ];
    out.extend(step.details.iter().map(|d| format!("  {d}")));
    out.extend([
        String::new(),
        "What went wrong:".into(),
        if failure.is_empty() {
            "(no details)".into()
        } else {
            failure.to_string()
        },
    ]);
    out.join("\n")
}

/// The steps block in a planner's answer and its steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Extracted {
    pub block: String,
    pub steps: Vec<Step>,
}

/// The steps block in a planner's answer: the ```tasklist (or markdown) block, else the whole answer.
pub fn extract_steps(answer: &str) -> Extracted {
    static FENCE: LazyLock<Regex> = LazyLock::new(|| {
        re(&format!(
            r"(?s)```(?i-u:tasklist|markdown|md)?{}*\n(.*?)```",
            js::S
        ))
    });
    static STEP: LazyLock<Regex> = LazyLock::new(|| re(r"^ ?- \[[ xX!-]\] "));
    static MARKED: LazyLock<Regex> = LazyLock::new(|| re(r"^ ?- \[[xX!-]\] "));
    static DETAIL: LazyLock<Regex> =
        LazyLock::new(|| re(&format!(r"^{s}{{2,}}[[^\s\x{{FEFF}}]\x{{85}}]", s = js::S)));
    let body = FENCE
        .captures(answer)
        .and_then(|c| c.get(1))
        .map_or(answer, |m| m.as_str());
    let block = body.replace("\r\n", "\n");
    let block = js::trim(&block);
    // Only the step lines and their details: drop headings and other text around them.
    let mut kept: Vec<String> = vec![];
    let mut in_step = false;
    for l in block.split('\n') {
        if STEP.is_match(l) {
            in_step = true;
            kept.push(MARKED.replace(l, "- [ ] ").into_owned());
        } else if in_step && (DETAIL.is_match(l) || js::trim(l).is_empty()) {
            kept.push(l.to_string());
        } else {
            in_step = false;
        }
    }
    let clean = js::trim(&kept.join("\n")).to_string();
    let steps = parse_plan(&clean).steps;
    Extracted {
        block: clean,
        steps,
    }
}

/// The highest whole step number in a task list (12 for "12" or "12b"), 0 when there is none.
pub fn last_number(text: &str) -> i64 {
    parse_plan(text)
        .steps
        .iter()
        .filter_map(|s| js::parse_int(s.number.as_deref().unwrap_or("")))
        .fold(0, i64::max)
}

/// Where a block's numbering starts: a number (`start`, `start + 1`, …) or a base ("12": 12a, 12b, …).
#[derive(Clone, Debug, PartialEq)]
pub enum Start<'a> {
    From(i64),
    Under(&'a str),
}

/// Gives the steps of a block new numbers.
pub fn renumber(block: &str, start: Start) -> String {
    static LINE: LazyLock<Regex> = LazyLock::new(|| {
        let (s, dot) = (js::S, js::DOT);
        re(&format!(
            r"^(- \[ \] )(?:\*\*(?:[0-9]+[a-z]?\.{s}+)?({dot}+?)\*\*({dot}*)|({dot}+))$"
        ))
    });
    let mut i: i64 = 0;
    let mut label = || {
        let l = match start {
            Start::From(n) => (n + i).to_string(),
            // String.fromCharCode(97 + i): past "z" the letters go on through the code points.
            Start::Under(b) => format!("{b}{}", char::from_u32(97 + i as u32).unwrap_or('?')),
        };
        i += 1;
        l
    };
    block
        .split('\n')
        .map(|l| match LINE.captures(l) {
            None => l.to_string(),
            Some(m) => {
                let title = js::trim(m.get(2).or(m.get(4)).map_or("", |x| x.as_str()));
                let rest = m.get(3).map_or("", |x| x.as_str());
                format!("{}**{}. {title}**{rest}", &m[1], label())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Numbers for `count` replacement steps of the step `number` that no step in the plan or its archive has (PLAN-F,
/// 2026-11-14). "12" → 12a, 12b, … (letters in use skipped; past z, 12-1, …); "97a" → 97a-1, 97a-2, …; "97a-2" →
/// the next free 97a-n; no number → after the highest whole number.
pub fn free_labels(text: &str, archive: &str, number: Option<&str>, count: usize) -> Vec<String> {
    static WHOLE: LazyLock<Regex> = LazyLock::new(|| re(r"^[0-9]+$"));
    static TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"-[0-9]+$"));
    let used: std::collections::HashSet<String> = parse_plan(text)
        .steps
        .into_iter()
        .chain(parse_plan(archive).steps)
        .filter_map(|s| s.number.filter(|n| !n.is_empty()))
        .collect();
    let mut out: Vec<String> = Vec::new();
    let take = |l: String, out: &mut Vec<String>| {
        if !used.contains(&l) && !out.contains(&l) {
            out.push(l);
        }
    };
    let Some(number) = number.filter(|n| !n.is_empty()) else {
        let mut n = last_number(text).max(last_number(archive)) + 1;
        while out.len() < count {
            take(n.to_string(), &mut out);
            n += 1;
        }
        return out;
    };
    if WHOLE.is_match(number) {
        for c in 0..26u8 {
            if out.len() >= count {
                break;
            }
            take(format!("{number}{}", (b'a' + c) as char), &mut out);
        }
        let mut k = 1;
        while out.len() < count {
            take(format!("{number}-{k}"), &mut out);
            k += 1;
        }
        return out;
    }
    let base = TAIL.replace(number, "").into_owned();
    let mut k = 1;
    while out.len() < count {
        take(format!("{base}-{k}"), &mut out);
        k += 1;
    }
    out
}

/// Gives the steps of a block the given numbers in order, dropping every number or label a planner wrote before a
/// title ("97b. 97a2. Schema", "97a-2a. Check" → "Schema", "Check"). Steps past the end of `labels` keep no number.
/// The block's Needs lines follow: a planner's own label there becomes the step's new number (PLAN-F 97a: "Needs:
/// 97a-2a" was left pointing at no step, and every step after it was skipped).
pub fn renumber_with(block: &str, labels: &[String]) -> String {
    static LINE: LazyLock<Regex> = LazyLock::new(|| {
        let dot = js::DOT;
        re(&format!(
            r"^(- \[ \] )(?:\*\*({dot}+?)\*\*({dot}*)|({dot}+))$"
        ))
    });
    static LABEL: LazyLock<Regex> = LazyLock::new(|| {
        re(&format!(
            r"^([0-9]+[a-z0-9]*(?:-[0-9]+[a-z0-9]*)*)\.{}+",
            js::S
        ))
    });
    static NEEDS: LazyLock<Regex> =
        LazyLock::new(|| re(&format!(r"^({s}*- Needs:{s}*)(.*)$", s = js::S)));
    let mut i = 0;
    let mut renamed: Vec<(String, String)> = Vec::new();
    let lines: Vec<String> = block
        .split('\n')
        .map(|l| match LINE.captures(l) {
            None => l.to_string(),
            Some(m) => {
                let label = labels.get(i).cloned();
                i += 1;
                let mut title =
                    js::trim(m.get(2).or(m.get(4)).map_or("", |x| x.as_str())).to_string();
                while let Some(c) = LABEL.captures(&title) {
                    let (old, all) = (c[1].to_string(), c[0].len());
                    if let Some(new) = &label
                        && !renamed.iter().any(|(o, _)| *o == old)
                    {
                        renamed.push((old, new.clone()));
                    }
                    title = title[all..].to_string();
                }
                let title = js::trim(&title);
                let rest = m.get(3).map_or("", |x| x.as_str());
                let label = label.map_or(String::new(), |l| format!("{l}. "));
                format!("{}**{label}{title}**{rest}", &m[1])
            }
        })
        .collect();
    lines
        .into_iter()
        .map(|l| match NEEDS.captures(&l) {
            Some(m) if !renamed.is_empty() => {
                let needs: Vec<String> = m[2]
                    .split(',')
                    .map(|n| {
                        let t = js::trim(n);
                        renamed
                            .iter()
                            .find(|(o, _)| o == t)
                            .map_or(t.to_string(), |(_, new)| new.clone())
                    })
                    .collect();
                format!("{}{}", &m[1], needs.join(", "))
            }
            _ => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Adds a block of steps as a new section at the end of a task list, keeping its line endings.
pub fn append_section(text: &str, title: &str, block: &str) -> String {
    static SPACES: LazyLock<Regex> = LazyLock::new(|| re(&format!("{}+", js::S)));
    let (_, eol) = split_lines(text);
    let title = SPACES.replace_all(title, " ");
    let title = js::cut(js::trim(&title), 80);
    let heading = format!(
        "## {}",
        if title.is_empty() {
            "Planned work"
        } else {
            title
        }
    );
    let body = [heading.as_str(), "", js::trim(block), ""]
        .join("\n")
        .replace('\n', eol);
    let base = js::trim_end(text);
    if base.is_empty() {
        body
    } else {
        format!("{base}{eol}{eol}{body}")
    }
}

/// Marks a failed step blocked with a note pointing at its replacement, and puts the replacement steps right after it
/// (after its details). The step is found by number and title, so other edits in the file do not matter.
pub fn replace_step(
    text: &str,
    number: Option<&str>,
    title: &str,
    block: &str,
    labels: &str,
) -> Result<String, String> {
    static MARK: LazyLock<Regex> = LazyLock::new(|| re(r"^( ?- \[)[ xX!-](\])"));
    let plan = parse_plan(text);
    let found = plan
        .steps
        .iter()
        .find(|s| s.number.as_deref() == number && s.title == title)
        .ok_or_else(|| "The step is no longer in the task list.".to_string())?;
    let (mut lines, eol) = split_lines(text);
    lines[found.line] = MARK
        .replace(&lines[found.line], |c: &Captures| {
            format!("{}!{}", &c[1], &c[2])
        })
        .into_owned();
    // The note belongs to the failed step: it goes right after its details, before the new steps.
    let note = format!(
        "  - Replaced by steps {labels}, rewritten by the planner after it failed on every model."
    );
    let mut insert = vec![note];
    insert.extend(js::trim(block).split('\n').map(String::from));
    let at = found.end.min(lines.len());
    lines.splice(at..at, insert);
    Ok(lines.join(eol))
}

/// "12a–12c" for a block numbered from base 12 with three steps.
pub fn label_range(block: &str) -> String {
    let nums: Vec<String> = parse_plan(block)
        .steps
        .into_iter()
        .filter_map(|s| s.number.filter(|n| !n.is_empty()))
        .collect();
    match nums.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, .., last] => format!("{first}–{last}"),
    }
}

/// A short section title from a request: its first sentence, cut at a word boundary to at most 60 characters.
pub fn section_title_from(request: &str) -> String {
    let first = js::trim(
        request
            .split('\n')
            .find(|l| !js::trim(l).is_empty())
            .unwrap_or(""),
    );
    // `first.split(/(?<=[.!?])\s/)[0]`: up to the first white space after a sentence's end.
    let mut sentence = first;
    let mut prev: Option<char> = None;
    for (i, c) in first.char_indices() {
        if matches!(prev, Some('.' | '!' | '?')) && is_js_space(c) {
            sentence = &first[..i];
            break;
        }
        prev = Some(c);
    }
    let sentence = sentence.trim_end_matches(['.', '!', '?']);
    if js::len(sentence) <= 60 {
        return if sentence.is_empty() {
            "Planned work".into()
        } else {
            sentence.into()
        };
    }
    let cut = js::cut(sentence, 60);
    let space = cut.rfind(' ').filter(|&i| js::len(&cut[..i]) > 30);
    let short = space.map_or(cut, |i| &cut[..i]);
    let short = short.strip_suffix([',', ';', ':']).unwrap_or(short);
    format!("{short}…")
}

fn is_js_space(c: char) -> bool {
    js::trim(c.encode_utf8(&mut [0; 4])).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::prompts::CLAUDE_CODE_MODELS;

    const ANSWER: &str = "I read src/cart.js and src/tax.js. Here is the plan:\n\n```tasklist\n## Plan\n- [ ] **1. Add VAT to the cart total**\n  - Give `cartTotal(items, { vat })` an optional VAT using `addVat` from src/tax.js.\n  - Validate: new tests in test/cart.test.js pass.\n  - Model: claude-sonnet-5-5\n\n- [x] **2. Document it**\n  - Add a README line.\n  - Model: claude-haiku-5-5\n```\n\nLet me know if you want changes.";

    fn numbers(text: &str) -> Vec<String> {
        parse_plan(text)
            .steps
            .into_iter()
            .filter_map(|s| s.number)
            .collect()
    }

    fn models() -> Vec<AgentModel> {
        CLAUDE_CODE_MODELS
            .iter()
            .map(|id| AgentModel {
                id: id.to_string(),
                label: String::new(),
                price: None,
            })
            .collect()
    }

    #[test]
    fn takes_only_the_steps_all_open() {
        let e = extract_steps(ANSWER);
        let got: Vec<_> = e
            .steps
            .iter()
            .map(|s| (s.number.as_deref(), s.title.as_str(), s.model.as_deref()))
            .collect();
        assert_eq!(
            got,
            [
                (
                    Some("1"),
                    "Add VAT to the cart total",
                    Some("claude-sonnet-5-5")
                ),
                (Some("2"), "Document it", Some("claude-haiku-5-5")),
            ]
        );
        assert!(!e.block.contains("## Plan"));
        assert!(!e.block.contains("- [x]"));
        assert!(extract_steps("I could not plan this.").steps.is_empty());
    }

    #[test]
    fn continues_after_the_last_number() {
        let block = extract_steps(ANSWER).block;
        assert_eq!(last_number("- [x] **3. A**\n- [ ] **12b. B**\n- [ ] C"), 12);
        assert_eq!(last_number("# empty"), 0);
        assert_eq!(numbers(&renumber(&block, Start::From(13))), ["13", "14"]);
        let sub = renumber(&block, Start::Under("12"));
        assert_eq!(numbers(&sub), ["12a", "12b"]);
        assert_eq!(label_range(&sub), "12a–12b");
        assert_eq!(
            renumber("- [ ] Plain step", Start::From(5)),
            "- [ ] **5. Plain step**"
        );
    }

    #[test]
    fn appends_a_section_keeping_crlf() {
        let out = append_section(
            "# Plan\r\n\r\n- [x] **1. Old**\r\n",
            "VAT in the cart",
            "- [ ] **2. New**\n  - detail",
        );
        assert_eq!(
            out,
            "# Plan\r\n\r\n- [x] **1. Old**\r\n\r\n## VAT in the cart\r\n\r\n- [ ] **2. New**\r\n  - detail\r\n"
        );
        assert_eq!(
            append_section("", "", "- [ ] **1. X**"),
            "## Planned work\n\n- [ ] **1. X**\n"
        );
    }

    #[test]
    fn blocks_the_failed_step_and_inserts_the_new_steps() {
        let file = "- [ ] **12. Big step**\n  - Do everything.\n- [ ] **13. Next**";
        let sub = renumber(
            "- [ ] **1. Part one**\n- [ ] **2. Part two**",
            Start::Under("12"),
        );
        let out = replace_step(file, Some("12"), "Big step", &sub, &label_range(&sub)).unwrap();
        assert_eq!(
            out.split('\n').collect::<Vec<_>>(),
            [
                "- [!] **12. Big step**",
                "  - Do everything.",
                "  - Replaced by steps 12a–12b, rewritten by the planner after it failed on every model.",
                "- [ ] **12a. Part one**",
                "- [ ] **12b. Part two**",
                "- [ ] **13. Next**",
            ]
        );
        assert!(replace_step(file, Some("99"), "Gone", &sub, "").is_err());
    }

    #[test]
    fn asks_for_a_read_only_plan() {
        let p = planner_prompt("Add VAT to the cart", "TASKLIST.md", &models());
        assert!(p.contains("Add VAT to the cart"));
        assert!(p.contains("```tasklist"));
        assert!(p.contains("claude-haiku-5-5"));
        assert!(p.contains("Do not change any file"));
        let step = &parse_plan("- [ ] **4. Sort**\n  - Sort it.").steps[0];
        let r = replan_prompt(
            "TASKLIST.md",
            step,
            "Tests kept failing: x !== y",
            &models(),
        );
        assert!(r.contains("**4. Sort**"));
        assert!(r.contains("Tests kept failing"));
    }

    #[test]
    fn uses_the_first_sentence() {
        assert_eq!(
            section_title_from("Add coupons. Then test them."),
            "Add coupons"
        );
        assert_eq!(
            section_title_from(
                "Add optional VAT to the cart total using addVat from src/tax.js, with tests."
            ),
            "Add optional VAT to the cart total using addVat from…"
        );
        assert_eq!(section_title_from("  "), "Planned work");
    }
}
