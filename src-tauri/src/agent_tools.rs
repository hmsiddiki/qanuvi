//! The tools Qanuvi's own agent gives a model (Milestone 24): read, write and edit files, find files and text, and run
//! commands, all inside the project folder. They have the same names and inputs as the command-line agent's tools, so
//! the session view, approvals and diffs show them the same way. Answers are kept short to save tokens.

use serde_json::{Value, json};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const NAMES: [&str; 6] = ["Read", "Write", "Edit", "Bash", "Glob", "Grep"];
/// Tools that change nothing.
pub const READ_ONLY: [&str; 3] = ["Read", "Glob", "Grep"];

const READ_LINES: usize = 400;
const MAX_ANSWER: usize = 24_000;
const BASH_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_HITS: usize = 120;

/// The tool descriptions, in the OpenAI function format (Ollama takes the same).
pub fn definitions() -> Value {
    let f = |name: &str, desc: &str, props: Value, required: &[&str]| {
        json!({"type": "function", "function": {
            "name": name,
            "description": desc,
            "parameters": {"type": "object", "properties": props, "required": required}
        }})
    };
    json!([
        f(
            "Read",
            "Read a text file. Returns numbered lines (at most 400; use offset to read further).",
            json!({
                "file_path": {"type": "string", "description": "Path relative to the project folder"},
                "offset": {"type": "integer", "description": "First line to read (1-based)"},
                "limit": {"type": "integer", "description": "Number of lines"}
            }),
            &["file_path"]
        ),
        f(
            "Write",
            "Create a file or replace all of its content.",
            json!({
                "file_path": {"type": "string"},
                "content": {"type": "string"}
            }),
            &["file_path", "content"]
        ),
        f(
            "Edit",
            "Replace text in a file. old_string must appear exactly once (copy it exactly from Read, without the line numbers), unless replace_all is true.",
            json!({
                "file_path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"}
            }),
            &["file_path", "old_string", "new_string"]
        ),
        f(
            "Bash",
            "Run a shell command in the project folder (2 minute limit). Use for tests, builds and git.",
            json!({
                "command": {"type": "string"},
                "description": {"type": "string", "description": "Five words on what it does"}
            }),
            &["command"]
        ),
        f(
            "Glob",
            "Find files by name pattern, for example **/*.ts. Skips ignored files.",
            json!({
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Folder to search in"}
            }),
            &["pattern"]
        ),
        f(
            "Grep",
            "Search file contents with a regular expression. Returns path:line: text.",
            json!({
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string", "description": "Only files matching this, for example *.rs"},
                "-i": {"type": "boolean", "description": "Ignore case"}
            }),
            &["pattern"]
        ),
    ])
}

/// What a tool answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub text: String,
    pub is_error: bool,
}

fn ok(text: impl Into<String>) -> Outcome {
    Outcome {
        text: text.into(),
        is_error: false,
    }
}

fn err(text: impl Into<String>) -> Outcome {
    Outcome {
        text: text.into(),
        is_error: true,
    }
}

fn s<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or("")
}

/// A path inside the project: relative ones are taken from the project folder; anything that leads outside it (with
/// `..`, an absolute path elsewhere, or a link) is refused.
pub fn inside(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("A file path is needed.".into());
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder cannot be read: {e}"))?;
    let given = Path::new(path);
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    // Lexical clean-up first (the file may not exist yet).
    let mut clean = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if !clean.pop() {
                    return Err(format!("{path} is outside the project."));
                }
            }
            Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    // Then the real location of the nearest existing part (links followed).
    let mut probe = clean.clone();
    let mut tail = Vec::new();
    while !probe.exists() {
        match probe.file_name() {
            Some(n) => tail.push(n.to_os_string()),
            None => break,
        }
        if !probe.pop() {
            break;
        }
    }
    let mut real = probe.canonicalize().unwrap_or(probe);
    for n in tail.into_iter().rev() {
        real.push(n);
    }
    if !real.starts_with(&root) {
        return Err(format!("{path} is outside the project."));
    }
    Ok(real)
}

fn rel(root: &Path, p: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    p.strip_prefix(&root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn clip(text: String) -> String {
    if text.len() <= MAX_ANSWER {
        return text;
    }
    let mut cut = MAX_ANSWER;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (cut; {} more bytes)", &text[..cut], text.len() - cut)
}

/// Runs a tool. `root` is the project folder.
pub fn run(root: &Path, name: &str, input: &Value) -> Outcome {
    let result = match name {
        "Read" => read(root, input),
        "Write" => write(root, input),
        "Edit" => edit(root, input, false),
        "Bash" => bash(root, s(input, "command")),
        "Glob" => glob(root, input),
        "Grep" => grep(root, input),
        _ => Err(format!(
            "There is no tool named {name}. The tools are: {}.",
            NAMES.join(", ")
        )),
    };
    match result {
        Ok(o) => Outcome {
            text: clip(o.text),
            ..o
        },
        Err(e) => err(e),
    }
}

fn read(root: &Path, input: &Value) -> Result<Outcome, String> {
    let p = inside(root, s(input, "file_path"))?;
    if p.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&p)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() {
                    format!("{n}/")
                } else {
                    n
                }
            })
            .collect();
        names.sort();
        return Ok(ok(format!(
            "{} is a folder. It contains:\n{}",
            rel(root, &p),
            names.join("\n")
        )));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", rel(root, &p)))?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Err(format!("{} is not a text file.", rel(root, &p)));
    }
    let text = String::from_utf8_lossy(&bytes);
    let offset = input["offset"].as_u64().unwrap_or(1).max(1) as usize;
    let limit = input["limit"]
        .as_u64()
        .map(|l| l as usize)
        .unwrap_or(READ_LINES)
        .clamp(1, 2000);
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Ok(ok(format!("{} is empty.", rel(root, &p))));
    }
    let end = (offset - 1 + limit).min(lines.len());
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate().take(end).skip(offset - 1) {
        out.push_str(&format!("{:>5}\t{line}\n", i + 1));
    }
    if end < lines.len() {
        out.push_str(&format!(
            "… {} more lines (read on with offset {}).",
            lines.len() - end,
            end + 1
        ));
    }
    Ok(ok(out))
}

fn write(root: &Path, input: &Value) -> Result<Outcome, String> {
    let p = inside(root, s(input, "file_path"))?;
    let content = input["content"]
        .as_str()
        .ok_or("Write needs the file's content.")?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let existed = p.exists();
    std::fs::write(&p, content).map_err(|e| format!("{}: {e}", rel(root, &p)))?;
    Ok(ok(format!(
        "{} {} ({} lines).",
        if existed { "Replaced" } else { "Created" },
        rel(root, &p),
        content.lines().count()
    )))
}

fn edit(root: &Path, input: &Value, dry: bool) -> Result<Outcome, String> {
    let p = inside(root, s(input, "file_path"))?;
    let old = input["old_string"]
        .as_str()
        .ok_or("Edit needs old_string.")?;
    let new = input["new_string"]
        .as_str()
        .ok_or("Edit needs new_string.")?;
    if old.is_empty() {
        return Err("old_string is empty; use Write to create a file.".into());
    }
    if old == new {
        return Err("old_string and new_string are the same.".into());
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", rel(root, &p)))?;
    // Files with Windows line ends: models usually send plain \n.
    let crlf = text.contains("\r\n") && !old.contains("\r\n");
    let (old, new) = if crlf {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };
    let count = text.matches(&old).count();
    let all = input["replace_all"].as_bool().unwrap_or(false);
    if count == 0 {
        return Err(format!(
            "old_string was not found in {}. Read the file again and copy the text exactly (without line numbers).",
            rel(root, &p)
        ));
    }
    if count > 1 && !all {
        return Err(format!(
            "old_string appears {count} times in {}; add more surrounding lines so it is unique, or set replace_all.",
            rel(root, &p)
        ));
    }
    let updated = if all {
        text.replace(&old, &new)
    } else {
        text.replacen(&old, &new, 1)
    };
    if dry {
        return Ok(ok(""));
    }
    std::fs::write(&p, updated).map_err(|e| e.to_string())?;
    Ok(ok(format!(
        "Edited {} ({} place{}).",
        rel(root, &p),
        if all { count } else { 1 },
        if all && count > 1 { "s" } else { "" }
    )))
}

/// The shell commands run in: bash where there is one (Git's bash on Windows), otherwise PowerShell on Windows.
pub fn shell() -> (PathBuf, Vec<&'static str>, &'static str) {
    if cfg!(windows) {
        // Git for Windows: <Git>\cmd\git.exe next to <Git>\bin\bash.exe. (Not System32's bash, which is WSL.)
        if let Some(git) = crate::terminal::find_on_path("git")
            && let Some(base) = git.parent().and_then(Path::parent)
        {
            let bash = base.join("bin").join("bash.exe");
            if bash.is_file() {
                return (bash, vec!["-c"], "bash (Git for Windows)");
            }
        }
        (
            PathBuf::from("powershell.exe"),
            vec!["-NoProfile", "-NonInteractive", "-Command"],
            "PowerShell",
        )
    } else {
        (PathBuf::from("/bin/sh"), vec!["-c"], "sh")
    }
}

fn bash(root: &Path, command: &str) -> Result<Outcome, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("Bash needs a command.".into());
    }
    let (program, args, _) = shell();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("The command could not start: {e}"))?;
    // Ends the command and anything it started, on a timeout or when this agent is stopped.
    let job = crate::browser::Job::for_child(&child);
    let mut out = child.stdout.take().ok_or("no output")?;
    let mut errs = child.stderr.take().ok_or("no output")?;
    let t1 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let t2 = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = errs.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let status = loop {
        if let Ok(Some(st)) = child.try_wait() {
            break Some(st);
        }
        if start.elapsed() > BASH_TIMEOUT {
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-9", &format!("-{}", child.id())])
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    drop(job);
    let stdout = String::from_utf8_lossy(&t1.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&t2.join().unwrap_or_default()).to_string();
    let mut text = String::new();
    // The end of long output matters most (test summaries, the last error).
    let tail = |t: &str, max: usize| {
        if t.len() <= max {
            t.to_string()
        } else {
            let mut cut = t.len() - max;
            while !t.is_char_boundary(cut) {
                cut += 1;
            }
            format!("… ({} bytes cut)\n{}", cut, &t[cut..])
        }
    };
    if !stdout.trim().is_empty() {
        text.push_str(&tail(stdout.trim_end(), 12_000));
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&tail(stderr.trim_end(), 6_000));
    }
    match status {
        None => Ok(err(format!(
            "{text}\nStopped after {} s.",
            BASH_TIMEOUT.as_secs()
        ))),
        Some(st) if st.success() => Ok(ok(if text.is_empty() {
            "(no output)".to_string()
        } else {
            text
        })),
        Some(st) => Ok(err(format!(
            "{text}\nExit code {}.",
            st.code().map(|c| c.to_string()).unwrap_or("?".into())
        ))),
    }
}

fn walker(dir: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(dir)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
}

fn glob(root: &Path, input: &Value) -> Result<Outcome, String> {
    let pattern = s(input, "pattern").trim();
    if pattern.is_empty() {
        return Err("Glob needs a pattern.".into());
    }
    let dir = match s(input, "path") {
        "" => root.canonicalize().map_err(|e| e.to_string())?,
        p => inside(root, p)?,
    };
    let matcher = ignore::overrides::OverrideBuilder::new(&dir)
        .add(pattern)
        .and_then(|b| b.build())
        .map_err(|e| format!("Bad pattern {pattern}: {e}"))?;
    let plain = !pattern.contains('/');
    let mut hits = Vec::new();
    for entry in walker(&dir).flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let p = entry.path();
        let r = p.strip_prefix(&dir).unwrap_or(p);
        // A pattern without a folder ("*.rs") matches the file name anywhere, as people expect.
        let m = matcher.matched(r, false).is_whitelist()
            || (plain
                && r.file_name()
                    .is_some_and(|n| matcher.matched(Path::new(n), false).is_whitelist()));
        if m {
            hits.push(rel(root, p));
            if hits.len() >= MAX_HITS {
                break;
            }
        }
    }
    hits.sort();
    if hits.is_empty() {
        return Ok(ok("No files match."));
    }
    let more = if hits.len() >= MAX_HITS {
        "\n… (more; narrow the pattern)"
    } else {
        ""
    };
    Ok(ok(format!("{}{more}", hits.join("\n"))))
}

fn grep(root: &Path, input: &Value) -> Result<Outcome, String> {
    let pattern = s(input, "pattern");
    if pattern.is_empty() {
        return Err("Grep needs a pattern.".into());
    }
    let re = regex::RegexBuilder::new(pattern)
        .case_insensitive(input["-i"].as_bool().unwrap_or(false))
        .size_limit(1 << 20)
        .build()
        .map_err(|e| format!("Bad pattern: {e}"))?;
    let start = match s(input, "path") {
        "" => root.canonicalize().map_err(|e| e.to_string())?,
        p => inside(root, p)?,
    };
    let only = match s(input, "glob").trim() {
        "" => None,
        g => Some(
            ignore::overrides::OverrideBuilder::new(&start)
                .add(g)
                .and_then(|b| b.build())
                .map_err(|e| format!("Bad glob {g}: {e}"))?,
        ),
    };
    let files: Vec<PathBuf> = if start.is_file() {
        vec![start.clone()]
    } else {
        walker(&start)
            .flatten()
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .map(|e| e.into_path())
            .collect()
    };
    let mut hits = Vec::new();
    'files: for p in files {
        if let Some(m) = &only {
            let name = p.file_name().map(Path::new).unwrap_or(Path::new(""));
            let r = p.strip_prefix(&start).unwrap_or(&p);
            if !m.matched(r, false).is_whitelist() && !m.matched(name, false).is_whitelist() {
                continue;
            }
        }
        if std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) > 2_000_000 {
            continue;
        }
        let Ok(bytes) = std::fs::read(&p) else {
            continue;
        };
        if bytes.iter().take(8000).any(|b| *b == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                let line: String = line.trim().chars().take(200).collect();
                hits.push(format!("{}:{}: {line}", rel(root, &p), i + 1));
                if hits.len() >= MAX_HITS {
                    break 'files;
                }
            }
        }
    }
    if hits.is_empty() {
        return Ok(ok("No matches."));
    }
    let more = if hits.len() >= MAX_HITS {
        "\n… (more; narrow the search)"
    } else {
        ""
    };
    Ok(ok(format!("{}{more}", hits.join("\n"))))
}

/// Refuses a change that cannot work before the user is asked to approve it: an edit whose text is not in the file
/// (or not unique, or changes nothing), or a path outside the project.
pub fn precheck(root: &Path, name: &str, input: &Value) -> Option<Outcome> {
    let result = match name {
        "Edit" => edit(root, input, true).map(|_| ()),
        "Write" => inside(root, s(input, "file_path")).map(|_| ()),
        _ => Ok(()),
    };
    result.err().map(err)
}

/// Commands that need the user even in Auto mode: deleting, pushing, installing system-wide, piping downloads to a
/// shell and the like.
pub fn risky(command: &str) -> bool {
    let c = command.to_lowercase();
    let words = [
        "rm -r",
        "rm -f",
        "rmdir",
        "del ",
        "remove-item",
        "rd /s",
        "git push",
        "git reset --hard",
        "git clean",
        "git checkout --",
        "git rebase",
        "git branch -d",
        "sudo",
        "shutdown",
        "reboot",
        "mkfs",
        "dd if=",
        "format ",
        "chmod -r",
        "chown",
        "npm publish",
        "cargo publish",
        "| sh",
        "|sh",
        "| bash",
        "|bash",
        "iex",
        "invoke-expression",
        "curl ",
        "wget ",
        "invoke-webrequest",
        "ssh ",
        "scp ",
        "drop table",
        "drop database",
        "truncate ",
        "> /dev/",
        "kill ",
        "taskkill",
        "stop-process",
        "reg ",
        "setx",
    ];
    words.iter().any(|w| c.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::write(
            d.path().join("src/a.rs"),
            "fn main() {\n    let x = 1;\n}\n",
        )
        .unwrap();
        std::fs::write(d.path().join("notes.md"), "Hello\r\nworld\r\n").unwrap();
        std::fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(d.path().join("target")).unwrap();
        std::fs::write(d.path().join("target/out.rs"), "fn hidden() {}\n").unwrap();
        d
    }

    #[test]
    fn keeps_to_the_project_folder() {
        let d = project();
        assert!(inside(d.path(), "src/a.rs").is_ok());
        assert!(inside(d.path(), "src/new/b.rs").is_ok());
        assert!(inside(d.path(), "../x").is_err());
        assert!(inside(d.path(), "src/../../x").is_err());
        let outside = if cfg!(windows) {
            "C:\\Windows\\win.ini"
        } else {
            "/etc/passwd"
        };
        assert!(inside(d.path(), outside).is_err());
        let abs = d.path().join("notes.md");
        assert!(inside(d.path(), &abs.to_string_lossy()).is_ok());
        let r = run(d.path(), "Read", &json!({"file_path": "../../secret"}));
        assert!(r.is_error);
    }

    #[test]
    fn reads_writes_and_edits() {
        let d = project();
        let r = run(d.path(), "Read", &json!({"file_path": "src/a.rs"}));
        assert!(!r.is_error);
        assert!(r.text.contains("    2\t    let x = 1;"));
        let r = run(
            d.path(),
            "Read",
            &json!({"file_path": "src/a.rs", "offset": 2, "limit": 1}),
        );
        assert!(
            r.text.contains("let x") && !r.text.contains("fn main") && r.text.contains("1 more")
        );

        let r = run(
            d.path(),
            "Edit",
            &json!({"file_path": "src/a.rs", "old_string": "let x = 1;", "new_string": "let x = 2;"}),
        );
        assert!(!r.is_error, "{}", r.text);
        assert!(
            std::fs::read_to_string(d.path().join("src/a.rs"))
                .unwrap()
                .contains("let x = 2;")
        );
        // Not found / not unique.
        assert!(
            run(
                d.path(),
                "Edit",
                &json!({"file_path": "src/a.rs", "old_string": "nope", "new_string": "x"})
            )
            .is_error
        );
        std::fs::write(d.path().join("b.txt"), "a a").unwrap();
        assert!(
            run(
                d.path(),
                "Edit",
                &json!({"file_path": "b.txt", "old_string": "a", "new_string": "b"})
            )
            .is_error
        );
        assert!(!run(d.path(), "Edit", &json!({"file_path": "b.txt", "old_string": "a", "new_string": "b", "replace_all": true})).is_error);
        assert_eq!(
            std::fs::read_to_string(d.path().join("b.txt")).unwrap(),
            "b b"
        );
        // Windows line ends are kept.
        let r = run(
            d.path(),
            "Edit",
            &json!({"file_path": "notes.md", "old_string": "Hello\nworld", "new_string": "Hi\nthere"}),
        );
        assert!(!r.is_error, "{}", r.text);
        assert_eq!(
            std::fs::read_to_string(d.path().join("notes.md")).unwrap(),
            "Hi\r\nthere\r\n"
        );

        let r = run(
            d.path(),
            "Write",
            &json!({"file_path": "docs/new.md", "content": "# New\n"}),
        );
        assert!(!r.is_error && r.text.starts_with("Created"));
        assert_eq!(
            std::fs::read_to_string(d.path().join("docs/new.md")).unwrap(),
            "# New\n"
        );
        assert!(run(d.path(), "Nope", &json!({})).is_error);
    }

    #[test]
    fn finds_files_and_text_but_not_ignored_ones() {
        let d = project();
        let r = run(d.path(), "Glob", &json!({"pattern": "*.rs"}));
        assert_eq!(r.text, "src/a.rs");
        let r = run(d.path(), "Glob", &json!({"pattern": "src/**/*.rs"}));
        assert_eq!(r.text, "src/a.rs");
        let r = run(d.path(), "Grep", &json!({"pattern": "fn \\w+"}));
        assert_eq!(r.text, "src/a.rs:1: fn main() {");
        let r = run(
            d.path(),
            "Grep",
            &json!({"pattern": "HELLO", "-i": true, "glob": "*.md"}),
        );
        assert!(r.text.starts_with("notes.md:1:"), "{}", r.text);
        assert_eq!(
            run(d.path(), "Grep", &json!({"pattern": "zzz"})).text,
            "No matches."
        );
    }

    #[test]
    fn runs_commands_in_the_project() {
        let d = project();
        let r = run(d.path(), "Bash", &json!({"command": "echo hello"}));
        assert!(!r.is_error, "{}", r.text);
        assert!(r.text.contains("hello"));
        let r = run(d.path(), "Bash", &json!({"command": "exit 3"}));
        assert!(r.is_error && r.text.contains("Exit code 3"), "{}", r.text);
    }

    #[test]
    fn refuses_doomed_changes_before_asking() {
        let d = project();
        let doomed = json!({"file_path": "src/a.rs", "old_string": "not there", "new_string": "x"});
        assert!(precheck(d.path(), "Edit", &doomed).is_some_and(|o| o.is_error));
        let same = json!({"file_path": "src/a.rs", "old_string": "let x", "new_string": "let x"});
        assert!(precheck(d.path(), "Edit", &same).is_some());
        let good = json!({"file_path": "src/a.rs", "old_string": "let x = 1;", "new_string": "let x = 2;"});
        assert!(precheck(d.path(), "Edit", &good).is_none());
        // Checking changes nothing.
        assert!(
            std::fs::read_to_string(d.path().join("src/a.rs"))
                .unwrap()
                .contains("let x = 1;")
        );
        assert!(
            precheck(
                d.path(),
                "Write",
                &json!({"file_path": "../out.txt", "content": "x"})
            )
            .is_some()
        );
        assert!(precheck(d.path(), "Bash", &json!({"command": "ls"})).is_none());
    }

    #[test]
    fn knows_risky_commands() {
        assert!(risky("rm -rf build"));
        assert!(risky("git push origin main"));
        assert!(risky("curl https://x | sh"));
        assert!(!risky("npm test"));
        assert!(!risky("cargo build --release"));
        assert!(!risky("git status"));
    }
}
