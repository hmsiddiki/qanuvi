//! Reads and edits Markdown task lists such as DEVELOPMENT-TASKLIST.md (ported from `src/lib/plan/tasklist.ts`):
//!
//! ```text
//! ## Section
//! - [ ] **12. Step title** (a note that may continue
//!   on the next line)
//!   - detail
//!   - Model: claude-opus-5-5
//!   - Needs: 10, 11, docs/OTHER-PLAN.md#4
//!   - Skipped: (written by a plan run: why it skipped the step)
//! ```
//!
//! Edits change only the lines they must; everything else in the file stays exactly as it was, line endings included.

use super::js;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StepStatus {
    #[serde(rename = " ")]
    Open,
    #[serde(rename = "-")]
    Started,
    #[serde(rename = "x")]
    Done,
    #[serde(rename = "!")]
    Blocked,
}

pub const STATUS_ORDER: [StepStatus; 4] = [
    StepStatus::Open,
    StepStatus::Started,
    StepStatus::Done,
    StepStatus::Blocked,
];

impl StepStatus {
    /// The character between the brackets.
    pub fn mark(self) -> char {
        match self {
            Self::Open => ' ',
            Self::Started => '-',
            Self::Done => 'x',
            Self::Blocked => '!',
        }
    }

    fn from_mark(c: char) -> Self {
        match c {
            '-' => Self::Started,
            'x' | 'X' => Self::Done,
            '!' => Self::Blocked,
            _ => Self::Open,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Not started",
            Self::Started => "In progress",
            Self::Done => "Done",
            Self::Blocked => "Blocked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Index of the step's line in the file.
    pub line: usize,
    pub status: StepStatus,
    /// For example "82" or "45e"; None when the step has no number.
    pub number: Option<String>,
    pub title: String,
    /// Text after the title, including continuation lines, for example "(Plan D, step 1 of 8)".
    pub note: String,
    /// Detail lines under the step, without their leading indentation.
    pub details: Vec<String>,
    pub model: Option<String>,
    /// What the step needs done first (its "- Needs:" line): step numbers, or "file#number" in another plan.
    #[serde(default)]
    pub needs: Vec<String>,
    /// First line after the step and its details.
    pub end: usize,
    pub section: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub level: usize,
    /// -1 for the part before the first heading.
    pub line: i64,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub sections: Vec<Section>,
    pub steps: Vec<Step>,
    pub eol: String,
}

/// Why an edit was refused.
pub const CHANGED: &str =
    "The task list changed since it was shown. It has been reloaded; try again.";

static STEP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^( ?)- \[([ xX!-])\] (.*)$").unwrap());
static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.*?)\s*#*\s*$").unwrap());
static MODEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s+- Model:\s*(\S+)\s*$").unwrap());
static NEEDS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^- Needs:\s*(.+)$").unwrap());
static SKIPPED_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\s+- Skipped:").unwrap());
static SKIPPED_DETAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^- Skipped:").unwrap());
static TITLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\*\*(?:([0-9]+[a-z]?(?:-[0-9]+)?)\.\s+)?(.+?)\*\*\s*(.*)$").unwrap()
});
/// A step written without bold: "1a. Title".
static PLAIN_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]+[a-z]?(?:-[0-9]+)?)\.\s+(.+)$").unwrap());
static LIST_ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([-*+]|[0-9]+\.)\s").unwrap());
static STATUS_MARK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^( ?- \[)[ xX!-](\])").unwrap());
static PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^( ?- \[[ xX!-]\] )").unwrap());
static SPACES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static NEEDS_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[,\s]+").unwrap());
static LINE_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\r?\n").unwrap());

/// The file's lines and its line ending ("\r\n" when it has any).
pub fn split_lines(text: &str) -> (Vec<String>, &'static str) {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    (LINE_SPLIT_RE.split(text).map(String::from).collect(), eol)
}

fn indent_of(line: &str) -> usize {
    line.chars().count() - js::trim_start(line).chars().count()
}

fn is_list_item(trimmed: &str) -> bool {
    LIST_ITEM_RE.is_match(trimmed)
}

fn blank(line: &str) -> bool {
    js::trim(line).is_empty()
}

pub fn parse_plan(text: &str) -> Plan {
    let (lines, eol) = split_lines(text);
    let mut sections: Vec<Section> = vec![Section {
        title: String::new(),
        level: 0,
        line: -1,
        steps: vec![],
    }];
    let mut steps: Vec<Step> = vec![];

    let mut i = 0;
    while i < lines.len() {
        if let Some(h) = HEADING_RE.captures(&lines[i]) {
            sections.push(Section {
                title: h[2].to_string(),
                level: h[1].len(),
                line: i as i64,
                steps: vec![],
            });
            i += 1;
            continue;
        }
        let Some(m) = STEP_RE.captures(&lines[i]) else {
            i += 1;
            continue;
        };

        let status = StepStatus::from_mark(m[2].chars().next().unwrap_or(' '));
        let rest = &m[3];
        let mut number: Option<String> = None;
        let mut title = js::trim(rest).to_string();
        let mut note = String::new();
        if let Some(t) = TITLE_RE.captures(rest) {
            number = t.get(1).map(|n| n.as_str().to_string());
            title = js::trim(&t[2]).to_string();
            note = js::trim(&t[3]).to_string();
        } else if let Some(p) = PLAIN_NUMBER_RE.captures(&title) {
            let (n, t) = (p[1].to_string(), js::trim(&p[2]).to_string());
            number = Some(n);
            title = t;
        }

        // Continuation of the step's own line, then details, until an unindented line or a heading.
        let mut j = i + 1;
        while j < lines.len()
            && !blank(&lines[j])
            && indent_of(&lines[j]) >= 2
            && !is_list_item(js::trim(&lines[j]))
        {
            note = js::trim(&format!("{note} {}", js::trim(&lines[j]))).to_string();
            j += 1;
        }
        let mut details: Vec<String> = vec![];
        let mut model: Option<String> = None;
        let mut end = j;
        while j < lines.len() {
            let line = &lines[j];
            if blank(line) {
                j += 1;
                continue;
            }
            if indent_of(line) < 2 || HEADING_RE.is_match(line) {
                break;
            }
            match MODEL_RE.captures(line) {
                Some(mm) if model.is_none() => model = Some(mm[1].to_string()),
                _ => details.push(line.strip_prefix("  ").unwrap_or(line).to_string()),
            }
            end = j + 1;
            j += 1;
        }

        let needs = details
            .iter()
            .find_map(|d| NEEDS_RE.captures(js::trim(d)).map(|c| c[1].to_string()))
            .map(|n| {
                NEEDS_SPLIT_RE
                    .split(&n)
                    .map(|t| js::trim(t).to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let section = sections.last_mut().expect("there is always a section");
        let step = Step {
            line: i,
            status,
            number,
            title,
            note,
            details,
            model,
            needs,
            end,
            section: section.title.clone(),
        };
        section.steps.push(step.clone());
        steps.push(step);
        i = end;
    }

    Plan {
        sections: sections
            .into_iter()
            .filter(|s| !s.steps.is_empty() || s.line >= 0)
            .collect(),
        steps,
        eol: eol.to_string(),
    }
}

/// The step at `line` in freshly parsed text, refusing if the file no longer has a step there.
fn step_at(
    text: &str,
    line: usize,
    expect_title: Option<&str>,
) -> Result<(Plan, Step, Vec<String>), String> {
    let plan = parse_plan(text);
    let step = plan
        .steps
        .iter()
        .find(|s| s.line == line)
        .filter(|s| expect_title.is_none_or(|t| s.title == t))
        .cloned()
        .ok_or_else(|| CHANGED.to_string())?;
    Ok((plan, step, split_lines(text).0))
}

pub fn set_step_status(
    text: &str,
    line: usize,
    status: StepStatus,
    expect_title: Option<&str>,
) -> Result<String, String> {
    let (plan, _, mut lines) = step_at(text, line, expect_title)?;
    let mark = status.mark().to_string();
    lines[line] = STATUS_MARK_RE
        .replace(&lines[line], |c: &regex::Captures| {
            format!("{}{mark}{}", &c[1], &c[2])
        })
        .into_owned();
    Ok(lines.join(&plan.eol))
}

pub fn set_step_title(
    text: &str,
    line: usize,
    title: &str,
    expect_title: Option<&str>,
) -> Result<String, String> {
    let spaced = SPACES_RE.replace_all(title, " ").replace("**", "");
    let clean = js::trim(&spaced);
    if clean.is_empty() {
        return Err("A step needs a title.".into());
    }
    let (plan, step, mut lines) = step_at(text, line, expect_title)?;
    let prefix = PREFIX_RE
        .captures(&lines[line])
        .map(|c| c[1].to_string())
        .unwrap_or_default();
    let rest = lines[line][prefix.len()..].to_string();
    let num = step
        .number
        .as_ref()
        .map(|n| format!("{n}. "))
        .unwrap_or_default();
    lines[line] = match TITLE_RE.captures(&rest) {
        Some(t) => {
            let after = &t[3];
            let after = if after.is_empty() {
                String::new()
            } else {
                format!(" {after}")
            };
            format!("{prefix}**{num}{clean}**{after}")
        }
        None => format!("{prefix}{num}{clean}"),
    };
    Ok(lines.join(&plan.eol))
}

/// Sets, changes or (with None) removes the step's "- Model:" line.
pub fn set_step_model(
    text: &str,
    line: usize,
    model: Option<&str>,
    expect_title: Option<&str>,
) -> Result<String, String> {
    let value = model.map(js::trim).filter(|v| !v.is_empty());
    if value.is_some_and(|v| v.chars().any(char::is_whitespace)) {
        return Err("A model name has no spaces.".into());
    }
    let (plan, step, mut lines) = step_at(text, line, expect_title)?;
    let existing = (line + 1..step.end.min(lines.len())).find(|&i| MODEL_RE.is_match(&lines[i]));
    match (existing, value) {
        (Some(at), Some(v)) => lines[at] = format!("  - Model: {v}"),
        (Some(at), None) => {
            lines.remove(at);
        }
        (None, Some(v)) => {
            let mut at = line + 1;
            while at < lines.len()
                && !blank(&lines[at])
                && indent_of(&lines[at]) >= 2
                && !is_list_item(js::trim(&lines[at]))
            {
                at += 1;
            }
            lines.insert(at, format!("  - Model: {v}"));
        }
        (None, None) => {}
    }
    Ok(lines.join(&plan.eol))
}

/// Sets, changes or (with None) removes the step's "- Skipped:" line: why a plan run skipped it (24f). A later run
/// that finishes the step removes it.
pub fn set_step_skipped(
    text: &str,
    line: usize,
    reason: Option<&str>,
    expect_title: Option<&str>,
) -> Result<String, String> {
    let value = reason
        .map(|r| js::trim(&SPACES_RE.replace_all(r, " ")).to_string())
        .filter(|v| !v.is_empty());
    let (plan, step, mut lines) = step_at(text, line, expect_title)?;
    let existing = (line + 1..step.end.min(lines.len())).find(|&i| SKIPPED_RE.is_match(&lines[i]));
    match (existing, value) {
        (Some(at), Some(v)) => lines[at] = format!("  - Skipped: {v}"),
        (Some(at), None) => {
            lines.remove(at);
        }
        (None, Some(v)) => {
            let mut at = line + 1;
            while at < lines.len() && !blank(&lines[at]) && indent_of(&lines[at]) >= 2 {
                at += 1;
            }
            lines.insert(at, format!("  - Skipped: {v}"));
        }
        (None, None) => {}
    }
    Ok(lines.join(&plan.eol))
}

pub fn next_status(status: StepStatus) -> StepStatus {
    let i = STATUS_ORDER.iter().position(|s| *s == status).unwrap_or(0);
    STATUS_ORDER[(i + 1) % STATUS_ORDER.len()]
}

/// Folders whose files are copies or someone else's code, never the user's plans.
static NOT_PLANS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|/)(node_modules|vendor|backups?|\.history)/").unwrap());
static PLAN_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^|/)[^/]*(task[-_ ]?list|todo|plan|roadmap)[^/]*\.md$").unwrap()
});
static ARCHIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.archive\.md$").unwrap());

/// Files in a project that look like task lists, the most likely first.
pub fn find_task_lists(paths: &[String]) -> Vec<String> {
    let mut found: Vec<String> = paths
        .iter()
        .filter(|p| PLAN_NAME.is_match(p) && !NOT_PLANS.is_match(p) && !ARCHIVE.is_match(p))
        .cloned()
        .collect();
    found.sort_by(|a, b| {
        a.split('/')
            .count()
            .cmp(&b.split('/').count())
            .then_with(|| locale_compare(a, b))
    });
    found
}

/// Close to `localeCompare` for file paths: punctuation before digits before letters, letters without case first and
/// lower case before upper case only to break a tie (as the default collation does).
fn locale_compare(a: &str, b: &str) -> Ordering {
    // ASCII punctuation in the default collation's order (white space first).
    const PUNCT: &str = " \t_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";
    fn key(s: &str) -> Vec<(u8, u32, char)> {
        s.chars()
            .map(|c| {
                if c.is_alphabetic() {
                    (3, 0, c.to_lowercase().next().unwrap_or(c))
                } else if c.is_numeric() {
                    (2, 0, c)
                } else {
                    let rank = PUNCT.find(c).map_or(c as u32 + 128, |i| i as u32);
                    (1, rank, c)
                }
            })
            .collect()
    }
    fn case(s: &str) -> Vec<bool> {
        s.chars().map(char::is_uppercase).collect()
    }
    key(a).cmp(&key(b)).then_with(|| case(a).cmp(&case(b)))
}

/// Removes "- Skipped:" lines that no longer hold: the step's needs (in this plan) are all done now. A run does this
/// before it starts, so the plan never says a step waits for one that is finished. `also_done`: numbers done in the
/// plan's archive.
pub fn clear_stale_skips(text: &str, also_done: &[String]) -> String {
    let plan = parse_plan(text);
    let done: HashSet<String> = also_done
        .iter()
        .cloned()
        .chain(
            plan.steps
                .iter()
                .filter(|s| s.status == StepStatus::Done)
                .filter_map(|s| s.number.clone()),
        )
        .collect();
    let mut out = text.to_string();
    // From the bottom up: removing a line moves the ones below it.
    for s in plan.steps.iter().rev() {
        if s.status == StepStatus::Done || s.needs.is_empty() {
            continue;
        }
        if !s
            .details
            .iter()
            .any(|d| SKIPPED_DETAIL_RE.is_match(js::trim(d)))
        {
            continue;
        }
        if s.needs
            .iter()
            .all(|n| !n.contains('#') && done.contains(js::trim(n)))
            && let Ok(next) = set_step_skipped(&out, s.line, None, Some(&s.title))
        {
            out = next;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Work map\n\n## Stage 1\n\n- [x] **1. Project setup and an empty window**\n  - Done on \
Windows.\n    More detail.\n\n- [-] **2. Layout: sidebar and panes** (built 2026-11-10, waiting for the owner's\n  \
review)\n  - Three panes.\n  - Model: claude-opus-5-5\n\n- [ ] **1b. Updates** (moved later)\n\n## Stage 2\n\n- [!] \
Plain step without bold\n- [ ] **15b. Git like VS Code** (added by the owner)\n  - Decorations.\n\nSome paragraph.";

    fn line(text: &str, i: usize) -> String {
        text.split('\n').nth(i).unwrap_or_default().to_string()
    }

    #[test]
    fn finds_sections_and_steps() {
        let plan = parse_plan(SAMPLE);
        let got: Vec<(char, Option<&str>, &str)> = plan
            .steps
            .iter()
            .map(|s| (s.status.mark(), s.number.as_deref(), s.title.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ('x', Some("1"), "Project setup and an empty window"),
                ('-', Some("2"), "Layout: sidebar and panes"),
                (' ', Some("1b"), "Updates"),
                ('!', None, "Plain step without bold"),
                (' ', Some("15b"), "Git like VS Code"),
            ]
        );
        let sections: Vec<(&str, usize)> = plan
            .sections
            .iter()
            .map(|s| (s.title.as_str(), s.steps.len()))
            .collect();
        assert_eq!(
            sections,
            vec![("Work map", 0), ("Stage 1", 3), ("Stage 2", 2)]
        );
        assert_eq!(
            plan.steps[1].note,
            "(built 2026-11-10, waiting for the owner's review)"
        );
        assert_eq!(
            plan.steps[0].details,
            vec!["- Done on Windows.", "  More detail."]
        );
        assert_eq!(plan.steps[1].details, vec!["- Three panes."]);
        assert_eq!(plan.steps[1].model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(plan.steps[0].model, None);
        assert_eq!(plan.steps[4].details, vec!["- Decorations."]);
        let crlf = parse_plan(&SAMPLE.replace('\n', "\r\n"));
        assert_eq!(crlf.eol, "\r\n");
        assert_eq!(crlf.steps.len(), 5);
    }

    #[test]
    fn edits_only_what_it_must() {
        let out = set_step_status(SAMPLE, 13, StepStatus::Done, None).unwrap();
        assert_eq!(line(&out, 13), "- [x] **1b. Updates** (moved later)");
        assert_eq!(
            out.split('\n')
                .zip(SAMPLE.split('\n'))
                .filter(|(a, b)| a != b)
                .count(),
            1
        );
        let crlf = SAMPLE.replace('\n', "\r\n");
        assert_eq!(
            set_step_status(&crlf, 4, StepStatus::Open, None).unwrap(),
            crlf.replace("- [x] **1. Project", "- [ ] **1. Project")
        );
        assert!(
            set_step_status(SAMPLE, 5, StepStatus::Done, None)
                .unwrap_err()
                .contains("changed")
        );
        assert!(set_step_status(SAMPLE, 4, StepStatus::Done, Some("Another title")).is_err());

        let out = set_step_title(SAMPLE, 13, "Updates and **channels**", None).unwrap();
        assert_eq!(
            line(&out, 13),
            "- [ ] **1b. Updates and channels** (moved later)"
        );
        assert_eq!(
            line(
                &set_step_title(SAMPLE, 17, "Plain renamed", None).unwrap(),
                17
            ),
            "- [!] Plain renamed"
        );
        assert!(set_step_title(SAMPLE, 13, "  ", None).is_err());

        let text = "# P\n\n- [ ] 1a. Add `slugify` to util.js\n- [x] 12-2. Docs\n- [ ] Plain step";
        let got: Vec<(Option<String>, String)> = parse_plan(text)
            .steps
            .into_iter()
            .map(|s| (s.number, s.title))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("1a".into()), "Add `slugify` to util.js".into()),
                (Some("12-2".into()), "Docs".into()),
                (None, "Plain step".into()),
            ]
        );
        assert_eq!(
            line(&set_step_title(text, 2, "Add slugify", None).unwrap(), 2),
            "- [ ] 1a. Add slugify"
        );
    }

    #[test]
    fn adds_changes_and_removes_the_model_line() {
        let added = set_step_model(SAMPLE, 13, Some("claude-haiku-5-5"), None).unwrap();
        assert_eq!(line(&added, 14), "  - Model: claude-haiku-5-5");
        assert_eq!(
            parse_plan(&added).steps[2].model.as_deref(),
            Some("claude-haiku-5-5")
        );
        let second = set_step_model(SAMPLE, 4, Some("claude-sonnet-5-5"), None).unwrap();
        assert_eq!(line(&second, 5), "  - Model: claude-sonnet-5-5");
        let changed = set_step_model(SAMPLE, 8, Some("claude-fable-5-2"), None).unwrap();
        assert_eq!(line(&changed, 11), "  - Model: claude-fable-5-2");
        let removed = set_step_model(SAMPLE, 8, None, None).unwrap();
        assert_eq!(removed.split('\n').count(), SAMPLE.split('\n').count() - 1);
        assert_eq!(parse_plan(&removed).steps[1].model, None);
        assert!(set_step_model(SAMPLE, 8, Some("two words"), None).is_err());
    }

    #[test]
    fn cycles_through_the_statuses() {
        assert_eq!(next_status(StepStatus::Open), StepStatus::Started);
        assert_eq!(next_status(StepStatus::Started), StepStatus::Done);
        assert_eq!(next_status(StepStatus::Done), StepStatus::Blocked);
        assert_eq!(next_status(StepStatus::Blocked), StepStatus::Open);
    }

    #[test]
    fn finds_likely_task_lists_top_level_first() {
        let paths: Vec<String> = [
            "docs/ROADMAP.md",
            "README.md",
            "DEVELOPMENT-TASKLIST.md",
            "node_modules/x/TODO.md",
            "notes/todo-list.md",
            "QLOPY-CORE-UPGRADE-PLAN.md",
            "content/plugins/shop/PLAN-D.md",
            "uploads/updates/backups/2026/content/plugins/shop/DEVELOPMENT-TASKLIST.md",
            "vendor/lib/TODO.md",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            find_task_lists(&paths),
            vec![
                "DEVELOPMENT-TASKLIST.md",
                "QLOPY-CORE-UPGRADE-PLAN.md",
                "docs/ROADMAP.md",
                "notes/todo-list.md",
                "content/plugins/shop/PLAN-D.md",
            ]
        );
    }

    const NEEDS: &str = "## Steps\n- [ ] **1. First**\n- [ ] **2. Second**\n  - do it\n  - Needs: 1, docs/OTHER.md#4\n- [ ] **3. Third**\n";

    #[test]
    fn reads_needs_and_writes_skipped_lines() {
        let steps = parse_plan(NEEDS).steps;
        assert_eq!(steps[1].needs, vec!["1", "docs/OTHER.md#4"]);
        assert!(steps[0].needs.is_empty());
        let once = set_step_skipped(
            NEEDS,
            2,
            Some("needs step 1, which is not done"),
            Some("Second"),
        )
        .unwrap();
        assert!(once.contains(
            "  - Needs: 1, docs/OTHER.md#4\n  - Skipped: needs step 1, which is not done\n- [ ] **3. Third**"
        ));
        let twice = set_step_skipped(&once, 2, Some("blocked"), Some("Second")).unwrap();
        assert_eq!(twice.matches("Skipped:").count(), 1);
        assert_eq!(
            set_step_skipped(&twice, 2, None, Some("Second")).unwrap(),
            NEEDS
        );
    }

    #[test]
    fn clears_skips_whose_needs_are_done() {
        let text = "- [x] **1. One**\n- [ ] **2. Two**\n  - Needs: 1\n  - Skipped: needs step 1, which is not done\n\
- [ ] **3. Three**\n  - Needs: 2\n  - Skipped: needs step 2, which is not done\n";
        let out = clear_stale_skips(text, &[]);
        assert!(!out.contains("needs step 1"));
        assert!(out.contains("needs step 2"));
        assert_eq!(clear_stale_skips(&out, &[]), out);
    }
}
