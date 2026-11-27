//! O3 / 24v: Qanuvi's own context compression, for any AI, no Python. Tool output a model reads (logs, JSON, search
//! results, diffs, web pages) is cut down before it is sent; what was left out is kept on this machine and the model can
//! ask for it back with the `qanuvi_retrieve` tool. Headroom's ideas (a router by content type, errors kept byte for
//! byte, statistics instead of keyword lists for JSON, originals kept for retrieval) were studied; this is our own code.
//!
//! Rules every compressor keeps:
//! - deterministic: the same text always gives the same result, so a conversation's earlier parts stay byte-identical
//!   from request to request and the provider's prompt cache keeps hitting;
//! - error lines (error, fail, fatal, panic, exception, traceback, denied ...) are kept as they are, with a line on
//!   each side;
//! - small output (under [`MIN_CHARS`]) and code read from files are never touched: an agent edits code by its exact
//!   text;
//! - the result says what was left out and how to get it back, in one line a model understands.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Output shorter than this is sent as it is (too small to be worth a retrieval).
pub const MIN_CHARS: usize = 2_000;
/// What a compressed result may still hold, at most, in characters (about 1,500 tokens).
const TARGET_CHARS: usize = 6_000;
/// The retrieval tool's name, for Qanuvi's own agent and the proxy's MCP tool.
pub const RETRIEVE_TOOL: &str = "qanuvi_retrieve";
/// How long originals are kept for retrieval.
const KEEP_SECS: u64 = 24 * 3600;

/// The machine setting that picks the compression (Settings → Agents): unset (automatic), "builtin", "headroom" (the
/// user's own Headroom proxy for Claude Code, as before), or "off".
pub const SETTING: &str = "compression";

/// Which compression this machine uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Builtin,
    Headroom,
    Off,
}

/// The mode from the machine's settings. Unset is automatic: Headroom when Claude Code is already sent to one
/// (`headroom_set_up`, so an update never moves a working Headroom setup aside without asking), else built in.
pub fn mode_of(settings: &serde_json::Map<String, Value>) -> Mode {
    mode_with(settings, headroom_set_up)
}

fn mode_with(settings: &serde_json::Map<String, Value>, headroom: impl Fn() -> bool) -> Mode {
    match settings.get(SETTING).and_then(Value::as_str) {
        Some("off") => Mode::Off,
        Some("headroom") => Mode::Headroom,
        Some("builtin") => Mode::Builtin,
        _ if headroom() => Mode::Headroom,
        _ => Mode::Builtin,
    }
}

/// Whether Claude Code's own settings on this computer send it to a Headroom proxy.
pub fn headroom_set_up() -> bool {
    crate::claude_update::settings_base_url().is_some_and(|u| looks_like_headroom(&u))
}

/// An address that is a Headroom proxy (its usual port or name).
pub fn looks_like_headroom(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains(":8787") || u.contains("headroom")
}

/// What kind of text a tool output is; the router picks the compressor by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Json,
    Log,
    Search,
    Diff,
    Html,
    Code,
    Text,
}

/// One compression: the text to send, what it was, and how much it saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Compressed {
    pub text: String,
    pub kind: Kind,
    /// The original's retrieval id when something was left out (its text is kept in the store).
    pub id: Option<String>,
    pub before: usize,
    pub after: usize,
}

impl Compressed {
    fn unchanged(text: &str, kind: Kind) -> Self {
        Compressed {
            text: text.to_string(),
            kind,
            id: None,
            before: text.len(),
            after: text.len(),
        }
    }
}

/// Where the text came from, as far as the caller knows: the tool's name and, for a shell tool, its command.
#[derive(Clone, Debug, Default)]
pub struct Hint<'a> {
    pub tool: &'a str,
    pub command: &'a str,
    pub path: &'a str,
}

/// A line that must reach the model as it is.
pub fn is_error_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    [
        "error",
        "fail",
        "fatal",
        "panic",
        "exception",
        "traceback",
        "denied",
        "refused",
        "cannot ",
        "can't ",
        "not found",
        "undefined",
        "segfault",
        "abort",
        "critical",
        "warning",
    ]
    .iter()
    .any(|w| l.contains(w))
}

/// The router: what kind of text this is.
pub fn kind_of(text: &str, hint: &Hint) -> Kind {
    let tool = hint.tool.to_ascii_lowercase();
    let path = hint.path.to_ascii_lowercase();
    // Files read for work: code and data an agent may edit stay whole (a .json file read is code here too).
    if tool == "read" || tool.ends_with("read_file") || tool == "view" {
        return Kind::Code;
    }
    if tool == "grep" || tool == "search" || tool.contains("search") || tool.contains("find") {
        return Kind::Search;
    }
    let t = text.trim_start();
    if (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<Value>(t).is_ok() {
        return Kind::Json;
    }
    if t.starts_with("diff --git")
        || (t.starts_with("--- ") && t.contains("\n+++ "))
        || t.contains("\n@@ -")
    {
        return Kind::Diff;
    }
    let low = t.chars().take(400).collect::<String>().to_ascii_lowercase();
    if low.starts_with("<!doctype html")
        || low.starts_with("<html")
        || (low.contains("<div") && low.contains("</"))
    {
        return Kind::Html;
    }
    if path.ends_with(".html") || path.ends_with(".htm") {
        return Kind::Html;
    }
    let lines: Vec<&str> = t.lines().take(200).collect();
    if lines.len() >= 5 {
        let grep_like = lines.iter().filter(|l| looks_like_match(l)).count();
        if grep_like * 10 >= lines.len() * 7 {
            return Kind::Search;
        }
    }
    if lines.len() >= 20 {
        return Kind::Log;
    }
    Kind::Text
}

/// A line without its Windows drive ("C:"), so its first ':' is the one after the path.
fn without_drive(line: &str) -> (&str, &str) {
    match line.as_bytes() {
        [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic() => (&line[..2], &line[2..]),
        _ => ("", line),
    }
}

/// The path of a "path:12:text" line, drive included.
fn match_file(line: &str) -> &str {
    let (drive, rest) = without_drive(line);
    let path = rest.split(':').next().unwrap_or("");
    &line[..drive.len() + path.len()]
}

/// "path:12:text" or "path:12: text" (grep, ripgrep, many search tools). The path must look like one (a separator or a
/// dot, not a date): "2026-11-27T08:00:00 INFO" is a log line, not a match. A Windows "C:" drive is skipped first.
fn looks_like_match(line: &str) -> bool {
    let (_, rest) = without_drive(line);
    let mut parts = rest.splitn(3, ':');
    let (Some(file), Some(num), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let date = file.as_bytes().first().is_some_and(u8::is_ascii_digit) && file.contains('-');
    !file.is_empty()
        && !file.contains(' ')
        && (file.contains('/') || file.contains('\\') || file.contains('.'))
        && !date
        && !num.is_empty()
        && num.bytes().all(|b| b.is_ascii_digit())
}

/// Compresses one tool output. Originals of anything left out go to `store` (when given) for retrieval.
pub fn compress(text: &str, hint: &Hint, store: Option<&Store>) -> Compressed {
    let kind = kind_of(text, hint);
    if text.len() < MIN_CHARS || kind == Kind::Code || kind == Kind::Text {
        return Compressed::unchanged(text, kind);
    }
    let id = id_of(text);
    let cut = match kind {
        Kind::Json => json(text, &id),
        Kind::Log => log(text, &id),
        Kind::Search => search(text, &id),
        Kind::Diff => diff(text, &id),
        Kind::Html => html(text, &id),
        Kind::Code | Kind::Text => None,
    };
    match cut {
        // Worth it only when it saves at least a fifth.
        Some(out) if out.len() * 5 < text.len() * 4 => {
            let kept = store.map(|s| s.put(&id, text).is_ok()).unwrap_or(false);
            let out = if kept {
                out
            } else {
                // Nothing to retrieve from: the note must not promise it.
                out.replace(&retrieve_note(&id), "(the rest was left out)")
            };
            Compressed {
                after: out.len(),
                text: out,
                kind,
                id: kept.then_some(id),
                before: text.len(),
            }
        }
        _ => Compressed::unchanged(text, kind),
    }
}

/// The id of an original: its content hash (the same text always gets the same id).
pub fn id_of(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(text.as_bytes());
    h.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

fn retrieve_note(id: &str) -> String {
    format!(
        "({RETRIEVE_TOOL} id \"{id}\" gives the whole output, or only the lines matching a search)"
    )
}

/// Logs and long command output: repeated lines said once with a count, error lines and their neighbours kept, the
/// start and the end kept, the middle left out.
fn log(text: &str, id: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 20 {
        return None;
    }
    // Repeats: a line equal to the one before once digits are ignored (timestamps, counters, ids).
    let shape = |l: &str| {
        l.chars()
            .filter(|c| !c.is_ascii_digit())
            .collect::<String>()
    };
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (first line, how many)
    for (i, l) in lines.iter().enumerate() {
        match runs.last_mut() {
            Some((start, n)) if shape(lines[*start]) == shape(l) && !is_error_line(l) => *n += 1,
            _ => runs.push((i, 1)),
        }
    }
    // Lines that must stay: errors with one line around them, the first 15 and the last 25.
    let mut keep = vec![false; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        if is_error_line(l) {
            let end = (i + 1).min(lines.len() - 1);
            keep[i.saturating_sub(1)..=end].fill(true);
        }
    }
    for k in keep.iter_mut().take(15) {
        *k = true;
    }
    let tail = lines.len().saturating_sub(25);
    for k in keep.iter_mut().skip(tail) {
        *k = true;
    }
    let mut out = String::new();
    let mut skipped = 0usize;
    for &(start, n) in &runs {
        let wanted = (start..start + n).any(|k| keep[k]);
        if !wanted {
            skipped += n;
            continue;
        }
        if skipped > 0 {
            out.push_str(&format!("[... {skipped} lines left out ...]\n"));
            skipped = 0;
        }
        out.push_str(lines[start]);
        out.push('\n');
        if n > 1 {
            out.push_str(&format!(
                "[... the line above {} more times, numbers changing ...]\n",
                n - 1
            ));
        }
    }
    if skipped > 0 {
        out.push_str(&format!("[... {skipped} lines left out ...]\n"));
    }
    // Still too long (errors everywhere): keep the error lines only, after the head.
    if out.len() > TARGET_CHARS * 2 {
        let mut short = String::new();
        for l in lines.iter().take(10) {
            short.push_str(l);
            short.push('\n');
        }
        let errors: Vec<&&str> = lines.iter().skip(10).filter(|l| is_error_line(l)).collect();
        let mut seen = std::collections::BTreeSet::new();
        for l in errors {
            if seen.insert(shape(l)) {
                short.push_str(l);
                short.push('\n');
            }
        }
        out = short;
    }
    Some(format!(
        "{out}[{} lines in all; repeats and the middle were left out; error lines are whole. {}]",
        lines.len(),
        retrieve_note(id)
    ))
}

/// Search results (grep, ripgrep): every file named, its first matches kept, the count of the rest said.
fn search(text: &str, id: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut files: Vec<(String, Vec<&str>)> = Vec::new();
    let mut other: Vec<&str> = Vec::new();
    for l in &lines {
        if looks_like_match(l) {
            let file = match_file(l).to_string();
            match files.iter_mut().find(|(f, _)| *f == file) {
                Some((_, v)) => v.push(l),
                None => files.push((file, vec![l])),
            }
        } else if !l.trim().is_empty() {
            other.push(l);
        }
    }
    if files.is_empty() {
        return log(text, id);
    }
    let per_file = if files.len() > 40 { 2 } else { 5 };
    let mut out = String::new();
    for l in other.iter().take(10) {
        out.push_str(l);
        out.push('\n');
    }
    for (file, hits) in &files {
        for h in hits.iter().take(per_file) {
            out.push_str(&clip(h, 300));
            out.push('\n');
        }
        if hits.len() > per_file {
            out.push_str(&format!(
                "{file}: [... {} more matches ...]\n",
                hits.len() - per_file
            ));
        }
    }
    let total: usize = files.iter().map(|(_, h)| h.len()).sum();
    Some(format!(
        "{out}[{total} matches in {} files; only the first ones per file are shown. {}]",
        files.len(),
        retrieve_note(id)
    ))
}

/// Diffs: file headers, hunk headers and changed lines kept, unchanged context cut to one line around changes.
fn diff(text: &str, id: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let changed = |l: &str| {
        (l.starts_with('+') || l.starts_with('-')) && !l.starts_with("+++") && !l.starts_with("---")
    };
    let header = |l: &str| {
        l.starts_with("diff ")
            || l.starts_with("index ")
            || l.starts_with("+++")
            || l.starts_with("---")
            || l.starts_with("@@")
            || l.starts_with("new file")
            || l.starts_with("deleted file")
            || l.starts_with("rename ")
    };
    let mut keep = vec![false; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        if header(l) {
            keep[i] = true;
        }
        if changed(l) {
            let end = (i + 1).min(lines.len().saturating_sub(1));
            keep[i.saturating_sub(1)..=end].fill(true);
        }
    }
    let mut out = String::new();
    let mut skipped = 0;
    for (i, l) in lines.iter().enumerate() {
        if keep[i] {
            if skipped > 0 {
                out.push_str(&format!(" [... {skipped} unchanged lines ...]\n"));
                skipped = 0;
            }
            out.push_str(l);
            out.push('\n');
        } else {
            skipped += 1;
        }
    }
    Some(format!(
        "{out}[unchanged lines were cut. {}]",
        retrieve_note(id)
    ))
}

/// Web pages: scripts, styles and comments gone, tags to plain text, blank runs closed up.
fn html(text: &str, id: &str) -> Option<String> {
    let mut s = text.to_string();
    for (open, close) in [
        ("<script", "</script>"),
        ("<style", "</style>"),
        ("<!--", "-->"),
        ("<svg", "</svg>"),
    ] {
        loop {
            let low = s.to_ascii_lowercase();
            let Some(a) = low.find(open) else { break };
            let Some(b) = low[a..].find(close) else { break };
            s.replace_range(a..a + b + close.len(), " ");
        }
    }
    let mut plain = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                plain.push(' ');
            }
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"");
    let mut out = String::new();
    let mut blank = false;
    for l in plain.lines() {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() {
            if !blank && !out.is_empty() {
                out.push('\n');
            }
            blank = true;
        } else {
            out.push_str(&l);
            out.push('\n');
            blank = false;
        }
    }
    let out = if out.len() > TARGET_CHARS * 2 {
        format!(
            "{}\n[... the rest of the page ...]\n",
            clip(&out, TARGET_CHARS * 2)
        )
    } else {
        out
    };
    Some(format!(
        "{out}[the page as plain text. {}]",
        retrieve_note(id)
    ))
}

/// JSON: written compactly; long arrays keep their first and last items, items with errors, and items whose numbers
/// or texts stand out from the rest (by the array's own statistics); the count of the rest is said in their place.
fn json(text: &str, id: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let cut = thin(&v, 0);
    let out = serde_json::to_string(&cut).ok()?;
    Some(format!(
        "{out}\n[JSON written compactly; long lists thinned. {}]",
        retrieve_note(id)
    ))
}

fn thin(v: &Value, depth: usize) -> Value {
    match v {
        Value::Array(items) if items.len() > 12 && depth < 8 => {
            let keep = chosen(items);
            let mut out = Vec::new();
            let mut gap = 0usize;
            for (i, item) in items.iter().enumerate() {
                if keep.contains(&i) {
                    if gap > 0 {
                        out.push(Value::String(format!(
                            "[... {gap} similar items left out ...]"
                        )));
                        gap = 0;
                    }
                    out.push(thin(item, depth + 1));
                } else {
                    gap += 1;
                }
            }
            if gap > 0 {
                out.push(Value::String(format!(
                    "[... {gap} similar items left out ...]"
                )));
            }
            Value::Array(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(|i| thin(i, depth + 1)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, x)| (k.clone(), thin(x, depth + 1)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Which items of a long array stay: the first 3, the last 2, items that mention an error, and items with a number
/// field more than 2 standard deviations from that field's mean, or a text field whose value is rare (one in twenty).
fn chosen(items: &[Value]) -> std::collections::BTreeSet<usize> {
    let mut keep: std::collections::BTreeSet<usize> = (0..3.min(items.len())).collect();
    keep.extend(items.len().saturating_sub(2)..items.len());
    for (i, item) in items.iter().enumerate() {
        let s = item.to_string();
        if is_error_line(&s) {
            keep.insert(i);
        }
    }
    // Field statistics, over objects only.
    let mut fields: std::collections::BTreeMap<&str, Vec<(usize, &Value)>> = Default::default();
    for (i, item) in items.iter().enumerate() {
        if let Value::Object(m) = item {
            for (k, x) in m {
                fields.entry(k.as_str()).or_default().push((i, x));
            }
        }
    }
    for vals in fields.values() {
        let nums: Vec<(usize, f64)> = vals
            .iter()
            .filter_map(|(i, x)| x.as_f64().map(|n| (*i, n)))
            .collect();
        if nums.len() >= 8 {
            let mean = nums.iter().map(|(_, n)| n).sum::<f64>() / nums.len() as f64;
            let var = nums.iter().map(|(_, n)| (n - mean).powi(2)).sum::<f64>() / nums.len() as f64;
            let sd = var.sqrt();
            if sd > 0.0 {
                for (i, n) in &nums {
                    if (n - mean).abs() > 2.0 * sd {
                        keep.insert(*i);
                    }
                }
            }
        }
        let texts: Vec<(usize, &str)> = vals
            .iter()
            .filter_map(|(i, x)| x.as_str().map(|s| (*i, s)))
            .collect();
        if texts.len() >= 20 {
            let mut count: std::collections::HashMap<&str, usize> = Default::default();
            for (_, t) in &texts {
                *count.entry(t).or_default() += 1;
            }
            // A field where most values repeat (a status, a level): its rare values stand out.
            if count.len() * 4 <= texts.len() {
                for (i, t) in &texts {
                    if count[t] * 20 <= texts.len() {
                        keep.insert(*i);
                    }
                }
            }
        }
    }
    // Never more than 40 kept: the earliest of the rest win.
    while keep.len() > 40 {
        let Some(&last) = keep.iter().rev().nth(2) else {
            break;
        };
        keep.remove(&last);
    }
    keep
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// The originals of compressed output, kept in the data folder for `qanuvi_retrieve` (24 hours).
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Self {
        Store {
            dir: data_dir.join("compress-store"),
        }
    }

    pub fn put(&self, id: &str, text: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let file = self.dir.join(format!("{id}.txt"));
        if !file.exists() {
            std::fs::write(file, text)?;
        }
        Ok(())
    }

    /// The original, or with `search` only its lines that hold it (case-insensitive), each with its line number.
    pub fn get(&self, id: &str, search: Option<&str>) -> Result<String, String> {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("\"{id}\" is not a {RETRIEVE_TOOL} id."));
        }
        let text = std::fs::read_to_string(self.dir.join(format!("{id}.txt"))).map_err(|_| {
            format!("Nothing is kept under id \"{id}\" any more (originals are kept for a day).")
        })?;
        match search.map(str::trim).filter(|s| !s.is_empty()) {
            None => Ok(text),
            Some(s) => {
                let want = s.to_lowercase();
                let hits: Vec<String> = text
                    .lines()
                    .enumerate()
                    .filter(|(_, l)| l.to_lowercase().contains(&want))
                    .map(|(n, l)| format!("{}: {l}", n + 1))
                    .collect();
                Ok(if hits.is_empty() {
                    format!("No line holds \"{s}\".")
                } else {
                    hits.join("\n")
                })
            }
        }
    }

    /// Removes originals older than a day.
    pub fn tidy(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > KEEP_SECS);
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// The tool definition for `qanuvi_retrieve` (OpenAI-style function), for Qanuvi's own agent.
pub fn retrieve_tool_definition() -> Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": RETRIEVE_TOOL,
            "description": "Gets back the whole of a tool output that was shortened (the note at its end gives the id). With `search`, only the lines that hold that text, with their line numbers.",
            "parameters": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "The id from the note."},
                    "search": {"type": "string", "description": "Optional: only lines holding this text."}
                },
                "required": ["id"]
            }
        }
    })
}

#[cfg(test)]
mod tests;
