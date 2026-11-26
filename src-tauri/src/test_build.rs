//! Test builds of a project's app (24y Y8): how a plan run builds the app into its own folder, starts it with its
//! own fresh data, checks it with a command and closes it after, never over the installed app.
//!
//! The recipe is kept per project in its settings ("Test build"); a plan file's own "Test build" section wins for
//! that plan. A step asks for it with a line "Check: test build"; the runner runs it after the step's other checks,
//! once free memory allows (builds wait when it is low).

use crate::checks::{self, CheckResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Free memory a test build waits for when the recipe names none.
pub const DEFAULT_MIN_FREE_GB: u32 = 4;
/// How long a test build waits for free memory before the step is set aside.
pub const MEMORY_WAIT: Duration = Duration::from_secs(20 * 60);
/// How often free memory is looked at again while waiting.
pub const MEMORY_POLL: Duration = Duration::from_secs(30);
/// A build's time limit (a first release build of a big project takes long).
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(90 * 60);
/// How long the started app gets to answer its check.
pub const READY_WAIT: Duration = Duration::from_secs(60);
/// One try of the check.
const CHECK_TRY: Duration = Duration::from_secs(30);
/// How the test build is named among a step's failing checks.
pub const NAME: &str = "test build";
/// How the attempt's note starts when the build waited too long for memory (the step is set aside, not retried).
pub const NO_MEMORY: &str = "Test build: not enough free memory: ";

/// A project's test build recipe. `{out}` (its own build folder, kept between builds) and `{data}` (fresh data,
/// removed after) are filled in by Qanuvi.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestBuild {
    /// Builds the app, for example `cargo build --release --target-dir {out}`.
    pub build: String,
    /// Starts it in the background (optional), for example `{out}/release/app --data {data}`.
    pub start: String,
    /// Checks it; tried again until it passes or a minute goes by while the started app comes up.
    pub check: String,
    /// Free memory (GB) the build waits for; 0 = the default, 4 GB.
    #[serde(skip_serializing_if = "is_zero")]
    pub min_free_gb: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl TestBuild {
    pub fn is_set(&self) -> bool {
        !self.build.trim().is_empty() || !self.check.trim().is_empty()
    }

    pub fn min_free_mb(&self) -> u64 {
        u64::from(if self.min_free_gb == 0 {
            DEFAULT_MIN_FREE_GB
        } else {
            self.min_free_gb
        }) * 1024
    }

    /// The recipe's commands that are set.
    pub fn commands(&self) -> Vec<&str> {
        [&self.build, &self.start, &self.check]
            .into_iter()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .collect()
    }

    /// Why the recipe cannot run as it is, in plain words; None when it can.
    pub fn problem(&self) -> Option<String> {
        if !self.is_set() {
            return Some(
                "the project has no test build recipe (set it in the project's settings, under Test build)".into(),
            );
        }
        if !self.start.trim().is_empty() && self.check.trim().is_empty() {
            return Some(
                "its test build starts the app but has no Check command to see that it works"
                    .into(),
            );
        }
        touches_installed(self)
    }
}

/// A step's check line that asks for the test build ("Check: test build", "the test build").
pub fn names_test_build(command: &str) -> bool {
    let t = command
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_lowercase()
        .replace('-', " ");
    let t = t
        .strip_prefix("the ")
        .or_else(|| t.strip_prefix("a "))
        .unwrap_or(&t);
    t == "test build" || t == "test build of the app"
}

/// The "Test build" section of a plan file, when it has one: lines "Build: ...", "Start: ...", "Check: ..." and
/// "Memory: 6 GB" (list marks and backticks may be around them), up to the next heading.
pub fn plan_test_build(text: &str) -> Option<TestBuild> {
    let mut found = false;
    let mut inside = false;
    let mut t = TestBuild::default();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') {
            inside = l
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("test build");
            found |= inside;
            continue;
        }
        if !inside {
            continue;
        }
        let l = l.trim_start_matches(['-', '*', '+', ' ']).trim();
        let Some((key, value)) = l.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('`').trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_lowercase().as_str() {
            "build" => t.build = value,
            "start" => t.start = value,
            "check" => t.check = value,
            "memory" => {
                t.min_free_gb = value
                    .split(|c: char| !c.is_ascii_digit())
                    .find(|s| !s.is_empty())
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0)
            }
            _ => {}
        }
    }
    found.then_some(t)
}

/// The recipe Qanuvi fills in for a project it knows: Qanuvi's own repository builds its master without a window,
/// starts it on fresh data and asks it for its status.
pub fn suggest(root: &Path) -> Option<TestBuild> {
    let tauri = root.join("src-tauri");
    if !tauri
        .join("src")
        .join("bin")
        .join("qanuvi-master.rs")
        .is_file()
    {
        return None;
    }
    let exe = if cfg!(windows) {
        "{out}/release/qanuvi-master.exe"
    } else {
        "{out}/release/qanuvi-master"
    };
    Some(TestBuild {
        build: "cargo build --release --manifest-path src-tauri/Cargo.toml --bin qanuvi-master --target-dir {out}"
            .into(),
        start: format!("{exe} core --data {{data}}"),
        check: format!("{exe} status --data {{data}}"),
        min_free_gb: 0,
    })
}

/// Why a recipe would reach the installed app or Qanuvi's own data, or None. A test build gets `{data}`; it has no
/// reason to name Qanuvi's data folders or the folder of the Qanuvi that runs it.
pub fn touches_installed(recipe: &TestBuild) -> Option<String> {
    let mut guarded: Vec<PathBuf> = Vec::new();
    let installed = crate::data_location::Defaults::installed();
    guarded.push(installed.base.clone());
    guarded.extend(installed.webview.clone());
    guarded.push(crate::paths::data_dir());
    if let Some(dir) = crate::data_location::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        guarded.push(dir);
    }
    touches(recipe, &guarded)
}

fn norm(s: &str) -> String {
    s.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn touches(recipe: &TestBuild, guarded: &[PathBuf]) -> Option<String> {
    let id = crate::data_location::IDENTIFIER.to_lowercase();
    for c in recipe.commands() {
        let n = norm(c);
        if n.contains(&id) {
            return Some(format!(
                "its test build names Qanuvi's own data folder (`{c}`); a test build gets fresh data as {{data}}"
            ));
        }
        for g in guarded {
            let g = norm(&g.to_string_lossy());
            if g.len() > 3 && n.contains(&g) {
                return Some(format!(
                    "its test build would reach the installed Qanuvi or its data (`{c}`)"
                ));
            }
        }
    }
    None
}

/// The folders of one project's test builds: `out` is kept (later builds are quick), `data` is made fresh each time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub root: PathBuf,
    pub out: PathBuf,
    pub data: PathBuf,
}

pub fn dirs(base: &Path, project_id: &str) -> Dirs {
    let id: String = project_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let root = base.join("test-builds").join(id);
    Dirs {
        out: root.join("out"),
        data: root.join("data"),
        root,
    }
}

/// Written at each test build, so the cleanup knows when a project's build files were last used.
const USED_FILE: &str = "used";

/// tb1: a project's kept build files go when nothing has built there for this long.
pub const KEEP_UNUSED: Duration = Duration::from_secs(14 * 24 * 3600);

/// tb1: removes the test builds (under `base`/test-builds) of projects no longer in `projects` (their ids), and those
/// not built for `unused_for` (the next build starts from nothing, as the first one did). Answers how many went.
pub fn sweep(base: &Path, projects: &[String], unused_for: Duration) -> usize {
    let known: Vec<PathBuf> = projects.iter().map(|id| dirs(base, id).root).collect();
    let now = std::time::SystemTime::now();
    let mut gone = 0;
    for e in std::fs::read_dir(base.join("test-builds"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let root = e.path();
        if !root.is_dir() {
            continue;
        }
        let last = std::fs::metadata(root.join(USED_FILE))
            .or_else(|_| std::fs::metadata(&root))
            .and_then(|m| m.modified())
            .ok();
        let stale = last
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > unused_for);
        if (!known.contains(&root) || stale) && std::fs::remove_dir_all(&root).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// The command with `{out}` and `{data}` filled in. On Windows the path written after them uses backslashes, so
/// `{out}/release/app.exe` is a program cmd can start; a path with spaces is quoted.
pub fn expand(command: &str, d: &Dirs) -> String {
    let mut out = String::with_capacity(command.len() + 64);
    let mut rest = command;
    while let Some(at) = rest.find('{') {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let (path, len) = if after.starts_with("{out}") {
            (&d.out, 5)
        } else if after.starts_with("{data}") {
            (&d.data, 6)
        } else {
            out.push('{');
            rest = &after[1..];
            continue;
        };
        let tail_end = after[len..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .map_or(after.len(), |i| i + len);
        let mut whole = format!("{}{}", path.display(), &after[len..tail_end]);
        if cfg!(windows) {
            whole = whole.replace('/', "\\");
        }
        if whole.contains(' ') {
            whole = format!("\"{whole}\"");
        }
        out.push_str(&whole);
        rest = &after[tail_end..];
    }
    out.push_str(rest);
    out
}

/// Waits until `free_mb` answers at least `need_mb`. Answers how long it waited, or why it gave up (still too
/// little after `max`, or the run was stopped). Free memory that cannot be read does not hold the build.
pub fn wait_for_memory(
    free_mb: &dyn Fn() -> Option<u64>,
    sleep: &dyn Fn(Duration),
    need_mb: u64,
    max: Duration,
    stop: &dyn Fn() -> bool,
    waiting: &mut dyn FnMut(bool),
) -> Result<Duration, String> {
    let mut waited = Duration::ZERO;
    loop {
        let Some(free) = free_mb() else {
            waiting(false);
            return Ok(waited);
        };
        if free >= need_mb {
            waiting(false);
            return Ok(waited);
        }
        if stop() {
            waiting(false);
            return Err("the run was stopped while the test build waited for free memory".into());
        }
        if waited >= max {
            waiting(false);
            return Err(format!(
                "{} GB free, the test build needs {} GB; it waited {} minutes",
                gb(free),
                gb(need_mb),
                max.as_secs() / 60
            ));
        }
        waiting(true);
        sleep(MEMORY_POLL);
        waited += MEMORY_POLL;
    }
}

fn gb(mb: u64) -> String {
    let g = mb as f64 / 1024.0;
    if g >= 10.0 || (g.fract() < 0.05) {
        format!("{g:.0}")
    } else {
        format!("{g:.1}")
    }
}

fn result(ok: bool, started: Instant, output: String) -> CheckResult {
    CheckResult {
        ok,
        exit_code: Some(if ok { 0 } else { 1 }),
        timed_out: false,
        duration_ms: started.elapsed().as_millis() as u64,
        output,
    }
}

/// Runs the recipe in the project's folder: the build, then (with fresh data) the start, then the check until it
/// passes or the app had a minute; whatever was started is closed after and its data removed. Passes only when the
/// build and the check pass. The output says which part failed, with that part's output.
pub fn run(root: &Path, recipe: &TestBuild, d: &Dirs, stop: &dyn Fn() -> bool) -> CheckResult {
    let started = Instant::now();
    if let Some(why) = recipe.problem() {
        return result(false, started, format!("Test build not run: {why}."));
    }
    let _ = std::fs::create_dir_all(&d.out);
    let _ = std::fs::write(d.root.join(USED_FILE), b"");
    let mut said = String::new();
    if !recipe.build.trim().is_empty() {
        let command = expand(recipe.build.trim(), d);
        match checks::run(root, &command, BUILD_TIMEOUT) {
            Ok(r) if r.ok => said.push_str(&format!(
                "Build passed ({} s): {command}\n",
                r.duration_ms / 1000
            )),
            Ok(r) => {
                let why = if r.timed_out {
                    "did not finish in time"
                } else {
                    "failed"
                };
                return result(
                    false,
                    started,
                    format!("Test build: the build {why}: {command}\n{}", r.output),
                );
            }
            Err(e) => {
                return result(
                    false,
                    started,
                    format!("Test build: the build could not start: {e}"),
                );
            }
        }
    }
    if stop() {
        return result(
            false,
            started,
            format!("{said}Test build: the run was stopped."),
        );
    }
    let _ = std::fs::remove_dir_all(&d.data);
    let _ = std::fs::create_dir_all(&d.data);
    let log = d.root.join("start.log");
    let mut child = None;
    if !recipe.start.trim().is_empty() {
        let command = expand(recipe.start.trim(), d);
        match checks::start(root, &command, &log) {
            Ok(c) => child = Some(c),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&d.data);
                return result(
                    false,
                    started,
                    format!("{said}Test build: the app could not start: {e}\n{command}"),
                );
            }
        }
        said.push_str(&format!("Started: {command}\n"));
    }
    let outcome = if recipe.check.trim().is_empty() {
        result(true, started, said.clone())
    } else {
        let command = expand(recipe.check.trim(), d);
        let ready_from = Instant::now();
        loop {
            let r = checks::run(root, &command, CHECK_TRY);
            let exited = child
                .as_mut()
                .and_then(|c: &mut std::process::Child| c.try_wait().ok().flatten());
            match r {
                Ok(r) if r.ok => {
                    break result(
                        true,
                        started,
                        format!("{said}Check passed: {command}\n{}", r.output),
                    );
                }
                r => {
                    let output = match r {
                        Ok(r) => r.output,
                        Err(e) => e,
                    };
                    let late = ready_from.elapsed() >= READY_WAIT || child.is_none();
                    if exited.is_some() || late || stop() {
                        let app = if let Some(status) = exited {
                            format!(
                                "\nThe started app ended by itself ({status}). Its output:\n{}",
                                tail(&log)
                            )
                        } else if child.is_some() {
                            format!("\nThe started app's output:\n{}", tail(&log))
                        } else {
                            String::new()
                        };
                        break result(
                            false,
                            started,
                            format!("{said}Test build: the check failed: {command}\n{output}{app}"),
                        );
                    }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    };
    if let Some(mut c) = child {
        checks::kill_tree(&mut c);
    }
    // Windows keeps a file in use a moment after its program ended.
    for _ in 0..10 {
        if std::fs::remove_dir_all(&d.data).is_ok() || !d.data.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    outcome
}

/// The last part of the started app's output.
fn tail(log: &Path) -> String {
    let bytes = std::fs::read(log).unwrap_or_default();
    let start = bytes.len().saturating_sub(8 * 1024);
    String::from_utf8_lossy(&bytes[start..]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn d(base: &Path) -> Dirs {
        dirs(base, "p1")
    }

    // tb1: kept build files of a removed project, or not built for two weeks, are cleaned up; the rest stay.
    #[test]
    fn old_and_orphaned_test_builds_are_cleaned_up() {
        let base = tempfile::tempdir().unwrap();
        let b = base.path();
        let make = |id: &str, age_days: u64| {
            let d = dirs(b, id);
            std::fs::create_dir_all(&d.out).unwrap();
            std::fs::write(d.out.join("app"), b"build").unwrap();
            let used = std::fs::File::create(d.root.join(USED_FILE)).unwrap();
            let at = std::time::SystemTime::now() - Duration::from_secs(age_days * 24 * 3600);
            used.set_modified(at).unwrap();
            d.root
        };
        let fresh = make("fresh", 1);
        let old = make("old", 20);
        let removed = make("removed", 0);
        let projects = vec!["fresh".to_string(), "old".to_string()];
        assert_eq!(sweep(b, &projects, KEEP_UNUSED), 2);
        assert!(fresh.join("out/app").exists());
        assert!(!old.exists() && !removed.exists());
        // No test builds at all: nothing to do.
        assert_eq!(sweep(&b.join("none"), &projects, KEEP_UNUSED), 0);
    }

    #[test]
    fn knows_a_step_line_that_asks_for_it() {
        for yes in [
            "test build",
            "the test build.",
            "A test-build",
            "Test build of the app",
        ] {
            assert!(names_test_build(yes), "{yes}");
        }
        for no in ["test", "npm test", "test builds pass", "cargo build"] {
            assert!(!names_test_build(no), "{no}");
        }
    }

    #[test]
    fn reads_a_plans_test_build_section() {
        let t = plan_test_build(
            "# Plan\n\n## Test build\n- Build: `cargo build --target-dir {out}`\n- Start: {out}/debug/app --data {data}\n- Check: `{out}/debug/app status`\n- Memory: 6 GB\n\n## Steps\n- Build: ignored\n",
        )
        .unwrap();
        assert_eq!(t.build, "cargo build --target-dir {out}");
        assert_eq!(t.start, "{out}/debug/app --data {data}");
        assert_eq!(t.check, "{out}/debug/app status");
        assert_eq!(t.min_free_gb, 6);
        assert_eq!(t.min_free_mb(), 6 * 1024);
        assert_eq!(plan_test_build("# Plan\n## Steps\n- Build: x\n"), None);
        assert_eq!(TestBuild::default().min_free_mb(), 4096);
    }

    #[test]
    fn suggests_the_recipe_for_qanuvis_own_repo_only() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(suggest(dir.path()), None);
        let bin = dir.path().join("src-tauri/src/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("qanuvi-master.rs"), "fn main() {}").unwrap();
        let t = suggest(dir.path()).unwrap();
        assert!(t.build.contains("--bin qanuvi-master --target-dir {out}"));
        assert!(t.start.ends_with("--data {data}"));
        assert!(t.check.contains(" status --data {data}"));
        assert_eq!(t.problem(), None);
    }

    #[test]
    fn fills_in_its_folders() {
        let base = PathBuf::from(if cfg!(windows) { "C:\\q" } else { "/q" });
        let dd = d(&base);
        let got = expand("{out}/release/app --data {data} {x}", &dd);
        if cfg!(windows) {
            assert_eq!(
                got,
                "C:\\q\\test-builds\\p1\\out\\release\\app --data C:\\q\\test-builds\\p1\\data {x}"
            );
        } else {
            assert_eq!(
                got,
                "/q/test-builds/p1/out/release/app --data /q/test-builds/p1/data {x}"
            );
        }
        let spaced = d(&base.join("My Data"));
        assert!(expand("{data}", &spaced).starts_with('"'));
    }

    #[test]
    fn never_reaches_the_installed_app() {
        let installed = PathBuf::from(if cfg!(windows) {
            "D:\\qanuvi"
        } else {
            "/opt/qanuvi"
        });
        let over = TestBuild {
            build: "cargo build".into(),
            start: if cfg!(windows) {
                "d:/Qanuvi/qanuvi.exe".into()
            } else {
                "/opt/qanuvi/qanuvi".into()
            },
            check: "x status".into(),
            min_free_gb: 0,
        };
        assert!(touches(&over, std::slice::from_ref(&installed)).is_some());
        let data = TestBuild {
            check: "app --data %APPDATA%/dev.qanuvi.desktop".into(),
            ..TestBuild::default()
        };
        assert!(touches(&data, &[]).is_some());
        let fine = TestBuild {
            build: "cargo build --target-dir {out}".into(),
            start: "{out}/app --data {data}".into(),
            check: "{out}/app status --data {data}".into(),
            min_free_gb: 0,
        };
        assert_eq!(touches(&fine, &[installed]), None);
        let no_check = TestBuild {
            start: "app".into(),
            build: "make".into(),
            ..TestBuild::default()
        };
        assert!(no_check.problem().unwrap().contains("no Check"));
        assert!(
            TestBuild::default()
                .problem()
                .unwrap()
                .contains("no test build recipe")
        );
    }

    #[test]
    fn waits_for_free_memory_then_goes_on_or_gives_up() {
        let free = RefCell::new(vec![1000u64, 2000, 5000]);
        let slept = Cell::new(0u32);
        let mut said = Vec::new();
        let waited = wait_for_memory(
            &|| {
                let mut f = free.borrow_mut();
                Some(if f.len() > 1 { f.remove(0) } else { f[0] })
            },
            &|_| slept.set(slept.get() + 1),
            4096,
            MEMORY_WAIT,
            &|| false,
            &mut |w| said.push(w),
        )
        .unwrap();
        assert_eq!(slept.get(), 2);
        assert_eq!(waited, MEMORY_POLL * 2);
        assert_eq!(said, vec![true, true, false]);
        let gave_up = wait_for_memory(
            &|| Some(1536),
            &|_| {},
            4096,
            MEMORY_POLL * 3,
            &|| false,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(
            gave_up.contains("1.5 GB free, the test build needs 4 GB"),
            "{gave_up}"
        );
        // Free memory that cannot be read does not hold the build.
        assert!(
            wait_for_memory(&|| None, &|_| {}, 4096, MEMORY_WAIT, &|| false, &mut |_| {}).is_ok()
        );
    }

    /// A small program as the "app": the build writes it, the start runs it on its data, the check reads what it
    /// wrote. Then the app is closed and its data removed.
    #[test]
    fn builds_starts_checks_and_closes_the_app() {
        let project = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let dd = d(base.path());
        let (build, start, check) = if cfg!(windows) {
            (
                "echo built > {out}/built.txt",
                "cmd /C \"echo up > {data}/up.txt && ping -n 60 127.0.0.1 >NUL\"",
                "type {data}/up.txt",
            )
        } else {
            (
                "echo built > {out}/built.txt",
                "sh -c 'echo up > {data}/up.txt; sleep 60'",
                "cat {data}/up.txt",
            )
        };
        let recipe = TestBuild {
            build: build.into(),
            start: start.into(),
            check: check.into(),
            min_free_gb: 0,
        };
        let r = run(project.path(), &recipe, &dd, &|| false);
        assert!(r.ok, "{}", r.output);
        assert!(r.output.contains("Check passed"));
        assert!(dd.out.join("built.txt").exists());
        assert!(!dd.data.exists(), "its data is removed after");

        let broken = TestBuild {
            check: if cfg!(windows) {
                "type {data}\\none.txt"
            } else {
                "cat {data}/none.txt"
            }
            .into(),
            start: String::new(),
            ..recipe
        };
        let r = run(project.path(), &broken, &dd, &|| false);
        assert!(!r.ok);
        assert!(
            r.output.contains("Test build: the check failed"),
            "{}",
            r.output
        );
    }
}
