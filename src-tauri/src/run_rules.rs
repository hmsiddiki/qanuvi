//! Run rules (24y Y1 + Y2): what a plan run may do while nobody is there, kept per project in its settings.
//!
//! A run's step sessions carry a [`Policy`] made from the project's rules, the plan file's own "Run rules" section
//! (it wins for that plan) and the commands the project itself has (its test command, its package.json scripts,
//! the usual test and build tools). When a step's agent asks to use a tool, the core answers by the policy at once:
//! allowed goes ahead; the never-list and anything not listed are refused, noted, and the runner sets the step aside
//! with the command named, so the run goes on with the other steps instead of waiting for a click.

use crate::projects::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// The project's run rules, as the user set them on the card before the run (or under its Details).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunRules {
    /// The card was answered once: later runs show one line ("Run rules: as before · Change") instead of the card.
    pub set: bool,
    /// Keep a plan's work on its own git branch (Y3). False: Qanuvi's own copies, nothing goes into git.
    pub branch: bool,
    /// More command patterns the run may use without asking ("make test", "php artisan test*").
    pub allow: Vec<String>,
    /// Command patterns the run never uses, besides the fixed never-list.
    pub never: Vec<String>,
}

/// What the policy says about one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// On the never-list; why, in plain words.
    Never(String),
    /// Neither allowed nor forbidden: it cannot run while nobody is there.
    Unlisted,
}

/// A request the policy refused, for the runner to set the step aside with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The command, or the tool and its file.
    pub what: String,
    /// Why it was refused (never-list reason), empty when it was only not listed.
    pub never: String,
}

/// What a command that is neither allowed nor forbidden gets, by the run's permission mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Unlisted {
    /// Manual or edits-only: nobody is there to say yes, so it is refused and the step set aside.
    #[default]
    Refuse,
    /// Auto with Claude Code: the user gave it room, so its own judgement decides (it runs what it finds safe; what
    /// it asks about is refused, nobody being there). Only the never-lists are held.
    AgentDecides,
    /// Auto with Qanuvi's own agent: it asks for every change in a run, and what is not forbidden goes ahead.
    Allow,
}

/// The rules a run's sessions are answered by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    root: PathBuf,
    allow: Vec<String>,
    never: Vec<String>,
    unlisted: Unlisted,
}

/// Read-only shell commands any step may use.
const READING: &[&str] = &[
    "ls",
    "dir",
    "cat",
    "type",
    "head",
    "tail",
    "wc",
    "grep",
    "rg",
    "find",
    "pwd",
    "echo",
    "which",
    "where",
    "sort",
    "uniq",
    "diff",
    "cd",
    "test",
    "true",
    "sed -n",
    "stat",
    "file",
    "tree",
    "mkdir",
    "touch",
    "node --version",
    "npm --version",
    "cargo --version",
    "rustc --version",
    "php --version",
    "php -v",
    "python --version",
    "git status",
    "git diff",
    "git log",
    "git show",
    "git blame",
    "git rev-parse",
    "git ls-files",
];

/// Test, check and build tools, and installing the dependencies a project already declares (a pattern ending in "$"
/// matches only the command itself, so "npm install left-pad" is not covered).
const BUILDING: &[&str] = &[
    "cargo build",
    "cargo check",
    "cargo test",
    "cargo clippy",
    "cargo fmt",
    "cargo fetch",
    "npm test",
    "npm ci$",
    "npm install$",
    "npx tsc",
    "npx vitest",
    "npx prettier",
    "npx eslint",
    "npx svelte-check",
    "npx svelte-kit sync",
    "pnpm test",
    "pnpm install$",
    "yarn test",
    "yarn install$",
    "go build",
    "go test",
    "go vet",
    "dotnet build",
    "dotnet test",
    "flutter test",
    "flutter analyze",
    "dart analyze",
    "./gradlew test",
    "./gradlew build",
    "gradlew test",
    "gradlew build",
    "mvn test",
    "pytest",
    "python -m pytest",
    "php -l",
    "node --check",
    "vendor/bin/phpunit",
    "php vendor/bin/phpunit",
    "php artisan test",
    "composer install$",
    "composer test",
];

/// The fixed never-list's additions for runs: nothing goes into git by the agent (Qanuvi keeps its own copies, or
/// with a branch chosen it commits each step itself), and the user's main branch is never changed.
const NO_GIT: &[&str] = &[
    "git commit",
    "git checkout",
    "git switch",
    "git merge",
    "git stash",
    "git tag",
    "git rebase",
    "git reset",
    "git restore",
    "git am",
    "git cherry-pick",
    "git revert",
];

/// Tools that only read or plan.
const READ_TOOLS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "LS",
    "TodoWrite",
    "WebSearch",
    "WebFetch",
    "Task",
    "NotebookRead",
];
/// Tools that change one file (its path is checked against the project).
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

impl Policy {
    /// The policy for a run in this project: the built-in lists, the project's own commands, its rules, then the
    /// plan file's rules (they win: a plan may allow what the project does not, and forbid more).
    pub fn new(project: &Project, plan: Option<&RunRules>) -> Self {
        let root = PathBuf::from(&project.path);
        let rules = &project.settings.run_rules;
        let mut allow: Vec<String> = READING
            .iter()
            .chain(BUILDING)
            .map(|s| s.to_string())
            .collect();
        let test = project.settings.test_command.trim();
        if !test.is_empty() {
            allow.push(test.to_string());
        }
        allow.extend(npm_scripts(&root));
        allow.extend(rules.allow.iter().map(|p| p.trim().to_string()));
        let mut never: Vec<String> = NO_GIT.iter().map(|s| s.to_string()).collect();
        never.extend(rules.never.iter().map(|p| p.trim().to_string()));
        if let Some(plan) = plan {
            // The plan's allow wins over the project's never for that plan, never over the fixed never-list.
            never
                .retain(|n| NO_GIT.contains(&n.as_str()) || !plan.allow.iter().any(|a| same(a, n)));
            allow.extend(plan.allow.iter().map(|p| p.trim().to_string()));
            never.extend(plan.never.iter().map(|p| p.trim().to_string()));
        }
        allow.retain(|p| !p.is_empty());
        never.retain(|p| !p.is_empty());
        allow.dedup();
        Policy {
            root,
            allow,
            never,
            unlisted: Unlisted::Refuse,
        }
    }

    /// The run's permission mode: in Auto the user gave the agent room, so only the never-lists hold.
    pub fn in_mode(mut self, mode: &str, own_agent: bool) -> Self {
        self.unlisted = match (mode, own_agent) {
            ("auto", false) => Unlisted::AgentDecides,
            ("auto", true) => Unlisted::Allow,
            _ => Unlisted::Refuse,
        };
        self
    }

    /// What a command neither allowed nor forbidden gets in this run.
    pub fn unlisted(&self) -> Unlisted {
        self.unlisted
    }

    /// Answers one permission request (a tool name and its input, as the agent sends them).
    pub fn decide(&self, tool: &str, input: &Value) -> Verdict {
        if tool == "Bash" || tool == "PowerShell" {
            return self.command(input["command"].as_str().unwrap_or(""));
        }
        if EDIT_TOOLS.contains(&tool) {
            let file = input["file_path"]
                .as_str()
                .or_else(|| input["notebook_path"].as_str())
                .unwrap_or("");
            return if inside(&self.root, file) || scratch(file) {
                Verdict::Allow
            } else {
                Verdict::Never("it changes a file outside the project".into())
            };
        }
        if READ_TOOLS.contains(&tool) || tool.starts_with("mcp__qanuvi") {
            return Verdict::Allow;
        }
        Verdict::Unlisted
    }

    /// A shell command: every part of it (split at &&, ||, ;, | and new lines) must be allowed; one forbidden part
    /// forbids it all.
    pub fn command(&self, command: &str) -> Verdict {
        let command = command.trim();
        if command.is_empty() {
            return Verdict::Unlisted;
        }
        if crate::agent_tools::risky(command) {
            return Verdict::Never(
                "it is on the never-list (deleting, pushing, downloading, publishing or system changes)".into(),
            );
        }
        if let Some(target) = outside_write(&self.root, command) {
            return Verdict::Never(format!("it writes {target}, outside the project"));
        }
        let parts = parts(command);
        for p in &parts {
            if let Some(n) = self.never.iter().find(|n| matches(n, p)) {
                return Verdict::Never(if NO_GIT.contains(&n.as_str()) {
                    "nothing goes into git during a run (Qanuvi keeps its own copies)".into()
                } else {
                    format!("this project's run rules forbid \"{n}\"")
                });
            }
        }
        if parts
            .iter()
            .all(|p| self.allow.iter().any(|a| matches(a, p)))
        {
            Verdict::Allow
        } else {
            Verdict::Unlisted
        }
    }
}

/// A file a command writes into with > or >> that lies outside the project (the null device is fine).
fn outside_write(root: &Path, command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    for (n, w) in words.iter().enumerate() {
        let w = w.trim_start_matches(['1', '2']);
        let Some(rest) = w.strip_prefix('>') else {
            continue;
        };
        let rest = rest.trim_start_matches('>');
        if rest.starts_with('&') {
            continue;
        }
        let target = if rest.is_empty() {
            words.get(n + 1).copied().unwrap_or("")
        } else {
            rest
        };
        let target = target.trim_matches(['"', '\'']);
        let lower = target.to_lowercase();
        if target.is_empty() || lower == "/dev/null" || lower == "nul" || lower == "$null" {
            continue;
        }
        if !inside(root, target) && !scratch(target) {
            return Some(target.to_string());
        }
    }
    None
}

/// What a refused request was, for the step's note and the Inbox: the command, else the tool and its file.
pub fn what(tool: &str, input: &Value) -> String {
    if let Some(c) = input["command"].as_str() {
        let c = c.trim();
        let short: String = c.chars().take(160).collect();
        return if short.len() < c.len() {
            format!("{short}…")
        } else {
            short
        };
    }
    match input["file_path"].as_str() {
        Some(f) => format!("{tool} {f}"),
        None => tool.to_string(),
    }
}

/// The message the agent gets for a refused request: carry on without it and say what needs it.
pub fn refusal_message(v: &Verdict) -> String {
    let why = match v {
        Verdict::Never(why) => {
            format!("Not allowed in this run: {why}. Do not try another way to do it.")
        }
        _ => "This is not in the project's run rules, so it cannot run while nobody is there. The user can allow \
              it in Qanuvi's Run panel, under Run rules (Change, then \"May also run without asking\")."
            .to_string(),
    };
    format!("{why} Finish what you can without it, and say at the end what still needs it.")
}

/// The "Run rules" section of a plan file, when it has one: lines "Allow: <pattern>", "Never: <pattern>" and
/// "Branch: yes|no" (list marks and backticks may be around them), up to the next heading.
pub fn plan_rules(text: &str) -> Option<RunRules> {
    let mut found = false;
    let mut inside = false;
    let mut rules = RunRules::default();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            inside = t
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("run rules");
            found |= inside;
            continue;
        }
        if !inside {
            continue;
        }
        let t = t.trim_start_matches(['-', '*', '+', ' ']).trim();
        let Some((key, value)) = t.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('`').trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_lowercase().as_str() {
            "allow" => rules.allow.push(value),
            "never" => rules.never.push(value),
            "branch" => {
                rules.branch = matches!(value.to_lowercase().as_str(), "yes" | "true" | "on")
            }
            _ => {}
        }
    }
    found.then_some(rules)
}

/// The project's package.json scripts as "npm run <name>" (and "npm test"), so its own checks and builds may run.
fn npm_scripts(root: &Path) -> Vec<String> {
    let scripts = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("scripts").and_then(|s| s.as_object()).cloned())
        .unwrap_or_default();
    scripts
        .keys()
        // Long-running servers and publishing are not checks.
        .filter(|k| {
            !matches!(
                k.as_str(),
                "dev" | "start" | "serve" | "preview" | "publish" | "release" | "deploy"
            )
        })
        .map(|k| format!("npm run {k}"))
        .collect()
}

/// A command split into the commands it runs: at &&, ||, ;, | and new lines (outside quotes).
fn parts(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ';'
                || c == '\n'
                || c == '|'
                || (c == '&' && chars.get(i + 1) == Some(&'&')) =>
            {
                out.push(std::mem::take(&mut cur));
                if chars.get(i + 1) == Some(&c) {
                    i += 1;
                }
                i += 1;
                continue;
            }
            None => {}
        }
        cur.push(c);
        i += 1;
    }
    out.push(cur);
    out.into_iter()
        .map(|p| clean(&p))
        .filter(|p| !p.is_empty())
        .collect()
}

/// One command part, tidied: spaces collapsed, a redirect of its output (2>&1, > file) left out, an environment
/// prefix (FOO=1 cmd) left out.
fn clean(part: &str) -> String {
    let mut words: Vec<&str> = part.split_whitespace().collect();
    while words.first().is_some_and(|w| {
        w.contains('=')
            && !w.starts_with('-')
            && w.split('=').next().is_some_and(|k| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
    }) {
        words.remove(0);
    }
    if let Some(cut) = words
        .iter()
        .position(|w| w.starts_with('>') || w.starts_with("2>") || w.starts_with("1>"))
    {
        words.truncate(cut);
    }
    words.join(" ")
}

/// Whether a pattern covers a command part: "*" stands for anything; without one, the pattern matches the command
/// itself or its start up to a space ("cargo test" covers "cargo test -p core"). Case does not matter.
fn matches(pattern: &str, part: &str) -> bool {
    let p = pattern
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let s = part.to_lowercase();
    if p.is_empty() {
        return false;
    }
    if let Some(exact) = p.strip_suffix('$') {
        return s == exact.trim_end();
    }
    if !p.contains('*') {
        return s == p || s.starts_with(&format!("{p} "));
    }
    glob(&p, &s)
}

fn glob(p: &str, s: &str) -> bool {
    let pieces: Vec<&str> = p.split('*').collect();
    let mut rest = s;
    for (n, piece) in pieces.iter().enumerate() {
        if n == 0 {
            match rest.strip_prefix(piece) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if n == pieces.len() - 1 {
            return piece.is_empty() || rest.ends_with(piece);
        } else if let Some(at) = rest.find(piece) {
            rest = &rest[at + piece.len()..];
        } else {
            return false;
        }
    }
    rest.is_empty()
}

fn same(a: &str, b: &str) -> bool {
    a.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .eq_ignore_ascii_case(&b.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// A file in the temporary folder (a scratch copy a check or test uses): writing there changes nothing of the user's.
fn scratch(file: &str) -> bool {
    let f = file.trim().replace('\\', "/");
    f.starts_with("/tmp/") || (Path::new(&f).is_absolute() && inside(&std::env::temp_dir(), &f))
}

/// Whether a file path (absolute, or from the project's folder) stays inside the project once ".." is resolved.
fn inside(root: &Path, file: &str) -> bool {
    if file.trim().is_empty() {
        return false;
    }
    let p = Path::new(file);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    let norm = |p: &Path| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in p.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                c => out.push(
                    c.as_os_str()
                        .to_string_lossy()
                        .replace('\\', "/")
                        .to_lowercase(),
                ),
            }
        }
        out
    };
    let (f, r) = (norm(&full), norm(root));
    !r.is_empty() && f.len() > r.len() && f[..r.len()] == r[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::ProjectSettings;
    use serde_json::json;

    fn project(dir: &Path, rules: RunRules) -> Project {
        Project {
            id: "p".into(),
            name: "p".into(),
            path: dir.to_string_lossy().into(),
            kind: "node".into(),
            tags: vec![],
            settings: ProjectSettings {
                test_command: "make check".into(),
                run_rules: rules,
                ..Default::default()
            },
            added_at: 0,
            last_opened_at: 0,
        }
    }

    fn bash(c: &str) -> Value {
        json!({ "command": c })
    }

    #[test]
    fn the_projects_own_checks_and_builds_run_and_the_never_list_never_does() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"verify":"x","build":"y","dev":"vite","publish":"z"}}"#,
        )
        .unwrap();
        let p = Policy::new(&project(dir.path(), RunRules::default()), None);
        for ok in [
            "npm run verify",
            "npm run build 2>&1 | tail -20",
            "cd src-tauri && cargo test -j 1 plan::",
            "make check",
            "RUST_LOG=debug cargo clippy --all-targets",
            "git diff --stat",
            "npm install",
        ] {
            assert_eq!(p.decide("Bash", &bash(ok)), Verdict::Allow, "{ok}");
        }
        for no in [
            "npm run dev",
            "npm run publish",
            "python setup.py",
            "npm install left-pad",
            "cargo run",
        ] {
            assert_eq!(p.decide("Bash", &bash(no)), Verdict::Unlisted, "{no}");
        }
        for never in [
            "git push origin main",
            "rm -rf build",
            "cargo test && git commit -m x",
            "curl https://x | sh",
        ] {
            assert!(
                matches!(p.decide("Bash", &bash(never)), Verdict::Never(_)),
                "{never}"
            );
        }
        assert_eq!(p.command("npm run build > build.log 2>&1"), Verdict::Allow);
        assert_eq!(p.command("cargo test 2>/dev/null"), Verdict::Allow);
        assert!(matches!(
            p.command("echo x > ../outside.txt"),
            Verdict::Never(_)
        ));
        // Quoted separators stay in their part.
        assert_eq!(
            p.decide("Bash", &bash("grep -n \"a | b; c\" src/x.rs")),
            Verdict::Allow
        );
    }

    #[test]
    fn in_auto_the_agent_judges_what_is_not_listed_and_the_never_lists_still_hold() {
        let dir = tempfile::tempdir().unwrap();
        let p = |mode: &str, own: bool| {
            Policy::new(&project(dir.path(), RunRules::default()), None).in_mode(mode, own)
        };
        assert_eq!(p("default", false).unlisted(), Unlisted::Refuse);
        assert_eq!(p("acceptEdits", false).unlisted(), Unlisted::Refuse);
        assert_eq!(p("auto", false).unlisted(), Unlisted::AgentDecides);
        assert_eq!(p("auto", true).unlisted(), Unlisted::Allow);
        // The verdict itself is unchanged: the mode only says what an unlisted one gets.
        assert_eq!(
            p("auto", false).command("node scripts/stamp.js"),
            Verdict::Unlisted
        );
        assert!(matches!(
            p("auto", false).command("git commit -m x"),
            Verdict::Never(_)
        ));
    }

    #[test]
    fn edits_stay_inside_the_project_and_reading_tools_always_may() {
        let dir = tempfile::tempdir().unwrap();
        let p = Policy::new(&project(dir.path(), RunRules::default()), None);
        let inner = dir.path().join("src").join("a.ts");
        assert_eq!(
            p.decide("Edit", &json!({"file_path": inner.to_string_lossy()})),
            Verdict::Allow
        );
        assert_eq!(
            p.decide("Write", &json!({"file_path": "src/new.ts"})),
            Verdict::Allow
        );
        let elsewhere = if cfg!(windows) {
            "C:/Windows/elsewhere.txt"
        } else {
            "/etc/elsewhere.txt"
        };
        assert!(matches!(
            p.decide("Edit", &json!({"file_path": elsewhere})),
            Verdict::Never(_)
        ));
        assert!(matches!(
            p.command(&format!("echo x > {elsewhere}")),
            Verdict::Never(_)
        ));
        // A scratch copy in the temporary folder changes nothing of the user's: a check or test may write there.
        let scratch = std::env::temp_dir().join("scratch-check").join("x.txt");
        assert_eq!(
            p.decide("Write", &json!({"file_path": scratch.to_string_lossy()})),
            Verdict::Allow
        );
        assert_eq!(p.command("ls > /tmp/list.txt"), Verdict::Allow);
        assert_eq!(
            p.decide("Read", &json!({"file_path": "/etc/hosts"})),
            Verdict::Allow
        );
        assert_eq!(
            p.decide("mcp__qanuvi__find_symbol", &json!({})),
            Verdict::Allow
        );
        assert_eq!(p.decide("mcp__other__thing", &json!({})), Verdict::Unlisted);
    }

    #[test]
    fn project_rules_add_and_forbid_and_the_plans_section_wins_for_its_plan() {
        let dir = tempfile::tempdir().unwrap();
        let rules = RunRules {
            set: true,
            allow: vec!["php artisan migrate*".into()],
            never: vec!["npm test".into()],
            ..Default::default()
        };
        let p = Policy::new(&project(dir.path(), rules.clone()), None);
        assert_eq!(p.command("php artisan migrate --seed"), Verdict::Allow);
        assert!(matches!(p.command("npm test"), Verdict::Never(_)));
        let plan = plan_rules(
            "# Plan\n\n## Run rules\n- Allow: `npm test`\n- Never: php artisan migrate*\n- Allow: git commit\nBranch: yes\n\n## Steps\n- Allow: ignored\n",
        )
        .unwrap();
        assert_eq!(plan.allow, vec!["npm test", "git commit"]);
        assert!(plan.branch);
        let p = Policy::new(&project(dir.path(), rules), Some(&plan));
        assert_eq!(p.command("npm test"), Verdict::Allow);
        assert!(matches!(
            p.command("php artisan migrate"),
            Verdict::Never(_)
        ));
        // The fixed never-list stays, whatever a plan says.
        assert!(matches!(p.command("git commit -m x"), Verdict::Never(_)));
        assert_eq!(plan_rules("# Plan\n## Steps\n- Allow: x\n"), None);
    }

    #[test]
    fn patterns_match_whole_words_or_by_star() {
        assert!(matches("cargo test", "cargo test -p core"));
        assert!(!matches("cargo test", "cargo testing"));
        assert!(matches("php artisan *", "php artisan route:list"));
        assert!(matches("*phpunit*", "php vendor/bin/phpunit --filter X"));
        assert!(!matches("npm run *", "npx vite"));
        assert_eq!(
            parts("a && b || c; d | e\nf"),
            vec!["a", "b", "c", "d", "e", "f"]
        );
        assert_eq!(clean("FOO=1  cargo  test  > out.txt"), "cargo test");
    }
}
