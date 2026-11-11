//! Qanuvi as a small MCP server (`qanuvi --mcp <project folder>`): read-only tools an agent calls to find code without
//! reading whole files. Every answer is short, plain text. Speaks JSON-RPC 2.0, one message per line, on stdin/stdout.

use crate::files;
use crate::index::ProjectIndex;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

pub const SERVER_NAME: &str = "qanuvi";
const PROTOCOL: &str = "2025-06-18";
const MAX_READ_LINES: usize = 300;

pub struct Server {
    root: PathBuf,
    index: ProjectIndex,
    /// The user's memory and the project it is searched for.
    memory: Option<(crate::store::Db, String)>,
}

/// The code-index tools (all read-only).
pub const INDEX_TOOLS: [&str; 5] = [
    "project_map",
    "find_symbol",
    "outline",
    "read_lines",
    "search",
];

/// The code-index tools in the shape OpenAI-compatible models take (for Qanuvi's own agent, which calls them
/// directly instead of through MCP).
pub fn index_tool_definitions() -> Value {
    Value::Array(
        tools()
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| {
                json!({"type": "function", "function": {
                    "name": t["name"], "description": t["description"], "parameters": t["inputSchema"]
                }})
            })
            .collect(),
    )
}

fn tools() -> Value {
    json!([
        {
            "name": "project_map",
            "description": "Folders and files of the project with how many definitions each has, to get oriented without listing everything. Ignored files are left out.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "Folder to start from, relative to the project. Default: the project folder."},
                "depth": {"type": "integer", "minimum": 1, "maximum": 4, "description": "How many folder levels to show (default 2)."}
            }}
        },
        {
            "name": "find_symbol",
            "description": "Find where functions, methods, classes and types are defined, by name (exact, prefix or part of the name; case-insensitive). Answers path:line for each, so you can read just those lines.",
            "inputSchema": {"type": "object", "properties": {
                "name": {"type": "string"},
                "kind": {"type": "string", "description": "Optional: function, method, class, interface, trait, enum, struct, type, module, constant, impl."}
            }, "required": ["name"]}
        },
        {
            "name": "outline",
            "description": "The definitions in one file with their line numbers, instead of reading the whole file.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
        },
        {
            "name": "read_lines",
            "description": "Read a range of lines of a file (at most 300), with line numbers.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string"},
                "start": {"type": "integer", "minimum": 1},
                "end": {"type": "integer", "minimum": 1}
            }, "required": ["path", "start", "end"]}
        },
        {
            "name": "search",
            "description": "Search the text of all project files (ignored files left out). Answers path:line: text, at most 60 results.",
            "inputSchema": {"type": "object", "properties": {
                "query": {"type": "string"},
                "regex": {"type": "boolean"},
                "case_sensitive": {"type": "boolean"}
            }, "required": ["query"]}
        }
    ])
}

fn memory_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "memory_search",
            "description": "Search the user's memory for this project (rules, style, facts, decisions, past solutions, approved examples). The most relevant entries already came with the task; use this when you need more.",
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}
        }),
        json!({
            "name": "memory_suggest",
            "description": "Suggest a lasting entry for the user's memory: a decision made, a fact learned, a solution worth reusing. The user approves it first. Keep it short and specific.",
            "inputSchema": {"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["fact", "decision", "solution", "rule", "style", "example"]},
                "title": {"type": "string"},
                "body": {"type": "string"},
                "all_projects": {"type": "boolean", "description": "For every project, not just this one."}
            }, "required": ["kind", "title", "body"]}
        }),
    ]
}

impl Server {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            index: ProjectIndex::default(),
            memory: None,
        }
    }

    pub fn with_memory(mut self, db: crate::store::Db, scope: &str) -> Self {
        self.memory = Some((db, scope.to_string()));
        self
    }

    /// Answers one JSON-RPC message; notifications get no answer.
    #[cfg(test)]
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        answer(self, msg)
    }

    /// Runs one tool and answers its text (Qanuvi's own agent calls the tools this way).
    pub fn call(&mut self, name: &str, args: &Value) -> Result<String, String> {
        self.tool(name, args)
    }

    fn run(&mut self, name: &str, args: &Value) -> Value {
        text_result(self.tool(name, args))
    }

    fn tool(&mut self, name: &str, args: &Value) -> Result<String, String> {
        let s = |k: &str| args[k].as_str().unwrap_or("").trim().to_string();
        match name {
            "find_symbol" => {
                self.index.refresh(&self.root)?;
                let kind = s("kind");
                let hits =
                    self.index
                        .find(&s("name"), (!kind.is_empty()).then_some(kind.as_str()), 30);
                if hits.is_empty() {
                    return Ok(format!(
                        "No definition matching \"{}\". Try search for other uses of the text.",
                        s("name")
                    ));
                }
                Ok(hits
                    .iter()
                    .map(|h| {
                        let inside = h
                            .symbol
                            .container
                            .as_ref()
                            .map(|c| format!(" (in {c})"))
                            .unwrap_or_default();
                        format!(
                            "{}:{}  {} {}{}",
                            h.path, h.symbol.line, h.symbol.kind, h.symbol.name, inside
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "outline" => {
                self.index.refresh(&self.root)?;
                let path = s("path").replace('\\', "/");
                let symbols = self.index.outline(&path).ok_or_else(|| {
                    format!("{path} is not an indexed source file (or does not exist).")
                })?;
                if symbols.is_empty() {
                    return Ok(format!("{path}: no definitions found."));
                }
                Ok(symbols
                    .iter()
                    .map(|sym| {
                        let indent = if sym.container.is_some() { "  " } else { "" };
                        format!("{indent}{}  {} {}", sym.line, sym.kind, sym.name)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "read_lines" => {
                let path = s("path");
                let start = args["start"].as_u64().unwrap_or(1).max(1) as usize;
                let end = args["end"].as_u64().unwrap_or(start as u64) as usize;
                if end < start {
                    return Err("end must not be before start.".into());
                }
                let end = end.min(start + MAX_READ_LINES - 1);
                let content = files::read_file(&self.root, &path)?;
                if content.binary || content.too_large {
                    return Err(format!("{path} is binary or too large to read here."));
                }
                let lines: Vec<&str> = content.text.lines().collect();
                if start > lines.len() {
                    return Err(format!("{path} has only {} lines.", lines.len()));
                }
                let width = end.min(lines.len()).to_string().len();
                Ok(lines[start - 1..end.min(lines.len())]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{:>width$}  {}", start + i, l))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "search" => {
                let result = files::search(
                    &self.root,
                    &s("query"),
                    args["regex"].as_bool().unwrap_or(false),
                    args["case_sensitive"].as_bool().unwrap_or(false),
                    60,
                )?;
                if result.matches.is_empty() {
                    return Ok("No matches.".into());
                }
                let mut out: Vec<String> = result
                    .matches
                    .iter()
                    .map(|m| format!("{}:{}: {}", m.path, m.line, m.text.trim()))
                    .collect();
                if result.truncated {
                    out.push("(more matches not shown; narrow the search)".into());
                }
                Ok(out.join("\n"))
            }
            "project_map" => {
                self.index.refresh(&self.root)?;
                let depth = args["depth"].as_u64().unwrap_or(2).clamp(1, 4) as usize;
                let start = s("path").replace('\\', "/").trim_matches('/').to_string();
                Ok(self.map(&start, depth))
            }
            "memory_search" => {
                let (db, scope) = self
                    .memory
                    .as_ref()
                    .ok_or("The memory is not available here.")?;
                let found = crate::memory::search(db, Some(scope), &s("query"), 10)?;
                if found.is_empty() {
                    return Ok("Nothing in the user's memory matches.".into());
                }
                Ok(found
                    .iter()
                    .map(|e| {
                        let body: String = e.body.chars().take(600).collect();
                        format!("- [{}] {}: {}", e.kind, e.title, body)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "memory_suggest" => {
                let (db, scope) = self
                    .memory
                    .as_ref()
                    .ok_or("The memory is not available here.")?;
                let scope = if args["all_projects"].as_bool().unwrap_or(false) {
                    ""
                } else {
                    scope.as_str()
                };
                crate::memory::suggest(db, &s("kind"), scope, &s("title"), &s("body"))?;
                Ok("Suggested. The user approves or dismisses it in Qanuvi's Memory view; until then it is not used.".into())
            }
            _ => Err(format!("Unknown tool {name}.")),
        }
    }

    /// A short tree: folders with their file and definition counts, down to `depth` levels.
    fn map(&self, start: &str, depth: usize) -> String {
        let (all, _) = files::list_files(&self.root).unwrap_or_default();
        let prefix = if start.is_empty() {
            String::new()
        } else {
            format!("{start}/")
        };
        let mut folders: std::collections::BTreeMap<String, (usize, usize)> =
            std::collections::BTreeMap::new();
        let mut top_files = Vec::new();
        for f in all.iter().filter(|f| f.starts_with(&prefix)) {
            let rest = &f[prefix.len()..];
            let parts: Vec<&str> = rest.split('/').collect();
            let defs = self.index.outline(f).map(|s| s.len()).unwrap_or(0);
            if parts.len() == 1 {
                top_files.push(format!(
                    "{rest}{}",
                    if defs > 0 {
                        format!(" ({defs} defs)")
                    } else {
                        String::new()
                    }
                ));
                continue;
            }
            for level in 1..=depth.min(parts.len() - 1) {
                let key = parts[..level].join("/");
                let e = folders.entry(key).or_default();
                e.0 += 1;
                e.1 += defs;
            }
        }
        let mut out = vec![format!(
            "{} indexed source files, {} definitions in the project.",
            self.index.file_count(),
            self.index.symbol_count()
        )];
        out.push(format!("{}/", if start.is_empty() { "." } else { start }));
        for (folder, (n, defs)) in folders.iter().take(200) {
            let level = folder.matches('/').count();
            out.push(format!(
                "{}{}/  {n} files, {defs} defs",
                "  ".repeat(level + 1),
                folder.rsplit('/').next().unwrap_or(folder)
            ));
        }
        for f in top_files.iter().take(60) {
            out.push(format!("  {f}"));
        }
        out.join("\n")
    }
}

/// A set of tools served over MCP.
pub trait Tools {
    fn name(&self) -> &'static str;
    fn instructions(&self) -> &'static str;
    fn list(&self) -> Value;
    /// Runs a tool and answers the MCP result (`content`, and `isError` when it failed).
    fn call(&mut self, name: &str, args: &Value) -> Value;
}

impl Tools for Server {
    fn name(&self) -> &'static str {
        SERVER_NAME
    }
    fn instructions(&self) -> &'static str {
        "Read-only code index of the project. Use find_symbol and outline to locate code, then read_lines for just the lines you need, instead of reading whole files."
    }
    fn list(&self) -> Value {
        let mut all = tools();
        if self.memory.is_some()
            && let Some(list) = all.as_array_mut()
        {
            list.extend(memory_tools());
        }
        all
    }
    fn call(&mut self, name: &str, args: &Value) -> Value {
        self.run(name, args)
    }
}

/// A text answer, or a failed one.
pub fn text_result(r: Result<String, String>) -> Value {
    match r {
        Ok(t) => json!({"content": [{"type": "text", "text": t}]}),
        Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
    }
}

/// Answers one JSON-RPC message; notifications get no answer.
pub fn answer(tools: &mut impl Tools, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": tools.name(), "version": env!("CARGO_PKG_VERSION")},
            "instructions": tools.instructions()
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools.list()})),
        "tools/call" => Ok(tools.call(
            msg["params"]["name"].as_str().unwrap_or(""),
            &msg["params"]["arguments"],
        )),
        _ => Err(json!({"code": -32601, "message": format!("Unknown method {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    })
}

/// Runs a server until stdin closes.
pub fn serve(tools: &mut impl Tools) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(reply) = answer(tools, &msg)
            && writeln!(stdout, "{reply}")
                .and_then(|_| stdout.flush())
                .is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("src/cart")).unwrap();
        std::fs::write(
            d.join("src/cart/Cart.php"),
            "<?php\nclass Cart\n{\n    public function total() {\n        return 1;\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            d.join("src/run.ts"),
            "export function nextModel() {}\n// nextModel is used here\n",
        )
        .unwrap();
        std::fs::write(d.join("README.md"), "Readme\n").unwrap();
        dir
    }

    fn call(server: &mut Server, name: &str, args: Value) -> (String, bool) {
        let reply = server
            .handle(&json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}}))
            .unwrap();
        let r = &reply["result"];
        (
            r["content"][0]["text"].as_str().unwrap().to_string(),
            r["isError"].as_bool().unwrap_or(false),
        )
    }

    #[test]
    fn speaks_the_mcp_handshake_and_lists_tools() {
        let dir = project();
        let mut server = Server::new(dir.path());
        let init = server
            .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}))
            .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "qanuvi");
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(
            server
                .handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .is_none()
        );
        let list = server
            .handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![
                "project_map",
                "find_symbol",
                "outline",
                "read_lines",
                "search"
            ]
        );
        let unknown = server
            .handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}))
            .unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }

    #[test]
    fn tools_answer_briefly() {
        let dir = project();
        let mut server = Server::new(dir.path());
        let (text, err) = call(&mut server, "find_symbol", json!({"name": "total"}));
        assert!(!err);
        assert_eq!(text, "src/cart/Cart.php:4  method total (in Cart)");
        let (outline, _) = call(&mut server, "outline", json!({"path": "src/cart/Cart.php"}));
        assert_eq!(outline, "2  class Cart\n  4  method total");
        let (lines, _) = call(
            &mut server,
            "read_lines",
            json!({"path": "src/cart/Cart.php", "start": 4, "end": 5}),
        );
        assert_eq!(
            lines,
            "4      public function total() {\n5          return 1;"
        );
        let (found, _) = call(&mut server, "search", json!({"query": "nextModel"}));
        assert_eq!(found.lines().count(), 2);
        let (map, _) = call(&mut server, "project_map", json!({}));
        assert!(map.contains("src/  2 files, 3 defs"), "{map}");
        assert!(map.contains("cart/  1 files, 2 defs"), "{map}");
        assert!(map.contains("README.md"), "{map}");
    }

    #[test]
    fn refuses_paths_outside_the_project_and_bad_ranges() {
        let dir = project();
        let mut server = Server::new(dir.path());
        assert!(
            call(
                &mut server,
                "read_lines",
                json!({"path": "../secret.txt", "start": 1, "end": 2})
            )
            .1
        );
        assert!(
            call(
                &mut server,
                "read_lines",
                json!({"path": "src/run.ts", "start": 5, "end": 2})
            )
            .1
        );
        assert!(
            call(
                &mut server,
                "read_lines",
                json!({"path": "src/run.ts", "start": 99, "end": 100})
            )
            .1
        );
        assert!(call(&mut server, "outline", json!({"path": "missing.php"})).1);
        assert!(call(&mut server, "nope", json!({})).1);
        let (none, err) = call(&mut server, "find_symbol", json!({"name": "zzzz"}));
        assert!(!err && none.starts_with("No definition"));
    }

    #[test]
    fn memory_tools_search_and_suggest_for_the_user_to_approve() {
        let dir = project();
        let db = crate::store::Db::open_in_memory().unwrap();
        crate::memory::save(
            &db,
            &crate::memory::Entry {
                id: String::new(),
                kind: "decision".into(),
                scope: "p1".into(),
                title: "Money in cents".into(),
                body: "Totals are integer cents.".into(),
                tags: String::new(),
                status: "active".into(),
                source: "user".into(),
                created_at: 0,
                updated_at: 0,
                uses: 0,
            },
        )
        .unwrap();
        let mut plain = Server::new(dir.path());
        let names = |s: &mut Server| -> Vec<String> {
            s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
                .unwrap()["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert!(!names(&mut plain).contains(&"memory_search".to_string()));
        let mut server = Server::new(dir.path()).with_memory(db.clone(), "p1");
        assert!(names(&mut server).contains(&"memory_suggest".to_string()));
        let (found, _) = call(
            &mut server,
            "memory_search",
            json!({"query": "cart total cents"}),
        );
        assert!(found.starts_with("- [decision] Money in cents"), "{found}");
        let (said, err) = call(
            &mut server,
            "memory_suggest",
            json!({"kind": "fact", "title": "Tests use node --test", "body": "Run npm test."}),
        );
        assert!(!err && said.starts_with("Suggested"));
        assert!(
            crate::memory::list(&db, Some("p1"), false).unwrap().len() == 1,
            "not used until approved"
        );
        assert_eq!(crate::memory::list(&db, Some("p1"), true).unwrap().len(), 2);
    }
}
