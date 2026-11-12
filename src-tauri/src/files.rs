//! Reading, writing, listing and searching files inside a project. Every path is relative to the project folder and
//! is refused if it would lead outside it.

use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Serialize;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

/// Files bigger than this are not opened in the editor.
pub const MAX_EDIT_BYTES: u64 = 10 * 1024 * 1024;
/// Files bigger than this are skipped by project search.
pub const MAX_SEARCH_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_LISTED_FILES: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    /// Path from the project folder, always with `/`.
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Left out by .gitignore and similar files (shown greyed).
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileContent {
    pub path: String,
    pub text: String,
    /// Fingerprint of the bytes on disk, used to notice changes made by someone else before saving.
    pub hash: String,
    pub size: u64,
    pub binary: bool,
    pub too_large: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchMatch {
    pub path: String,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column of the match, in characters.
    pub column: usize,
    /// The line, cut to 300 characters around the match.
    pub text: String,
    /// Where the matches are in `text`, in characters: [start, end).
    pub spans: Vec<[usize; 2]>,
    /// How many matches the line has.
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub files_searched: usize,
    pub truncated: bool,
}

pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}-{}", h.finish(), bytes.len())
}

fn rel_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Turns a project-relative path into a real path inside the project, or refuses it.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    for c in rel_path.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(format!("The path {rel} is outside the project.")),
        }
    }
    let root = root
        .canonicalize()
        .map_err(|_| "The project folder cannot be found.".to_string())?;
    let joined = root.join(rel_path);
    // Follow links: a link inside the project that points outside it is refused too.
    let check = if joined.exists() {
        joined.canonicalize().map_err(|e| e.to_string())?
    } else {
        let parent = joined.parent().ok_or("Invalid path.")?;
        let parent = parent
            .canonicalize()
            .map_err(|_| format!("The folder for {rel} does not exist."))?;
        parent.join(joined.file_name().ok_or("Invalid path.")?)
    };
    if !check.starts_with(&root) {
        return Err(format!("The path {rel} is outside the project."));
    }
    Ok(joined)
}

fn walker(dir: &Path) -> WalkBuilder {
    let mut b = WalkBuilder::new(dir);
    b.hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .ignore(true)
        .parents(true)
        .filter_entry(|e| e.file_name() != ".git");
    b
}

/// Folders never looked into for nested repositories (big and never a project of their own).
const SKIP_FOR_REPOS: &[&str] = &["node_modules", "vendor", "target", ".git", "dist", "build"];

/// Git repositories inside the project that the project's own ignore rules leave out (for example a plugin with its
/// own repository in a folder the parent ignores). Their files belong to the user's work too, so they are walked with
/// their own rules, as VS Code finds nested repositories.
fn hidden_repos(root: &Path) -> Vec<PathBuf> {
    let mut seen_dirs = std::collections::HashSet::new();
    for e in walker(root).build().flatten() {
        if e.file_type().is_some_and(|t| t.is_dir()) {
            seen_dirs.insert(e.path().to_path_buf());
        }
    }
    let mut repos = Vec::new();
    let mut b = WalkBuilder::new(root);
    b.standard_filters(false)
        .max_depth(Some(6))
        .filter_entry(|e| {
            !SKIP_FOR_REPOS.iter().any(|s| e.file_name() == *s) || e.file_name() == ".git"
        });
    for e in b.build().flatten() {
        if e.file_name() == ".git"
            && let Some(repo) = e.path().parent()
            && repo != root
            && !seen_dirs.contains(repo)
            && !repos.iter().any(|r: &PathBuf| repo.starts_with(r))
        {
            repos.push(repo.to_path_buf());
        }
    }
    repos
}

/// Every Markdown file in the project, ignore rules or not (a plan is a plan in any folder, a plugin's too), leaving
/// out only bulky folders. The Plan view picks the task lists among them.
pub fn markdown_files(root: &Path) -> Result<Vec<String>, String> {
    const MAX: usize = 20_000;
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let mut b = WalkBuilder::new(&root_c);
    b.standard_filters(false)
        .max_depth(Some(10))
        .filter_entry(|e| !SKIP_FOR_REPOS.iter().any(|s| e.file_name() == *s));
    let mut out = Vec::new();
    for e in b.build().flatten() {
        if e.file_type().is_some_and(|t| t.is_file())
            && e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("md"))
        {
            out.push(rel_string(&root_c, e.path()));
            if out.len() >= MAX {
                break;
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Every file Qanuvi works with: the project's own (by its ignore rules), then those of hidden nested repositories.
fn walk_files(root: &Path) -> impl Iterator<Item = ignore::DirEntry> {
    std::iter::once(root.to_path_buf())
        .chain(hidden_repos(root))
        .flat_map(|dir| walker(&dir).build().flatten())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
}

/// The entries of one folder: folders first, then files, by name. Ignored entries are listed too (greyed in the tree,
/// as in VS Code), except the `.git` folder.
pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = resolve(root, rel)?;
    if !dir.is_dir() {
        return Err(format!("{rel} is not a folder."));
    }
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let dir_c = dir.canonicalize().map_err(|e| e.to_string())?;
    let kept: std::collections::HashSet<std::ffi::OsString> = walker(&dir_c)
        .max_depth(Some(1))
        .build()
        .flatten()
        .filter(|e| e.depth() == 1)
        .map(|e| e.file_name().to_os_string())
        .collect();
    let mut entries: Vec<Entry> = std::fs::read_dir(&dir_c)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_name() != ".git")
        .map(|e| {
            let meta = e.metadata().ok();
            Entry {
                name: e.file_name().to_string_lossy().to_string(),
                path: rel_string(&root_c, &e.path()),
                is_dir: e.file_type().is_ok_and(|t| t.is_dir()),
                size: meta.map(|m| m.len()).unwrap_or(0),
                ignored: !kept.contains(&e.file_name()),
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

/// Every file in the project that is not ignored, for "Go to file".
pub fn list_files(root: &Path) -> Result<(Vec<String>, bool), String> {
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in walk_files(&root_c) {
        if files.len() >= MAX_LISTED_FILES {
            return Ok((files, true));
        }
        files.push(rel_string(&root_c, entry.path()));
    }
    files.sort();
    Ok((files, false))
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

pub fn read_file(root: &Path, rel: &str) -> Result<FileContent, String> {
    let path = resolve(root, rel)?;
    let meta = std::fs::metadata(&path).map_err(|_| format!("{rel} cannot be found."))?;
    if meta.is_dir() {
        return Err(format!("{rel} is a folder."));
    }
    let size = meta.len();
    if size > MAX_EDIT_BYTES {
        return Ok(FileContent {
            path: rel.into(),
            text: String::new(),
            hash: String::new(),
            size,
            binary: false,
            too_large: true,
        });
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let binary = looks_binary(&bytes);
    Ok(FileContent {
        path: rel.into(),
        text: if binary {
            String::new()
        } else {
            String::from_utf8_lossy(&bytes).to_string()
        },
        hash: fingerprint(&bytes),
        size,
        binary,
        too_large: false,
    })
}

/// Saves a file. If `expected_hash` is given and the file on disk changed since it was read, nothing is written.
/// The new content is written to a temporary file first and then moved into place, so a crash never leaves half a
/// file.
pub fn write_file(
    root: &Path,
    rel: &str,
    text: &str,
    expected_hash: Option<&str>,
) -> Result<String, String> {
    let path = resolve(root, rel)?;
    write_path(&path, rel, text, expected_hash)
}

/// Files in the project modified at or after \`since\` (what a step changed), Qanuvi's own folder left out; at most
/// \`limit\`. Works for folders inside other repositories too, where git cannot tell.
pub fn changed_since(
    root: &Path,
    since: std::time::SystemTime,
    limit: usize,
) -> Result<Vec<String>, String> {
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for e in walk_files(&root_c) {
        let rel = rel_string(&root_c, e.path());
        if rel.starts_with(".qanuvi/") {
            continue;
        }
        if e.metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .is_some_and(|t| t >= since)
        {
            out.push(rel);
            if out.len() >= limit {
                break;
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Where plan runs keep a log per step (attempts, failures, results): any model in any later session can read them.
pub const PLAN_LOGS: &str = ".qanuvi/plan-logs/";

/// Adds to a step's log (made with its folders when new). Only under .qanuvi/plan-logs; .qanuvi gets a .gitignore so
/// the logs never go into the project's history.
pub fn log_append(root: &Path, rel: &str, text: &str) -> Result<(), String> {
    let rel = rel.replace('\\', "/");
    if !rel.starts_with(PLAN_LOGS) || rel.contains("..") || !rel.ends_with(".md") {
        return Err("A step log lives under .qanuvi/plan-logs and ends in .md.".into());
    }
    // The folders first (only plain names are allowed above), then the usual check that it stays in the project.
    if let Some(dir) = Path::new(&rel).parent() {
        std::fs::create_dir_all(root.join(dir)).map_err(|e| e.to_string())?;
    }
    let path = resolve(root, &rel)?;
    let ignore = root.join(".qanuvi").join(".gitignore");
    if !ignore.exists() {
        let _ = std::fs::write(
            &ignore,
            "# Qanuvi's own files (plan logs): not part of the project.\n*\n",
        );
    }
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{rel}: {e}"))?;
    f.write_all(text.as_bytes())
        .map_err(|e| format!("{rel}: {e}"))
}

/// Makes a new empty file or a folder inside the project (the project panel's New file / New folder). Something
/// already there with that name is never replaced.
pub fn create(root: &Path, rel: &str, folder: bool) -> Result<(), String> {
    let rel = rel.trim().trim_matches('/');
    if rel.is_empty() {
        return Err("Give a name.".into());
    }
    let path = resolve(root, rel)?;
    if path.exists() {
        return Err(format!("{rel} exists already."));
    }
    if folder {
        std::fs::create_dir(&path).map_err(|e| format!("{rel}: {e}"))
    } else {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map(|_| ())
            .map_err(|e| format!("{rel}: {e}"))
    }
}

/// Renames or moves a file or folder inside the project (right-click → Rename). Never replaces something already
/// there.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let (from, to) = (from.trim().trim_matches('/'), to.trim().trim_matches('/'));
    if from.is_empty() || to.is_empty() {
        return Err("Give a name.".into());
    }
    let a = resolve(root, from)?;
    if !a.exists() {
        return Err(format!("{from} is not there any more."));
    }
    let b = resolve(root, to)?;
    if b.exists() {
        return Err(format!("{to} exists already."));
    }
    std::fs::rename(&a, &b).map_err(|e| format!("{from}: {e}"))
}

/// Moves a file or folder of the project to the system's Recycle Bin (Trash), where it can be restored.
pub fn trash(root: &Path, rel: &str) -> Result<(), String> {
    let rel = rel.trim().trim_matches('/');
    if rel.is_empty() || rel == "." {
        return Err("The project folder itself is not deleted from here.".into());
    }
    let p = resolve(root, rel)?;
    if !p.exists() {
        return Err(format!("{rel} is not there any more."));
    }
    trash::delete(&p).map_err(|e| format!("{rel} could not be moved to the Recycle Bin: {e}"))
}

/// A folder inside the project to start a terminal in (right-click → Open in terminal); the project root otherwise.
pub fn folder_in(root: &Path, rel: Option<&str>) -> Result<std::path::PathBuf, String> {
    match rel
        .map(|r| r.trim().trim_matches('/'))
        .filter(|r| !r.is_empty())
    {
        Some(r) => {
            let p = resolve(root, r)?;
            if p.is_dir() {
                Ok(p)
            } else {
                Err(format!("{r} is not a folder."))
            }
        }
        None => Ok(root.to_path_buf()),
    }
}

/// Saves to a path that was already checked by the caller (see `write_file` for the rules).
pub fn write_path(
    path: &Path,
    label: &str,
    text: &str,
    expected_hash: Option<&str>,
) -> Result<String, String> {
    let rel = label;
    if let Some(expected) = expected_hash.filter(|h| !h.is_empty())
        && let Ok(current) = std::fs::read(path)
        && fingerprint(&current) != expected
    {
        return Err(format!(
            "{rel} was changed on disk after it was opened. Reload it or save again to overwrite."
        ));
    }
    let dir = path.parent().ok_or("Invalid path.")?;
    let tmp = dir.join(format!(
        ".{}.qanuvi-{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.to_string());
    }
    Ok(fingerprint(text.as_bytes()))
}

/// How to search (15k): the query's kind, and which files take part.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    /// Only whole words match ("cart" not in "carton").
    pub whole_word: bool,
    /// Files to include, comma-separated globs ("src/**, *.php"); empty: all.
    pub include: String,
    /// Files to leave out, comma-separated globs ("vendor, *.min.js").
    pub exclude: String,
}

/// Comma-separated globs as people write them: "*.php" and "vendor" match at any depth, a folder matches what is in it.
fn glob_set(list: &str) -> Result<Option<globset::GlobSet>, String> {
    let mut b = globset::GlobSetBuilder::new();
    let mut any = false;
    for raw in list.split(',') {
        let p = raw.trim().trim_start_matches("./").replace('\\', "/");
        if p.is_empty() {
            continue;
        }
        let p = p.trim_end_matches('/');
        let base = if let Some(rooted) = p.strip_prefix('/') {
            rooted.to_string()
        } else if p.starts_with("**/") {
            p.to_string()
        } else {
            format!("**/{p}")
        };
        for g in [base.clone(), format!("{base}/**")] {
            b.add(
                globset::GlobBuilder::new(&g)
                    .literal_separator(true)
                    .case_insensitive(cfg!(windows))
                    .build()
                    .map_err(|e| format!("The pattern {p} is not valid: {e}"))?,
            );
        }
        any = true;
    }
    if !any {
        return Ok(None);
    }
    b.build().map(Some).map_err(|e| e.to_string())
}

/// The compiled search: the pattern and the file filters.
struct Searcher {
    re: regex::Regex,
    include: Option<globset::GlobSet>,
    exclude: Option<globset::GlobSet>,
}

impl Searcher {
    fn new(query: &str, o: &SearchOptions) -> Result<Self, String> {
        let mut pattern = if o.regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        if o.whole_word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        let re = RegexBuilder::new(&pattern)
            .case_insensitive(!o.case_sensitive)
            .size_limit(1 << 20)
            .build()
            .map_err(|e| format!("The search pattern is not valid: {e}"))?;
        Ok(Searcher {
            re,
            include: glob_set(&o.include)?,
            exclude: glob_set(&o.exclude)?,
        })
    }

    fn wants(&self, rel: &str) -> bool {
        self.include.as_ref().is_none_or(|g| g.is_match(rel))
            && !self.exclude.as_ref().is_some_and(|g| g.is_match(rel))
    }
}

/// A file's text, when it is a searchable text file.
fn text_of(path: &Path) -> Option<String> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) > MAX_SEARCH_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if looks_binary(&bytes) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// One line's result: the text around the matches (at most 300 characters) and where each match is in it.
fn line_hit(line: &str, found: &[(usize, usize)]) -> (String, usize, Vec<[usize; 2]>) {
    let col = |b: usize| line[..b].chars().count();
    let column = col(found[0].0) + 1;
    let total = line.chars().count();
    let from = if total <= 300 {
        0
    } else {
        (column - 1).saturating_sub(100)
    };
    let text: String = line.chars().skip(from).take(300).collect();
    let shown = text.chars().count();
    let spans = found
        .iter()
        .map(|&(a, b)| [col(a).saturating_sub(from), col(b).saturating_sub(from)])
        .filter(|[a, _]| *a < shown)
        .map(|[a, b]| [a, b.min(shown)])
        .collect();
    let text = if from == 0 {
        text.trim_end().to_string()
    } else {
        text
    };
    (text, column, spans)
}

/// Searches every non-ignored text file. `regex` treats the query as a regular expression; otherwise it is plain
/// text.
pub fn search(
    root: &Path,
    query: &str,
    regex: bool,
    case_sensitive: bool,
    limit: usize,
) -> Result<SearchResult, String> {
    let o = SearchOptions {
        regex,
        case_sensitive,
        ..Default::default()
    };
    search_with(root, query, &o, limit)
}

/// Searches with all options: whole words, files to include and leave out. Each matching line comes once, with
/// every match in it marked.
pub fn search_with(
    root: &Path,
    query: &str,
    o: &SearchOptions,
    limit: usize,
) -> Result<SearchResult, String> {
    let mut result = SearchResult {
        matches: vec![],
        files_searched: 0,
        truncated: false,
    };
    if query.is_empty() {
        return Ok(result);
    }
    let s = Searcher::new(query, o)?;
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    for entry in walk_files(&root_c) {
        let rel = rel_string(&root_c, entry.path());
        if !s.wants(&rel) {
            continue;
        }
        let Some(text) = text_of(entry.path()) else {
            continue;
        };
        result.files_searched += 1;
        for (i, line) in text.lines().enumerate() {
            let found: Vec<(usize, usize)> =
                s.re.find_iter(line)
                    .filter(|m| m.end() > m.start())
                    .map(|m| (m.start(), m.end()))
                    .collect();
            if found.is_empty() {
                continue;
            }
            let (text, column, spans) = line_hit(line, &found);
            result.matches.push(SearchMatch {
                path: rel.clone(),
                line: i + 1,
                column,
                text,
                spans,
                count: found.len(),
            });
            if result.matches.len() >= limit {
                result.truncated = true;
                return Ok(result);
            }
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplaceResult {
    pub files: usize,
    pub replacements: usize,
}

/// Replaces every match in the files searched with the same options (only in `paths` when given). A regular
/// expression's replacement may use $1 and ${name}; plain text is used as it is. Files keep their line endings.
pub fn replace_with(
    root: &Path,
    query: &str,
    o: &SearchOptions,
    replacement: &str,
    paths: Option<&[String]>,
) -> Result<ReplaceResult, String> {
    let mut done = ReplaceResult {
        files: 0,
        replacements: 0,
    };
    if query.is_empty() {
        return Ok(done);
    }
    let s = Searcher::new(query, o)?;
    let root_c = root.canonicalize().map_err(|e| e.to_string())?;
    for entry in walk_files(&root_c) {
        let rel = rel_string(&root_c, entry.path());
        if !s.wants(&rel) || paths.is_some_and(|p| !p.iter().any(|x| x == &rel)) {
            continue;
        }
        // Only files that are valid UTF-8, so nothing else in them changes.
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.len() as u64 > MAX_SEARCH_BYTES || looks_binary(&bytes) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let n =
            s.re.find_iter(&text)
                .filter(|m| m.end() > m.start())
                .count();
        if n == 0 {
            continue;
        }
        let next = if o.regex {
            s.re.replace_all(&text, replacement)
        } else {
            s.re.replace_all(&text, regex::NoExpand(replacement))
        };
        std::fs::write(entry.path(), next.as_bytes()).map_err(|e| format!("{rel}: {e}"))?;
        done.files += 1;
        done.replacements += n;
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        fs::create_dir_all(p.join("src/inner")).unwrap();
        fs::create_dir_all(p.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(p.join(".git")).unwrap();
        fs::write(p.join(".gitignore"), "node_modules/\n*.log\n").unwrap();
        fs::write(
            p.join("src/main.php"),
            "<?php\necho 'Hello';\n$price = 100;\n",
        )
        .unwrap();
        fs::write(
            p.join("src/inner/Cart.php"),
            "class Cart { function price() {} }\n",
        )
        .unwrap();
        fs::write(p.join("README.md"), "Price list\n").unwrap();
        fs::write(p.join("debug.log"), "price price\n").unwrap();
        fs::write(p.join("node_modules/pkg/index.js"), "price\n").unwrap();
        fs::write(p.join("image.bin"), [0u8, 1, 2, 3, 0, 5]).unwrap();
        fs::write(p.join(".git/config"), "x").unwrap();
        dir
    }

    #[test]
    fn refuses_paths_outside_the_project() {
        let dir = project();
        assert!(resolve(dir.path(), "../outside.txt").is_err());
        assert!(resolve(dir.path(), "src/../../outside.txt").is_err());
        assert!(resolve(dir.path(), "/etc/passwd").is_err());
        // A drive path is only "outside" on Windows; elsewhere the same text is an ordinary (odd) file name.
        #[cfg(windows)]
        assert!(resolve(dir.path(), r"C:\Windows\win.ini").is_err());
        assert!(resolve(dir.path(), "src/main.php").is_ok());
        assert!(
            resolve(dir.path(), "src/new-file.txt").is_ok(),
            "a new file in an existing folder is allowed"
        );
    }

    #[test]
    fn lists_a_folder_with_ignored_entries_marked_folders_first() {
        let dir = project();
        let names: Vec<(String, bool)> = list_dir(dir.path(), "")
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.ignored))
            .collect();
        let want = [
            ("node_modules", true),
            ("src", false),
            (".gitignore", false),
            ("debug.log", true),
            ("image.bin", false),
            ("README.md", false),
        ];
        assert_eq!(
            names,
            want.iter()
                .map(|(n, i)| (n.to_string(), *i))
                .collect::<Vec<_>>(),
            ".git is never listed"
        );
        let inner = list_dir(dir.path(), "src").unwrap();
        assert_eq!(inner[0].path, "src/inner");
        assert!(inner[0].is_dir);
        assert_eq!(inner[1].path, "src/main.php");
    }

    #[test]
    fn lists_all_files_for_go_to_file() {
        let dir = project();
        let (files, truncated) = list_files(dir.path()).unwrap();
        assert!(!truncated);
        assert_eq!(
            files,
            vec![
                ".gitignore",
                "README.md",
                "image.bin",
                "src/inner/Cart.php",
                "src/main.php"
            ]
        );
    }

    #[test]
    fn reads_text_and_recognises_binary_files() {
        let dir = project();
        let text = read_file(dir.path(), "src/main.php").unwrap();
        assert!(text.text.contains("Hello"));
        assert!(!text.binary);
        assert_eq!(text.hash, fingerprint(text.text.as_bytes()));
        let bin = read_file(dir.path(), "image.bin").unwrap();
        assert!(bin.binary);
        assert!(bin.text.is_empty());
        assert!(read_file(dir.path(), "src").is_err());
    }

    #[test]
    fn saves_and_refuses_to_overwrite_a_file_changed_on_disk() {
        let dir = project();
        let opened = read_file(dir.path(), "src/main.php").unwrap();
        let new_hash = write_file(
            dir.path(),
            "src/main.php",
            "<?php\n// edited\n",
            Some(&opened.hash),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.php")).unwrap(),
            "<?php\n// edited\n"
        );

        // Someone else changes the file; saving with the old fingerprint is refused.
        fs::write(dir.path().join("src/main.php"), "changed elsewhere").unwrap();
        let err = write_file(dir.path(), "src/main.php", "mine", Some(&new_hash)).unwrap_err();
        assert!(err.contains("changed on disk"));
        assert_eq!(
            fs::read_to_string(dir.path().join("src/main.php")).unwrap(),
            "changed elsewhere"
        );

        // Saving without a fingerprint overwrites, and no temporary file is left behind.
        write_file(dir.path(), "src/main.php", "mine", None).unwrap();
        let leftovers = fs::read_dir(dir.path().join("src"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        assert!(write_file(dir.path(), "../escape.txt", "x", None).is_err());
    }

    #[test]
    fn searches_text_files_only_and_respects_gitignore() {
        let dir = project();
        let result = search(dir.path(), "price", false, false, 100).unwrap();
        let paths: Vec<&str> = result.matches.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths.len(), 3, "{paths:?}");
        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"src/main.php"));
        assert!(paths.contains(&"src/inner/Cart.php"));
        let readme = result
            .matches
            .iter()
            .find(|m| m.path == "README.md")
            .unwrap();
        assert_eq!((readme.line, readme.column), (1, 1));
    }

    #[test]
    fn search_options_case_regex_and_limit() {
        let dir = project();
        assert_eq!(
            search(dir.path(), "Price", false, true, 100)
                .unwrap()
                .matches
                .len(),
            1
        );
        let re = search(dir.path(), r"\$price\s*=", true, false, 100).unwrap();
        assert_eq!(re.matches.len(), 1);
        assert_eq!(re.matches[0].line, 3);
        let limited = search(dir.path(), "price", false, false, 1).unwrap();
        assert!(limited.truncated);
        assert_eq!(limited.matches.len(), 1);
        assert!(search(dir.path(), "(", true, false, 10).is_err());
        assert!(
            search(dir.path(), "", false, false, 10)
                .unwrap()
                .matches
                .is_empty()
        );
    }
}

#[cfg(test)]
mod create_tests {
    use super::*;

    #[test]
    fn makes_files_and_folders_inside_the_project_only() {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), "src", true).unwrap();
        create(dir.path(), "src/new.ts", false).unwrap();
        assert!(dir.path().join("src/new.ts").is_file());
        assert!(
            create(dir.path(), "src/new.ts", false)
                .unwrap_err()
                .contains("exists")
        );
        assert!(create(dir.path(), "../outside.txt", false).is_err());
        assert!(create(dir.path(), "missing/x.txt", false).is_err());
        assert!(create(dir.path(), "  ", false).is_err());
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn renames_inside_the_project_without_replacing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(
            rename(dir.path(), "a.txt", "b.txt")
                .unwrap_err()
                .contains("exists")
        );
        rename(dir.path(), "a.txt", "sub/c.txt").unwrap();
        assert!(dir.path().join("sub/c.txt").is_file());
        assert!(rename(dir.path(), "b.txt", "../out.txt").is_err());
        assert!(rename(dir.path(), "gone.txt", "x.txt").is_err());
        assert!(trash(dir.path(), "").is_err());
        assert!(trash(dir.path(), "../x").is_err());
        assert_eq!(
            folder_in(dir.path(), Some("sub")).unwrap(),
            resolve(dir.path(), "sub").unwrap()
        );
        assert!(folder_in(dir.path(), Some("b.txt")).is_err());
        assert_eq!(folder_in(dir.path(), None).unwrap(), dir.path());
    }

    #[test]
    fn a_nested_repository_the_parent_ignores_is_listed_and_searched_with_its_own_rules() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join(".gitignore"), "/plugins/*\n").unwrap();
        std::fs::write(p.join("index.php"), "core").unwrap();
        let plugin = p.join("plugins/shop");
        std::fs::create_dir_all(plugin.join(".git")).unwrap();
        std::fs::create_dir_all(plugin.join("cache")).unwrap();
        std::fs::write(plugin.join(".gitignore"), "cache/\n").unwrap();
        std::fs::write(plugin.join("PLAN-D.md"), "- [ ] **1. Step** amirul").unwrap();
        std::fs::write(plugin.join("cache/x.md"), "amirul").unwrap();
        std::fs::create_dir_all(p.join("plugins/other")).unwrap();
        std::fs::write(p.join("plugins/other/notes.md"), "amirul").unwrap();

        let (files, _) = list_files(p).unwrap();
        assert!(
            files.contains(&"plugins/shop/PLAN-D.md".to_string()),
            "{files:?}"
        );
        assert!(files.contains(&"plugins/shop/.gitignore".to_string()));
        assert!(
            !files.iter().any(|f| f.contains("cache/")),
            "the repository's own rules apply"
        );
        assert!(
            !files.iter().any(|f| f.contains("plugins/other")),
            "an ignored folder that is no repository stays out"
        );
        assert!(files.contains(&"index.php".to_string()));

        let found = search(p, "amirul", false, false, 50).unwrap();
        let paths: Vec<_> = found.matches.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(paths, ["plugins/shop/PLAN-D.md"]);
    }

    #[test]
    fn markdown_files_are_found_in_ignored_folders_with_or_without_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join(".gitignore"), "/plugins/*\n").unwrap();
        std::fs::create_dir_all(p.join("plugins/shop")).unwrap();
        std::fs::write(p.join("plugins/shop/PLAN-D.md"), "x").unwrap();
        std::fs::create_dir_all(p.join("node_modules/lib")).unwrap();
        std::fs::write(p.join("node_modules/lib/TODO.md"), "x").unwrap();
        std::fs::write(p.join("README.md"), "x").unwrap();
        std::fs::write(p.join("index.php"), "x").unwrap();
        assert_eq!(
            markdown_files(p).unwrap(),
            ["README.md", "plugins/shop/PLAN-D.md"]
        );
    }

    #[test]
    fn whole_words_include_exclude_spans_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::create_dir_all(p.join("vendor/lib")).unwrap();
        std::fs::write(p.join("src/cart.php"), "cart carton cart\r\nno\r\n").unwrap();
        std::fs::write(p.join("src/notes.md"), "cart").unwrap();
        std::fs::write(p.join("vendor/lib/x.php"), "cart").unwrap();
        let o = |w: bool, inc: &str, exc: &str| SearchOptions {
            whole_word: w,
            include: inc.into(),
            exclude: exc.into(),
            ..Default::default()
        };

        let r = search_with(p, "cart", &o(true, "*.php", "vendor"), 50).unwrap();
        assert_eq!(r.matches.len(), 1, "{:?}", r.matches);
        let m = &r.matches[0];
        assert_eq!((m.path.as_str(), m.line, m.count), ("src/cart.php", 1, 2));
        assert_eq!(m.spans, vec![[0, 4], [12, 16]]);
        assert_eq!(
            search_with(p, "cart", &o(false, "", ""), 50)
                .unwrap()
                .matches
                .len(),
            3
        );
        assert_eq!(
            search_with(p, "cart", &o(false, "src/**", "*.md"), 50)
                .unwrap()
                .matches
                .len(),
            1
        );
        assert!(search_with(p, "x", &o(false, "[", ""), 50).is_err());

        let done = replace_with(p, "cart", &o(true, "", "vendor"), "basket", None).unwrap();
        assert_eq!((done.files, done.replacements), (2, 3));
        assert_eq!(
            std::fs::read_to_string(p.join("src/cart.php")).unwrap(),
            "basket carton basket\r\nno\r\n"
        );
        assert_eq!(
            std::fs::read_to_string(p.join("vendor/lib/x.php")).unwrap(),
            "cart"
        );

        let re = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let only = vec!["src/notes.md".to_string()];
        let done = replace_with(p, "(bask)et", &re, "$1-it $$", Some(&only)).unwrap();
        assert_eq!(done.files, 1);
        assert_eq!(
            std::fs::read_to_string(p.join("src/notes.md")).unwrap(),
            "bask-it $"
        );
        let plain = SearchOptions::default();
        replace_with(p, "bask-it", &plain, "$1", Some(&only)).unwrap();
        assert_eq!(
            std::fs::read_to_string(p.join("src/notes.md")).unwrap(),
            "$1 $"
        );
    }

    #[test]
    fn step_logs_append_under_their_folder_and_stay_out_of_git() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        log_append(p, ".qanuvi/plan-logs/PLAN-D/82b.md", "one\n").unwrap();
        log_append(p, ".qanuvi/plan-logs/PLAN-D/82b.md", "two\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(p.join(".qanuvi/plan-logs/PLAN-D/82b.md")).unwrap(),
            "one\ntwo\n"
        );
        assert!(
            std::fs::read_to_string(p.join(".qanuvi/.gitignore"))
                .unwrap()
                .contains('*')
        );
        assert!(log_append(p, "src/x.md", "no").is_err());
        assert!(log_append(p, ".qanuvi/plan-logs/../../x.md", "no").is_err());
        assert!(log_append(p, ".qanuvi/plan-logs/a.txt", "no").is_err());
    }

    #[test]
    fn finds_the_files_a_step_changed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("old.php"), "a").unwrap();
        let since = std::time::SystemTime::now();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("src/new.php"), "b").unwrap();
        log_append(p, ".qanuvi/plan-logs/P/1.md", "x").unwrap();
        assert_eq!(changed_since(p, since, 50).unwrap(), ["src/new.php"]);
    }
}
