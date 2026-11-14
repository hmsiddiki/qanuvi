//! Work packets (Milestone 12), ported from `src/lib/plan/packet.ts`: what Qanuvi hands an agent with a step so it
//! does not have to search for it, and what it sends back after running the tests itself.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::LazyLock;

use super::js;

/// The files and code names a step's text mentions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mentions {
    /// File paths written in the step, for example `src/price.js`.
    pub paths: Vec<String>,
    /// Names that look like code: `backticked`, camelCase, PascalCase or snake_case words.
    pub names: Vec<String>,
}

const STOP: [&str; 13] = [
    "Model", "Validate", "Done", "This", "Then", "When", "With", "From", "Into", "JSON", "HTML",
    "TODO", "README",
];

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

fn looks_like_code(w: &str) -> bool {
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| re(r"[a-z][A-Z]"));
    static PASCAL: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Z][a-z]+[A-Z]"));
    CAMEL.is_match(w) || w.contains('_') || PASCAL.is_match(w)
}

/// The files and code names a step's text mentions, in the order they appear, without repeats.
pub fn mentions(text: &str) -> Mentions {
    // The TypeScript ends a path with a lookahead; here the end is matched too, and the next search starts where the
    // path ends, so the character after one path can still start the next (as with the lookahead).
    static PATH: LazyLock<Regex> = LazyLock::new(|| {
        let s = js::S;
        re(&format!(
            r#"(?:^|{s}|[`'"(])((?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+\.(?:php|phtml|js|mjs|cjs|jsx|ts|tsx|svelte|vue|rs|py|dart|cs|go|java|kt|json|ya?ml|toml|css|scss|html|md|sql))(?:$|{s}|[`'"),.:;])"#
        ))
    });
    // A run of 2 to 80 characters (the TypeScript counts UTF-16 units: a character outside the BMP counts twice there).
    static TICK: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`\n]{2,80})`"));
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| re(r"(?-u:\b)([A-Za-z_][A-Za-z0-9_]{3,})(?-u:\b)"));
    static CALL: LazyLock<Regex> =
        LazyLock::new(|| re(&format!(r"([A-Za-z_$][A-Za-z0-9_$]*){}*\(", js::S)));
    static NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^[A-Za-z_$][A-Za-z0-9_$]*$"));

    let mut paths: Vec<String> = vec![];
    let mut names: Vec<String> = vec![];
    let mut add_name = |n: &str| {
        let n = n.strip_suffix("()").unwrap_or(n);
        let n = n
            .strip_prefix(|c| c == '$' || c == '@' || c == '#')
            .unwrap_or(n);
        if !NAME.is_match(n) || n.len() < 3 || STOP.contains(&n) {
            return;
        }
        if !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    };
    let mut at = 0;
    while let Some(c) = PATH.captures_at(text, at) {
        let m = c.get(1).unwrap();
        let p = m.as_str();
        let p = p.strip_prefix("./").unwrap_or(p);
        if !paths.iter().any(|x| x == p) {
            paths.push(p.to_string());
        }
        at = m.end();
    }
    for c in TICK.captures_iter(text) {
        let inner = js::trim(&c[1]);
        if paths.iter().any(|p| inner.ends_with(p.as_str())) {
            continue;
        }
        // A call or a dotted name: take the last name before the brackets.
        match CALL.captures(inner) {
            Some(call) => add_name(&call[1]),
            None => add_name(inner.rsplit(['.', ':']).next().unwrap_or(inner)),
        }
    }
    for c in WORD.captures_iter(text) {
        if looks_like_code(&c[1]) {
            add_name(&c[1]);
        }
    }
    Mentions { paths, names }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Excerpt {
    pub path: String,
    /// First line of the excerpt (1-based).
    pub start: i64,
    pub text: String,
    /// Why it is here, for example "definition of total".
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lines {
    pub start: i64,
    pub text: String,
}

/// Lines of a file around a definition: a few before, the definition, and the body up to `after` lines.
pub fn excerpt_around(file_text: &str, line: i64, before: i64, after: i64) -> Lines {
    let (lines, _) = super::tasklist::split_lines(file_text);
    let start = (line - before).max(1);
    let end = (lines.len() as i64).min(line + after);
    let from = (start - 1).max(0) as usize;
    let to = end.max(0) as usize;
    let text = if from < to {
        lines[from..to].join("\n")
    } else {
        String::new()
    };
    Lines { start, text }
}

/// The code part of a packet, cut to `max_chars` (whole excerpts only, the first ones kept).
pub fn context_block(excerpts: &[Excerpt], max_chars: usize) -> String {
    let mut parts: Vec<String> = vec![];
    let mut used = 0;
    for e in excerpts {
        let numbered = e
            .text
            .split('\n')
            .enumerate()
            .map(|(i, l)| format!("{:>4}  {l}", e.start + i as i64))
            .collect::<Vec<_>>()
            .join("\n");
        let part = format!("{} ({}):\n{numbered}", e.path, e.reason);
        let len = js::len(&part);
        if used + len > max_chars && !parts.is_empty() {
            break;
        }
        parts.push(if len > max_chars {
            js::cut(&part, max_chars).to_string()
        } else {
            part
        });
        used += len;
    }
    parts.join("\n\n")
}

/// The useful part of a failed test run: lines that report failures with a little context, then the last lines.
/// Lines that never help are dropped first: passing tests, zero counters, and stack frames inside the test runner or
/// the language.
pub fn failure_summary(output: &str, max_chars: usize) -> String {
    static ANSI: LazyLock<Regex> = LazyLock::new(|| re(r"\x1b\[[0-9;]*m"));
    static NOISE: LazyLock<Regex> = LazyLock::new(|| {
        let (s, dot) = (js::S, js::DOT);
        re(&format!(
            r"^{s}*(?:✔|✓|√|ok [0-9]|PASS(?-u:\b))|^{s}*ℹ (?:cancelled|skipped|todo|duration_ms) |^{s}*at {dot}*(?:node:internal|node:async_hooks|internal/|node_modules[\\/](?:jest|vitest|mocha))"
        ))
    });
    static FAIL: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"(?i-u:\b(?:fail(?:ed|ure|ing)?|error|errors|assert\w*|expected|exception|panicked|not ok|traceback)\b)|✗|✖|×|(?i-u:FAIL|ERR!)",
        )
    });
    let clean = ANSI.replace_all(output, "");
    let (all, _) = super::tasklist::split_lines(&clean);
    let lines: Vec<String> = all.into_iter().filter(|l| !NOISE.is_match(l)).collect();
    let n = lines.len();
    let mut keep = BTreeSet::new();
    for (i, l) in lines.iter().enumerate() {
        if FAIL.is_match(l) {
            for j in i.saturating_sub(1)..=(i + 3).min(n.saturating_sub(1)) {
                keep.insert(j);
            }
        }
    }
    for j in n.saturating_sub(12)..n {
        keep.insert(j);
    }
    let mut out: Vec<&str> = vec![];
    let mut last: i64 = -2;
    for i in keep {
        if js::trim(&lines[i]).is_empty() {
            continue;
        }
        if i as i64 > last + 1 && !out.is_empty() {
            out.push("…");
        }
        out.push(&lines[i]);
        last = i as i64;
    }
    let text = out.join("\n");
    let len = js::len(&text);
    if len > max_chars {
        format!("…\n{}", js::from_unit(&text, len - max_chars))
    } else {
        text
    }
}

/// The step request with the packet added: the code it needs, and who runs the tests.
pub fn packet_prompt(base: &str, context: &str, test_command: &str) -> String {
    let mut parts = vec![base.to_string()];
    if !context.is_empty() {
        parts.push(String::new());
        parts.push("Where the code this step mentions is, from Qanuvi's index (go straight there instead of searching):".into());
        parts.push(context.to_string());
    }
    if !test_command.is_empty() {
        parts.push(String::new());
        parts.push(format!("Qanuvi runs `{test_command}` itself when you finish and sends you only the failures. Do not run the whole test suite yourself; you may run a single test while working."));
    }
    parts.join("\n")
}

/// The follow-up sent in the same session when the tests failed.
pub fn fix_prompt(test_command: &str, summary: &str, round: u32, max_rounds: u32) -> String {
    [
        format!("`{test_command}` failed (fix round {round} of {max_rounds}). The failures:"),
        "```".into(),
        summary.to_string(),
        "```".into(),
        "Fix the cause, then mark the step done again only when it is complete.".into(),
        r#"If a failure was not caused by this step, fix it only when the fix is small and clearly right; otherwise mark the step "- [!]" and say why."#.into(),
    ]
    .join("\n")
}

/// A definition from the code index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: i64,
    pub container: Option<String>,
}

/// A definition in a file's outline (its path is the outline's).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineItem {
    pub name: String,
    pub kind: String,
    pub line: i64,
    pub container: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    pub path: String,
    pub items: Vec<OutlineItem>,
}

/// Where the code a step mentions is: one line per definition and a short outline per mentioned file. Measured on
/// 2026-11-10: pasting the code itself did not pay off with Claude Code, whose Edit tool needs the file read anyway;
/// the locations save searching for it at a few hundred tokens.
pub fn location_block(
    defs: &[Location],
    outlines: &[Outline],
    new_files: &[String],
    max_chars: usize,
) -> String {
    let mut lines: Vec<String> = vec![];
    for d in defs {
        let inside = d
            .container
            .as_deref()
            .filter(|c| !c.is_empty())
            .map(|c| format!(" in {c}"))
            .unwrap_or_default();
        lines.push(format!(
            "{}: {}{inside} at {}:{}",
            d.name, d.kind, d.path, d.line
        ));
    }
    for o in outlines {
        let items = o
            .items
            .iter()
            .map(|i| format!("{} ({}, line {})", i.name, i.kind, i.line))
            .collect::<Vec<_>>()
            .join(", ");
        let items = if items.is_empty() {
            "no definitions".to_string()
        } else {
            items
        };
        lines.push(format!("{}: {items}", o.path));
    }
    for f in new_files {
        lines.push(format!("{f}: does not exist yet"));
    }
    let mut text = String::new();
    let mut len = 0;
    for l in lines {
        let n = js::len(&l);
        if len + n + 1 > max_chars {
            break;
        }
        if !text.is_empty() {
            text.push('\n');
            len += 1;
        }
        text.push_str(&l);
        len += n;
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_and_code_names_in_a_step() {
        let m = mentions(
            "**3. Add a discount** to `src/price.js`: make `applyDiscount(price, percent)` use roundMoney and keep MAX_PERCENT. Validate: tests in test/price.test.js pass.",
        );
        assert_eq!(m.paths, ["src/price.js", "test/price.test.js"]);
        assert_eq!(m.names, ["applyDiscount", "roundMoney", "MAX_PERCENT"]);
    }

    #[test]
    fn ignores_plain_words_and_repeats() {
        let m = mentions(
            "Sort the list. Model: claude-sonnet-5-5. Validate: the items are sorted. `Done`",
        );
        assert!(m.names.is_empty());
        assert_eq!(
            mentions("`CartService::addMany` and CartService again").names,
            ["addMany", "CartService"]
        );
        // Paths next to each other: the space after one starts the next.
        assert_eq!(mentions("a.js b.ts").paths, ["a.js", "b.ts"]);
    }

    #[test]
    fn takes_a_few_lines_before_and_the_body_after() {
        let text = (1..=50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            excerpt_around(&text, 10, 2, 3),
            Lines {
                start: 8,
                text: "line 8\nline 9\nline 10\nline 11\nline 12\nline 13".into()
            }
        );
        assert_eq!(excerpt_around(&text, 1, 2, 1).start, 1);
        assert!(excerpt_around(&text, 50, 2, 30).text.ends_with("line 50"));
    }

    #[test]
    fn numbers_the_lines_and_stops_at_the_size_limit() {
        let block = context_block(
            &[
                Excerpt {
                    path: "src/a.js".into(),
                    start: 4,
                    text: "x\ny".into(),
                    reason: "definition of total".into(),
                },
                Excerpt {
                    path: "src/b.js".into(),
                    start: 1,
                    text: "z".repeat(500),
                    reason: "mentioned".into(),
                },
            ],
            120,
        );
        assert!(block.contains("src/a.js (definition of total):\n   4  x\n   5  y"));
        assert!(!block.contains("src/b.js"));
    }

    #[test]
    fn keeps_failure_lines_and_the_end() {
        let mut out = vec!["\x1b[32m✔ adds\x1b[0m".to_string()];
        out.extend((0..30).map(|i| format!("ok {i}")));
        out.extend([
            "✖ applies a discount".to_string(),
            "  AssertionError: expected 90 to equal 81".into(),
            "    at test/price.test.js:12:5".into(),
        ]);
        out.extend((0..30).map(|i| format!("ok more {i}")));
        out.extend(["# tests 3".to_string(), "# fail 1".into()]);
        let s = failure_summary(&out.join("\n"), 3000);
        assert!(s.contains("✖ applies a discount"));
        assert!(s.contains("AssertionError: expected 90 to equal 81"));
        assert!(s.contains("# fail 1"));
        assert!(!s.contains("ok 5"));
        assert!(!s.contains("\x1b["));
        assert!(js::len(&failure_summary(&"x".repeat(5000), 100)) <= 102);
    }

    #[test]
    fn drops_passing_lines_zero_counters_and_runner_frames() {
        let out = [
            "✔ rounds the total (0.1ms)",
            "✖ percent has a space before the sign (1.5ms)",
            "  AssertionError: Expected values to be strictly equal:",
            "  '12.5%' !== '12.5 %'",
            "      at TestContext.<anonymous> (file:///x/test/format.test.js:6:10)",
            "      at Test.runInAsyncScope (node:async_hooks:227:14)",
            "      at Test.run (node:internal/test_runner/test:1402:25)",
            "✔ formatPrice rounds (0.1ms)",
            "ℹ pass 12",
            "ℹ fail 1",
            "ℹ cancelled 0",
            "ℹ skipped 0",
        ]
        .join("\n");
        let s = failure_summary(&out, 3000);
        assert!(s.contains("✖ percent has a space before the sign"));
        assert!(s.contains("'12.5%' !== '12.5 %'"));
        assert!(s.contains("format.test.js:6:10"));
        assert!(s.contains("ℹ fail 1"));
        assert!(!s.contains('✔'));
        assert!(!s.contains("node:internal"));
        assert!(!s.contains("cancelled"));
    }

    #[test]
    fn adds_the_code_and_the_test_rule_only_when_there_is_something_to_add() {
        assert_eq!(packet_prompt("Do it.", "", ""), "Do it.");
        let p = packet_prompt("Do it.", "src/a.js (x):\n   1  y", "npm test");
        assert!(p.contains("Where the code this step mentions is"));
        assert!(p.contains("Qanuvi runs `npm test` itself"));
        assert!(fix_prompt("npm test", "boom", 1, 2).contains("fix round 1 of 2"));
    }

    #[test]
    fn says_where_things_are_in_one_line_each() {
        let text = location_block(
            &[Location {
                name: "applyDiscount".into(),
                kind: "function".into(),
                path: "src/price.js".into(),
                line: 6,
                container: None,
            }],
            &[Outline {
                path: "src/price.js".into(),
                items: vec![OutlineItem {
                    name: "MAX_PERCENT".into(),
                    kind: "constant".into(),
                    line: 3,
                    container: None,
                }],
            }],
            &["src/cart.js".into()],
            2500,
        );
        assert_eq!(
            text,
            "applyDiscount: function at src/price.js:6\nsrc/price.js: MAX_PERCENT (constant, line 3)\nsrc/cart.js: does not exist yet"
        );
        assert_eq!(
            location_block(&[], &[], &["a.js".into(), "b.js".into()], 30),
            "a.js: does not exist yet"
        );
    }
}
