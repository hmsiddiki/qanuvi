//! Qanuvi's own record of a project's files before a piece of work, so what the work changed can be shown with or
//! without git. Git is the user's choice: projects without git, folders the project's git ignores and repositories
//! nested inside the project all work the same. A snapshot keeps size, time and a fingerprint per file; the text of
//! each text file is stored once by its fingerprint in the data folder, so later snapshots only read changed files.

use crate::files;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Text files up to this size are kept, so their changes can be shown; bigger ones are only noticed.
pub const MAX_TEXT_BYTES: u64 = files::MAX_SEARCH_BYTES;
/// At most this many files per snapshot.
pub const MAX_FILES: usize = files::MAX_LISTED_FILES;
/// Snapshots older than this are removed (the latest of each project is always kept).
const KEEP_DAYS: u64 = 14;
/// One file's shown change is cut after this many characters.
const MAX_DIFF_CHARS: usize = 40_000;
/// Folders never looked into, ignore rules or not.
const BULKY: &[&str] = &["node_modules", "vendor", "target", ".git", "dist", "build"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Mark {
    size: u64,
    /// Milliseconds since 1970.
    modified: u128,
    /// The fingerprint; empty for a file too large to read.
    hash: String,
    /// A text file whose content is stored.
    text: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    id: String,
    at: u128,
    root: String,
    truncated: bool,
    files: BTreeMap<String, Mark>,
}

/// What `take` did.
#[derive(Debug, Clone, Serialize)]
pub struct Taken {
    pub id: String,
    pub files: usize,
    /// Texts stored for the first time.
    pub stored: usize,
    /// More files than MAX_FILES: the rest are not followed.
    pub truncated: bool,
    pub ms: u128,
    /// The `also` folders no git repository sees (none, or one that ignores them): with git chosen, their changes
    /// come from Qanuvi's snapshots.
    pub git_blind: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    /// "added", "changed" or "deleted".
    pub kind: &'static str,
    /// The change as a unified diff (or a line saying why it cannot be shown).
    pub diff: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Changes {
    pub changes: Vec<Change>,
    /// With git chosen: changed files git cannot see (no repository, or one that ignores them); Qanuvi's own
    /// snapshot showed them.
    pub unseen: Vec<String>,
    /// The snapshot taken now (the "before" of the next piece of work).
    pub after: String,
}

fn dir() -> PathBuf {
    crate::paths::data_dir().join("snapshots")
}

fn blob_path(hash: &str) -> PathBuf {
    let head: String = hash.chars().take(2).collect();
    dir().join("blobs").join(head).join(hash)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn root_key(root: &Path) -> String {
    files::fingerprint(root.to_string_lossy().to_lowercase().as_bytes())
}

fn manifest_path(id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("Unknown snapshot.".into());
    }
    Ok(dir().join(format!("{id}.json")))
}

fn load(id: &str) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(manifest_path(id)?).map_err(|_| {
        "The snapshot is gone (older than two weeks, or the data folder moved).".to_string()
    })?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The latest snapshot of a project, to reuse fingerprints of files that did not change since.
fn latest(root: &Path) -> Option<Snapshot> {
    let id = std::fs::read_to_string(dir().join(format!("latest-{}", root_key(root)))).ok()?;
    load(id.trim()).ok()
}

/// Every file to follow: the project's own (its ignore rules and hidden nested repositories, as the file tree), and
/// everything under `also` (plan folders) whatever the ignore rules say.
fn paths(root: &Path, also: &[String]) -> (BTreeSet<String>, bool) {
    let mut out = BTreeSet::new();
    let keep = |rel: &str| !rel.starts_with(".qanuvi/") && rel != ".qanuvi";
    for e in files::walk_files(root) {
        let rel = files::rel_string(root, e.path());
        if keep(&rel) {
            out.insert(rel);
        }
        if out.len() >= MAX_FILES {
            return (out, true);
        }
    }
    for a in also {
        let Ok(d) = files::resolve(root, a) else {
            continue;
        };
        if !d.is_dir() {
            continue;
        }
        let mut b = ignore::WalkBuilder::new(&d);
        b.standard_filters(false)
            .filter_entry(|e| !BULKY.iter().any(|s| e.file_name() == *s));
        for e in b.build().flatten() {
            if !e.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let rel = files::rel_string(root, e.path());
            if keep(&rel) {
                out.insert(rel);
            }
            if out.len() >= MAX_FILES {
                return (out, true);
            }
        }
    }
    (out, false)
}

/// `prev_at`: when the snapshot `prev` comes from was taken. Its fingerprint is reused only for a file untouched for
/// two seconds before that (a write in the same moment, or on a disk with coarse times, could keep size and time).
fn mark_of(path: &Path, prev: Option<&Mark>, prev_at: u128, stored: &mut usize) -> Option<Mark> {
    let meta = std::fs::metadata(path).ok()?;
    let size = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    if let Some(p) = prev
        && p.size == size
        && p.modified == modified
        && modified + 2_000 < prev_at
        && (!p.text || blob_path(&p.hash).exists())
    {
        return Some(p.clone());
    }
    if size > MAX_TEXT_BYTES {
        return Some(Mark {
            size,
            modified,
            hash: String::new(),
            text: false,
        });
    }
    let bytes = std::fs::read(path).ok()?;
    let hash = files::fingerprint(&bytes);
    let text = !files::looks_binary(&bytes);
    if text {
        let blob = blob_path(&hash);
        if !blob.exists() && write_atomic(&blob, &bytes).is_ok() {
            *stored += 1;
        }
    }
    Some(Mark {
        size,
        modified,
        hash,
        text,
    })
}

/// Records the project's files now. `also`: folders (from the project) followed even where ignore rules leave them
/// out, such as the folder of the plan being run.
pub fn take(root: &Path, also: &[String]) -> Result<Taken, String> {
    let started = now_ms();
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let prev = latest(&root);
    let (list, truncated) = paths(&root, also);
    let mut stored = 0;
    let mut marks = BTreeMap::new();
    for rel in list {
        let before = prev.as_ref().and_then(|p| p.files.get(&rel));
        let prev_at = prev.as_ref().map_or(0, |p| p.at);
        if let Some(m) = mark_of(&root.join(&rel), before, prev_at, &mut stored) {
            marks.insert(rel, m);
        }
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let snap = Snapshot {
        id: id.clone(),
        at: now_ms(),
        root: root.to_string_lossy().to_string(),
        truncated,
        files: marks,
    };
    let json = serde_json::to_vec(&snap).map_err(|e| e.to_string())?;
    write_atomic(&manifest_path(&id)?, &json)?;
    write_atomic(
        &dir().join(format!("latest-{}", root_key(&root))),
        id.as_bytes(),
    )?;
    prune_now_and_then();
    Ok(Taken {
        id,
        files: snap.files.len(),
        stored,
        truncated,
        ms: now_ms().saturating_sub(started),
        git_blind: also
            .iter()
            .filter(|a| {
                let probe = if a.is_empty() {
                    "x".to_string()
                } else {
                    format!("{a}/x")
                };
                owning_repo(&root, &probe)
                    .is_none_or(|(repo, inner)| crate::git::is_ignored(&repo, &inner))
            })
            .cloned()
            .collect(),
    })
}

/// The files that changed since snapshot `before` (files `skip` names left out), with their changes. With `git`, a
/// file that a repository tracks or sees shows its change against that repository's last commit; files git cannot
/// see are listed in `unseen` and shown from Qanuvi's own snapshot.
pub fn changes(
    root: &Path,
    before: &str,
    also: &[String],
    git: bool,
    skip: &dyn Fn(&str) -> bool,
) -> Result<Changes, String> {
    let was = load(before)?;
    let now = take(root, also)?;
    let after = load(&now.id)?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut out = Changes {
        after: now.id,
        ..Default::default()
    };
    let names: BTreeSet<&String> = was.files.keys().chain(after.files.keys()).collect();
    for path in names {
        if skip(path) {
            continue;
        }
        let (b, a) = (was.files.get(path), after.files.get(path));
        let kind = match (b, a) {
            (None, Some(_)) => "added",
            (Some(_), None) => "deleted",
            (Some(b), Some(a)) if same(b, a) => continue,
            (Some(_), Some(_)) => "changed",
            (None, None) => continue,
        };
        let diff = if git {
            match git_diff(&root, path) {
                Some(d) if !d.trim().is_empty() => d,
                Some(_) => own_diff(path, b, a),
                None => {
                    out.unseen.push(path.clone());
                    own_diff(path, b, a)
                }
            }
        } else {
            own_diff(path, b, a)
        };
        out.changes.push(Change {
            path: path.clone(),
            kind,
            diff: cut(diff),
        });
    }
    Ok(out)
}

fn same(b: &Mark, a: &Mark) -> bool {
    if b.hash.is_empty() || a.hash.is_empty() {
        b.size == a.size && b.modified == a.modified
    } else {
        b.hash == a.hash
    }
}

fn cut(s: String) -> String {
    if s.len() <= MAX_DIFF_CHARS {
        return s;
    }
    let mut end = MAX_DIFF_CHARS;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (the rest of this file's change is cut)", &s[..end])
}

fn blob_text(m: Option<&Mark>) -> Option<String> {
    let m = m?;
    if !m.text {
        return None;
    }
    std::fs::read(blob_path(&m.hash))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).to_string())
}

fn own_diff(path: &str, b: Option<&Mark>, a: Option<&Mark>) -> String {
    let readable = |m: Option<&Mark>| m.is_none_or(|m| m.text);
    if !readable(b) || !readable(a) {
        let size = |m: Option<&Mark>| m.map_or("none".to_string(), |m| format!("{} bytes", m.size));
        return format!(
            "{path}: binary or too large to show (before: {}, after: {}).",
            size(b),
            size(a)
        );
    }
    let before = if b.is_some() {
        blob_text(b)
    } else {
        Some(String::new())
    };
    let after = if a.is_some() {
        blob_text(a)
    } else {
        Some(String::new())
    };
    match (before, after) {
        (Some(x), Some(y)) => unified(path, &x, &y),
        _ => format!("{path}: changed (its earlier text is no longer stored)."),
    }
}

/// The repository that holds `rel` (the nearest folder with .git, the project's own or one nested in it) and the
/// path inside it; None when there is none or it ignores the file.
fn owning_repo(root: &Path, rel: &str) -> Option<(PathBuf, String)> {
    let full = root.join(rel);
    let mut d = full.parent()?;
    loop {
        if d.join(".git").exists() {
            let inner = full.strip_prefix(d).ok()?;
            let inner = files::rel_string(Path::new(""), inner);
            return Some((d.to_path_buf(), inner));
        }
        if d == root {
            return None;
        }
        d = d.parent()?;
    }
}

fn git_diff(root: &Path, rel: &str) -> Option<String> {
    let (repo, inner) = owning_repo(root, rel)?;
    if crate::git::is_ignored(&repo, &inner) {
        return None;
    }
    crate::git::diff(&repo, &inner).ok()
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Same,
    Del,
    Add,
}

/// The line changes from `a` to `b` (Myers' algorithm on what is left after the common start and end), or None when
/// they differ too much to work out quickly.
fn line_ops(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    const MAX_D: usize = 2_000;
    let start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let end = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (x, y) = (&a[start..a.len() - end], &b[start..b.len() - end]);
    let (n, m) = (x.len() as isize, y.len() as isize);
    let max = (n + m) as usize;
    let off = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    // Per round d, only v[-d-1 ..= d+1] (what the walk back reads), so memory grows with d², not d × length.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = max == 0;
    if !found {
        'outer: for d in 0..=max.min(MAX_D) as isize {
            trace.push(v[(off - d - 1) as usize..=(off + d + 1) as usize].to_vec());
            let mut k = -d;
            while k <= d {
                let i = (k + off) as usize;
                let mut px = if k == -d || (k != d && v[i - 1] < v[i + 1]) {
                    v[i + 1]
                } else {
                    v[i - 1] + 1
                };
                let mut py = px - k;
                while px < n && py < m && x[px as usize] == y[py as usize] {
                    px += 1;
                    py += 1;
                }
                v[i] = px;
                if px >= n && py >= m {
                    found = true;
                    break 'outer;
                }
                k += 2;
            }
        }
    }
    if !found {
        return None;
    }
    // Walk back through the trace.
    let mut ops = Vec::new();
    let (mut px, mut py) = (n, m);
    for d in (1..trace.len() as isize).rev() {
        let t = &trace[d as usize];
        let at = |k: isize| t[(k + d + 1) as usize];
        let k = px - py;
        let prev_k = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while px > prev_x && py > prev_y {
            ops.push(Op::Same);
            px -= 1;
            py -= 1;
        }
        if px == prev_x {
            ops.push(Op::Add);
            py -= 1;
        } else {
            ops.push(Op::Del);
            px -= 1;
        }
    }
    while px > 0 && py > 0 {
        ops.push(Op::Same);
        px -= 1;
        py -= 1;
    }
    ops.reverse();
    let mut all = vec![Op::Same; start];
    all.extend(ops);
    all.extend(std::iter::repeat_n(Op::Same, end));
    Some(all)
}

/// A unified diff of two texts with three lines of context; "" when they are the same.
pub fn unified(path: &str, before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let head = format!("--- {path} (before this step)\n+++ {path} (after)\n");
    let Some(ops) = line_ops(&a, &b) else {
        let mut s = head;
        s.push_str(&format!(
            "@@ -1,{} +1,{} @@ (too many changes to line up: the whole new text)\n",
            a.len(),
            b.len()
        ));
        for l in &b {
            s.push('+');
            s.push_str(l);
            s.push('\n');
        }
        return s;
    };
    if ops.iter().all(|o| *o == Op::Same) {
        // Only line endings or the last newline differ.
        return format!("{head}(only line endings or the final newline changed)\n");
    }
    // Positions in a and b for every op.
    let mut pos = Vec::with_capacity(ops.len());
    let (mut i, mut j) = (0usize, 0usize);
    for o in &ops {
        pos.push((i, j));
        match o {
            Op::Same => {
                i += 1;
                j += 1;
            }
            Op::Del => i += 1,
            Op::Add => j += 1,
        }
    }
    const CTX: usize = 3;
    let changed: Vec<usize> = (0..ops.len()).filter(|&k| ops[k] != Op::Same).collect();
    let mut out = head;
    let mut g = 0;
    while g < changed.len() {
        let from = changed[g].saturating_sub(CTX);
        let mut last = changed[g];
        while g + 1 < changed.len() && changed[g + 1] <= last + 2 * CTX + 1 {
            g += 1;
            last = changed[g];
        }
        let to = (last + CTX + 1).min(ops.len());
        let (ai, bj) = pos[from];
        let dels = ops[from..to].iter().filter(|o| **o != Op::Add).count();
        let adds = ops[from..to].iter().filter(|o| **o != Op::Del).count();
        out.push_str(&format!("@@ -{},{dels} +{},{adds} @@\n", ai + 1, bj + 1));
        for k in from..to {
            let (pi, pj) = pos[k];
            match ops[k] {
                Op::Same => {
                    out.push(' ');
                    out.push_str(a[pi]);
                }
                Op::Del => {
                    out.push('-');
                    out.push_str(a[pi]);
                }
                Op::Add => {
                    out.push('+');
                    out.push_str(b[pj]);
                }
            }
            out.push('\n');
        }
        g += 1;
    }
    out
}

static LAST_PRUNE: std::sync::Mutex<u128> = std::sync::Mutex::new(0);

/// At most once an hour: removes old snapshots (keeping each project's latest) and texts no snapshot needs.
fn prune_now_and_then() {
    let now = now_ms();
    {
        let Ok(mut last) = LAST_PRUNE.lock() else {
            return;
        };
        if now.saturating_sub(*last) < 3_600_000 {
            return;
        }
        *last = now;
    }
    let _ = prune(now, KEEP_DAYS as u128 * 86_400_000);
}

fn prune(now: u128, keep_ms: u128) -> Result<(), String> {
    let d = dir();
    let mut latest_ids = BTreeSet::new();
    let mut manifests = Vec::new();
    for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("latest-") {
            if let Ok(id) = std::fs::read_to_string(e.path()) {
                latest_ids.insert(id.trim().to_string());
            }
        } else if let Some(id) = name.strip_suffix(".json") {
            manifests.push(id.to_string());
        }
    }
    let mut needed = BTreeSet::new();
    for id in manifests {
        let Ok(s) = load(&id) else { continue };
        if !latest_ids.contains(&id) && now.saturating_sub(s.at) > keep_ms {
            let _ = std::fs::remove_file(manifest_path(&id)?);
            continue;
        }
        needed.extend(s.files.into_values().filter(|m| m.text).map(|m| m.hash));
    }
    let blobs = d.join("blobs");
    for sub in std::fs::read_dir(&blobs).into_iter().flatten().flatten() {
        for b in std::fs::read_dir(sub.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = b.file_name().to_string_lossy().to_string();
            // A text written in the last hour may belong to a snapshot being taken right now.
            let fresh = b
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .is_some_and(|t| now.saturating_sub(t.as_millis()) < 3_600_000);
            if !needed.contains(&name) && !fresh {
                let _ = std::fs::remove_file(b.path());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
