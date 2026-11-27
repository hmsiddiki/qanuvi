//! s3 (ST2): files made or changed outside Qanuvi (a terminal, git, other tools, an agent) show at once, as in VS
//! Code. One watcher per project folder while a window follows it; changes are gathered for a moment and sent as the
//! changed paths (relative, with `/`), with `git: true` when the repository's state changed (a commit, a branch).
//! Build output and package folders are left out, and so is the inside of .git (only HEAD, the index and refs count).

use crate::sink::Sink;
use notify::{RecursiveMode, Watcher};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long changes are gathered before they are sent (a save, a build or a checkout writes many files at once).
const GATHER: Duration = Duration::from_millis(300);
/// At most this many paths in one note; more than that and the window reloads what it shows.
const MAX_PATHS: usize = 200;

/// Folders whose changes never matter to the tree or the Git list (and can hold many thousands of files).
const SKIPPED: &[&str] = &[
    "node_modules",
    "target",
    ".svelte-kit",
    "dist",
    "build",
    ".next",
    ".venv",
    "__pycache__",
    ".cache",
    "vendor",
];

/// What a changed path means for the window: left out, a file of the project, or the repository's own state.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Skip,
    File(String),
    Git,
}

fn kind_of(root: &Path, path: &Path) -> Kind {
    let Ok(rel) = path.strip_prefix(root) else {
        return Kind::Skip;
    };
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some(first) = parts.first() else {
        return Kind::Skip;
    };
    if first == ".git" {
        let inner = parts.get(1).map(String::as_str).unwrap_or("");
        return if matches!(inner, "HEAD" | "index" | "refs" | "packed-refs") {
            Kind::Git
        } else {
            Kind::Skip
        };
    }
    if parts.iter().any(|p| SKIPPED.contains(&p.as_str())) {
        return Kind::Skip;
    }
    Kind::File(parts.join("/"))
}

struct One {
    /// Kept so the watch goes on; dropped with the last follower.
    _watcher: notify::RecommendedWatcher,
    sinks: Arc<Mutex<Vec<Sink<Value>>>>,
}

/// The master's watchers, one per project folder.
#[derive(Default)]
pub struct FileWatches(Mutex<HashMap<PathBuf, One>>);

impl FileWatches {
    /// Follows a project folder: `sink` hears `{"paths": [...], "git": bool, "more": bool}` after changes. A folder
    /// already watched gets one more follower; followers that are gone are dropped, and with the last one the watch.
    pub fn follow(&self, root: &Path, sink: Sink<Value>) -> Result<(), String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        // Watches no window follows any more end here (each send drops the followers that are gone).
        all.retain(|_, one| {
            let mut s = one.sinks.lock().unwrap_or_else(|e| e.into_inner());
            s.retain(|k| k.send(json!({"ping": true})).is_ok());
            !s.is_empty()
        });
        if let Some(one) = all.get(&root) {
            one.sinks
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(sink);
            return Ok(());
        }
        let sinks: Arc<Mutex<Vec<Sink<Value>>>> = Arc::new(Mutex::new(vec![sink]));
        let pending: Arc<Mutex<(BTreeSet<String>, bool, bool)>> = Arc::default();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let base = root.clone();
        let gathered = pending.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            let mut p = gathered.lock().unwrap_or_else(|e| e.into_inner());
            let mut any = false;
            for path in &event.paths {
                match kind_of(&base, path) {
                    Kind::Skip => {}
                    Kind::Git => {
                        p.1 = true;
                        any = true;
                    }
                    Kind::File(rel) => {
                        if p.0.len() < MAX_PATHS {
                            p.0.insert(rel);
                        } else {
                            p.2 = true;
                        }
                        any = true;
                    }
                }
            }
            if any {
                let _ = tx.send(());
            }
        })
        .map_err(|e| format!("Watching the project folder failed: {e}"))?;
        watcher
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| format!("Watching the project folder failed: {e}"))?;
        // The sender: waits for a first change, gathers the rest for a moment, sends them as one note.
        let followers = sinks.clone();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                std::thread::sleep(GATHER);
                while rx.try_recv().is_ok() {}
                let (paths, git, more) = {
                    let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
                    (
                        std::mem::take(&mut p.0),
                        std::mem::take(&mut p.1),
                        std::mem::take(&mut p.2),
                    )
                };
                let note = json!({"paths": paths, "git": git, "more": more});
                let mut s = followers.lock().unwrap_or_else(|e| e.into_inner());
                s.retain(|k| k.send(note.clone()).is_ok());
            }
        });
        all.insert(
            root,
            One {
                _watcher: watcher,
                sinks,
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_output_and_git_inside_are_left_out() {
        let root = Path::new("/p");
        assert_eq!(
            kind_of(root, Path::new("/p/src/a.ts")),
            Kind::File("src/a.ts".into())
        );
        assert_eq!(
            kind_of(root, Path::new("/p/node_modules/x/y.js")),
            Kind::Skip
        );
        assert_eq!(
            kind_of(root, Path::new("/p/app/target/debug/x")),
            Kind::Skip
        );
        assert_eq!(
            kind_of(root, Path::new("/p/.git/objects/ab/cd")),
            Kind::Skip
        );
        assert_eq!(kind_of(root, Path::new("/p/.git/HEAD")), Kind::Git);
        assert_eq!(
            kind_of(root, Path::new("/p/.git/refs/heads/main")),
            Kind::Git
        );
        assert_eq!(kind_of(root, Path::new("/elsewhere/a")), Kind::Skip);
        assert_eq!(kind_of(root, Path::new("/p")), Kind::Skip);
    }

    #[test]
    fn a_file_made_outside_is_heard() {
        let dir = tempfile::tempdir().unwrap();
        let w = FileWatches::default();
        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        let tx = Mutex::new(tx);
        w.follow(
            dir.path(),
            Sink::new(move |v: Value| tx.lock().unwrap().send(v).is_ok()),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.path().join("new.txt"), "hi").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/x")).unwrap();
        std::fs::write(dir.path().join("node_modules/x/i.js"), "x").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut heard: Vec<String> = Vec::new();
        while std::time::Instant::now() < deadline && !heard.iter().any(|p| p == "new.txt") {
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(500))
                && let Some(list) = v["paths"].as_array()
            {
                heard.extend(list.iter().filter_map(|p| p.as_str().map(String::from)));
            }
        }
        assert!(heard.iter().any(|p| p == "new.txt"), "{heard:?}");
        assert!(
            !heard.iter().any(|p| p.starts_with("node_modules")),
            "{heard:?}"
        );
    }
}
