//! What a running step is told and what it leaves, ported from `src/lib/plan/guidance.ts`: the owner's messages while
//! a step runs (guidance, stop and go-on orders), the run's notes and private details, handovers to a new session,
//! step logs, and the plan's shared knowledge (MAP.md, FACTS.md, API.md). The kept run settings and plan notes move
//! from localStorage to the DB in R2.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use super::js;
use super::runner::{RunStepStatus, StepRun};
use super::tasklist::{Step, StepStatus};

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// Where the note starts in a sent message (the stored and shown message is the user's words only).
pub const GUIDE_MARK: &str = "\n\n(Qanuvi: the user wrote this while you work on step ";

/// The note sent with the user's words.
pub fn guidance_note(step: &str, file: &str) -> String {
    format!(
        "{GUIDE_MARK}{step} of {file}. The user is watching. Take it in now and decide: if it changes how to do this \
         step, follow it; if it says not to do something, do not do it, even when the step asks for it. If it asks for \
         something more: when it is small, do it now as part of this step and add a short note of it under the step in \
         the plan; when it is big, do not do it now: add it to the plan as a new open step right after {step}, numbered \
         {step}-1 (then {step}-2), in the plan's format, and say so; it runs after this step. If it asks you to \
         stop, stop and say where you left off. If it asks to go back to an earlier step (one that is blocked, skipped \
         or not finished), set that step back to [ ] in the plan and remove its Blocked or Skipped line; Qanuvi runs it \
         right after this step. Answer it in one short line before going on.)"
    )
}

/// The user's words only: the message without Qanuvi's note.
pub fn without_guidance(text: &str) -> &str {
    let at = [text.find(GUIDE_MARK), text.find(STOPPED_MARK)]
        .into_iter()
        .flatten()
        .min();
    at.map_or(text, |i| &text[..i])
}

/// Where the note starts in a message written in the chat of a step the run stopped at (shown: the words only).
pub const STOPPED_MARK: &str = "\n\n(Qanuvi: the plan run is stopped at step ";

/// The question the agent ends with when the run could go on; the owner's yes to it resumes the run.
pub const RESUME_ASK: &str = "Should I resume the plan run from step";

/// Writing in the chat of a step the run stopped at is a chat, not the run: the agent answers and does what the owner
/// asks in this session, and asks before the run goes on.
pub fn stopped_chat_note(step: &str, file: &str) -> String {
    format!(
        "{STOPPED_MARK}{step} of {file}; this is a chat with the owner, not the run. Answer what they ask first, \
         clearly. When they ask you to do something, do it here (with the usual approvals) and keep the plan file true: \
         the step's mark says what is really so. Do not go on with other steps. When the owner wants the run to go on, or \
         what blocked the step is settled, end your reply with this question on its own line: \
         \"{RESUME_ASK} {step}? (yes/no)\". Qanuvi resumes the run only when the owner answers yes or says to continue.)"
    )
}

/// A whole message that is one of the given phrases (any case), with white space, dots and "!" around it.
fn whole(phrases: &str) -> Regex {
    let s = js::S;
    re(&format!(r"^{s}*(?i-u:{phrases})(?:{s}|[.!])*$"))
}

/// A plain order to go on with the run ("continue", "resume the run", "retry the step").
pub fn is_go_on_order(text: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        whole(
            "continue|resume|go on|carry on|proceed|retry|run it|run it again|run again|retry (?:it|the step|this step)|(?:continue|resume|run) the (?:run|plan|plan run|step)(?: again)?",
        )
    });
    RE.is_match(text)
}

/// A plain yes ("yes", "ok", "go ahead").
pub fn is_yes(text: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        whole(
            "y|yes|yeah|yep|yup|ok|okay|sure|go|go ahead|do it|please do|yes please|yes,? (?:resume|continue|go on)(?: it)?",
        )
    });
    RE.is_match(text)
}

/// Whether the agent's reply asked to resume the run (so a yes to it resumes).
pub fn asked_to_resume(reply: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:resume the (?:plan )?run)"));
    RE.is_match(reply) && reply.contains('?')
}

/// A plain order to stop the run ("stop", "stop the run", "halt now"): Qanuvi stops it at once.
pub fn is_stop_order(text: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        whole("(?:stop|halt|cancel|abort)(?: (?:it|now|the run|the plan|everything|running|here))*")
    });
    RE.is_match(text)
}

pub fn step_key(number: Option<&str>, title: &str) -> String {
    format!("{}|{title}", number.unwrap_or(""))
}

fn key_of(s: &Step) -> String {
    step_key(s.number.as_deref(), &s.title)
}

/// Open steps in the plan now that the run did not know: added while it ran (by the agent or the user).
pub fn added_steps<'a>(now: &'a [Step], known: &HashSet<String>) -> Vec<&'a Step> {
    now.iter()
        .filter(|s| {
            s.status != StepStatus::Done
                && s.status != StepStatus::Blocked
                && !known.contains(&key_of(s))
        })
        .collect()
}

/// Steps the run already passed (blocked, failed or skipped) that are open in the plan again: the owner asked to go
/// back to them. Each comes back at most once per run.
pub fn reopened_steps<'a>(
    run_steps: &[StepRun],
    now: &'a [Step],
    reopened: &HashSet<String>,
) -> Vec<&'a Step> {
    let keys = |ok: &dyn Fn(RunStepStatus) -> bool| -> HashSet<String> {
        run_steps
            .iter()
            .filter(|r| ok(r.status))
            .map(|r| step_key(r.number.as_deref(), &r.title))
            .collect()
    };
    use RunStepStatus::*;
    let passed = keys(&|s| matches!(s, Blocked | Failed | Skipped));
    let waiting = keys(&|s| s == Waiting);
    now.iter()
        .filter(|s| {
            let k = key_of(s);
            s.status == StepStatus::Open
                && passed.contains(&k)
                && !waiting.contains(&k)
                && !reopened.contains(&k)
        })
        .collect()
}

/// A session's context size now: what its last model call read (new, cached and cache-written tokens).
pub fn context_size(lines: &[String]) -> u64 {
    for l in lines.iter().rev() {
        if !l.contains("\"usage\"") || !l.contains("\"assistant\"") {
            continue;
        }
        // Not a JSON line: look further back.
        let Ok(v) = serde_json::from_str::<Value>(l) else {
            continue;
        };
        let u = &v["message"]["usage"];
        let truthy = match u {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64() != Some(0.0),
            Value::String(s) => !s.is_empty(),
            _ => true,
        };
        if truthy {
            let n = |k: &str| u.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            return (n("input_tokens")
                + n("cache_read_input_tokens")
                + n("cache_creation_input_tokens")) as u64;
        }
    }
    0
}

/// Default context size at which a step hands over to a new session in the middle of its work (24p).
pub const HANDOVER_DEFAULT: u64 = 200_000;
/// At most this many handovers in one step: past that, the step goes on in its session (it may be stuck).
pub const HANDOVER_MAX: u32 = 4;

/// Whether a running step should hand over now: its context has grown past the limit (0 = never).
pub fn handover_due(context: u64, limit: u64, handovers: u32) -> bool {
    limit > 0 && context >= limit && handovers < HANDOVER_MAX
}

const PROGRESS_NOTE: &str = "a new session can work from without guessing: what is done and checked, what is left (in order), the files you touched and why, anything half-way, decisions you made and their reasons, and facts about the code you learned (names, places, traps). Do not mark the step done unless it is. Then stop and answer \"Handed over\".)";

/// What the step's agent is told when its context is too big: finish the edit in hand, write a careful note, stop.
pub fn handover_request(log: &str, context: u64) -> String {
    format!(
        "(Qanuvi: this session's context is now about {}k tokens. A new session takes over this step, so the work \
         stays focused and the cost stays down. Finish the edit you are making so no file is left half-written or \
         broken; do not start anything new. Then add a \"Progress\" note to {log} that {PROGRESS_NOTE}",
        js::number(js::round(context as f64 / 1000.0))
    )
}

/// The first words of the new session that takes a step over.
pub const HANDOVER_RESUME: &str = "An earlier session worked on this step until its context grew too big, and handed it over to you. Read its Progress note in the step's log first. Before you go on, read the files it names and check that what it calls done really is (fix it if not). Then finish what is left; do not redo what is done and correct.";

/// What the step's agent is told when the owner picks another model for the step while it works.
pub fn model_switch_request(log: &str, model: &str) -> String {
    format!(
        "(Qanuvi: the owner picked {model} for this step; it takes over in a new session. Finish the edit you are \
         making so no file is left half-written or broken; do not start anything new. Then add a \"Progress\" note to \
         {log} that {PROGRESS_NOTE}"
    )
}

/// The first words of the session on the model the owner switched the step to.
pub const SWITCH_RESUME: &str = "The owner switched this step to you while another model was working on it; that model stopped after the edit in hand and wrote a Progress note in the step's log. Read the note first. Before you go on, read the files it names and check that what it calls done really is (fix it if not). Then finish what is left; do not redo what is done and correct.";

/// How an agent leaves something for the later steps of a run (a decision, a command), in its answer.
pub const NOTE_PREFIX: &str = "Note for the next steps:";

/// The run's notes for a step's request, and how to leave new ones.
pub fn notes_block(notes: &[String]) -> String {
    let mut lines: Vec<String> = vec![];
    if !notes.is_empty() {
        lines.push(String::new());
        lines.push("Notes from earlier in this run (the owner's words and what earlier steps left); follow them:".into());
        lines.extend(notes.iter().map(|n| format!("- {n}")));
    }
    lines.push(String::new());
    lines.push(format!(
        "If later steps need something you learned or decided, add a line starting with \"{NOTE_PREFIX}\" to your last \
         answer. Never put a password or key in it: the project's secrets reach every step as environment variables."
    ));
    lines.join("\n")
}

/// Notes an agent left in its answer.
pub fn notes_in(text: &str) -> Vec<String> {
    let mut out = vec![];
    for line in text.split('\n') {
        let Some(i) = line.find(NOTE_PREFIX) else {
            continue;
        };
        let rest = line[i + NOTE_PREFIX.len()..].replace('*', "");
        let note = js::trim(&rest);
        if !note.is_empty() && !looks_secret(note) {
            out.push(js::cut(note, 400).to_string());
        }
    }
    out
}

/// A message that seems to hold a password or key (kept out of notes; Qanuvi offers project secrets instead).
pub fn looks_secret(text: &str) -> bool {
    static KEY: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?-u:\b)(?:sk|pk|ghp|gho|xox[abp])[-_][A-Za-z0-9_-]{16,}"));
    static HINT: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i-u:\b(?:pass(?:word|wd)?|pwd|secret|token|api[ _-]?key|credentials?|login|db user)\b)",
        )
    });
    KEY.is_match(text) || (HINT.is_match(text) && has_mixed_value(text))
}

/// `(?<!\w)(?=[C]*\d)(?=[C]*[A-Za-z])[C]{5,}` with C = `[\w!@#$%^&*.-]`: a run of at least five such characters,
/// not right after a word character, with letters and digits mixed (like most passwords, not plain words).
fn has_mixed_value(text: &str) -> bool {
    let b = text.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let class = |c: u8| word(c) || b"!@#$%^&*.-".contains(&c);
    for p in 0..b.len() {
        if p > 0 && word(b[p - 1]) {
            continue;
        }
        let run = b[p..].iter().take_while(|&&c| class(c)).count();
        let r = &b[p..p + run];
        if run >= 5 && r.iter().any(u8::is_ascii_digit) && r.iter().any(u8::is_ascii_alphabetic) {
            return true;
        }
    }
    false
}

/// The steps one session carried so far.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carry {
    pub model: String,
    pub section: String,
    pub tokens: u64,
    pub steps: u32,
}

/// At most this many steps share one session, whatever its size says.
pub const CARRY_MAX_STEPS: u32 = 6;

/// Whether the next step continues the session: same model, same section, still small.
pub fn can_carry(carry: Option<&Carry>, model: &str, section: &str, limit: u64) -> bool {
    carry.is_some_and(|c| {
        c.model == model && c.section == section && c.tokens < limit && c.steps < CARRY_MAX_STEPS
    })
}

/// A step's number and its Needs line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeedStep {
    pub number: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
}

/// How many of the steps still to run need the blocked one, directly or through others. Many: the run stops and
/// asks, rather than skipping them all.
pub fn needing_count(blocked: &str, steps: &[NeedStep], waiting: &HashSet<String>) -> usize {
    let mut chain: HashSet<String> = HashSet::from([blocked.to_string()]);
    let mut n = 0;
    for s in steps {
        let Some(num) = s.number.as_deref().filter(|n| !n.is_empty()) else {
            continue;
        };
        if !s.needs.iter().any(|x| chain.contains(js::trim(x))) {
            continue;
        }
        chain.insert(num.to_string());
        if waiting.contains(num) {
            n += 1;
        }
    }
    n
}

/// The plan's private details for a step's request, with the rule for them.
pub fn private_block(details: &[String]) -> String {
    static BREAK: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"{s}*\n{s}*", s = js::S)));
    if details.is_empty() {
        return String::new();
    }
    let mut out = vec![
        String::new(),
        "Private details the owner gave earlier in this plan (logins, passwords). Use them where the work needs them; \
         never write them into files, notes, commits, plans or your answers:"
            .to_string(),
    ];
    // One line each: the stored copy masks each "- " line, so a detail must not run on to a next line.
    out.extend(
        details
            .iter()
            .map(|d| format!("- {}", BREAK.replace_all(d, " "))),
    );
    out.join("\n")
}

fn unsafe_chars(s: &str) -> String {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"[^A-Za-z0-9._-]+"));
    RE.replace_all(s, "_").into_owned()
}

/// The folder of a plan's step logs.
pub fn log_dir(plan_file: &str) -> String {
    let p = plan_file.replace('\\', "/");
    let stem = if p.len() >= 3
        && p.is_char_boundary(p.len() - 3)
        && p[p.len() - 3..].eq_ignore_ascii_case(".md")
    {
        &p[..p.len() - 3]
    } else {
        &p
    };
    format!(".qanuvi/plan-logs/{}", unsafe_chars(stem))
}

/// One step's log.
pub fn log_path(plan_file: &str, number: Option<&str>, title: &str) -> String {
    let name = number.unwrap_or_else(|| js::cut(title, 40));
    format!("{}/{}.md", log_dir(plan_file), unsafe_chars(name))
}

/// One attempt, for a step's log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptLog {
    /// When it ran, in milliseconds since 1970 (UTC).
    pub when: i64,
    pub model: String,
    pub outcome: String,
    pub cost_usd: f64,
    pub seconds: f64,
    #[serde(default)]
    pub tests_passed: Option<bool>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub failure: Option<String>,
    #[serde(default)]
    pub answer: Option<String>,
}

/// One attempt in a step's log.
pub fn attempt_entry(a: &AttemptLog) -> String {
    let cut = |t: &str, n: usize| {
        if js::len(t) > n {
            format!("{} …", js::cut(t, n))
        } else {
            t.to_string()
        }
    };
    let when = jiff::Timestamp::from_millisecond(a.when)
        .map(|t| t.strftime("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let outcome = if a.outcome == "open" {
        "turn ended"
    } else {
        &a.outcome
    };
    let mut lines = vec![
        String::new(),
        format!(
            "## Attempt {when} · {} · {outcome} · ${} · {} s",
            a.model,
            js::to_fixed(a.cost_usd, 2),
            js::number(js::round(a.seconds))
        ),
    ];
    if let Some(passed) = a.tests_passed {
        lines.push(
            if passed {
                "Tests: passed."
            } else {
                "Tests: failed."
            }
            .into(),
        );
    }
    let some = |v: &Option<String>| v.clone().filter(|s| !s.is_empty());
    if let Some(note) = some(&a.note) {
        lines.push(format!("Note: {note}"));
    }
    if let Some(failure) = some(&a.failure) {
        lines.extend([
            "Failure:".into(),
            "```".into(),
            cut(&failure, 1500),
            "```".into(),
        ]);
    }
    if let Some(answer) = some(&a.answer) {
        lines.extend([
            "The agent's last answer:".into(),
            String::new(),
            cut(js::trim(&answer), 1500),
        ]);
    }
    lines.join("\n") + "\n"
}

/// A step's result in its log.
pub fn result_entry(status: &str, reason: Option<&str>) -> String {
    let reason = reason
        .filter(|r| !r.is_empty())
        .map(|r| format!(" — {r}"))
        .unwrap_or_default();
    format!("\n**Result: {status}**{reason}\n")
}

/// What a step is told about the logs.
pub fn logs_block(plan_file: &str, own: &str, own_exists: bool) -> String {
    let mut out = vec![
        String::new(),
        format!(
            "Every step of this plan has a log in {}/ (its attempts, failures and results). Read the log of a step you depend on or that failed, when it helps.",
            log_dir(plan_file)
        ),
    ];
    if own_exists {
        out.push(format!(
            "This step was tried before: read its log {own} first and do not repeat what failed."
        ));
    }
    out.join("\n")
}

/// The plan map: what earlier steps built and where (kept by the steps themselves).
pub fn map_path(plan_file: &str) -> String {
    format!("{}/MAP.md", log_dir(plan_file))
}

/// The files a step changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub number: Option<String>,
    pub files: Vec<String>,
}

fn recent_lines(recent: &[Recent]) -> Vec<String> {
    let shown: Vec<&Recent> = recent.iter().filter(|r| !r.files.is_empty()).collect();
    shown[shown.len().saturating_sub(3)..]
        .iter()
        .map(|r| {
            let files = r
                .files
                .iter()
                .take(25)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = if r.files.len() > 25 { ", …" } else { "" };
            format!("- {}: {files}{more}", r.number.as_deref().unwrap_or("step"))
        })
        .collect()
}

const INDEX_HINT: &str = "Finding your way: use Qanuvi's code index (find_symbol for where something is defined, outline for what a file holds, read_lines for just the lines you need, project_map for the layout) rather than reading whole files with cat or sed.";

/// Text cut to 8000 characters ("…" on a line of its own when cut).
fn cut_8000(t: &str) -> String {
    if js::len(t) > 8000 {
        format!("{}\n…", js::cut(t, 8000))
    } else {
        t.to_string()
    }
}

/// What a step gets so it does not have to find its way from nothing: the plan map, the files the last steps
/// changed, how to find code (Qanuvi's index), and the rule to keep the map current.
pub fn context_block(plan_file: &str, map: &str, recent: &[Recent], index_tools: bool) -> String {
    let path = map_path(plan_file);
    let mut lines = vec![String::new()];
    if !js::trim(map).is_empty() {
        lines.push(format!(
            "The plan map ({path}): what earlier steps built and where. Start from it."
        ));
        lines.push(js::trim(&cut_8000(map)).to_string());
        lines.push(String::new());
    }
    let shown = recent_lines(recent);
    if !shown.is_empty() {
        lines.push("Files the last steps changed:".into());
        lines.extend(shown);
        lines.push(String::new());
    }
    if index_tools {
        lines.push(INDEX_HINT.into());
    }
    lines.push(format!(
        "When the step is done, update the plan map {path} (create it if it is missing): under 80 lines, one line per file, \"path — what it holds or does\", grouped by area; add what this step created or changed, keep earlier lines unless they are wrong now."
    ));
    lines.join("\n")
}

pub const MAP_LINES: usize = 80;
pub const FACTS_LINES: usize = 60;

pub fn facts_path(plan_file: &str) -> String {
    format!("{}/FACTS.md", log_dir(plan_file))
}

pub fn api_path(plan_file: &str) -> String {
    format!("{}/API.md", log_dir(plan_file))
}

fn line_count(text: &str) -> usize {
    text.split('\n').filter(|l| !js::trim(l).is_empty()).count()
}

/// What every step gets first (the same for steps in a row, so it is read from the cache): where the plan's
/// knowledge is (MAP.md, FACTS.md, API.md, a log per step), the map and the facts, the files the last steps changed,
/// how to find code, and the rule to keep the files current.
pub fn knowledge_block(
    plan_file: &str,
    map: &str,
    facts: &str,
    recent: &[Recent],
    index_tools: bool,
) -> String {
    let dir = log_dir(plan_file);
    let cut = |t: &str| {
        if js::len(t) > 8000 {
            cut_8000(t)
        } else {
            js::trim(t).to_string()
        }
    };
    let mut lines = vec![format!(
        "This plan's knowledge, shared by every step and model, is in {dir}/: MAP.md (what is where), FACTS.md (decisions, conventions, how to run and test, gotchas), API.md (functions and classes the plan added, with file and line; read it before opening files) and a log per step. Start from it; do not rediscover what it says."
    )];
    if !js::trim(map).is_empty() {
        lines.extend([String::new(), "MAP.md:".into(), cut(map)]);
    }
    if !js::trim(facts).is_empty() {
        lines.extend([String::new(), "FACTS.md:".into(), cut(facts)]);
    }
    let shown = recent_lines(recent);
    if !shown.is_empty() {
        lines.extend([String::new(), "Files the last steps changed:".into()]);
        lines.extend(shown);
    }
    if index_tools {
        lines.extend([String::new(), INDEX_HINT.into()]);
    }
    let mut tidy: Vec<String> = vec![];
    if line_count(map) > MAP_LINES {
        tidy.push(format!(
            "MAP.md has {} lines: tidy it to under {MAP_LINES}",
            line_count(map)
        ));
    }
    if line_count(facts) > FACTS_LINES {
        tidy.push(format!(
            "FACTS.md has {} lines: tidy it to under {FACTS_LINES}",
            line_count(facts)
        ));
    }
    let tidy = if tidy.is_empty() {
        String::new()
    } else {
        format!(" {}.", tidy.join("; "))
    };
    lines.extend([
        String::new(),
        format!(
            "Before you finish: update {dir}/MAP.md (one line per file, \"path — what it holds or does\", under {MAP_LINES} lines; create it if missing) and {dir}/FACTS.md (what later steps must know: decisions, conventions, how to run things, gotchas; under {FACTS_LINES} lines; never passwords or keys). Change or remove lines that are no longer right instead of adding more.{tidy}"
        ),
        String::new(),
    ]);
    lines.join("\n")
}

/// Whether a changed file is code the index outlines.
pub fn is_code(path: &str) -> bool {
    static CODE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i-u)\.(?:php|js|mjs|cjs|ts|tsx|jsx|svelte|vue|py|rs|go|java|kt|cs|rb|swift|dart|c|cc|cpp|h|hpp)$",
        )
    });
    static SKIP: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?:^|/)(?:node_modules|vendor|dist|build)/"));
    CODE.is_match(path) && !SKIP.is_match(path)
}

/// A definition in a file's outline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiItem {
    pub name: String,
    pub kind: String,
    pub line: i64,
    pub container: Option<String>,
}

/// One file's part of API.md, from the code index's outline.
pub fn api_section(path: &str, items: &[ApiItem]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = vec![format!("### {path}")];
    out.extend(items.iter().take(40).map(|i| {
        let inside = i
            .container
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| format!("{c}::"))
            .unwrap_or_default();
        format!("- {} {inside}{} :{}", i.kind, i.name, i.line)
    }));
    out.join("\n")
}

/// API.md with the given files' parts replaced (an empty part removes the file); files in path order.
pub fn merge_api(existing: &str, updates: &[(String, String)]) -> String {
    static HEAD: LazyLock<Regex> = LazyLock::new(|| re(&format!(r"^### ({}+)", js::DOT)));
    let mut parts: BTreeMap<Vec<u16>, (String, String)> = BTreeMap::new();
    // `existing.split(/\n(?=### )/)`.
    let mut chunks = vec![];
    let mut from = 0;
    for (i, _) in existing.match_indices("\n### ") {
        chunks.push(&existing[from..i]);
        from = i + 1;
    }
    chunks.push(&existing[from..]);
    let key = |p: &str| -> Vec<u16> { p.encode_utf16().collect() };
    for chunk in chunks {
        if let Some(m) = HEAD.captures(chunk) {
            let path = js::trim(&m[1]).to_string();
            parts.insert(key(&path), (path, js::trim(chunk).to_string()));
        }
    }
    for (path, section) in updates {
        if section.is_empty() {
            parts.remove(&key(path));
        } else {
            parts.insert(key(path), (path.clone(), section.clone()));
        }
    }
    let mut out = vec![
        "# What this plan added (written by Qanuvi from its code index after each step)"
            .to_string(),
        String::new(),
    ];
    for (_, (_, b)) in parts {
        out.push(b);
        out.push(String::new());
    }
    out.join("\n")
}

/// Other plans this one builds on: those its steps need ("Needs: OTHER-PLAN.md#4") and those its text names.
pub fn referenced_plans(plan_text: &str, plan_file: &str) -> Vec<String> {
    static NEEDS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i-u:needs):[^\n]*"));
    static NAMED: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?i-u)[\w./-]*(?:task[-_ ]?list|todo|plan|roadmap)[\w.-]*\.md"));
    let me = plan_file.replace('\\', "/").to_lowercase();
    let mut found: Vec<String> = vec![];
    let mut add = |p: String| {
        if !found.contains(&p) {
            found.push(p);
        }
    };
    for m in NEEDS.find_iter(plan_text) {
        for part in m.as_str()[6..].split(',') {
            if let Some(h) = part.rfind('#') {
                add(js::trim(&part[..h]).to_string());
            }
        }
    }
    for m in NAMED.find_iter(plan_text) {
        add(m.as_str().to_string());
    }
    found
        .into_iter()
        .map(|p| {
            let p = p.replace('\\', "/");
            p.strip_prefix("./").map(String::from).unwrap_or(p)
        })
        .filter(|p| {
            let low = p.to_lowercase();
            !p.is_empty()
                && low != me
                && !low.ends_with(&format!("/{me}"))
                && !me.ends_with(&format!("/{low}"))
                && !p.starts_with(".qanuvi/")
        })
        .collect()
}

/// The lines telling a step where referenced plans keep their knowledge.
pub fn referenced_block(plans: &[String]) -> String {
    if plans.is_empty() {
        return String::new();
    }
    let mut out = vec![
        String::new(),
        "Plans this one builds on keep their own knowledge (read their MAP.md, FACTS.md and API.md when the step touches their work):".to_string(),
    ];
    out.extend(
        plans
            .iter()
            .take(8)
            .map(|p| format!("- {p}: {}/", log_dir(p))),
    );
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::tasklist::parse_plan;
    use serde_json::json;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn keeps_only_the_users_words_when_shown() {
        let sent = format!("use the shop currency{}", guidance_note("82a", "PLAN-D.md"));
        assert!(sent.contains("82a-1"));
        assert_eq!(without_guidance(&sent), "use the shop currency");
    }

    #[test]
    fn knows_a_plain_stop_order() {
        for t in ["stop", "Stop!", "stop the run", "halt now", "cancel"] {
            assert!(is_stop_order(t), "{t}");
        }
        for t in [
            "stop using floats",
            "do not stop",
            "stop adding tests to the cart",
        ] {
            assert!(!is_stop_order(t), "{t}");
        }
    }

    #[test]
    fn finds_steps_added_while_the_run_went() {
        let before = parse_plan("- [ ] **82a. One**\n- [ ] **82b. Two**\n").steps;
        let now =
            parse_plan("- [x] **82a. One**\n- [ ] **82a-1. Extra**\n- [ ] **82b. Two**\n").steps;
        let known: HashSet<String> = before.iter().map(key_of).collect();
        let added = added_steps(&now, &known);
        assert_eq!(
            added
                .iter()
                .map(|s| s.number.as_deref())
                .collect::<Vec<_>>(),
            [Some("82a-1")]
        );
    }

    #[test]
    fn passes_notes_on_and_reads_the_ones_agents_leave() {
        assert!(
            notes_block(&strings(&["use the shop currency"])).contains("- use the shop currency")
        );
        assert_eq!(
            notes_in("Done.\n**Note for the next steps:** tests need php 8.2\nbye"),
            ["tests need php 8.2"]
        );
        assert!(notes_in("Note for the next steps: password: hunter22").is_empty());
    }

    #[test]
    fn notices_passwords_and_keys() {
        assert!(looks_secret("root qz7test9 if you need db user pass"));
        assert!(looks_secret("the db password is abc12345"));
        assert!(looks_secret("key sk-test-abcdefghijklmnopqrstu"));
        assert!(!looks_secret("use the shop currency"));
        assert!(!looks_secret("do not change the login page"));
    }

    #[test]
    fn runs_a_passed_step_again_once() {
        let run: Vec<StepRun> = serde_json::from_value(json!([
            { "line": 0, "number": "82a", "title": "One", "status": "done" },
            { "line": 1, "number": "82b", "title": "Two", "status": "blocked" },
            { "line": 2, "number": "82c", "title": "Three", "status": "running" },
        ]))
        .unwrap();
        let now =
            parse_plan("- [x] **82a. One**\n- [ ] **82b. Two**\n- [ ] **82c. Three**\n").steps;
        let got = reopened_steps(&run, &now, &HashSet::new());
        assert_eq!(
            got.iter().map(|s| s.number.as_deref()).collect::<Vec<_>>(),
            [Some("82b")]
        );
        assert!(reopened_steps(&run, &now, &HashSet::from(["82b|Two".to_string()])).is_empty());
        let still = parse_plan("- [!] **82b. Two**\n").steps;
        assert!(reopened_steps(&run, &still, &HashSet::new()).is_empty());
    }

    #[test]
    fn reads_the_context_size_from_the_last_model_call() {
        let line = |i: u64, c: u64| {
            json!({ "type": "assistant", "message": { "usage": { "input_tokens": i, "cache_read_input_tokens": c } } })
                .to_string()
        };
        let lines = vec![
            line(10, 100),
            "x".into(),
            line(5, 2000),
            r#"{"type":"result"}"#.into(),
        ];
        assert_eq!(context_size(&lines), 2005);
        assert_eq!(context_size(&[]), 0);
        let one = json!({
            "type": "assistant",
            "message": { "usage": { "input_tokens": 10, "cache_read_input_tokens": 180_000, "cache_creation_input_tokens": 25_000 } },
        });
        assert_eq!(context_size(&[one.to_string()]), 205_010);
    }

    #[test]
    fn continues_only_on_the_same_model_and_section_while_small() {
        let c = Carry {
            model: "opus".into(),
            section: "S82".into(),
            tokens: 40000,
            steps: 2,
        };
        assert!(can_carry(Some(&c), "opus", "S82", 80000));
        assert!(!can_carry(Some(&c), "sonnet", "S82", 80000));
        assert!(!can_carry(Some(&c), "opus", "S83", 80000));
        assert!(!can_carry(
            Some(&Carry {
                tokens: 90000,
                ..c.clone()
            }),
            "opus",
            "S82",
            80000
        ));
        assert!(!can_carry(
            Some(&Carry {
                steps: 6,
                ..c.clone()
            }),
            "opus",
            "S82",
            80000
        ));
        assert!(!can_carry(None, "opus", "S82", 80000));
    }

    #[test]
    fn counts_the_steps_a_blocked_one_holds_up() {
        let s = |n: &str, need: &str| NeedStep {
            number: Some(n.into()),
            needs: vec![need.into()],
        };
        let steps = [
            s("82c", "82a"),
            s("82d", "82b"),
            s("82e", "82d"),
            s("83a", "82z"),
        ];
        let waiting: HashSet<String> = strings(&["82c", "82d", "82e", "83a"]).into_iter().collect();
        assert_eq!(needing_count("82b", &steps, &waiting), 2);
    }

    #[test]
    fn keeps_private_details_on_one_line_each() {
        assert_eq!(private_block(&[]), "");
        let b = private_block(&strings(&["db login: root / example1"]));
        assert!(b.contains("- db login: root / example1"));
        assert!(b.contains("never write them into files"));
        let b = private_block(&strings(&["db user: test\npassword: x1"]));
        assert_eq!(
            b.split('\n')
                .filter(|l| l.starts_with("- "))
                .collect::<Vec<_>>(),
            ["- db user: test password: x1"]
        );
    }

    #[test]
    fn has_one_log_per_step() {
        assert_eq!(
            log_path("content/plugins/qcommerce/PLAN-D.md", Some("82b"), "x"),
            ".qanuvi/plan-logs/content_plugins_qcommerce_PLAN-D/82b.md"
        );
        assert_eq!(
            log_path("PLAN.md", None, "Add greet test"),
            ".qanuvi/plan-logs/PLAN/Add_greet_test.md"
        );
    }

    #[test]
    fn records_attempts_and_results() {
        let e = attempt_entry(&AttemptLog {
            when: "2026-11-12T10:00:00Z"
                .parse::<jiff::Timestamp>()
                .unwrap()
                .as_millisecond(),
            model: "claude-opus-5-5".into(),
            outcome: "open".into(),
            cost_usd: 2.19,
            seconds: 349.0,
            tests_passed: Some(false),
            note: None,
            failure: Some("Expected 1200, got 1250".into()),
            answer: Some("The cart cache was not cleared.".into()),
        });
        assert!(
            e.contains(
                "## Attempt 2026-11-12 10:00 · claude-opus-5-5 · turn ended · $2.19 · 349 s"
            ),
            "{e}"
        );
        assert!(e.contains("Tests: failed."));
        assert!(e.contains("Expected 1200, got 1250"));
        assert!(
            result_entry("blocked", Some("no database login"))
                .contains("**Result: blocked** — no database login")
        );
        assert!(
            logs_block("PLAN.md", ".qanuvi/plan-logs/PLAN/2.md", true).contains("tried before")
        );
        assert!(
            !logs_block("PLAN.md", ".qanuvi/plan-logs/PLAN/2.md", false).contains("tried before")
        );
    }

    #[test]
    fn passes_the_map_the_last_changes_and_the_index_hint() {
        let b = context_block(
            "PLAN-D.md",
            "src/Cart/CartLine.php — a cart line and its chosen amount",
            &[
                Recent {
                    number: Some("82d".into()),
                    files: strings(&["src/Checkout/Separate.php"]),
                },
                Recent {
                    number: Some("82e".into()),
                    files: vec![],
                },
            ],
            true,
        );
        assert!(b.contains("CartLine.php — a cart line"));
        assert!(b.contains("- 82d: src/Checkout/Separate.php"));
        assert!(!b.contains("- 82e"));
        assert!(b.contains("find_symbol"));
        assert!(b.contains(&format!("update the plan map {}", map_path("PLAN-D.md"))));
        assert!(!context_block("P.md", "", &[], false).contains("find_symbol"));
    }

    #[test]
    fn gives_every_step_the_map_and_facts_first() {
        let long_map = (0..90)
            .map(|i| format!("f{i}.php — x"))
            .collect::<Vec<_>>()
            .join("\n");
        let b = knowledge_block(
            "PLAN-D.md",
            &long_map,
            "Tests: php tests/run.php --strict",
            &[],
            true,
        );
        assert!(b.starts_with("This plan's knowledge"));
        assert!(b.contains("FACTS.md:\nTests: php tests/run.php --strict"));
        assert!(b.contains("MAP.md has 90 lines: tidy it to under 80"));
        assert!(b.contains("never passwords or keys"));
    }

    #[test]
    fn keeps_api_md_one_part_per_file() {
        assert!(is_code("src/Cart/CartLine.php"));
        assert!(!is_code("README.md"));
        assert!(!is_code("vendor/x/y.php"));
        let a = api_section(
            "src/A.php",
            &[ApiItem {
                name: "resetCache".into(),
                kind: "method".into(),
                line: 88,
                container: Some("CartStorage".into()),
            }],
        );
        assert_eq!(a, "### src/A.php\n- method CartStorage::resetCache :88");
        let api = merge_api(
            "",
            &[
                ("src/B.php".into(), "### src/B.php\n- class B :3".into()),
                ("src/A.php".into(), a.clone()),
            ],
        );
        assert!(api.find("src/A.php") < api.find("src/B.php"));
        let api = merge_api(&api, &[("src/B.php".into(), String::new())]);
        assert!(!api.contains("src/B.php"));
        assert!(api.contains("CartStorage::resetCache :88"));
    }

    #[test]
    fn finds_the_plans_a_plan_needs_or_names() {
        let text = "Builds on content/plugins/qcommerce/PLAN-C.md and the DEVELOPMENT-TASKLIST.md.\n- [ ] **90. Hand over**\n  - Needs: 89z, docs/OTHER-PLAN.md#4, docs/OTHER-PLAN.md#5\nSee PLAN-D.md for this plan.";
        let mut refs = referenced_plans(text, "PLAN-D.md");
        refs.sort();
        assert_eq!(
            refs,
            [
                "DEVELOPMENT-TASKLIST.md",
                "content/plugins/qcommerce/PLAN-C.md",
                "docs/OTHER-PLAN.md"
            ]
        );
        assert!(
            referenced_block(&strings(&["docs/OTHER-PLAN.md"]))
                .contains(".qanuvi/plan-logs/docs_OTHER-PLAN/")
        );
        assert_eq!(referenced_block(&[]), "");
    }

    #[test]
    fn a_stopped_steps_chat_asks_before_the_run_goes_on() {
        let note = stopped_chat_note("53z", "PLAN-E.md");
        assert_eq!(
            without_guidance(&format!("what is your best recommendation?{note}")),
            "what is your best recommendation?"
        );
        assert!(note.contains(&format!("{RESUME_ASK} 53z? (yes/no)")));
        assert!(note.contains("Answer what they ask first"));
        for t in [
            "continue",
            "Resume.",
            "go on",
            "retry the step",
            "continue the run",
            "run it again",
            "resume the plan run",
        ] {
            assert!(is_go_on_order(t), "{t}");
        }
        for t in [
            "continue?",
            "should we continue the run",
            "what is your best recommendation",
            "continue with option 1 but",
        ] {
            assert!(!is_go_on_order(t), "{t}");
        }
        for t in ["yes", "Yes!", "ok", "go ahead", "yes, resume"] {
            assert!(is_yes(t), "{t}");
        }
        for t in ["yes but first explain", "no", "1"] {
            assert!(!is_yes(t), "{t}");
        }
        assert!(asked_to_resume(&format!(
            "Done.\n{RESUME_ASK} 53z? (yes/no)"
        )));
        assert!(!asked_to_resume("Should I use option 1? (yes/no)"));
    }

    #[test]
    fn hands_over_only_past_the_limit() {
        assert!(!handover_due(150_000, 200_000, 0));
        assert!(handover_due(200_000, 200_000, 0));
        assert!(!handover_due(900_000, 0, 0));
        assert!(handover_due(300_000, 200_000, HANDOVER_MAX - 1));
        assert!(!handover_due(300_000, 200_000, HANDOVER_MAX));
        let ask = handover_request("PLAN-D/logs/83b.md", 212_345);
        assert!(ask.contains("212k"));
        assert!(ask.contains("PLAN-D/logs/83b.md"));
        assert!(ask.contains("half-written"));
        assert!(ask.contains("Do not mark the step done unless it is"));
        assert!(HANDOVER_RESUME.contains("check that what it calls done really is"));
        let r = model_switch_request(".qanuvi/plan-logs/p/93a.md", "Opus 5.5");
        assert!(r.contains("Opus 5.5"));
        assert!(r.contains("Finish the edit you are making"));
        assert!(r.contains(".qanuvi/plan-logs/p/93a.md"));
        assert!(r.contains("Do not mark the step done unless it is"));
        assert!(SWITCH_RESUME.contains("Progress note"));
    }
}
