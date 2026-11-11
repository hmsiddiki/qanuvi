//! Code index: the definitions (functions, classes, methods, types) in a project's files, found with per-language
//! patterns. Used by "Go to symbol" in the editor and by the tools agents call instead of reading whole files.
//! Only changed files are read again (by size and modification time); ignored files are left out.

use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// Files larger than this are not indexed (minified bundles, generated code).
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
pub const MAX_INDEXED_FILES: usize = 50_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    pub name: String,
    /// function, method, class, interface, trait, enum, struct, type, module, constant
    pub kind: &'static str,
    /// 1-based line.
    pub line: usize,
    /// The class or type the symbol belongs to, when known.
    pub container: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub path: String,
    pub symbol: Symbol,
}

struct Pattern {
    re: Regex,
    kind: &'static str,
    /// Defines a container (class, impl…) for the symbols indented below it.
    container: bool,
}

fn p(re: &str, kind: &'static str, container: bool) -> Pattern {
    Pattern {
        re: Regex::new(re).expect("valid pattern"),
        kind,
        container,
    }
}

fn patterns(lang: &str) -> &'static [Pattern] {
    static CACHE: OnceLock<HashMap<&'static str, Vec<Pattern>>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        let mut m: HashMap<&'static str, Vec<Pattern>> = HashMap::new();
        m.insert(
            "php",
            vec![
                p(r"^\s*(?:(?:abstract|final|readonly)\s+)*class\s+([A-Za-z_]\w*)", "class", true),
                p(r"^\s*interface\s+([A-Za-z_]\w*)", "interface", true),
                p(r"^\s*trait\s+([A-Za-z_]\w*)", "trait", true),
                p(r"^\s*enum\s+([A-Za-z_]\w*)", "enum", true),
                p(
                    r"^\s+(?:(?:public|protected|private|static|abstract|final)\s+)*function\s+&?([A-Za-z_]\w*)\s*\(",
                    "method",
                    false,
                ),
                p(r"^function\s+&?([A-Za-z_]\w*)\s*\(", "function", false),
                p(r"^\s*(?:const|define\(\s*['\x22])\s*([A-Z_][A-Z0-9_]*)", "constant", false),
            ],
        );
        m.insert(
            "js",
            vec![
                p(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)", "class", true),
                p(r"^\s*(?:export\s+)?interface\s+([A-Za-z_$][\w$]*)", "interface", false),
                p(r"^\s*(?:export\s+)?type\s+([A-Za-z_$][\w$]*)\s*(?:<[^=]*>)?\s*=", "type", false),
                p(r"^\s*(?:export\s+)?(?:const\s+)?enum\s+([A-Za-z_$][\w$]*)", "enum", false),
                p(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][\w$]*)", "function", false),
                p(
                    r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=]+)?=\s*(?:async\s+)?(?:\([^)]*\)|[A-Za-z_$][\w$]*)\s*(?::[^=]+)?=>",
                    "function",
                    false,
                ),
                p(
                    r"^\s+(?:(?:public|private|protected|static|async|readonly|override|get|set)\s+)*#?([A-Za-z_$][\w$]*)\s*\([^)]*\)\s*(?::\s*[^{]+)?\{\s*$",
                    "method",
                    false,
                ),
            ],
        );
        m.insert(
            "rust",
            vec![
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Za-z_]\w*)", "struct", false),
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+([A-Za-z_]\w*)", "enum", false),
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+([A-Za-z_]\w*)", "trait", true),
                p(r"^\s*impl(?:<[^>]*>)?\s+(?:[A-Za-z_][\w:<>, ]*\s+for\s+)?([A-Za-z_]\w*)", "impl", true),
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_]\w*)", "module", false),
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?type\s+([A-Za-z_]\w*)", "type", false),
                p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Z_][A-Z0-9_]*)\s*:", "constant", false),
                p(
                    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+([A-Za-z_]\w*)",
                    "function",
                    false,
                ),
            ],
        );
        m.insert(
            "python",
            vec![
                p(r"^\s*class\s+([A-Za-z_]\w*)", "class", true),
                p(r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)", "function", false),
            ],
        );
        m.insert(
            "dart",
            vec![
                p(r"^\s*(?:abstract\s+|sealed\s+|base\s+|final\s+)*(?:class|mixin|extension)\s+([A-Za-z_]\w*)", "class", true),
                p(r"^\s*enum\s+([A-Za-z_]\w*)", "enum", false),
                p(
                    r"^\s*(?:static\s+|external\s+|@override\s+)*(?:Future<[^>]*>|[A-Za-z_][\w<>?, ]*)\s+([a-z_]\w*)\s*\([^;]*\)\s*(?:async\s*)?[{=]",
                    "function",
                    false,
                ),
            ],
        );
        m.insert(
            "csharp",
            vec![
                p(
                    r"^\s*(?:(?:public|private|protected|internal|static|abstract|sealed|partial)\s+)*(?:class|record|struct)\s+([A-Za-z_]\w*)",
                    "class",
                    true,
                ),
                p(r"^\s*(?:(?:public|private|protected|internal)\s+)*interface\s+([A-Za-z_]\w*)", "interface", true),
                p(r"^\s*(?:(?:public|private|protected|internal)\s+)*enum\s+([A-Za-z_]\w*)", "enum", false),
                p(
                    r"^\s*(?:(?:public|private|protected|internal|static|virtual|override|abstract|async|sealed)\s+)+[A-Za-z_][\w<>\[\],? ]*\s+([A-Za-z_]\w*)\s*\(",
                    "method",
                    false,
                ),
            ],
        );
        m.insert(
            "go",
            vec![
                p(r"^type\s+([A-Za-z_]\w*)\s+(?:struct|interface)", "type", false),
                p(r"^func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)\s*\(", "function", false),
            ],
        );
        m.insert(
            "java",
            vec![
                p(
                    r"^\s*(?:(?:public|private|protected|static|abstract|final|data|open|sealed)\s+)*(?:class|interface|object|enum)\s+([A-Za-z_]\w*)",
                    "class",
                    true,
                ),
                p(r"^\s*(?:(?:public|private|protected|override|suspend|inline)\s+)*fun\s+(?:<[^>]*>\s*)?([A-Za-z_]\w*)", "function", false),
                p(
                    r"^\s*(?:(?:public|private|protected|static|final|abstract|synchronized)\s+)+[A-Za-z_][\w<>\[\], ]*\s+([A-Za-z_]\w*)\s*\(",
                    "method",
                    false,
                ),
            ],
        );
        m
    });
    all.get(lang).map(|v| v.as_slice()).unwrap_or(&[])
}

/// The language whose patterns a file uses, from its name.
pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "php" | "phtml" => "php",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx" | "svelte" | "vue" => "js",
        "rs" => "rust",
        "py" | "pyw" => "python",
        "dart" => "dart",
        "cs" => "csharp",
        "go" => "go",
        "java" | "kt" | "kts" => "java",
        _ => return None,
    })
}

const KEYWORDS: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "return",
    "function",
    "else",
    "new",
    "await",
    "constructor_",
    "match",
];

/// The definitions in one file's text.
pub fn extract(lang: &str, text: &str) -> Vec<Symbol> {
    let pats = patterns(lang);
    let mut out = Vec::new();
    // Open containers: (indent, name). A symbol indented deeper than a container belongs to it.
    let mut containers: Vec<(usize, String)> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.len() > 400 {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("//")
            || trimmed.starts_with('#') && lang != "python"
            || trimmed.starts_with('*')
        {
            continue;
        }
        let indent = line.len() - trimmed.len();
        // A line at or left of a container closes it, except an opening brace on its own line (PHP, C# style).
        let closes = !trimmed.is_empty() && !trimmed.starts_with('{');
        while closes && containers.last().is_some_and(|(ind, _)| *ind >= indent) {
            containers.pop();
        }
        for pat in pats {
            let Some(c) = pat.re.captures(line) else {
                continue;
            };
            let name = c[1].to_string();
            if KEYWORDS.contains(&name.as_str()) {
                break;
            }
            let container = containers.last().map(|(_, n)| n.clone());
            let kind = if pat.kind == "function" && container.is_some() && lang != "rust" {
                "method"
            } else {
                pat.kind
            };
            out.push(Symbol {
                name: name.clone(),
                kind,
                line: i + 1,
                container,
            });
            if pat.container {
                containers.push((indent, name));
            }
            break;
        }
    }
    out
}

struct Entry {
    stamp: (u64, Option<SystemTime>),
    symbols: Vec<Symbol>,
}

/// The index of one project, kept between queries; refreshing only reads files whose size or time changed.
#[derive(Default)]
pub struct ProjectIndex {
    files: HashMap<String, Entry>,
}

impl ProjectIndex {
    pub fn refresh(&mut self, root: &Path) -> Result<(), String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let mut seen = std::collections::HashSet::new();
        let mut walker = WalkBuilder::new(&root);
        walker
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .require_git(false)
            .filter_entry(|e| {
                e.file_name() != ".git"
                    && e.file_name() != "node_modules"
                    && e.file_name() != "vendor"
            });
        for entry in walker.build().flatten() {
            if seen.len() >= MAX_INDEXED_FILES {
                break;
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let rel = rel(&root, entry.path());
            let Some(lang) = language_of(&rel) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() > MAX_INDEX_BYTES {
                continue;
            }
            seen.insert(rel.clone());
            let stamp = (meta.len(), meta.modified().ok());
            if self.files.get(&rel).is_some_and(|e| e.stamp == stamp) {
                continue;
            }
            let text = std::fs::read(entry.path())
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            self.files.insert(
                rel,
                Entry {
                    stamp,
                    symbols: extract(lang, &text),
                },
            );
        }
        self.files.retain(|k, _| seen.contains(k));
        Ok(())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.files.values().map(|e| e.symbols.len()).sum()
    }

    pub fn outline(&self, path: &str) -> Option<&[Symbol]> {
        self.files.get(path).map(|e| e.symbols.as_slice())
    }

    /// Symbols whose name matches: exact matches first, then names starting with the query, then names containing it
    /// (all case-insensitive). `kind` narrows the result.
    pub fn find(&self, query: &str, kind: Option<&str>, limit: usize) -> Vec<Found> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let mut hits: Vec<(u8, Found)> = Vec::new();
        for (path, e) in &self.files {
            for s in &e.symbols {
                if kind.is_some_and(|k| k != s.kind) {
                    continue;
                }
                let n = s.name.to_lowercase();
                let rank = if n == q {
                    0
                } else if n.starts_with(&q) {
                    1
                } else if n.contains(&q) {
                    2
                } else {
                    continue;
                };
                hits.push((
                    rank,
                    Found {
                        path: path.clone(),
                        symbol: s.clone(),
                    },
                ));
            }
        }
        hits.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(a.1.path.cmp(&b.1.path))
                .then(a.1.symbol.line.cmp(&b.1.symbol.line))
        });
        hits.into_iter().take(limit).map(|(_, f)| f).collect()
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// One index per project folder, shared by the app's commands.
#[derive(Default)]
pub struct Indexes {
    map: Mutex<HashMap<PathBuf, ProjectIndex>>,
}

impl Indexes {
    pub fn with<T>(&self, root: &Path, f: impl FnOnce(&ProjectIndex) -> T) -> Result<T, String> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let index = map.entry(root.to_path_buf()).or_default();
        index.refresh(root)?;
        Ok(f(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(lang: &str, src: &str) -> Vec<(String, &'static str, usize, Option<String>)> {
        extract(lang, src)
            .into_iter()
            .map(|s| (s.name, s.kind, s.line, s.container))
            .collect()
    }

    #[test]
    fn php_classes_methods_functions_and_constants() {
        let src = "<?php\nnamespace App;\nfinal class CartService\n{\n    public const MAX = 5;\n    public function addMany(array $x): void {}\n    private static function total() {}\n}\nfunction qcommerce_user_owns($id) {}\ndefine('QP_VERSION', '1');\n";
        assert_eq!(
            names("php", src),
            vec![
                ("CartService".into(), "class", 3, None),
                ("addMany".into(), "method", 6, Some("CartService".into())),
                ("total".into(), "method", 7, Some("CartService".into())),
                ("qcommerce_user_owns".into(), "function", 9, None),
                ("QP_VERSION".into(), "constant", 10, None),
            ]
        );
    }

    #[test]
    fn typescript_and_svelte_scripts() {
        let src = "export interface Step {\n  line: number;\n}\nexport type Mode = \"a\" | \"b\";\nexport function parsePlan(text: string): Plan {\n  if (x) {\n  }\n}\nconst fuzzy = (q: string) => 1;\nexport class RunnerStore {\n  async start(project: Project): Promise<void> {\n  }\n}\n";
        assert_eq!(
            names("js", src),
            vec![
                ("Step".into(), "interface", 1, None),
                ("Mode".into(), "type", 4, None),
                ("parsePlan".into(), "function", 5, None),
                ("fuzzy".into(), "function", 9, None),
                ("RunnerStore".into(), "class", 10, None),
                ("start".into(), "method", 11, Some("RunnerStore".into())),
            ]
        );
    }

    #[test]
    fn rust_items() {
        let src = "pub struct Db(Mutex<Connection>);\nimpl Db {\n    pub fn open(path: &Path) -> Self {}\n}\npub(crate) async fn run() {}\npub const MAX_X: usize = 1;\nmod tests {}\n";
        let found = names("rust", src);
        assert_eq!(found[0], ("Db".into(), "struct", 1, None));
        assert_eq!(found[1], ("Db".into(), "impl", 2, None));
        assert_eq!(found[2], ("open".into(), "function", 3, Some("Db".into())));
        assert!(found.contains(&("run".into(), "function", 5, None)));
        assert!(found.contains(&("MAX_X".into(), "constant", 6, None)));
        assert!(found.contains(&("tests".into(), "module", 7, None)));
    }

    #[test]
    fn python_dart_csharp_go_java() {
        assert_eq!(
            names(
                "python",
                "class Shop:\n    def total(self):\n        pass\ndef main():\n    pass\n"
            ),
            vec![
                ("Shop".into(), "class", 1, None),
                ("total".into(), "method", 2, Some("Shop".into())),
                ("main".into(), "function", 4, None),
            ]
        );
        let dart = names(
            "dart",
            "class CartPage extends StatelessWidget {\n  Widget build(BuildContext context) {\n  }\n}\n",
        );
        assert_eq!(
            dart[1],
            ("build".into(), "method", 2, Some("CartPage".into()))
        );
        let cs = names(
            "csharp",
            "public class HomeController : Controller\n{\n    public IActionResult Index()\n    {\n    }\n}\n",
        );
        assert_eq!(
            cs[1],
            ("Index".into(), "method", 3, Some("HomeController".into()))
        );
        assert_eq!(
            names("go", "func (s *Server) Start() error {\n}\n")[0].0,
            "Start"
        );
        assert_eq!(
            names("java", "fun main(args: Array<String>) {\n}\n")[0],
            ("main".into(), "function", 1, None)
        );
    }

    #[test]
    fn skips_comments_and_control_flow() {
        assert!(names("js", "// function hidden() {}\n  if (a) {\n  }\n").is_empty());
        assert!(names("php", " * function notReal() in a doc block\n").is_empty());
    }

    #[test]
    fn indexes_a_project_finds_symbols_and_refreshes_only_changes() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("node_modules/x")).unwrap();
        std::fs::write(
            d.join("src/cart.php"),
            "<?php\nclass Cart {\n    public function total() {}\n}\n",
        )
        .unwrap();
        std::fs::write(
            d.join("src/plan.ts"),
            "export function parsePlan() {}\nexport function planTotal() {}\n",
        )
        .unwrap();
        std::fs::write(d.join("node_modules/x/i.js"), "function total() {}\n").unwrap();
        std::fs::write(d.join("README.md"), "function readme() {}\n").unwrap();

        let mut idx = ProjectIndex::default();
        idx.refresh(d).unwrap();
        assert_eq!(idx.file_count(), 2);
        let hits = idx.find("total", None, 10);
        assert_eq!(
            hits.len(),
            2,
            "exact match and contains; node_modules ignored"
        );
        assert_eq!(hits[0].symbol.name, "total");
        assert_eq!(hits[0].path, "src/cart.php");
        assert_eq!(hits[1].symbol.name, "planTotal");
        assert_eq!(idx.find("plan", Some("function"), 10).len(), 2);
        assert_eq!(idx.outline("src/cart.php").unwrap().len(), 2);

        std::fs::write(d.join("src/plan.ts"), "export function renamed() {}\n").unwrap();
        std::fs::remove_file(d.join("src/cart.php")).unwrap();
        idx.refresh(d).unwrap();
        assert_eq!(idx.file_count(), 1);
        assert!(idx.find("parsePlan", None, 10).is_empty());
        assert_eq!(idx.find("renamed", None, 10).len(), 1);
    }
}
