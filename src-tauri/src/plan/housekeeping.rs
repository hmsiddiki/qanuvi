//! Plan housekeeping (24j), ported from `src/lib/plan/housekeeping.ts`: move finished sections to an archive file next
//! to the plan, and add another plan's steps into this one. Both are pure: they return the new texts and what
//! changed, so the Plan view can show a preview and write only after the user confirms.

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::js;
use super::tasklist::{Section, Step, StepStatus, parse_plan, split_lines};

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

fn ends_with_md(path: &str) -> bool {
    path.len() >= 3
        && path.is_char_boundary(path.len() - 3)
        && path[path.len() - 3..].eq_ignore_ascii_case(".md")
}

/// The archive next to a plan: "docs/PLAN.md" becomes "docs/PLAN.archive.md".
pub fn archive_path(plan_path: &str) -> String {
    if ends_with_md(plan_path) {
        format!("{}.archive.md", &plan_path[..plan_path.len() - 3])
    } else {
        format!("{plan_path}.archive.md")
    }
}

/// Whether a file is a plan archive (never offered as a task list itself).
pub fn is_archive(path: &str) -> bool {
    let n = ".archive.md".len();
    path.len() >= n
        && path.is_char_boundary(path.len() - n)
        && path[path.len() - n..].eq_ignore_ascii_case(".archive.md")
}

/// Step numbers marked done in a plan's text (its archive included, when given), in the order found.
pub fn done_numbers(texts: &[Option<&str>]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for t in texts.iter().flatten().filter(|t| !t.is_empty()) {
        for s in parse_plan(t).steps {
            if s.status == StepStatus::Done
                && let Some(n) = s.number.filter(|n| !n.is_empty())
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivedSection {
    pub title: String,
    pub steps: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub plan: String,
    pub archive: String,
    pub sections: Vec<ArchivedSection>,
}

/// Sections under a heading whose steps are all done.
pub fn finished_sections(text: &str) -> Vec<Section> {
    parse_plan(text)
        .sections
        .into_iter()
        .filter(|s| {
            s.line >= 0
                && !s.steps.is_empty()
                && s.steps.iter().all(|x| x.status == StepStatus::Done)
        })
        .collect()
}

/// Moves every finished section (heading, steps and any text under it, up to the next heading) to the end of the
/// archive. `archive_text` is the archive as it is now, or None when there is none yet. None when nothing is finished.
pub fn archive_finished(
    text: &str,
    archive_text: Option<&str>,
    plan_path: &str,
) -> Option<ArchiveResult> {
    static HEADING: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^#{{1,6}}{}", js::S)));
    let (lines, eol) = split_lines(text);
    let all_headings: Vec<usize> = (0..lines.len())
        .filter(|&i| HEADING.is_match(&lines[i]))
        .collect();
    let finished = finished_sections(text);
    if finished.is_empty() {
        return None;
    }
    let blocks: Vec<(usize, usize)> = finished
        .iter()
        .map(|s| {
            let from = s.line as usize;
            let to = all_headings
                .iter()
                .copied()
                .find(|&h| h > from)
                .unwrap_or(lines.len());
            (from, to)
        })
        .collect();
    let moved: Vec<Vec<String>> = blocks
        .iter()
        .map(|&(from, to)| {
            let mut block = lines[from..to].to_vec();
            while block.last().is_some_and(|l| js::trim(l).is_empty()) {
                block.pop();
            }
            block
        })
        .collect();
    let mut kept = lines.clone();
    for &(from, to) in blocks.iter().rev() {
        kept.drain(from..to);
    }
    // The plan's end stays as it was: one final newline if it had one.
    while kept.len() > 1
        && js::trim(&kept[kept.len() - 1]).is_empty()
        && js::trim(&kept[kept.len() - 2]).is_empty()
    {
        kept.pop();
    }
    // An archive that is there but empty is like none (the TypeScript tests it for truth).
    let there = archive_text.filter(|a| !a.is_empty());
    let aeol = there.map_or(eol, |a| split_lines(a).1);
    let head = match there {
        Some(a) => js::trim_end(a).to_string(),
        None => [
            format!("# Archive of {}", plan_path.rsplit('/').next().unwrap_or(plan_path)),
            String::new(),
            format!("Finished sections moved here from {plan_path}. Plan runs still count their steps as done."),
        ]
        .join(aeol),
    };
    let mut parts = vec![head];
    parts.extend(moved.iter().map(|m| m.join(aeol)));
    let archive = parts.join(&format!("{aeol}{aeol}")) + aeol;
    Some(ArchiveResult {
        plan: kept.join(eol),
        archive,
        sections: finished
            .iter()
            .map(|s| ArchivedSection {
                title: s.title.clone(),
                steps: s.steps.len(),
            })
            .collect(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub text: String,
    /// Steps added.
    pub added: usize,
    /// Done steps left out (only with `only_open`).
    pub left_out: usize,
    /// Numbers that were taken in this plan and got a new one: old to new.
    pub renumbered: Vec<(String, String)>,
    /// Section titles added, with their step counts.
    pub sections: Vec<ArchivedSection>,
}

/// Adds the steps of another plan (`source`, at `source_path`) to the end of this one, under their own headings.
/// Numbers this plan already uses get the next free ones (a step "45e" keeps its letter: "112e"), and the added steps'
/// "- Needs:" lines follow: a need on another added step takes its new number, a need on a step left out points to
/// the source plan ("PLAN.md#4"), and a need on this plan ("THIS.md#4") becomes plain "4". `taken`: numbers used
/// elsewhere that must not be reused (the plan's archive). The source file itself is not changed.
pub fn merge_plans(
    target: &str,
    target_path: &str,
    source: &str,
    source_path: &str,
    only_open: bool,
    taken: &[String],
) -> Option<MergeResult> {
    static TITLE_NUM: LazyLock<Regex> = LazyLock::new(|| {
        re(&format!(
            r"^( ?- \[[ xX!-]\] \*\*)([0-9]+[a-z]?(?:-[0-9]+)?)(\.{})",
            js::S
        ))
    });
    static NEEDS_LINE: LazyLock<Regex> = LazyLock::new(|| {
        let (s, dot) = (js::S, js::DOT);
        re(&format!(r"^({s}*- (?i-u:needs):{s}*)({dot}+)$"))
    });
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"(?:,|{})+", js::S)));
    static DIGITS: LazyLock<Regex> = LazyLock::new(|| re(r"[0-9]+"));

    let t_plan = parse_plan(target);
    let (t_lines, eol) = split_lines(target);
    let s_plan = parse_plan(source);
    let (s_lines, _) = split_lines(source);

    let base = |n: Option<&str>| n.filter(|n| !n.is_empty()).and_then(js::parse_int);
    // Numbers in use: this plan's, and those given as taken (its archive's).
    let t_bases: HashSet<i64> = t_plan
        .steps
        .iter()
        .map(|s| base(s.number.as_deref()))
        .chain(taken.iter().map(|n| base(Some(n))))
        .flatten()
        .collect();
    let mut s_bases: Vec<i64> = s_plan
        .steps
        .iter()
        .filter_map(|s| base(s.number.as_deref()))
        .collect();
    s_bases.sort();
    s_bases.dedup();
    let mut free = t_bases.iter().chain(&s_bases).copied().fold(0, i64::max) + 1;
    let mut base_map: HashMap<i64, i64> = HashMap::new();
    for &b in &s_bases {
        let to = if t_bases.contains(&b) {
            free += 1;
            free - 1
        } else {
            b
        };
        base_map.insert(b, to);
    }
    let renumber = |n: &str| -> String {
        DIGITS
            .replace_all(n, |c: &Captures| {
                let d = &c[0];
                match js::parse_int(d).and_then(|v| base_map.get(&v)) {
                    Some(m) => m.to_string(),
                    None => d.to_string(),
                }
            })
            .into_owned()
    };

    let keep = |s: &Step| !only_open || s.status != StepStatus::Done;
    let kept: HashSet<&str> = s_plan
        .steps
        .iter()
        .filter(|s| keep(s))
        .filter_map(|s| s.number.as_deref().filter(|n| !n.is_empty()))
        .collect();
    let mut renumbered: Vec<(String, String)> = vec![];
    for s in &s_plan.steps {
        if let Some(n) = s.number.as_deref().filter(|n| !n.is_empty())
            && keep(s)
            && renumber(n) != n
        {
            renumbered.push((n.to_string(), renumber(n)));
        }
    }

    let strip = |f: &str| f.strip_prefix("./").unwrap_or(f).to_string();
    let same_file = |f: &str| strip(f) == strip(target_path);
    let map_need = |need: &str| -> String {
        if let Some(hash) = need.rfind('#').filter(|&h| h > 0) {
            return if same_file(&need[..hash]) {
                need[hash + 1..].to_string()
            } else {
                need.to_string()
            };
        }
        if !need.starts_with(|c: char| c.is_ascii_digit()) {
            return need.to_string();
        }
        if kept.contains(need) {
            renumber(need)
        } else {
            format!("{source_path}#{need}")
        }
    };

    let mut out: Vec<String> = vec![];
    let mut sections: Vec<ArchivedSection> = vec![];
    let mut added = 0;
    for sec in &s_plan.sections {
        let steps: Vec<&Step> = sec.steps.iter().filter(|s| keep(s)).collect();
        if steps.is_empty() {
            continue;
        }
        let heading = if sec.line >= 0 {
            s_lines[sec.line as usize].clone()
        } else {
            format!("## From {source_path}")
        };
        let mut block = vec![heading, String::new()];
        for st in &steps {
            for (k, source_line) in s_lines[st.line..st.end].iter().enumerate() {
                let mut line = source_line.clone();
                if k == 0 {
                    line = TITLE_NUM
                        .replace(&line, |c: &Captures| {
                            format!("{}{}{}", &c[1], renumber(&c[2]), &c[3])
                        })
                        .into_owned();
                } else if let Some(nm) = NEEDS_LINE.captures(&line) {
                    let needs: Vec<String> = SPLIT
                        .split(&nm[2])
                        .map(js::trim)
                        .filter(|x| !x.is_empty())
                        .map(map_need)
                        .collect();
                    line = format!("{}{}", &nm[1], needs.join(", "));
                }
                block.push(line);
            }
        }
        while block.last().is_some_and(|l| js::trim(l).is_empty()) {
            block.pop();
        }
        out.push(block.join(eol));
        sections.push(ArchivedSection {
            title: if sec.title.is_empty() {
                format!("From {source_path}")
            } else {
                sec.title.clone()
            },
            steps: steps.len(),
        });
        added += steps.len();
    }
    if added == 0 {
        return None;
    }

    let mut body = t_lines;
    let had_final_newline = body.len() > 1 && body.last().is_some_and(String::is_empty);
    while body.last().is_some_and(|l| js::trim(l).is_empty()) {
        body.pop();
    }
    let mut parts = vec![body.join(eol)];
    parts.extend(out);
    parts.retain(|p| !p.is_empty());
    let text = parts.join(&format!("{eol}{eol}"))
        + if had_final_newline || body.is_empty() {
            eol
        } else {
            ""
        };
    let kept_count = s_plan.steps.iter().filter(|s| keep(s)).count();
    Some(MergeResult {
        text,
        added,
        left_out: s_plan.steps.len() - kept_count,
        renumbered,
        sections,
    })
}

/// Every step number in a plan's text, done or not.
pub fn step_numbers(text: Option<&str>) -> Vec<String> {
    match text.filter(|t| !t.is_empty()) {
        Some(t) => parse_plan(t)
            .steps
            .into_iter()
            .filter_map(|s| s.number.filter(|n| !n.is_empty()))
            .collect(),
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::tasklist::find_task_lists;

    const PLAN: &str = "# Work map\n\n## Stage 1\n\n- [x] **1. Setup**\n  - Done.\n- [x] **2. Layout**\n\n## Stage 2\n\nSome words about stage 2.\n\n- [x] **3. Editor**\n- [ ] **4. Search**\n  - Needs: 1, 3\n\n## Stage 3\n\n- [x] **5. Done too**\n";

    fn numbers(text: &str) -> Vec<String> {
        parse_plan(text)
            .steps
            .into_iter()
            .filter_map(|s| s.number)
            .collect()
    }

    #[test]
    fn puts_the_archive_next_to_the_plan() {
        assert_eq!(archive_path("docs/PLAN.md"), "docs/PLAN.archive.md");
        assert!(is_archive("docs/PLAN.archive.md"));
        assert!(!is_archive("docs/PLAN.md"));
        assert_eq!(
            find_task_lists(&["PLAN.md".into(), "PLAN.archive.md".into()]),
            ["PLAN.md"]
        );
    }

    #[test]
    fn moves_only_sections_whose_steps_are_all_done() {
        let r = archive_finished(PLAN, None, "docs/PLAN.md").unwrap();
        assert_eq!(
            r.sections,
            [
                ArchivedSection {
                    title: "Stage 1".into(),
                    steps: 2
                },
                ArchivedSection {
                    title: "Stage 3".into(),
                    steps: 1
                },
            ]
        );
        assert_eq!(
            r.plan,
            "# Work map\n\n## Stage 2\n\nSome words about stage 2.\n\n- [x] **3. Editor**\n- [ ] **4. Search**\n  - Needs: 1, 3\n"
        );
        assert!(r.archive.starts_with(
            "# Archive of PLAN.md\n\nFinished sections moved here from docs/PLAN.md."
        ));
        assert!(r.archive.contains(
            "## Stage 1\n\n- [x] **1. Setup**\n  - Done.\n- [x] **2. Layout**\n\n## Stage 3\n\n- [x] **5. Done too**\n"
        ));
        assert!(r.archive.ends_with('\n'));
    }

    #[test]
    fn appends_to_an_archive_that_is_there() {
        let r = archive_finished(
            PLAN,
            Some("# Archive of PLAN.md\n\n## Old\n\n- [x] **0. Old**\n"),
            "PLAN.md",
        )
        .unwrap();
        assert!(
            r.archive
                .starts_with("# Archive of PLAN.md\n\n## Old\n\n- [x] **0. Old**\n\n## Stage 1")
        );
    }

    #[test]
    fn keeps_windows_line_endings() {
        let lone = re(r"[^\r]\n");
        let r = archive_finished(&PLAN.replace('\n', "\r\n"), None, "PLAN.md").unwrap();
        assert!(!lone.is_match(&r.plan));
        assert!(!lone.is_match(&r.archive));
    }

    #[test]
    fn nothing_finished_gives_none() {
        assert_eq!(
            archive_finished("## A\n\n- [ ] **1. Open**\n", None, "PLAN.md"),
            None
        );
    }

    #[test]
    fn archived_steps_still_count_as_done() {
        let r = archive_finished(PLAN, None, "PLAN.md").unwrap();
        let done = done_numbers(&[Some(&r.plan), Some(&r.archive)]);
        assert!(
            ["1", "2", "3", "5"]
                .iter()
                .all(|n| done.contains(&n.to_string()))
        );
        assert_eq!(numbers(&r.plan), ["3", "4"]);
    }

    const TARGET: &str = "# Plan\n\n## A\n\n- [ ] **1. One**\n- [ ] **2. Two**\n";
    const SOURCE: &str = "## Other\n\n- [x] **1. Old done**\n- [ ] **2. Next**\n  - Needs: 1\n  - Model: claude-opus-5-5\n- [ ] **2b. Sub**\n  - Needs: 2, PLAN.md#1, LIB.md#3\n- [ ] **9. Free number**\n";

    fn step<'a>(steps: &'a [Step], n: &str) -> &'a Step {
        steps
            .iter()
            .find(|s| s.number.as_deref() == Some(n))
            .unwrap()
    }

    #[test]
    fn adds_the_steps_and_renumbers_taken_numbers() {
        let r = merge_plans(TARGET, "PLAN.md", SOURCE, "OTHER.md", false, &[]).unwrap();
        assert_eq!(r.added, 4);
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            r.renumbered,
            [pair("1", "10"), pair("2", "11"), pair("2b", "11b")]
        );
        let plan = parse_plan(&r.text);
        assert_eq!(numbers(&r.text), ["1", "2", "10", "11", "11b", "9"]);
        assert_eq!(step(&plan.steps, "11").needs, ["10"]);
        assert_eq!(
            step(&plan.steps, "11").model.as_deref(),
            Some("claude-opus-5-5")
        );
        assert_eq!(step(&plan.steps, "11b").needs, ["11", "1", "LIB.md#3"]);
        assert!(
            r.text
                .starts_with(&format!("{}\n\n## Other\n\n", TARGET.trim_end()))
        );
        assert!(r.text.ends_with("**9. Free number**\n"));
    }

    #[test]
    fn can_leave_done_steps_out() {
        let r = merge_plans(TARGET, "PLAN.md", SOURCE, "OTHER.md", true, &[]).unwrap();
        assert_eq!(r.added, 3);
        assert_eq!(r.left_out, 1);
        assert_eq!(step(&parse_plan(&r.text).steps, "11").needs, ["OTHER.md#1"]);
    }

    #[test]
    fn gives_loose_steps_a_heading() {
        let r = merge_plans(
            TARGET,
            "PLAN.md",
            "- [ ] **7. Loose**\n",
            "notes/TODO.md",
            false,
            &[],
        )
        .unwrap();
        assert!(
            r.text
                .contains("## From notes/TODO.md\n\n- [ ] **7. Loose**")
        );
    }

    #[test]
    fn nothing_to_add_gives_none() {
        assert_eq!(
            merge_plans(TARGET, "PLAN.md", "- [x] **1. Done**\n", "O.md", true, &[]),
            None
        );
    }

    #[test]
    fn does_not_reuse_numbers_in_the_archive() {
        let taken = step_numbers(Some(
            "## Old\n\n- [x] **2. Old two**\n- [x] **9. Old nine**\n",
        ));
        let r = merge_plans(
            "## A\n\n- [ ] **5. Five**\n",
            "PLAN.md",
            "## B\n\n- [ ] **2. Two**\n",
            "O.md",
            false,
            &taken,
        )
        .unwrap();
        assert_eq!(r.renumbered, [("2".to_string(), "10".to_string())]);
    }
}
