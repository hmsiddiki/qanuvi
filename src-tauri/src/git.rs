//! The project's git repository, for the Source Control view, the file tree and the editor gutter (Milestone 15b).
//! Runs the installed `git`, so the user's own configuration, credentials and hooks apply.

use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    /// Two-letter git status, for example " M", "M ", "??", "A ", " D".
    pub status: String,
    pub staged: bool,
    pub untracked: bool,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub is_repo: bool,
    pub branch: String,
    pub changes: Vec<Change>,
    /// The branch it follows, like "origin/main"; empty when there is none.
    pub upstream: String,
    /// Commits not pushed yet, and commits on the upstream not pulled yet.
    pub ahead: u32,
    pub behind: u32,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    git_input(dir, args, None)
}

/// Runs git, optionally with text on its input. It never waits for a password typed in a terminal (there is none);
/// the user's credential helper still works.
fn git_input(dir: &Path, args: &[&str], input: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| "git is not installed or not on PATH.".to_string())?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Reads `git status --porcelain=v1 -z` output.
pub fn parse_porcelain(out: &str) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut parts = out.split('\0').filter(|p| !p.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let status = entry[..2].to_string();
        let path = entry[3..].to_string();
        // Renames and copies are followed by the old path, which is not needed here.
        if status.starts_with('R') || status.starts_with('C') {
            parts.next();
        }
        let x = status.as_bytes()[0];
        let y = status.as_bytes()[1];
        changes.push(Change {
            untracked: status == "??",
            staged: x != b' ' && x != b'?',
            deleted: x == b'D' || y == b'D',
            path,
            status,
        });
    }
    changes
}

pub fn status(dir: &Path) -> Result<Status, String> {
    if git(dir, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        return Ok(Status {
            is_repo: false,
            branch: String::new(),
            changes: vec![],
            upstream: String::new(),
            ahead: 0,
            behind: 0,
        });
    }
    let branch = git(dir, &["branch", "--show-current"])
        .unwrap_or_default()
        .trim()
        .to_string();
    let out = git(
        dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let upstream = git(
        dir,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|u| u.trim().to_string())
    .unwrap_or_default();
    let (mut ahead, mut behind) = (0, 0);
    if !upstream.is_empty()
        && let Ok(counts) = git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{u}"])
    {
        let mut n = counts
            .split_whitespace()
            .map(|x| x.parse::<u32>().unwrap_or(0));
        ahead = n.next().unwrap_or(0);
        behind = n.next().unwrap_or(0);
    }
    Ok(Status {
        is_repo: true,
        branch,
        changes: parse_porcelain(&out),
        upstream,
        ahead,
        behind,
    })
}

fn valid_rel(rel: &str) -> Result<(), String> {
    if rel.is_empty() || rel.starts_with('-') || rel.contains("..") {
        return Err("Invalid file path.".into());
    }
    Ok(())
}

/// Whether the repository at `dir` ignores `rel` (git's own answer, its ignore files and global rules included).
pub fn is_ignored(dir: &Path, rel: &str) -> bool {
    valid_rel(rel).is_ok() && git(dir, &["check-ignore", "-q", "--", rel]).is_ok()
}

/// Runs git for tests in other modules.
#[cfg(test)]
pub(crate) fn run_for_test(dir: &Path, args: &[&str]) -> Result<String, String> {
    git(dir, args)
}

/// The unified diff of one file against the last commit (staged and unstaged changes together). An untracked file is
/// shown as entirely added.
pub fn diff(dir: &Path, rel: &str) -> Result<String, String> {
    valid_rel(rel)?;
    let tracked = git(dir, &["ls-files", "--error-unmatch", "--", rel]).is_ok();
    if tracked {
        let has_head = git(dir, &["rev-parse", "--verify", "HEAD"]).is_ok();
        if has_head {
            return git(dir, &["diff", "HEAD", "--no-color", "--", rel]);
        }
        return git(dir, &["diff", "--cached", "--no-color", "--", rel]);
    }
    let content = crate::files::read_file(dir, rel)?;
    if content.binary || content.too_large {
        return Ok(format!("New file {rel} (binary or too large to show)."));
    }
    let mut text = format!("--- /dev/null\n+++ b/{rel}\n");
    let lines: Vec<&str> = content.text.lines().collect();
    text.push_str(&format!("@@ -0,0 +1,{} @@\n", lines.len()));
    for line in lines {
        text.push('+');
        text.push_str(line);
        text.push('\n');
    }
    Ok(text)
}

/// Accepts a change by staging it.
pub fn stage(dir: &Path, rel: &str) -> Result<(), String> {
    valid_rel(rel)?;
    git(dir, &["add", "--", rel]).map(|_| ())
}

/// Undoes a change to a tracked file (back to the last commit). Untracked files are not deleted here.
pub fn revert(dir: &Path, rel: &str) -> Result<(), String> {
    valid_rel(rel)?;
    if git(dir, &["ls-files", "--error-unmatch", "--", rel]).is_err() {
        return Err(
            "This file is new and not in git yet; delete it yourself if you do not want it.".into(),
        );
    }
    git(
        dir,
        &[
            "restore",
            "--staged",
            "--worktree",
            "--source=HEAD",
            "--",
            rel,
        ],
    )
    .map(|_| ())
}

const MAX_VERSION_BYTES: usize = 2 * 1024 * 1024;

/// A file as it is in the last commit and in the staging area, in working-tree form (line endings as on disk), so it
/// compares line by line with the file. None where the file is not there (new, or binary or too large to compare).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Versions {
    pub head: Option<String>,
    pub index: Option<String>,
}

fn version(dir: &Path, spec: &str, rel: &str) -> Option<String> {
    let out = git(dir, &["cat-file", "--filters", &format!("{spec}:./{rel}")]).ok()?;
    (out.len() <= MAX_VERSION_BYTES && !out.contains('\0')).then_some(out)
}

pub fn versions(dir: &Path, rel: &str) -> Result<Versions, String> {
    valid_rel(rel)?;
    Ok(Versions {
        head: version(dir, "HEAD", rel),
        index: version(dir, "", rel),
    })
}

/// Puts this text in the staging area for a file (git's own filters turn it into repository form). Used to stage or
/// unstage one change of a file.
pub fn set_index(dir: &Path, rel: &str, text: &str) -> Result<(), String> {
    valid_rel(rel)?;
    let path_arg = format!("--path={rel}");
    let sha = git_input(
        dir,
        &["hash-object", "-w", "--stdin", &path_arg],
        Some(text),
    )?;
    let sha = sha.trim();
    let mode = git(dir, &["ls-files", "-s", "--", rel])
        .ok()
        .and_then(|l| l.split_whitespace().next().map(String::from))
        .filter(|m| m.len() == 6)
        .unwrap_or_else(|| "100644".into());
    // update-index takes paths from the repository's top folder.
    let prefix = git(dir, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    let full = format!("{}{rel}", prefix.trim());
    let top = git(dir, &["rev-parse", "--show-toplevel"])?;
    git(
        Path::new(top.trim()),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("{mode},{sha},{full}"),
        ],
    )
    .map(|_| ())
}

/// Takes a file (or "." for all) out of the staging area; the file itself keeps its changes.
pub fn unstage(dir: &Path, rel: &str) -> Result<(), String> {
    if rel != "." {
        valid_rel(rel)?;
    }
    if git(dir, &["rev-parse", "--verify", "HEAD"]).is_ok() {
        git(dir, &["restore", "--staged", "--", rel]).map(|_| ())
    } else {
        git(dir, &["rm", "--cached", "-r", "-q", "--", rel]).map(|_| ())
    }
}

/// Stages every change under the project folder.
pub fn stage_all(dir: &Path) -> Result<(), String> {
    git(dir, &["add", "-A", "--", "."]).map(|_| ())
}

/// Commits what is staged (or, with `all`, every change to tracked files). Answers git's one-line summary.
pub fn commit(dir: &Path, message: &str, amend: bool, all: bool) -> Result<String, String> {
    if message.trim().is_empty() && !amend {
        return Err("Write a commit message first.".into());
    }
    let mut args = vec!["commit", "--cleanup=strip"];
    if all {
        args.push("--all");
    }
    if amend {
        args.push("--amend");
        if message.trim().is_empty() {
            args.push("--no-edit");
        }
    }
    let out = if message.trim().is_empty() {
        git(dir, &args)?
    } else {
        args.extend(["-F", "-"]);
        git_input(dir, &args, Some(message))?
    };
    Ok(out.lines().next().unwrap_or("").trim().to_string())
}

/// Fetch, pull or push. Push sets the upstream the first time (to origin, under the same branch name).
pub fn sync(dir: &Path, action: &str) -> Result<String, String> {
    let has_upstream = git(dir, &["rev-parse", "--abbrev-ref", "@{u}"]).is_ok();
    let args: Vec<&str> = match action {
        "fetch" => vec!["fetch", "--prune"],
        "pull" if has_upstream => vec!["pull"],
        "pull" => {
            return Err("This branch does not follow a remote branch yet; push it first.".into());
        }
        "push" if has_upstream => vec!["push"],
        "push" => {
            let remotes = git(dir, &["remote"])?;
            let remote = remotes
                .lines()
                .find(|r| r.trim() == "origin")
                .or_else(|| remotes.lines().next())
                .ok_or("This repository has no remote to push to.")?;
            git(dir, &["push", "-u", remote.trim(), "HEAD"])?;
            return Ok(format!(
                "Pushed to {} and set it as the upstream.",
                remote.trim()
            ));
        }
        _ => return Err(format!("Unknown action {action}.")),
    };
    git(dir, &args)?;
    let upstream = git(dir, &["rev-parse", "--abbrev-ref", "@{u}"]).unwrap_or_default();
    Ok(match action {
        "fetch" => "Fetched.".to_string(),
        "pull" => format!("Pulled from {}.", upstream.trim()),
        _ => format!("Pushed to {}.", upstream.trim()),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
}

pub fn branches(dir: &Path) -> Result<Vec<Branch>, String> {
    let out = git(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(HEAD)%09%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            let name = p.next()?.to_string();
            let current = p.next()? == "*";
            let full = p.next()?;
            (!full.ends_with("/HEAD")).then(|| Branch {
                remote: full.starts_with("refs/remotes/"),
                name,
                current,
            })
        })
        .collect())
}

fn valid_branch(dir: &Path, name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('-')
        || git(dir, &["check-ref-format", "--branch", name]).is_err()
    {
        return Err(format!("\"{name}\" is not a valid branch name."));
    }
    Ok(())
}

/// Switches to a branch; a remote branch like origin/feature gets a local branch that follows it.
pub fn switch(dir: &Path, name: &str, create: bool) -> Result<(), String> {
    let name = name.trim();
    if create {
        valid_branch(dir, name)?;
        return git(dir, &["switch", "-c", name]).map(|_| ());
    }
    if name.starts_with('-') || name.is_empty() {
        return Err("Choose a branch.".into());
    }
    let is = |r: String| git(dir, &["rev-parse", "--verify", "-q", &r]).is_ok();
    if !is(format!("refs/heads/{name}")) && is(format!("refs/remotes/{name}")) {
        return git(dir, &["switch", "--track", name]).map(|_| ());
    }
    git(dir, &["switch", name]).map(|_| ())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stash {
    pub index: u32,
    pub message: String,
}

pub fn stashes(dir: &Path) -> Result<Vec<Stash>, String> {
    Ok(git(dir, &["stash", "list", "--format=%gs"])?
        .lines()
        .enumerate()
        .map(|(i, m)| Stash {
            index: i as u32,
            message: m.to_string(),
        })
        .collect())
}

/// "push" (with a message, untracked files included), "pop", "apply" or "drop" of stash number `index`.
pub fn stash(dir: &Path, action: &str, index: u32, message: &str) -> Result<(), String> {
    let r = format!("stash@{{{index}}}");
    match action {
        "push" => {
            let mut args = vec!["stash", "push", "--include-untracked"];
            if !message.trim().is_empty() {
                args.extend(["-m", message.trim()]);
            }
            git(dir, &args).map(|_| ())
        }
        "pop" | "apply" | "drop" => git(dir, &["stash", action, &r]).map(|_| ()),
        _ => Err(format!("Unknown action {action}.")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

/// The last commits, of the whole project or of one file (following renames).
pub fn log(dir: &Path, rel: Option<&str>, max: u32) -> Result<Vec<Commit>, String> {
    let n = format!("-n{}", max.clamp(1, 500));
    let mut args = vec![
        "log",
        n.as_str(),
        "--format=%H%x09%h%x09%an%x09%ad%x09%s",
        "--date=short",
    ];
    if let Some(r) = rel {
        valid_rel(r)?;
        args.extend(["--follow", "--", r]);
    }
    let out = match git(dir, &args) {
        Ok(o) => o,
        // A new repository has no commits yet.
        Err(e) if e.contains("does not have any commits") => String::new(),
        Err(e) => return Err(e),
    };
    Ok(out
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.splitn(5, '\t').collect();
            (p.len() == 5).then(|| Commit {
                sha: p[0].into(),
                short: p[1].into(),
                author: p[2].into(),
                date: p[3].into(),
                subject: p[4].into(),
            })
        })
        .collect())
}

/// What a commit changed in one file (or in the whole project).
pub fn show(dir: &Path, sha: &str, rel: Option<&str>) -> Result<String, String> {
    if sha.len() < 4 || sha.len() > 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid commit.".into());
    }
    let mut args = vec![
        "show",
        "--no-color",
        "--format=%h %an, %ad%n%n%B",
        "--date=short",
        sha,
    ];
    if let Some(r) = rel {
        valid_rel(r)?;
        args.extend(["--", r]);
    }
    git(dir, &args)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Blame {
    pub sha: String,
    pub author: String,
    /// Seconds since 1970.
    pub time: i64,
    pub summary: String,
    /// The line is changed in the working copy and not committed yet.
    pub uncommitted: bool,
}

pub fn parse_blame(out: &str) -> Option<Blame> {
    let mut lines = out.lines();
    let sha = lines.next()?.split_whitespace().next()?.to_string();
    let mut b = Blame {
        uncommitted: sha.chars().all(|c| c == '0'),
        sha,
        author: String::new(),
        time: 0,
        summary: String::new(),
    };
    for l in lines {
        if let Some(v) = l.strip_prefix("author ") {
            b.author = v.to_string();
        } else if let Some(v) = l.strip_prefix("author-time ") {
            b.time = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = l.strip_prefix("summary ") {
            b.summary = v.to_string();
        }
    }
    Some(b)
}

/// Who last changed a line (1-based), from the file as it is now.
pub fn blame(dir: &Path, rel: &str, line: u32) -> Result<Blame, String> {
    valid_rel(rel)?;
    let range = format!("{0},{0}", line.max(1));
    let out = git(dir, &["blame", "--porcelain", "-L", &range, "--", rel])?;
    parse_blame(&out).ok_or_else(|| "No blame information for this line.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_porcelain_status() {
        let out =
            " M src/a.php\0M  staged.txt\0?? new file.txt\0 D gone.txt\0R  new.txt\0old.txt\0";
        let changes = parse_porcelain(out);
        assert_eq!(changes.len(), 5);
        assert_eq!(changes[0].path, "src/a.php");
        assert!(!changes[0].staged);
        assert!(changes[1].staged);
        assert!(changes[2].untracked);
        assert_eq!(changes[2].path, "new file.txt");
        assert!(changes[3].deleted);
        assert_eq!(changes[4].path, "new.txt");
    }

    #[test]
    fn parses_blame() {
        let out = "abc123 1 1 1\nauthor Test Person\nauthor-time 1700000000\nsummary Add the cart\n\tline\n";
        let b = parse_blame(out).unwrap();
        assert_eq!(b.author, "Test Person");
        assert_eq!(b.time, 1700000000);
        assert_eq!(b.summary, "Add the cart");
        assert!(!b.uncommitted);
        assert!(
            parse_blame("0000000000000000000000000000000000000000 1 1 1\n")
                .unwrap()
                .uncommitted
        );
    }

    fn repo() -> Option<tempfile::TempDir> {
        git(Path::new("."), &["--version"]).ok()?;
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        git(d, &["init", "-q"]).unwrap();
        git(d, &["config", "user.email", "test@example.com"]).unwrap();
        git(d, &["config", "user.name", "Test"]).unwrap();
        git(d, &["config", "core.autocrlf", "false"]).unwrap();
        Some(dir)
    }

    #[test]
    fn works_with_a_real_repository() {
        let Some(dir) = repo() else { return };
        let d = dir.path();
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        git(d, &["add", "a.txt"]).unwrap();
        git(d, &["commit", "-q", "-m", "first"]).unwrap();

        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(d.join("b.txt"), "new\n").unwrap();
        let st = status(d).unwrap();
        assert!(st.is_repo);
        assert_eq!(st.changes.len(), 2);
        assert!(diff(d, "a.txt").unwrap().contains("+two"));
        assert!(diff(d, "b.txt").unwrap().contains("+new"));

        revert(d, "a.txt").unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("a.txt"))
                .unwrap()
                .replace("\r\n", "\n"),
            "one\n"
        );
        assert!(
            revert(d, "b.txt").is_err(),
            "new files are never deleted by revert"
        );
        stage(d, "b.txt").unwrap();
        let st = status(d).unwrap();
        assert!(st.changes.iter().any(|c| c.path == "b.txt" && c.staged));
        assert!(diff(d, "../x").is_err());

        let plain = tempfile::tempdir().unwrap();
        assert!(!status(plain.path()).unwrap().is_repo);
    }

    #[test]
    fn stages_parts_commits_branches_stashes_and_history() {
        let Some(dir) = repo() else { return };
        let d = dir.path();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("src/a.txt"), "one\ntwo\nthree\n").unwrap();
        stage_all(d).unwrap();
        assert!(commit(d, "  ", false, false).is_err());
        assert!(
            commit(d, "First commit", false, false)
                .unwrap()
                .contains("First commit")
        );
        let first = status(d).unwrap().branch;

        // Stage only part of a change: the staged text is what Qanuvi computed.
        std::fs::write(d.join("src/a.txt"), "ONE\ntwo\nTHREE\n").unwrap();
        let v = versions(d, "src/a.txt").unwrap();
        assert_eq!(v.head.as_deref(), Some("one\ntwo\nthree\n"));
        set_index(d, "src/a.txt", "ONE\ntwo\nthree\n").unwrap();
        let v = versions(d, "src/a.txt").unwrap();
        assert_eq!(v.index.as_deref(), Some("ONE\ntwo\nthree\n"));
        assert_eq!(status(d).unwrap().changes[0].status, "MM");
        unstage(d, "src/a.txt").unwrap();
        assert_eq!(status(d).unwrap().changes[0].status, " M");

        // Commit every tracked change; history, a commit's change and blame.
        commit(d, "Shout", false, true).unwrap();
        let history = log(d, Some("src/a.txt"), 10).unwrap();
        let subjects: Vec<&str> = history.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, vec!["Shout", "First commit"]);
        assert!(
            show(d, &history[0].sha, Some("src/a.txt"))
                .unwrap()
                .contains("+ONE")
        );
        assert!(show(d, "nothex!", None).is_err());
        let b = blame(d, "src/a.txt", 2).unwrap();
        assert_eq!(b.summary, "First commit");
        assert_eq!(b.author, "Test");

        // Branches.
        switch(d, "feature/x", true).unwrap();
        assert!(switch(d, "bad name", true).is_err());
        let all = branches(d).unwrap();
        assert!(all.iter().any(|b| b.name == "feature/x" && b.current));
        assert!(all.iter().any(|b| b.name == first && !b.current));
        switch(d, &first, false).unwrap();
        assert_eq!(status(d).unwrap().branch, first);

        // Stash with an untracked file, then bring it back.
        std::fs::write(d.join("new.txt"), "new\n").unwrap();
        stash(d, "push", 0, "Work in progress").unwrap();
        assert!(status(d).unwrap().changes.is_empty());
        assert!(stashes(d).unwrap()[0].message.contains("Work in progress"));
        stash(d, "pop", 0, "").unwrap();
        assert!(d.join("new.txt").exists());

        // No remote: push and pull explain instead of failing obscurely.
        assert!(sync(d, "push").unwrap_err().contains("no remote"));
        assert!(sync(d, "pull").is_err());
    }
}
