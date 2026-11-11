//! Qanuvi's own agent (Milestone 24): `qanuvi --agent` (or `qanuvi-master --agent`) runs in a project folder and
//! talks to models through the brain. It speaks the same stream of JSON lines as the command-line agent Qanuvi already
//! runs (user messages in; system, assistant, user tool results, permission requests and results out), so sessions,
//! approvals, history, costs and Stop work the same, on this computer and on servers.

use crate::agent_tools;
use crate::brain::{self, Job, Records, Setup};
use crate::llm;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// The setup (providers with keys, and models) comes in this environment variable, never on the command line.
pub const SETUP_ENV: &str = "QANUVI_AGENT_SETUP";
const MAX_ROUNDS: usize = 40;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Where the agent's JSON lines go: stdout, or a test's channel.
pub struct Out(Box<dyn Fn(&Value) + Send>);

impl Out {
    pub fn stdout() -> Self {
        Out(Box::new(|v| {
            let mut o = std::io::stdout().lock();
            let _ = writeln!(o, "{v}");
            let _ = o.flush();
        }))
    }

    pub fn to(f: impl Fn(&Value) + Send + 'static) -> Self {
        Out(Box::new(f))
    }

    fn send(&self, v: &Value) {
        (self.0)(v)
    }
}

struct Options {
    model: String,
    mode: String,
    resume: Option<String>,
    data: PathBuf,
}

fn options(args: &[String]) -> Options {
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    Options {
        model: value("--model").unwrap_or_else(|| brain::AUTO.into()),
        mode: value("--permission-mode").unwrap_or_else(|| "default".into()),
        resume: value("--resume"),
        data: value("--data")
            .map(PathBuf::from)
            .unwrap_or_else(crate::paths::data_dir),
    }
}

fn history_file(data: &Path, session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    data.join("agent-history").join(format!("{safe}.json"))
}

/// Every tool the agent has: the file and command tools, then the code-index tools.
const TOOL_NAMES: [&str; 11] = [
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Glob",
    "Grep",
    "project_map",
    "find_symbol",
    "outline",
    "read_lines",
    "search",
];

fn known(name: &str) -> bool {
    TOOL_NAMES.contains(&name)
}

/// Tools that change nothing (no approval, allowed in plan mode).
fn read_only(name: &str) -> bool {
    agent_tools::READ_ONLY.contains(&name) || crate::mcp::INDEX_TOOLS.contains(&name)
}

/// The tool definitions sent to the model.
fn tool_definitions() -> Value {
    let mut all = agent_tools::definitions()
        .as_array()
        .cloned()
        .unwrap_or_default();
    all.extend(
        crate::mcp::index_tool_definitions()
            .as_array()
            .cloned()
            .unwrap_or_default(),
    );
    Value::Array(all)
}

fn system_prompt(root: &Path, mode: &str, model: &str) -> String {
    let (_, _, shell) = agent_tools::shell();
    let mut p = format!(
        "You are the Qanuvi agent, the coding agent built into the Qanuvi editor. Right now you run on the model {model}; \
         if asked who or what you are, say that, and do not claim to be any other assistant or company's product.\n\
         You work in the project folder {} on {}. The Bash tool runs {shell}.\n\
         Use the tools: look at files before changing them. To find your way, prefer find_symbol (where something is \
         defined), outline (what a file defines) and project_map, then read just the lines you need with read_lines; \
         search finds text. Read whole files only when they are small. Change files only with Edit or Write \
         (read the lines first, so old_string matches exactly); \
         make the smallest correct change; never guess what a file contains. Paths are relative to the project folder. \
         Call tools through the tool interface, one step at a time. When the work is done, answer with a short summary \
         of what you changed. Answer questions briefly.",
        root.display(),
        std::env::consts::OS
    );
    // What Qanuvi is and how its plans work (24g), so no model has to search for it.
    p.push_str(
        "

",
    );
    p.push_str(crate::brief::BRIEF);
    // The project's own instructions, kept short.
    if let Ok(rules) = std::fs::read_to_string(root.join("AGENTS.md")) {
        let rules: String = rules.chars().take(4000).collect();
        if !rules.trim().is_empty() {
            p.push_str(
                "

The project's instructions (AGENTS.md):
",
            );
            p.push_str(rules.trim());
        }
    }
    if mode == "plan" {
        p.push_str(
            "\nPlan mode: change nothing and run no commands. Read what you need, then answer with a short numbered plan.",
        );
    }
    p
}

/// Rough token count (about 3.5 characters a token for code and English).
fn tokens_of(v: &Value) -> usize {
    v.to_string().len() * 2 / 7
}

/// Keeps the conversation inside the model's context: old tool answers are shortened first, then the oldest
/// exchanges are left out (always from a user message on, so no answer loses its question).
pub fn fit(history: &[Value], budget: usize) -> Vec<Value> {
    let mut h: Vec<Value> = history.to_vec();
    let total = |h: &[Value]| h.iter().map(tokens_of).sum::<usize>();
    if total(&h) <= budget {
        return h;
    }
    let keep_whole = h.len().saturating_sub(6);
    for m in h.iter_mut().take(keep_whole) {
        if m["role"] == "tool"
            && let Some(c) = m["content"].as_str()
            && c.len() > 400
        {
            let mut cut = 300;
            while !c.is_char_boundary(cut) {
                cut -= 1;
            }
            m["content"] = json!(format!("{}… (older output shortened)", &c[..cut]));
        }
    }
    while total(&h) > budget {
        // The next user message after the first one: drop everything before it.
        let next = h
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, m)| m["role"] == "user")
            .map(|(i, _)| i);
        match next {
            Some(i) => {
                h.drain(..i);
            }
            None => break,
        }
    }
    // One long turn: shorten its tool answers too, all but the newest.
    if total(&h) > budget {
        let last = h.len().saturating_sub(1);
        for m in h.iter_mut().take(last) {
            if m["role"] == "tool"
                && let Some(c) = m["content"].as_str()
                && c.len() > 1200
            {
                let mut cut = 1000;
                while !c.is_char_boundary(cut) {
                    cut -= 1;
                }
                m["content"] = json!(format!("{}… (shortened to fit)", &c[..cut]));
            }
        }
    }
    h
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

struct Agent {
    out: Out,
    root: PathBuf,
    opts: Options,
    setup: Setup,
    session: String,
    history: Vec<Value>,
    records: Records,
    input: Receiver<Value>,
    queued: VecDeque<Value>,
    cost_total: f64,
    asked: u64,
    /// The code index of the project (built on first use, kept up to date as files change).
    index: crate::mcp::Server,
}

enum Answer {
    Allow(Value),
    Deny,
    Gone,
}

impl Agent {
    /// Tells the window which model works on the turn now and why.
    fn notice(&self, model: &str, text: &str) {
        self.out.send(&json!({
            "type": "system", "subtype": "qanuvi_route", "text": text, "model": model, "session_id": self.session
        }));
    }

    /// Why the brain gave the turn to this model first, in words.
    fn why(&self, model: &str, job: Job) -> String {
        let local = self.setup.candidate(model).is_some_and(|c| c.local);
        let place = if local {
            "on this computer"
        } else {
            "in the cloud"
        };
        if self.opts.model != brain::AUTO {
            return format!("{model} ({place}) works on this: you chose it.");
        }
        match (job, local) {
            (Job::Light, true) => {
                format!("A question: {model} on this computer answers it, which costs nothing.")
            }
            (Job::Light, false) => format!(
                "A question: {model} ({place}) answers it; no model on this computer is set up."
            ),
            (Job::Code, _) => format!(
                "A change: {model} ({place}) works on it, the best model in your list for changes."
            ),
        }
    }

    fn save(&self) {
        let f = history_file(&self.opts.data, &self.session);
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, Value::Array(self.history.clone()).to_string());
    }

    /// Asks the user (through Qanuvi) whether a tool may run; other messages that arrive meanwhile wait their turn.
    fn ask(&mut self, tool_use_id: &str, name: &str, input: &Value) -> Answer {
        self.asked += 1;
        let request_id = format!("qx-perm-{}", self.asked);
        self.out.send(&json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {"subtype": "can_use_tool", "tool_name": name, "input": input, "tool_use_id": tool_use_id}
        }));
        loop {
            let Ok(msg) = self.input.recv() else {
                return Answer::Gone;
            };
            if msg["type"] == "control_response" && msg["response"]["request_id"] == request_id {
                let r = &msg["response"]["response"];
                return if r["behavior"] == "allow" {
                    let updated = &r["updatedInput"];
                    Answer::Allow(if updated.is_object() {
                        updated.clone()
                    } else {
                        input.clone()
                    })
                } else {
                    Answer::Deny
                };
            }
            self.queued.push_back(msg);
        }
    }

    fn needs_approval(&self, name: &str, input: &Value) -> bool {
        if read_only(name) {
            return false;
        }
        match self.opts.mode.as_str() {
            "acceptEdits" => name == "Bash",
            "auto" => name == "Bash" && agent_tools::risky(input["command"].as_str().unwrap_or("")),
            _ => true,
        }
    }

    fn budget(&self, model: &str) -> usize {
        let ctx = self
            .setup
            .candidate(model)
            .map(|c| c.context)
            .filter(|c| *c > 0)
            .unwrap_or_else(|| match self.setup.endpoint_of(model) {
                Some((e, _)) if e.kind == llm::Kind::Ollama => llm::OLLAMA_CONTEXT,
                _ => 32_000,
            }) as usize;
        // Room for the instructions, the tool descriptions and the answer.
        ctx.saturating_sub(3_500).max(2_000) * 3 / 4
    }

    /// Messages the user sent while a turn runs: added to the conversation now (other requests wait their turn).
    fn take_added_messages(&mut self) {
        while let Ok(m) = self.input.try_recv() {
            if m["type"] != "user" {
                self.queued.push_back(m);
                continue;
            }
            let text = text_of(&m["message"]["content"]);
            if text.trim().is_empty() {
                continue;
            }
            self.history.push(json!({
                "role": "user",
                "content": format!("(The user added this while you were working; take it into account from now on:)\n{text}")
            }));
            self.notice("", "Your new message reached the agent while it was working; it takes it into account now.");
        }
    }

    fn turn(&mut self, text: &str) {
        let start = Instant::now();
        let job = if self.opts.mode == "plan" {
            Job::Light
        } else {
            brain::job_of(text)
        };
        let order = brain::order(&self.setup, &self.opts.model, job, &self.records, now());
        self.history.push(json!({"role": "user", "content": text}));
        let tools = tool_definitions();
        let (mut input_tokens, mut output_tokens, mut cost) = (0i64, 0i64, 0.0f64);
        let mut at = 0usize;
        let mut rounds = 0usize;
        let (mut calls, mut broken, mut broken_in_row) = (0u32, 0u32, 0u32);
        let mut final_text = String::new();
        let mut problems: Vec<String> = Vec::new();
        // Local servers (Ollama, OmniRoute) this turn already tried to start.
        let mut started: Vec<String> = Vec::new();
        let mut used = String::new();
        let outcome: Result<(), String> = loop {
            let Some(model) = order.get(at).cloned() else {
                // A model the user chose is the only one tried; when it fails, the user is offered Auto (the window
                // shows a button for this exact wording).
                break Err(if self.opts.model != brain::AUTO {
                    format!(
                        "{} did not answer: {}. Switch to Auto to let Qanuvi core use your other models.",
                        self.opts.model,
                        problems.join(" · ")
                    )
                } else {
                    format!("No model could answer. {}", problems.join(" · "))
                });
            };
            if model != used {
                if used.is_empty() {
                    self.notice(&model, &self.why(&model, job));
                }
                if !used.is_empty() {
                    self.records.turn(&used, false, calls, broken);
                    (calls, broken) = (0, 0);
                }
                used = model.clone();
            }
            if rounds >= MAX_ROUNDS {
                break Err(format!("Stopped after {MAX_ROUNDS} steps."));
            }
            rounds += 1;
            let Some((ep, name)) = self.setup.endpoint_of(&model) else {
                at += 1;
                continue;
            };
            let ep = ep.clone();
            // What the user wrote while this turn runs is taken in before the next step, as a person would.
            self.take_added_messages();
            let system = system_prompt(&self.root, &self.opts.mode, &model);
            let mut messages = vec![json!({"role": "system", "content": system})];
            messages.extend(fit(&self.history, self.budget(&model)));
            // Text is shown as it is written (the same partial-message events as the CLI agent's); the whole
            // message follows and takes its place.
            let out = &self.out;
            let session = self.session.clone();
            let mut on_text = |t: &str| {
                out.send(&json!({
                    "type": "stream_event", "session_id": session,
                    "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": t}}
                }));
            };
            // A model on this computer loads only when the computer has room for it now; otherwise the next one
            // takes the turn (and this one is left out for two minutes).
            if ep.kind == llm::Kind::Ollama
                && let Some(need) = llm::ollama_need_gb(&ep, &name)
                && need > 0.0
                && let Err(why) = crate::machine::check(&name, need)
            {
                self.records
                    .failed(&model, llm::FailKind::Down, &why, now());
                problems.push(format!("{model}: {why}"));
                at += 1;
                let next = order.get(at).cloned().unwrap_or_default();
                self.notice(&next, &format!("Not loading {model}: {why}"));
                continue;
            }
            let reply = match llm::chat(&ep, &name, &messages, &tools, &TOOL_NAMES, &mut on_text) {
                Ok(r) => r,
                Err(f) => {
                    // Ollama or OmniRoute here but not running: start it once, then ask the same model again.
                    if f.kind == llm::FailKind::Down
                        && crate::local_servers::program_for(&ep).is_some()
                        && !started.contains(&ep.id)
                    {
                        started.push(ep.id.clone());
                        self.notice(
                            &model,
                            &format!("{} is not running; Qanuvi starts it.", ep.id),
                        );
                        if crate::local_servers::start(&ep, None).is_ok() {
                            continue;
                        }
                    }
                    self.records.failed(&model, f.kind, &f.reason, now());
                    problems.push(format!("{model}: {}", f.reason));
                    at += 1;
                    if let Some(next) = order.get(at) {
                        self.notice(
                            next,
                            &format!(
                                "{model} could not answer ({}); {next} takes over.",
                                f.reason
                            ),
                        );
                    }
                    continue;
                }
            };
            input_tokens += reply.input_tokens;
            output_tokens += reply.output_tokens;
            cost += reply.cost_usd;

            // What the user sees, in the command-line agent's shape.
            let mut blocks = Vec::new();
            if !reply.text.trim().is_empty() {
                blocks.push(json!({"type": "text", "text": reply.text}));
            }
            for c in &reply.calls {
                blocks.push(
                    json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.input}),
                );
            }
            if blocks.is_empty() {
                // An empty answer: let the next model try.
                problems.push(format!("{model}: an empty answer"));
                at += 1;
                continue;
            }
            self.out.send(&json!({
                "type": "assistant",
                "message": {"id": format!("msg_{}", crate::llm::short_id()), "role": "assistant", "model": model, "content": blocks},
                "parent_tool_use_id": null,
                "session_id": self.session
            }));
            let mut msg = json!({"role": "assistant", "content": reply.text});
            if !reply.calls.is_empty() {
                msg["tool_calls"] = Value::Array(
                    reply
                        .calls
                        .iter()
                        .map(|c| {
                            json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.input.to_string()}})
                        })
                        .collect(),
                );
            }
            self.history.push(msg);
            if reply.calls.is_empty() {
                final_text = reply.text;
                break Ok(());
            }

            let mut results = Vec::new();
            let mut stop_turn = false;
            for c in reply.calls {
                calls += 1;
                let bad = !known(&c.name) || c.input.get("_unparsed").is_some();
                let outcome = if bad {
                    broken += 1;
                    broken_in_row += 1;
                    agent_tools::Outcome {
                        text: format!(
                            "That tool call could not be used. The tools are {}; give the arguments as a JSON object.",
                            TOOL_NAMES.join(", ")
                        ),
                        is_error: true,
                    }
                } else if self.opts.mode == "plan" && !read_only(&c.name) {
                    agent_tools::Outcome {
                        text: "Plan mode: nothing may be changed or run. Answer with the plan."
                            .into(),
                        is_error: true,
                    }
                } else if let Some(refused) = agent_tools::precheck(&self.root, &c.name, &c.input) {
                    // A change that cannot work is answered at once; the user is not asked about it.
                    refused
                } else {
                    broken_in_row = 0;
                    let input = if self.needs_approval(&c.name, &c.input) {
                        match self.ask(&c.id, &c.name, &c.input) {
                            Answer::Allow(i) => Some(i),
                            Answer::Deny => None,
                            Answer::Gone => {
                                stop_turn = true;
                                None
                            }
                        }
                    } else {
                        Some(c.input.clone())
                    };
                    match input {
                        Some(i) if crate::mcp::INDEX_TOOLS.contains(&c.name.as_str()) => {
                            match self.index.call(&c.name, &i) {
                                Ok(text) => agent_tools::Outcome { text, is_error: false },
                                Err(text) => agent_tools::Outcome { text, is_error: true },
                            }
                        }
                        Some(i) => agent_tools::run(&self.root, &c.name, &i),
                        None => agent_tools::Outcome {
                            text: "The user refused this action. Do not try it again; ask or do something else.".into(),
                            is_error: true,
                        },
                    }
                };
                // The session view gets a short answer; the model the whole (already shortened) one.
                let shown: String = outcome.text.chars().take(2_000).collect();
                results.push(json!({"type": "tool_result", "tool_use_id": c.id, "content": shown, "is_error": outcome.is_error}));
                self.history.push(json!({"role": "tool", "tool_call_id": c.id, "name": c.name, "content": outcome.text}));
            }
            self.out.send(&json!({
                "type": "user",
                "message": {"role": "user", "content": results},
                "parent_tool_use_id": null,
                "session_id": self.session
            }));
            if stop_turn {
                break Err("Qanuvi closed the session.".into());
            }
            // A model that keeps getting tool calls wrong hands the turn to the next one.
            if broken_in_row >= 2 && at + 1 < order.len() {
                problems.push(format!("{model}: broken tool calls"));
                at += 1;
                broken_in_row = 0;
                self.notice(
                    &order[at],
                    &format!(
                        "{model} keeps getting tool calls wrong; {} takes over.",
                        order[at]
                    ),
                );
            }
        };
        if !used.is_empty() {
            self.records.turn(&used, outcome.is_ok(), calls, broken);
        }
        self.records.save(&self.opts.data);
        self.cost_total += cost;
        self.save();
        let (is_error, subtype, result) = match &outcome {
            Ok(()) => (false, "success", final_text),
            Err(e) => (true, "error_during_execution", e.clone()),
        };
        self.out.send(&json!({
            "type": "result",
            "subtype": subtype,
            "is_error": is_error,
            "duration_ms": start.elapsed().as_millis() as i64,
            "num_turns": rounds,
            "result": result,
            "session_id": self.session,
            "model": used,
            "total_cost_usd": self.cost_total,
            "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens}
        }));
    }
}

/// `--agent [--model M] [--permission-mode MODE] [--resume SESSION] [--data DIR]`, in the project folder.
pub fn run(args: &[String]) -> i32 {
    let setup: Setup = match std::env::var(SETUP_ENV)
        .map_err(|_| "No models are set up.".to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .and_then(|s: Setup| s.check().map(|_| s))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && tx.send(v).is_err()
            {
                break;
            }
        }
    });
    serve(args, setup, root, rx, Out::stdout())
}

/// The agent's conversation: messages from `input` until it closes, lines to `out`.
pub fn serve(args: &[String], setup: Setup, root: PathBuf, rx: Receiver<Value>, out: Out) -> i32 {
    let opts = options(args);
    let session = opts
        .resume
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let history: Vec<Value> = std::fs::read_to_string(history_file(&opts.data, &session))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let records = Records::load(&opts.data);
    out.send(&json!({
        "type": "system", "subtype": "init", "session_id": session,
        "model": if opts.model == brain::AUTO { "Qanuvi core (auto)".to_string() } else { opts.model.clone() },
        "cwd": root.to_string_lossy(), "tools": TOOL_NAMES, "permissionMode": opts.mode
    }));
    let index = crate::mcp::Server::new(&root);
    let mut agent = Agent {
        index,
        out,
        root,
        opts,
        setup,
        session,
        history,
        records,
        input: rx,
        queued: VecDeque::new(),
        cost_total: 0.0,
        asked: 0,
    };
    loop {
        let msg = match agent.queued.pop_front() {
            Some(m) => m,
            None => match agent.input.recv() {
                Ok(m) => m,
                Err(_) => return 0,
            },
        };
        match msg["type"].as_str().unwrap_or("") {
            "control_request" => {
                // The same requests the CLI agent takes: a model or permission mode chosen mid-session.
                let r = &msg["request"];
                match r["subtype"].as_str().unwrap_or("") {
                    "set_model" => {
                        if let Some(m) = r["model"].as_str().filter(|m| !m.trim().is_empty()) {
                            agent.opts.model = m.trim().to_string();
                        }
                    }
                    "set_permission_mode" => {
                        if let Some(m) = r["mode"].as_str().filter(|m| !m.trim().is_empty()) {
                            agent.opts.mode = m.trim().to_string();
                        }
                    }
                    _ => {}
                }
                agent.out.send(&json!({
                    "type": "control_response",
                    "response": {"subtype": "success", "request_id": msg["request_id"], "response": {}}
                }))
            }
            "user" => {
                let content = &msg["message"]["content"];
                let images = content
                    .as_array()
                    .map_or(0, |a| a.iter().filter(|b| b["type"] == "image").count());
                if images > 0 {
                    agent.notice(
                        "",
                        &format!(
                            "{images} attached image{} left out: Qanuvi core's models read text only. Claude Code can see images.",
                            if images == 1 { "" } else { "s" }
                        ),
                    );
                }
                let text = text_of(content);
                if !text.trim().is_empty() {
                    agent.turn(&text);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_conversation_inside_the_context() {
        let big = "x".repeat(4000);
        let mut h = Vec::new();
        for i in 0..6 {
            h.push(json!({"role": "user", "content": format!("question {i}")}));
            h.push(json!({"role": "assistant", "content": "", "tool_calls": [{"id": format!("c{i}"), "type": "function", "function": {"name": "Read", "arguments": "{}"}}]}));
            h.push(json!({"role": "tool", "tool_call_id": format!("c{i}"), "content": big}));
            h.push(json!({"role": "assistant", "content": "done"}));
        }
        // Enough room: unchanged.
        assert_eq!(fit(&h, 1_000_000).len(), h.len());
        // Less room: old tool answers shortened, the newest kept whole.
        let f = fit(&h, 4_000);
        assert_eq!(f.len(), h.len());
        assert!(f[2]["content"].as_str().unwrap().contains("shortened"));
        assert_eq!(f[h.len() - 2]["content"].as_str().unwrap().len(), 4000);
        // Very little room: whole old exchanges go, starting at a user message.
        let f = fit(&h, 1_500);
        assert!(f.len() < h.len());
        assert_eq!(f[0]["role"], "user");
        assert_eq!(f.last().unwrap()["content"], "done");
    }

    /// A stand-in OpenAI-compatible model server: "busy" is rate limited; "good" reads a file, edits it, then answers.
    fn fake_models() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let mut conn = conn;
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let body = loop {
                    let n = conn.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break String::new();
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            break String::from_utf8_lossy(&buf[end + 4..end + 4 + len])
                                .to_string();
                        }
                    }
                };
                let req: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let model = req["model"].as_str().unwrap_or("").to_string();
                log.lock().unwrap().push(model.clone());
                let tools_done = req["messages"]
                    .as_array()
                    .map(|m| m.iter().filter(|x| x["role"] == "tool").count())
                    .unwrap_or(0);
                let call = |name: &str, args: Value| {
                    json!({"choices": [{"message": {"role": "assistant", "content": "", "tool_calls": [
                    {"id": format!("c{tools_done}"), "type": "function", "function": {"name": name, "arguments": args.to_string()}}
                ]}}], "usage": {"prompt_tokens": 100, "completion_tokens": 10}})
                };
                let (status, answer) = match (model.as_str(), tools_done) {
                    ("busy", _) => (429, json!({"error": {"message": "rate limited"}})),
                    (_, 0) => (200, call("Read", json!({"file_path": "notes.txt"}))),
                    (_, 1) => (
                        200,
                        call(
                            "Edit",
                            json!({"file_path": "notes.txt", "old_string": "hello", "new_string": "hello world"}),
                        ),
                    ),
                    _ => (
                        200,
                        json!({"choices": [{"message": {"role": "assistant", "content": "Done: notes.txt updated."}}], "usage": {"prompt_tokens": 120, "completion_tokens": 8}}),
                    ),
                };
                // Answers are streamed like real servers do: text and tool arguments in pieces, usage at the end.
                let (kind, text) = if status == 200 {
                    let msg = &answer["choices"][0]["message"];
                    let content = msg["content"].as_str().unwrap_or("");
                    let (a, b) = content.split_at(content.len() / 2);
                    let mut events = Vec::new();
                    for piece in [a, b].into_iter().filter(|p| !p.is_empty()) {
                        events.push(json!({"choices": [{"delta": {"content": piece}}]}));
                    }
                    for (i, c) in msg["tool_calls"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        let args = c["function"]["arguments"].as_str().unwrap_or("");
                        let (x, y) = args.split_at(args.len() / 2);
                        events.push(json!({"choices": [{"delta": {"tool_calls": [{"index": i, "id": c["id"], "function": {"name": c["function"]["name"], "arguments": x}}]}}]}));
                        events.push(json!({"choices": [{"delta": {"tool_calls": [{"index": i, "function": {"arguments": y}}]}}]}));
                    }
                    events.push(json!({"choices": [], "usage": answer["usage"]}));
                    let body: String = events
                        .iter()
                        .map(|e| format!(": keep-alive\n\ndata: {e}\n\n"))
                        .collect();
                    ("text/event-stream", format!("{body}data: [DONE]\n\n"))
                } else {
                    ("application/json", answer.to_string())
                };
                let _ = write!(
                    conn,
                    "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (url, seen)
    }

    #[test]
    fn has_the_code_index_tools_read_only() {
        let defs = tool_definitions();
        let names: Vec<&str> = defs
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, TOOL_NAMES);
        assert!(defs[7]["function"]["parameters"]["required"][0] == "name");
        for n in crate::mcp::INDEX_TOOLS {
            assert!(known(n) && read_only(n), "{n}");
        }
        assert!(!read_only("Edit"));
        // The index answers through the agent's own copy.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn total() -> i32 { 1 }\n").unwrap();
        let mut index = crate::mcp::Server::new(dir.path());
        let hit = index
            .call("find_symbol", &json!({"name": "total"}))
            .unwrap();
        assert!(hit.contains("a.rs:1"), "{hit}");
    }

    #[test]
    fn works_a_turn_through_the_brain_with_approval_and_fallback() {
        let (url, seen) = fake_models();
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.txt"), "hello\n").unwrap();
        let data = tempfile::tempdir().unwrap();
        let setup: Setup = serde_json::from_value(json!({
            "endpoints": [{"id": "fake", "kind": "openai", "url": url}],
            "models": [{"id": "fake/busy"}, {"id": "fake/good"}]
        }))
        .unwrap();
        let (tx, rx) = channel();
        let (out_tx, out_rx) = channel::<Value>();
        let out_tx = std::sync::Mutex::new(out_tx);
        let args: Vec<String> = ["--model", "auto", "--permission-mode", "default", "--data"]
            .iter()
            .map(|s| s.to_string())
            .chain([data.path().to_string_lossy().to_string()])
            .collect();
        let root = project.path().to_path_buf();
        let agent = std::thread::spawn(move || {
            serve(
                &args,
                setup,
                root,
                rx,
                Out::to(move |v| {
                    let _ = out_tx.lock().unwrap().send(v.clone());
                }),
            )
        });
        tx.send(
            json!({"type": "user", "message": {"role": "user", "content": "Update the notes"}}),
        )
        .unwrap();
        let mut lines = Vec::new();
        loop {
            let v = out_rx
                .recv_timeout(std::time::Duration::from_secs(20))
                .expect("the agent stopped answering");
            if v["type"] == "control_request" {
                // The edit needs the user's approval.
                assert_eq!(v["request"]["tool_name"], "Edit");
                tx.send(json!({"type": "control_response", "response": {"subtype": "success", "request_id": v["request_id"],
                    "response": {"behavior": "allow", "updatedInput": v["request"]["input"]}}})).unwrap();
            }
            let done = v["type"] == "result";
            lines.push(v);
            if done {
                break;
            }
        }
        // The answer's text arrived in pieces before the whole message.
        let streamed: String = lines
            .iter()
            .filter(|v| v["type"] == "stream_event")
            .map(|v| v["event"]["delta"]["text"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(streamed, "Done: notes.txt updated.");
        // The brain says which model it picked and why, then which one took over.
        let routes: Vec<&Value> = lines
            .iter()
            .filter(|v| v["subtype"] == "qanuvi_route")
            .collect();
        assert_eq!(routes[0]["model"], "fake/busy");
        assert!(routes[0]["text"].as_str().unwrap().starts_with("A change:"));
        assert_eq!(routes[1]["model"], "fake/good");
        let kinds: Vec<String> = lines
            .iter()
            .filter(|v| v["type"] != "stream_event")
            .map(|v| {
                format!(
                    "{}{}",
                    v["type"].as_str().unwrap_or(""),
                    v["subtype"]
                        .as_str()
                        .map(|s| format!(":{s}"))
                        .unwrap_or_default()
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "system:init",
                "system:qanuvi_route",
                "system:qanuvi_route",
                "assistant",
                "user",
                "assistant",
                "control_request",
                "user",
                "assistant",
                "result:success"
            ]
        );
        let result = lines.last().unwrap();
        assert_eq!(result["is_error"], false);
        assert_eq!(result["result"], "Done: notes.txt updated.");
        assert_eq!(result["model"], "fake/good");
        assert_eq!(result["usage"]["input_tokens"], 320);
        assert_eq!(
            std::fs::read_to_string(project.path().join("notes.txt")).unwrap(),
            "hello world\n"
        );
        assert_eq!(
            seen.lock().unwrap().first().map(String::as_str),
            Some("busy")
        );
        // The brain rests the busy model and kept the conversation for a resume.
        let records = Records::load(data.path());
        assert!(records.models["fake/busy"].rest_until > now());
        assert_eq!(records.models["fake/good"].finished, 1);
        let session = lines[0]["session_id"].as_str().unwrap().to_string();
        assert!(history_file(data.path(), &session).is_file());
        drop(tx);
        assert_eq!(agent.join().unwrap(), 0);
    }

    #[test]
    fn reads_the_options() {
        let a: Vec<String> = [
            "--model",
            "ollama/qwen2.5-coder:3b",
            "--permission-mode",
            "plan",
            "--resume",
            "s1",
            "--data",
            "d",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let o = options(&a);
        assert_eq!(o.model, "ollama/qwen2.5-coder:3b");
        assert_eq!(o.mode, "plan");
        assert_eq!(o.resume.as_deref(), Some("s1"));
        assert_eq!(o.data, PathBuf::from("d"));
        assert_eq!(options(&[]).model, "auto");
    }
}
