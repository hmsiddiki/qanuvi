//! Agent sessions: runs Claude Code (`claude -p` with streamed JSON in and out) in a project folder, passes its
//! messages to the interface, asks the user before tools that need permission, and records tokens and cost.

use crate::projects::Project;
use crate::sink::Sink;
use crate::store::{Db, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Qanuvi's own agent (Milestone 24); sessions of the command-line agent are "claude-code".
pub const OWN_AGENT: &str = "qanuvi";

/// Permission modes offered in the interface (a subset of what the agent supports; never "bypass all").
pub const MODES: [&str; 4] = ["default", "acceptEdits", "auto", "plan"];

/// Tools a lean session keeps (comma-separated, as `--tools` expects).
pub const LEAN_TOOLS: &str = "Read,Edit,Write,Bash,Glob,Grep";

/// Where the browser tools of a session note the page the agent is on (for the preview tab to follow).
pub fn browser_state_file(session_id: &str) -> Result<std::path::PathBuf, String> {
    let safe: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    Ok(crate::paths::data_dir()
        .join("browser")
        .join(format!("{safe}.json")))
}

/// Writes the MCP config for a session (Qanuvi's code-index tools, its test browser, or both) and returns its path.
fn mcp_config(
    project: &Project,
    session_id: &str,
    index: bool,
    browser: bool,
) -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = crate::paths::data_dir().join("mcp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Old configs (one per session) are removed after a week.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > std::time::Duration::from_secs(7 * 86400));
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let file = dir.join(format!("{session_id}.json"));
    let state = browser_state_file(session_id)?;
    let db_file = crate::paths::data_dir().join("qanuvi.db");
    let memory_db = db_file
        .is_file()
        .then(|| db_file.to_string_lossy().to_string());
    let config = mcp_config_json(
        &exe.to_string_lossy(),
        &project.path,
        index,
        browser
            .then(|| state.to_string_lossy().to_string())
            .as_deref(),
        memory_db.as_deref().map(|db| (db, project.id.as_str())),
    );
    std::fs::write(&file, config).map_err(|e| e.to_string())?;
    Ok(file)
}

/// The MCP config: the code-index tools and/or the test browser (with the file it notes the page in).
pub fn mcp_config_json(
    exe: &str,
    project_path: &str,
    index: bool,
    browser_state: Option<&str>,
    memory: Option<(&str, &str)>,
) -> String {
    let mut servers = serde_json::Map::new();
    if index {
        let mut args = vec![json!("--mcp"), json!(project_path)];
        // The user's memory for this project: search it, and suggest entries for the user to approve.
        if let Some((db, scope)) = memory {
            args.extend([json!("--memory"), json!(db), json!("--scope"), json!(scope)]);
        }
        servers.insert(
            "qanuvi".into(),
            json!({"type": "stdio", "command": exe, "args": args}),
        );
    }
    if let Some(state) = browser_state {
        servers.insert(
            "qanuvi-browser".into(),
            json!({"type": "stdio", "command": exe, "args": ["--mcp-browser", project_path, state]}),
        );
    }
    json!({"mcpServers": servers}).to_string()
}

/// Effort levels the agent accepts (`--effort`).
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Checks the address of a proxy the agent should talk to (for example a local context optimiser). Empty means the
/// agent's own setting is used.
pub fn check_base_url(url: &str) -> Result<Option<String>, String> {
    let url = url.trim();
    if url.is_empty() {
        return Ok(None);
    }
    let ok = (url.starts_with("http://") || url.starts_with("https://"))
        && url.len() <= 300
        && !url.chars().any(|c| c.is_whitespace() || c.is_control());
    if !ok {
        return Err(format!(
            "The agent address must start with http:// or https:// and have no spaces: {url}"
        ));
    }
    Ok(Some(url.to_string()))
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub available: bool,
    pub path: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub agent: String,
    pub model: String,
    pub mode: String,
    /// running, waiting (for the user's approval), idle (ready for the next message), stopped, failed or ended.
    pub status: String,
    pub agent_session_id: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost_usd: f64,
    pub duration_ms: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AgentEvent {
    /// One JSON message from the agent, as received.
    Line {
        line: String,
    },
    /// The session record changed (status, tokens, cost).
    Session {
        session: Box<Session>,
    },
    /// Something the agent printed on its error stream.
    Error {
        text: String,
    },
    Exit {
        code: Option<i32>,
    },
}

struct Running {
    stdin: ChildStdin,
    child: Child,
    project_id: String,
    /// Permission requests waiting for an answer: request id -> the request (tool name and the input to pass back).
    pending: Arc<Mutex<HashMap<String, Value>>>,
    channel: Arc<Mutex<Sink<AgentEvent>>>,
}

/// How long a "for a few minutes" approval lasts.
pub const GRANT_MINUTES: u64 = 10;

/// How many minutes a "minutes" answer allows: "minutes:15" → 15, "minutes" → the default; between 1 and 240.
fn grant_minutes(remember: &str) -> Option<u64> {
    let rest = remember.strip_prefix("minutes")?;
    let n = rest
        .strip_prefix(':')
        .and_then(|n| n.trim().parse::<u64>().ok());
    Some(n.unwrap_or(GRANT_MINUTES).clamp(1, 240))
}

/// Approvals the user gave once for more than one request: session id -> group -> until (None: the whole session).
type Grants = Arc<Mutex<HashMap<String, HashMap<String, Option<Instant>>>>>;

/// What one "allow without asking" covers: all file changes together, otherwise the tool itself.
pub fn grant_group(tool: &str) -> String {
    match tool {
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "edits".into(),
        t => t.to_string(),
    }
}

/// Whether a permission request is covered by an earlier "allow without asking". Risky commands always ask.
fn granted(grants: &Grants, session_id: &str, request: &Value) -> bool {
    let tool = request["tool_name"].as_str().unwrap_or("");
    let command = request["input"]["command"].as_str().unwrap_or("");
    if !command.is_empty() && crate::agent_tools::risky(command) {
        return false;
    }
    grants
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(session_id)
        .and_then(|g| g.get(&grant_group(tool)))
        .is_some_and(|until| until.is_none_or(|t| Instant::now() < t))
}

fn allow_line(request_id: &str, input: &Value) -> String {
    json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id,
                     "response": {"behavior": "allow", "updatedInput": input}}
    })
    .to_string()
}

/// A standing approval, as the window shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub group: String,
    /// Seconds left; None while the session runs.
    pub seconds_left: Option<u64>,
}

#[derive(Default)]
pub struct Agents {
    map: Arc<Mutex<HashMap<String, Running>>>,
    grants: Grants,
}

fn find_claude() -> Option<std::path::PathBuf> {
    crate::terminal::find_on_path("claude")
}

pub fn info() -> AgentInfo {
    match find_claude() {
        None => AgentInfo {
            available: false,
            path: String::new(),
            version: String::new(),
        },
        Some(path) => {
            let mut cmd = Command::new(&path);
            cmd.arg("--version");
            no_window(&mut cmd);
            let version = cmd
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            AgentInfo {
                available: true,
                path: path.to_string_lossy().into(),
                version,
            }
        }
    }
}

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn from_row(row: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        agent: row.get("agent")?,
        model: row.get("model")?,
        mode: row.get("mode")?,
        status: row.get("status")?,
        agent_session_id: row.get("agent_session_id")?,
        input_tokens: row.get("input_tokens")?,
        output_tokens: row.get("output_tokens")?,
        cache_read_tokens: row.get("cache_read_tokens")?,
        cost_usd: row.get("cost_usd")?,
        duration_ms: row.get("duration_ms")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn get(db: &Db, id: &str) -> Result<Session, String> {
    db.conn()
        .query_row("SELECT * FROM sessions WHERE id = ?1", [id], from_row)
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That session no longer exists.".to_string())
}

pub fn list_sessions(db: &Db, project_id: &str) -> Result<Vec<Session>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT * FROM sessions WHERE project_id = ?1 ORDER BY updated_at DESC LIMIT 200")
        .map_err(|e| e.to_string())?;

    stmt.query_map([project_id], from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

/// An agent session that changed a file (the project panel's Timeline, 15h).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTouch {
    pub session_id: String,
    pub title: String,
    pub agent: String,
    pub model: String,
    pub updated_at: i64,
    /// How many of the session's tool calls changed the file.
    pub edits: u32,
}

/// Tools that only look at a file (they are not changes).
fn reads_only(tool: &str) -> bool {
    let t = tool.to_ascii_lowercase();
    [
        "read", "grep", "glob", "search", "outline", "find", "list", "ls",
    ]
    .iter()
    .any(|r| {
        t == *r
            || t.starts_with(&format!("{r}_"))
            || t.ends_with(&format!("_{r}"))
            || t.contains("__read")
    })
}

/// Whether a path an agent used (absolute or relative, either slash) names the project file `rel`.
fn same_file(used: &str, rel: &str) -> bool {
    let used = used.replace('\\', "/");
    let rel = rel.trim_start_matches("./").replace('\\', "/");
    !rel.is_empty() && (used == rel || used.ends_with(&format!("/{rel}")))
}

/// The tool calls in one stored line that changed `rel`.
fn edits_in(line: &str, rel: &str) -> u32 {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return 0;
    };
    let Some(blocks) = v["message"]["content"].as_array() else {
        return 0;
    };
    blocks
        .iter()
        .filter(|b| b["type"] == "tool_use" && !reads_only(b["name"].as_str().unwrap_or("")))
        .filter(|b| {
            ["file_path", "path", "notebook_path"]
                .iter()
                .any(|k| b["input"][*k].as_str().is_some_and(|p| same_file(p, rel)))
        })
        .count() as u32
}

/// The agent sessions of a project that changed a file, newest first.
pub fn file_history(db: &Db, project_id: &str, rel: &str) -> Result<Vec<FileTouch>, String> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if name.is_empty() {
        return Ok(vec![]);
    }
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.title, s.agent, s.model, s.updated_at, e.line
             FROM sessions s JOIN session_events e ON e.session_id = s.id
             WHERE s.project_id = ?1 AND e.line LIKE '%tool_use%' AND e.line LIKE ?2
             ORDER BY s.updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![project_id, format!("%{name}%")], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out: Vec<FileTouch> = Vec::new();
    for row in rows {
        let (id, title, agent, model, updated_at, line) = row.map_err(|e| e.to_string())?;
        let n = edits_in(&line, rel);
        if n == 0 {
            continue;
        }
        match out.iter_mut().find(|t| t.session_id == id) {
            Some(t) => t.edits += n,
            None => out.push(FileTouch {
                session_id: id,
                title,
                agent,
                model,
                updated_at,
                edits: n,
            }),
        }
        if out.len() >= 50 {
            break;
        }
    }
    Ok(out)
}

pub fn session_events(db: &Db, session_id: &str) -> Result<Vec<String>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT line FROM session_events WHERE session_id = ?1 ORDER BY seq")
        .map_err(|e| e.to_string())?;

    stmt.query_map([session_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn rename(db: &Db, id: &str, title: &str) -> Result<(), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A session needs a title.".into());
    }
    let title: String = title.chars().take(120).collect();
    db.conn()
        .execute(
            "UPDATE sessions SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete(db: &Db, id: &str) -> Result<(), String> {
    db.conn()
        .execute("DELETE FROM sessions WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Usage {
    pub cost_usd: f64,
    pub sessions: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

/// What all agent sessions cost since a moment (seconds since 1970), for the monthly budget.
pub fn usage_since(db: &Db, since: i64) -> Result<Usage, String> {
    db.conn()
        .query_row(
            "SELECT COALESCE(SUM(cost_usd), 0), COUNT(*), COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0)
             FROM sessions WHERE created_at >= ?1",
            [since],
            |r| {
                Ok(Usage {
                    cost_usd: r.get(0)?,
                    sessions: r.get(1)?,
                    input_tokens: r.get(2)?,
                    output_tokens: r.get(3)?,
                })
            },
        )
        .map_err(|e| e.to_string())
}

/// Sessions that were busy when the app closed cannot continue in the same process; they can be resumed.
pub fn mark_interrupted(db: &Db) {
    let _ = db.conn().execute(
        "UPDATE sessions SET status = 'ended' WHERE status IN ('running', 'waiting', 'idle')",
        [],
    );
}

fn set_status(db: &Db, id: &str, status: &str) {
    let _ = db.conn().execute(
        "UPDATE sessions SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status, now()],
    );
}

/// A short title from the first message.
pub fn title_from(prompt: &str) -> String {
    let line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("New session")
        .trim();
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title.push('…');
    }
    title
}

/// What a `result` message adds to the session totals.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Totals {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub turn_cost_usd: f64,
    pub duration_ms: i64,
}

pub fn totals_from_result(msg: &Value, cost_so_far_in_process: f64) -> Totals {
    let usage = &msg["usage"];
    let n = |v: &Value| v.as_i64().unwrap_or(0);
    let total_cost = msg["total_cost_usd"].as_f64().unwrap_or(0.0);
    Totals {
        input_tokens: n(&usage["input_tokens"]) + n(&usage["cache_creation_input_tokens"]),
        output_tokens: n(&usage["output_tokens"]),
        cache_read_tokens: n(&usage["cache_read_input_tokens"]),
        // The agent reports the cost of the whole process so far; keep only this turn's part.
        turn_cost_usd: (total_cost - cost_so_far_in_process).max(0.0),
        duration_ms: n(&msg["duration_ms"]),
    }
}

/// Adds a line to a session's history (agent output, or Qanuvi's own notes of the user's messages and answers).
pub fn append_event(db: &Db, session_id: &str, line: &str) {
    let _ = db.conn().execute(
        "INSERT INTO session_events (session_id, seq, line)
         SELECT ?1, COALESCE(MAX(seq), 0) + 1, ?2 FROM session_events WHERE session_id = ?1",
        params![session_id, line],
    );
}

/// The message the agent gets: notes Qanuvi adds (a slice of the user's memory) before the user's own words. Only the
/// user's words are stored and shown as their message.
pub fn with_context(context: Option<&str>, prompt: &str) -> String {
    match context.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => format!("{c}\n\n---\n\n{prompt}"),
        None => prompt.to_string(),
    }
}

/// A file the user attached to a message: an image the agent sees, or a text file or web page it reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    /// "image", "text" or "page".
    pub kind: String,
    /// For images: image/png, image/jpeg, image/gif or image/webp.
    #[serde(default)]
    pub media_type: String,
    /// Images: base64. Text files and pages: the text.
    pub data: String,
}

/// The text of attached files and pages, as notes before the user's words (images go as images instead).
pub fn attached_text(files: &[Attachment]) -> Option<String> {
    let parts: Vec<String> = files
        .iter()
        .filter(|f| f.kind != "image")
        .map(|f| {
            let what = if f.kind == "page" {
                "Web page"
            } else {
                "Attached file"
            };
            format!("{what}: {}\n```\n{}\n```", f.name, f.data.trim_end())
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// The notes Qanuvi adds (memory) and the text of attachments, together.
fn context_with(context: Option<&str>, files: &[Attachment]) -> Option<String> {
    let parts: Vec<String> = context
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(String::from)
        .into_iter()
        .chain(attached_text(files))
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// The user's line as stored and shown: their words and the names of what they attached.
fn user_line(prompt: &str, files: &[Attachment]) -> String {
    let mut v = json!({"type": "qanuvi_user", "text": prompt});
    if !files.is_empty() {
        v["attachments"] = json!(files.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    }
    v.to_string()
}

fn user_message(text: &str, files: &[Attachment]) -> String {
    let images: Vec<Value> = files
        .iter()
        .filter(|f| f.kind == "image")
        .map(|f| json!({"type": "image", "source": {"type": "base64", "media_type": f.media_type, "data": f.data}}))
        .collect();
    let content = if images.is_empty() {
        json!(text)
    } else {
        let mut blocks = vec![json!({"type": "text", "text": text})];
        blocks.extend(images);
        Value::Array(blocks)
    };
    json!({
        "type": "user",
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": null,
        "session_id": ""
    })
    .to_string()
}

fn send_line(stdin: &mut ChildStdin, line: &str) -> Result<(), String> {
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|_| stdin.flush())
        .map_err(|_| "The agent is no longer running.".to_string())
}

impl Agents {
    /// Starts a session, or sends the next message to one that is already running.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        db: &Db,
        project: &Project,
        session_id: Option<&str>,
        prompt: &str,
        model: &str,
        mode: &str,
        base_url: Option<&str>,
        effort: Option<&str>,
        lean: bool,
        index_tools: bool,
        browser_tools: bool,
        context: Option<&str>,
        files: &[Attachment],
        kind: &str,
        setup: Option<&str>,
        channel: Sink<AgentEvent>,
    ) -> Result<Session, String> {
        let context = context_with(context, files);
        let context = context.as_deref();
        let own = kind == OWN_AGENT;
        if !own && kind != "claude-code" {
            return Err(format!("Unknown agent {kind}."));
        }
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Write a message first.".into());
        }
        let base_url = check_base_url(base_url.unwrap_or(""))?;
        let effort = effort.map(str::trim).filter(|e| !e.is_empty());
        if let Some(e) = effort
            && !EFFORTS.contains(&e)
        {
            return Err(format!("Unknown effort level {e}."));
        }
        if !MODES.contains(&mode) {
            return Err(format!("Unknown permission mode {mode}."));
        }
        let model = model.trim();
        if model.is_empty() || model.len() > 160 || model.contains(char::is_whitespace) {
            return Err("Choose a model.".into());
        }

        // Follow-up message to a running session.
        if let Some(id) = session_id {
            let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(running) = map.get_mut(id) {
                *running.channel.lock().unwrap_or_else(|e| e.into_inner()) = channel;
                // The user chose another model or permission mode for this session: the running agent hears it first
                // (otherwise it keeps the ones it was started with).
                if let Ok(s) = get(db, id) {
                    if s.model != model {
                        send_line(
                            &mut running.stdin,
                            &json!({"type": "control_request", "request_id": format!("qanuvi-model-{}", crate::llm::short_id()),
                                "request": {"subtype": "set_model", "model": model}})
                            .to_string(),
                        )?;
                    }
                    if s.mode != mode {
                        send_line(
                            &mut running.stdin,
                            &json!({"type": "control_request", "request_id": format!("qanuvi-mode-{}", crate::llm::short_id()),
                                "request": {"subtype": "set_permission_mode", "mode": mode}})
                            .to_string(),
                        )?;
                    }
                    let _ = db.conn().execute(
                        "UPDATE sessions SET model = ?2, mode = ?3 WHERE id = ?1",
                        params![id, model, mode],
                    );
                }
                // A message while the agent is working joins the task (both agents take it in mid-turn).
                let during =
                    get(db, id).is_ok_and(|s| s.status == "running" || s.status == "waiting");
                send_line(
                    &mut running.stdin,
                    &user_message(&with_context(context, prompt), files),
                )?;
                let mut line: Value =
                    serde_json::from_str(&user_line(prompt, files)).unwrap_or(Value::Null);
                if during {
                    line["during"] = json!(true);
                }
                append_event(db, id, &line.to_string());
                drop(map);
                set_status(db, id, "running");
                return get(db, id);
            }
        }

        // Qanuvi's own agent: its models and providers (keys included) go in an environment variable.
        let own_setup = if own {
            let raw = setup.ok_or("Add a model first (Agents → Qanuvi agent).")?;
            let parsed: crate::brain::Setup = serde_json::from_str(raw)
                .map_err(|e| format!("The model setup is not valid: {e}"))?;
            parsed.check()?;
            Some(raw.to_string())
        } else {
            None
        };
        let program = if own {
            std::env::current_exe().map_err(|e| e.to_string())?
        } else {
            find_claude()
                .ok_or("Claude Code (the `claude` command) was not found on this machine.")?
        };
        let existing = match session_id {
            Some(id) => Some(get(db, id)?),
            None => None,
        };
        let session = match existing {
            Some(s) => {
                db.conn()
                    .execute(
                        "UPDATE sessions SET model = ?2, mode = ?3, status = 'running', updated_at = ?4 WHERE id = ?1",
                        params![s.id, model, mode, now()],
                    )
                    .map_err(|e| e.to_string())?;
                get(db, &s.id)?
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                db.conn()
                    .execute(
                        "INSERT INTO sessions (id, project_id, title, agent, model, mode, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?7, ?4, ?5, 'running', ?6, ?6)",
                        params![id, project.id, title_from(prompt), model, mode, now(), kind],
                    )
                    .map_err(|e| e.to_string())?;
                get(db, &id)?
            }
        };

        let mut cmd = Command::new(&program);
        cmd.current_dir(&project.path);
        if let Some(raw) = &own_setup {
            cmd.args([
                "--agent",
                "--model",
                model,
                "--permission-mode",
                mode,
                "--data",
            ])
            .arg(crate::paths::data_dir())
            .env(crate::local_agent::SETUP_ENV, raw)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
            if let Some(agent_id) = &session.agent_session_id {
                cmd.args(["--resume", agent_id]);
            }
        } else {
            cmd.args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--model",
                model,
                "--permission-mode",
                mode,
                "--permission-prompt-tool",
                "stdio",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
            if let Some(agent_id) = &session.agent_session_id {
                cmd.args(["--resume", agent_id]);
            }
            // A proxy chosen in Qanuvi (for example Headroom) wins over the agent's own setting for this session.
            if let Some(e) = effort {
                cmd.args(["--effort", e]);
            }
            // Lean sessions (plan runs): no MCP servers and only the tools a step needs. This halves the start-up context
            // and keeps it identical from session to session, so later sessions read it from the cache.
            if lean {
                cmd.args(["--strict-mcp-config", "--tools", LEAN_TOOLS]);
            }
            // Qanuvi introduces itself (24g): what it is and its plan format, so "make a plan" needs no search.
            if let Some(brief) = crate::brief::for_session(lean) {
                cmd.arg("--append-system-prompt")
                    .arg(crate::brief::as_arg(brief, &program));
            }
            // Qanuvi's own tools: the code index (read-only) and the test browser (its own empty profile, nothing of the
            // user's), so they need no approval.
            if index_tools || browser_tools {
                let config = mcp_config(project, &session.id, index_tools, browser_tools)?;
                cmd.arg("--mcp-config").arg(config);
                let mut allowed = Vec::new();
                if index_tools {
                    allowed.push("mcp__qanuvi");
                }
                if browser_tools {
                    allowed.push("mcp__qanuvi-browser");
                }
                cmd.arg("--allowedTools").arg(allowed.join(","));
            }
            if let Some(url) = &base_url {
                cmd.env("ANTHROPIC_BASE_URL", url);
            }
        }
        no_window(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| {
            if own {
                format!("Qanuvi's agent could not start: {e}")
            } else {
                format!("Claude Code could not start: {e}")
            }
        })?;
        let mut stdin = child.stdin.take().ok_or("No input to the agent.")?;
        let stdout = child.stdout.take().ok_or("No output from the agent.")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("No error output from the agent.")?;

        send_line(
            &mut stdin,
            &json!({"type": "control_request", "request_id": "qanuvi-init", "request": {"subtype": "initialize"}})
                .to_string(),
        )?;
        send_line(
            &mut stdin,
            &user_message(&with_context(context, prompt), files),
        )?;
        append_event(db, &session.id, &user_line(prompt, files));

        let pending = Arc::new(Mutex::new(HashMap::new()));
        let channel = Arc::new(Mutex::new(channel));
        self.map.lock().unwrap_or_else(|e| e.into_inner()).insert(
            session.id.clone(),
            Running {
                stdin,
                child,
                project_id: project.id.clone(),
                pending: pending.clone(),
                channel: channel.clone(),
            },
        );

        // Error stream: passed on as it comes.
        {
            let channel = channel.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        let _ = channel
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .send(AgentEvent::Error { text: line });
                    }
                }
            });
        }

        // Output: stored, interpreted for the session record, and passed on.
        let session_id = session.id.clone();
        let map = self.map.clone();
        let grants = self.grants.clone();
        let thread_db = db.clone();
        std::thread::spawn(move || {
            let db = &thread_db;
            let mut cost_in_process = 0.0;
            let mut got_result = false;
            let send = |event: AgentEvent| {
                let _ = channel
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .send(event);
            };
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let kind = msg["type"].as_str().unwrap_or("");
                let subtype = msg["subtype"].as_str().unwrap_or("");
                // Progress counters and rate-limit notices are not worth keeping.
                let noise = (kind == "system" && subtype == "thinking_tokens")
                    || kind == "rate_limit_event"
                    || (kind == "control_response"
                        && msg["response"]["request_id"] == "qanuvi-init");
                if noise {
                    continue;
                }
                // Text as it is written: shown live, not stored (the whole message follows).
                if kind == "stream_event" {
                    send(AgentEvent::Line { line });
                    continue;
                }
                append_event(db, &session_id, &line);
                let mut changed = false;
                let mut auto: Option<(String, Value)> = None;
                match kind {
                    "system" if subtype == "init" => {
                        if let Some(agent_id) = msg["session_id"].as_str() {
                            let _ = db.conn().execute(
                                "UPDATE sessions SET agent_session_id = ?2 WHERE id = ?1",
                                params![session_id, agent_id],
                            );
                            changed = true;
                        }
                    }
                    "control_request" if msg["request"]["subtype"] == "can_use_tool" => {
                        if let Some(rid) = msg["request_id"].as_str() {
                            if granted(&grants, &session_id, &msg["request"]) {
                                auto = Some((rid.to_string(), msg["request"]["input"].clone()));
                            } else {
                                pending
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner())
                                    .insert(rid.to_string(), msg["request"].clone());
                                set_status(db, &session_id, "waiting");
                                changed = true;
                            }
                        }
                    }
                    "result" => {
                        got_result = true;
                        let t = totals_from_result(&msg, cost_in_process);
                        cost_in_process += t.turn_cost_usd;
                        let status = if msg["is_error"].as_bool().unwrap_or(false) {
                            "failed"
                        } else {
                            "idle"
                        };
                        let _ = db.conn().execute(
                            "UPDATE sessions SET input_tokens = input_tokens + ?2, output_tokens = output_tokens + ?3,
                             cache_read_tokens = cache_read_tokens + ?4, cost_usd = cost_usd + ?5,
                             duration_ms = duration_ms + ?6, status = ?7, updated_at = ?8 WHERE id = ?1",
                            params![
                                session_id,
                                t.input_tokens,
                                t.output_tokens,
                                t.cache_read_tokens,
                                t.turn_cost_usd,
                                t.duration_ms,
                                status,
                                now()
                            ],
                        );
                        changed = true;
                    }
                    "user" | "assistant" => {
                        // Once a permission was answered the agent carries on.
                        let waiting = get(db, &session_id)
                            .map(|s| s.status == "waiting")
                            .unwrap_or(false);
                        if waiting && pending.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
                            set_status(db, &session_id, "running");
                            changed = true;
                        }
                    }
                    _ => {}
                }
                send(AgentEvent::Line { line });
                // Allowed earlier for the session or a few minutes: answered at once, shown as approved.
                if let Some((rid, input)) = auto {
                    let sent = map
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get_mut(&session_id)
                        .map(|r| send_line(&mut r.stdin, &allow_line(&rid, &input)));
                    if matches!(sent, Some(Ok(()))) {
                        let note = json!({"type": "qanuvi_permission", "request_id": rid, "allow": true, "auto": true})
                            .to_string();
                        append_event(db, &session_id, &note);
                        send(AgentEvent::Line { line: note });
                    }
                }
                if changed && let Ok(s) = get(db, &session_id) {
                    send(AgentEvent::Session {
                        session: Box::new(s),
                    });
                }
            }
            // The process ended.
            let running = map
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&session_id);
            let code = running
                .and_then(|mut r| r.child.wait().ok())
                .and_then(|s| s.code());
            let current = get(db, &session_id).map(|s| s.status).unwrap_or_default();
            if current != "stopped" {
                let failed = !got_result || matches!(code, Some(c) if c != 0);
                set_status(
                    db,
                    &session_id,
                    if failed && current != "idle" {
                        "failed"
                    } else {
                        "ended"
                    },
                );
            }
            if let Ok(s) = get(db, &session_id) {
                send(AgentEvent::Session {
                    session: Box::new(s),
                });
            }
            send(AgentEvent::Exit { code });
        });

        get(db, &session.id)
    }

    /// Answers a permission request. Allowing passes the tool's input back unchanged.
    pub fn respond(
        &self,
        db: &Db,
        session_id: &str,
        request_id: &str,
        allow: bool,
        remember: &str,
    ) -> Result<(), String> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        let running = map
            .get_mut(session_id)
            .ok_or("The session is no longer running.")?;
        let request = running
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(request_id)
            .ok_or("That request was already answered.")?;
        let line = if allow {
            allow_line(request_id, &request["input"])
        } else {
            json!({
                "type": "control_response",
                "response": {"subtype": "success", "request_id": request_id,
                             "response": {"behavior": "deny", "message": "The user refused this action in Qanuvi."}}
            })
            .to_string()
        };
        send_line(&mut running.stdin, &line)?;
        // "For this session" / "for N minutes" ("minutes:15"; plain "minutes" is the default length): later requests
        // of the same kind are answered without asking.
        let minutes = grant_minutes(remember);
        if allow && (remember == "session" || minutes.is_some()) {
            let until = minutes.map(|m| Instant::now() + Duration::from_secs(m * 60));
            self.grants
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(session_id.to_string())
                .or_default()
                .insert(
                    grant_group(request["tool_name"].as_str().unwrap_or("")),
                    until,
                );
        }
        append_event(
            db,
            session_id,
            &json!({"type": "qanuvi_permission", "request_id": request_id, "allow": allow})
                .to_string(),
        );
        Ok(())
    }

    /// The standing approvals of a session that are still in force.
    pub fn grants(&self, session_id: &str) -> Vec<Grant> {
        let now = Instant::now();
        let mut list: Vec<Grant> = self
            .grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .map(|g| {
                g.iter()
                    .filter(|(_, until)| until.is_none_or(|t| now < t))
                    .map(|(group, until)| Grant {
                        group: group.clone(),
                        seconds_left: until.map(|t| (t - now).as_secs()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        list.sort_by(|a, b| a.group.cmp(&b.group));
        list
    }

    /// Takes back the standing approvals of a session: everything asks again.
    pub fn forget_grants(&self, session_id: &str) {
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
    }

    /// Stops a session's agent process.
    pub fn stop(&self, session_id: &str) {
        let running = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
        if let Some(mut r) = running {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }

    pub fn stop_project(&self, project_id: &str) {
        let ids: Vec<String> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(_, r)| r.project_id == project_id)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.stop(&id);
        }
    }

    pub fn stop_all(&self) {
        let ids: Vec<String> = self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.stop(&id);
        }
    }
}

/// Marks a session stopped by the user (kept apart from `stop` so the database is updated from the command).
pub fn mark_stopped(db: &Db, id: &str) {
    set_status(db, id, "stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_approvals_cover_their_kind_until_they_end_and_never_risky_commands() {
        let grants: Grants = Default::default();
        let edit = json!({"tool_name": "Write", "input": {"file_path": "a.txt"}});
        let safe = json!({"tool_name": "Bash", "input": {"command": "npm test"}});
        let risky = json!({"tool_name": "Bash", "input": {"command": "git push --force"}});
        assert!(!granted(&grants, "s1", &edit));
        {
            let mut g = grants.lock().unwrap();
            g.entry("s1".into())
                .or_default()
                .insert(grant_group("Edit"), None);
            g.entry("s1".into()).or_default().insert(
                grant_group("Bash"),
                Some(Instant::now() + Duration::from_secs(60)),
            );
            g.entry("s2".into()).or_default().insert(
                grant_group("Edit"),
                Some(Instant::now() - Duration::from_secs(1)),
            );
        }
        assert!(
            granted(&grants, "s1", &edit),
            "Edit allowed covers Write too"
        );
        assert!(granted(&grants, "s1", &safe));
        assert!(!granted(&grants, "s1", &risky), "risky commands always ask");
        assert!(!granted(&grants, "s2", &edit), "ended");
        assert!(!granted(&grants, "s3", &edit), "other sessions ask");
        assert_eq!(grant_minutes("minutes:15"), Some(15));
        assert_eq!(grant_minutes("minutes"), Some(GRANT_MINUTES));
        assert_eq!(grant_minutes("minutes:9999"), Some(240));
        assert_eq!(grant_minutes("session"), None);
    }

    #[test]
    fn usage_adds_up_sessions_since_a_moment() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        for (id, at, cost) in [("a", 100, 0.5), ("b", 200, 0.25), ("c", 300, 1.0)] {
            db.conn()
                .execute(
                    "INSERT INTO sessions (id, project_id, title, agent, model, mode, status, cost_usd, input_tokens, created_at, updated_at)
                     VALUES (?1, ?2, 'T', 'claude-code', 'm', 'default', 'ended', ?3, 10, ?4, ?4)",
                    params![id, project.id, cost, at],
                )
                .unwrap();
        }
        let u = usage_since(&db, 200).unwrap();
        assert_eq!(u.sessions, 2);
        assert!((u.cost_usd - 1.25).abs() < 1e-9);
        assert_eq!(u.input_tokens, 20);
        assert_eq!(usage_since(&db, 1000).unwrap().cost_usd, 0.0);
    }

    #[test]
    fn titles_come_from_the_first_line() {
        assert_eq!(
            title_from("\n  Fix the cart total\nmore"),
            "Fix the cart total"
        );
        let long = "x".repeat(80);
        assert_eq!(title_from(&long).chars().count(), 61);
        assert_eq!(title_from("   "), "New session");
    }

    #[test]
    fn result_totals_count_cache_writes_as_input_and_keep_only_this_turns_cost() {
        let msg = json!({
            "type": "result",
            "duration_ms": 1200,
            "total_cost_usd": 0.05,
            "usage": {"input_tokens": 10, "cache_creation_input_tokens": 90, "cache_read_input_tokens": 500, "output_tokens": 40}
        });
        let t = totals_from_result(&msg, 0.02);
        assert_eq!(t.input_tokens, 100);
        assert_eq!(t.output_tokens, 40);
        assert_eq!(t.cache_read_tokens, 500);
        assert!((t.turn_cost_usd - 0.03).abs() < 1e-9);
        assert_eq!(t.duration_ms, 1200);
        assert_eq!(totals_from_result(&json!({}), 0.0), Totals::default());
    }

    #[test]
    fn sessions_are_stored_renamed_listed_and_deleted() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        db.conn()
            .execute(
                "INSERT INTO sessions (id, project_id, title, agent, model, mode, status, created_at, updated_at)
                 VALUES ('s1', ?1, 'First', 'claude-code', 'm', 'default', 'running', 1, 1)",
                [&project.id],
            )
            .unwrap();
        db.conn()
            .execute(
                "INSERT INTO session_events (session_id, seq, line) VALUES ('s1', 1, '{}')",
                [],
            )
            .unwrap();
        assert_eq!(list_sessions(&db, &project.id).unwrap().len(), 1);
        assert_eq!(session_events(&db, "s1").unwrap(), vec!["{}"]);
        rename(&db, "s1", "  Better title ").unwrap();
        assert_eq!(get(&db, "s1").unwrap().title, "Better title");
        assert!(rename(&db, "s1", " ").is_err());

        mark_interrupted(&db);
        assert_eq!(get(&db, "s1").unwrap().status, "ended");

        delete(&db, "s1").unwrap();
        assert!(list_sessions(&db, &project.id).unwrap().is_empty());
        assert!(
            session_events(&db, "s1").unwrap().is_empty(),
            "events go with the session"
        );
    }

    #[test]
    fn removing_a_project_removes_its_sessions() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        db.conn()
            .execute(
                "INSERT INTO sessions (id, project_id, title, agent, model, mode, status, created_at, updated_at)
                 VALUES ('s1', ?1, 'First', 'claude-code', 'm', 'default', 'ended', 1, 1)",
                [&project.id],
            )
            .unwrap();
        crate::projects::remove(&db, &project.id).unwrap();
        assert!(get(&db, "s1").is_err());
    }

    #[test]
    fn history_keeps_the_order_of_agent_output_and_user_notes() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = crate::projects::add(&db, &dir.path().to_string_lossy()).unwrap();
        db.conn()
            .execute(
                "INSERT INTO sessions (id, project_id, title, agent, model, mode, status, created_at, updated_at)
                 VALUES ('s1', ?1, 'T', 'claude-code', 'm', 'default', 'idle', 1, 1)",
                [&project.id],
            )
            .unwrap();
        append_event(&db, "s1", "a");
        append_event(&db, "s1", "b");
        append_event(&db, "s1", "c");
        assert_eq!(session_events(&db, "s1").unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn checks_the_proxy_address_and_offers_auto_but_never_bypass() {
        assert_eq!(check_base_url("").unwrap(), None);
        assert_eq!(check_base_url("  ").unwrap(), None);
        assert_eq!(
            check_base_url(" http://127.0.0.1:8787 ")
                .unwrap()
                .as_deref(),
            Some("http://127.0.0.1:8787")
        );
        assert!(check_base_url("https://proxy.example.com/v1").is_ok());
        assert!(check_base_url("127.0.0.1:8787").is_err());
        assert!(check_base_url("http://a b").is_err());
        assert!(check_base_url("file:///etc/passwd").is_err());
        assert!(MODES.contains(&"auto"));
        assert!(!MODES.contains(&"bypassPermissions"));
    }

    #[test]
    fn mcp_config_starts_qanuvi_for_the_index_and_the_browser() {
        let v: Value = serde_json::from_str(&mcp_config_json(
            r"C:Appsqanuvi.exe",
            r"E:Sitesshop",
            true,
            None,
            None,
        ))
        .unwrap();
        let s = &v["mcpServers"]["qanuvi"];
        assert_eq!(s["command"], r"C:Appsqanuvi.exe");
        assert_eq!(s["args"], json!(["--mcp", r"E:Sitesshop"]));
        assert!(v["mcpServers"].get("qanuvi-browser").is_none());
        let both: Value = serde_json::from_str(&mcp_config_json(
            "q.exe",
            "/p",
            false,
            Some("/data/browser/s1.json"),
            None,
        ))
        .unwrap();
        assert!(both["mcpServers"].get("qanuvi").is_none());
        assert_eq!(
            both["mcpServers"]["qanuvi-browser"]["args"],
            json!(["--mcp-browser", "/p", "/data/browser/s1.json"])
        );
    }

    #[test]
    fn memory_goes_before_the_message_and_only_when_there_is_some() {
        assert_eq!(with_context(None, "Fix it"), "Fix it");
        assert_eq!(with_context(Some("  "), "Fix it"), "Fix it");
        assert_eq!(
            with_context(Some("Notes"), "Fix it"),
            "Notes

---

Fix it"
        );
        let v: Value = serde_json::from_str(&mcp_config_json(
            "q",
            "/p",
            true,
            None,
            Some(("/d/qanuvi.db", "p1")),
        ))
        .unwrap();
        assert_eq!(
            v["mcpServers"]["qanuvi"]["args"],
            json!(["--mcp", "/p", "--memory", "/d/qanuvi.db", "--scope", "p1"])
        );
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;

    fn file(name: &str, kind: &str, data: &str) -> Attachment {
        Attachment {
            name: name.into(),
            kind: kind.into(),
            media_type: "image/png".into(),
            data: data.into(),
        }
    }

    #[test]
    fn images_go_as_images_and_text_goes_before_the_words() {
        let files = [
            file("shot.png", "image", "QUJD"),
            file("notes.txt", "text", "milk"),
        ];
        let msg: Value = serde_json::from_str(&user_message("Look", &files)).unwrap();
        let content = &msg["message"]["content"];
        assert_eq!(content[0]["text"], "Look");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["data"], "QUJD");
        let ctx = context_with(Some("memory"), &files).unwrap();
        assert!(ctx.starts_with("memory"));
        assert!(
            ctx.contains("Attached file: notes.txt")
                && ctx.contains("milk")
                && !ctx.contains("QUJD")
        );
        let line: Value = serde_json::from_str(&user_line("Look", &files)).unwrap();
        assert_eq!(line["attachments"], json!(["shot.png", "notes.txt"]));
    }

    #[test]
    fn without_attachments_the_message_stays_plain_text() {
        let msg: Value = serde_json::from_str(&user_message("Hi", &[])).unwrap();
        assert_eq!(msg["message"]["content"], "Hi");
        assert!(context_with(None, &[]).is_none());
        let line: Value = serde_json::from_str(&user_line("Hi", &[])).unwrap();
        assert!(line.get("attachments").is_none());
    }
}

#[cfg(test)]
mod file_history_tests {
    use super::*;

    #[test]
    fn changes_are_found_by_any_path_form_and_reads_are_not() {
        let edit = |name: &str, path: &str| {
            json!({"type": "assistant", "message": {"content": [
                {"type": "tool_use", "name": name, "input": {"file_path": path}}
            ]}})
            .to_string()
        };
        assert_eq!(
            edits_in(
                &edit("Edit", "E:\\Projects\\shop\\src\\cart.ts"),
                "src/cart.ts"
            ),
            1
        );
        assert_eq!(edits_in(&edit("Write", "src/cart.ts"), "src/cart.ts"), 1);
        assert_eq!(edits_in(&edit("Read", "src/cart.ts"), "src/cart.ts"), 0);
        assert_eq!(edits_in(&edit("Edit", "src/mycart.ts"), "src/cart.ts"), 0);
        assert_eq!(
            edits_in(
                &edit("mcp__qanuvi__read_lines", "src/cart.ts"),
                "src/cart.ts"
            ),
            0
        );
        assert_eq!(edits_in("not json", "src/cart.ts"), 0);
    }
}
