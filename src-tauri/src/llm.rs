//! Talking to language models for Qanuvi's own agent (Milestone 24): Ollama on this machine or the network, and any
//! OpenAI-compatible API (llama.cpp's server, LM Studio, OmniRoute, OpenRouter and others). Messages are kept in the
//! OpenAI chat format and converted for Ollama.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Ollama's own API (`/api/chat`), which lets Qanuvi set the context size.
    Ollama,
    /// Any OpenAI-compatible API (`/chat/completions`).
    Openai,
    /// OpenRouter: OpenAI-compatible, with prices and free models in its model list.
    Openrouter,
}

/// A place models are reached: its id is the first part of a model id (`ollama/qwen2.5-coder:3b`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub kind: Kind,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// The context size Qanuvi asks Ollama for (its own default is small for an agent).
pub const OLLAMA_CONTEXT: u32 = 16384;

fn is_local_host(url: &str) -> bool {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("");
    let host = if host.starts_with('[') {
        host.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

impl Endpoint {
    /// Checks the endpoint: a short id of letters, digits and dashes, and an http(s) address. A key is never sent over
    /// plain http to another machine.
    pub fn check(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self.id.len() <= 32
            && self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !id_ok {
            return Err(format!(
                "A model provider's id must be letters, digits or dashes: {}",
                self.id
            ));
        }
        let url = self.url.trim();
        let ok = (url.starts_with("http://") || url.starts_with("https://"))
            && url.len() <= 300
            && !url.chars().any(|c| c.is_whitespace() || c.is_control());
        if !ok {
            return Err(format!(
                "The address must start with http:// or https://: {url}"
            ));
        }
        if self.key.as_deref().is_some_and(|k| !k.is_empty())
            && url.starts_with("http://")
            && !is_local_host(url)
        {
            return Err(format!(
                "{} has a key but its address is not https; the key would travel unencrypted.",
                self.id
            ));
        }
        Ok(())
    }

    /// Models on this machine cost nothing and send nothing away.
    pub fn is_local(&self) -> bool {
        is_local_host(&self.url)
    }

    fn base(&self) -> &str {
        self.url.trim().trim_end_matches('/')
    }
}

/// A short random id for tool calls and messages.
pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// A tool the model asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// What the provider says the request cost (OpenRouter tells it; others are free or unknown).
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// Rate limited or overloaded: try another model now, this one again later.
    Busy,
    /// The model cannot be used (not found, no tools, no credits, refused key): skip it.
    Unusable,
    /// The provider could not be reached.
    Down,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub kind: FailKind,
    pub reason: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

fn fail(kind: FailKind, reason: impl Into<String>) -> Failure {
    Failure {
        kind,
        reason: reason.into(),
    }
}

fn http(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(timeout))
            .build(),
    )
}

fn post(ep: &Endpoint, path: &str, body: &Value, timeout: Duration) -> Result<Value, Failure> {
    let res = send(ep, path, body, timeout)?;
    json_answer(ep, res)
}

fn send(
    ep: &Endpoint,
    path: &str,
    body: &Value,
    timeout: Duration,
) -> Result<ureq::http::Response<ureq::Body>, Failure> {
    let url = format!("{}{path}", ep.base());
    let mut req = http(timeout)
        .post(&url)
        .header("Content-Type", "application/json");
    if let Some(k) = ep.key.as_deref().filter(|k| !k.is_empty()) {
        req = req.header("Authorization", &format!("Bearer {k}"));
    }
    if ep.kind == Kind::Openrouter {
        req = req
            .header("HTTP-Referer", "https://qanuvi.com")
            .header("X-Title", "Qanuvi");
    }
    req.send(body.to_string()).map_err(|e| {
        fail(
            FailKind::Down,
            format!("{} could not be reached: {e}", ep.id),
        )
    })
}

/// A whole JSON answer (or an error answer) read and checked.
fn json_answer(ep: &Endpoint, mut res: ureq::http::Response<ureq::Body>) -> Result<Value, Failure> {
    let status = res.status().as_u16();
    let text = res
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| fail(FailKind::Down, format!("{}: {e}", ep.id)))?;
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
    let message = error_text(&v).unwrap_or_else(|| text.chars().take(300).collect());
    match status {
        200..=299 => match error_text(&v) {
            // OpenRouter sometimes answers 200 with an error inside.
            Some(m) if v.get("choices").is_none() && v.get("message").is_none() => Err(classify(
                v["error"]["code"].as_u64().unwrap_or(500) as u16,
                &m,
            )),
            _ => Ok(v),
        },
        _ => Err(classify(status, &message)),
    }
}

fn error_text(v: &Value) -> Option<String> {
    match &v["error"] {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("message").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// Sorts an error answer into what the brain should do about it.
pub fn classify(status: u16, message: &str) -> Failure {
    let lower = message.to_lowercase();
    let kind = if status == 429
        || status == 503
        || status == 529
        || lower.contains("rate limit")
        || lower.contains("overloaded")
    {
        FailKind::Busy
    } else if (400..500).contains(&status) {
        FailKind::Unusable
    } else {
        FailKind::Busy
    };
    fail(kind, format!("{status}: {}", message.trim()))
}

fn to_ollama(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let mut m = m.clone();
            if let Some(calls) = m.get_mut("tool_calls").and_then(Value::as_array_mut) {
                for c in calls.iter_mut() {
                    let args = &c["function"]["arguments"];
                    if let Some(s) = args.as_str() {
                        c["function"]["arguments"] =
                            serde_json::from_str(s).unwrap_or_else(|_| json!({}));
                    }
                }
            }
            m
        })
        .collect()
}

/// Reads a streamed answer and puts it together in the shape of a whole one, passing text on as it arrives.
/// Ollama sends JSON lines; OpenAI-compatible servers send "data: {...}" events. A server that ignores the stream
/// request and answers in one piece works too.
fn streamed(
    ep: &Endpoint,
    path: &str,
    body: &Value,
    timeout: Duration,
    on_text: &mut dyn FnMut(&str),
) -> Result<Value, Failure> {
    use std::io::BufRead;
    let mut res = send(ep, path, body, timeout)?;
    let status = res.status().as_u16();
    let kind = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let whole = kind.contains("application/json") && !kind.contains("ndjson");
    if !(200..300).contains(&status) || whole {
        return json_answer(ep, res);
    }
    let reader = std::io::BufReader::new(
        res.body_mut()
            .with_config()
            .limit(50 * 1024 * 1024)
            .reader(),
    );
    let broken = |e: std::io::Error| {
        fail(
            FailKind::Down,
            format!("{}: the answer broke off ({e})", ep.id),
        )
    };
    let mut text = String::new();
    let mut calls: Vec<Value> = Vec::new();
    if ep.kind == Kind::Ollama {
        let mut last = Value::Null;
        for line in reader.lines() {
            let line = line.map_err(broken)?;
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(m) = error_text(&v) {
                return Err(classify(500, &m));
            }
            if let Some(t) = v["message"]["content"].as_str().filter(|t| !t.is_empty()) {
                text.push_str(t);
                on_text(t);
            }
            if let Some(c) = v["message"]["tool_calls"].as_array() {
                calls.extend(c.iter().cloned());
            }
            if v["done"].as_bool() == Some(true) {
                last = v;
                break;
            }
        }
        return Ok(json!({
            "message": {"content": text, "tool_calls": calls},
            "prompt_eval_count": last["prompt_eval_count"],
            "eval_count": last["eval_count"],
        }));
    }
    let mut usage = Value::Null;
    for line in reader.lines() {
        let line = line.map_err(broken)?;
        let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else {
            continue; // Blank lines and keep-alive comments.
        };
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(m) = error_text(&v) {
            return Err(classify(
                v["error"]["code"].as_u64().unwrap_or(500) as u16,
                &m,
            ));
        }
        if !v["usage"].is_null() {
            usage = v["usage"].clone();
        }
        let delta = &v["choices"][0]["delta"];
        if let Some(t) = delta["content"].as_str().filter(|t| !t.is_empty()) {
            text.push_str(t);
            on_text(t);
        }
        for (n, part) in delta["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let i = part["index"].as_u64().map(|i| i as usize).unwrap_or(n);
            while calls.len() <= i {
                calls.push(json!({"id": "", "function": {"name": "", "arguments": ""}}));
            }
            let call = &mut calls[i];
            if let Some(id) = part["id"].as_str().filter(|s| !s.is_empty()) {
                call["id"] = json!(id);
            }
            if let Some(name) = part["function"]["name"].as_str().filter(|s| !s.is_empty()) {
                call["function"]["name"] = json!(name);
            }
            match &part["function"]["arguments"] {
                Value::String(s) => {
                    let joined = format!(
                        "{}{s}",
                        call["function"]["arguments"].as_str().unwrap_or("")
                    );
                    call["function"]["arguments"] = json!(joined);
                }
                Value::Object(_) => {
                    call["function"]["arguments"] = part["function"]["arguments"].clone()
                }
                _ => {}
            }
        }
    }
    if text.is_empty() && calls.is_empty() {
        return Err(fail(FailKind::Busy, format!("{}: an empty answer", ep.id)));
    }
    Ok(json!({
        "choices": [{"message": {"role": "assistant", "content": text, "tool_calls": calls}}],
        "usage": usage,
    }))
}

fn without_empty_tools(mut body: Value) -> Value {
    if body["tools"].as_array().is_some_and(|t| t.is_empty())
        && let Some(o) = body.as_object_mut()
    {
        o.remove("tools");
        o.remove("tool_choice");
    }
    body
}

/// One step of a conversation: the model's answer to the messages so far, with the tools it may use. Text is passed
/// to `on_text` as it is written.
pub fn chat(
    ep: &Endpoint,
    model: &str,
    messages: &[Value],
    tools: &Value,
    known_tools: &[&str],
    on_text: &mut dyn FnMut(&str),
) -> Result<Reply, Failure> {
    // Small local models can be slow, especially the first time they load.
    let timeout = Duration::from_secs(if ep.is_local() { 600 } else { 240 });
    match ep.kind {
        Kind::Ollama => {
            let body = json!({
                "model": model,
                "messages": to_ollama(messages),
                "tools": tools,
                "stream": true,
                "keep_alive": "15m",
                "options": { "num_ctx": OLLAMA_CONTEXT },
            });
            // No tools (a decision or a review): leave the field out; some servers refuse an empty list.
            let body = without_empty_tools(body);
            let v = streamed(ep, "/api/chat", &body, timeout, on_text)?;
            let msg = &v["message"];
            let text = msg["content"].as_str().unwrap_or("").to_string();
            let mut calls: Vec<ToolCall> = msg["tool_calls"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(i, c)| ToolCall {
                            id: format!("call_{}_{i}", crate::llm::short_id()),
                            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
                            input: arguments(&c["function"]["arguments"]),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let text = if calls.is_empty() {
                let (rest, found) = calls_in_text(&text, known_tools);
                calls = found;
                rest
            } else {
                text
            };
            Ok(Reply {
                text,
                calls,
                input_tokens: v["prompt_eval_count"].as_i64().unwrap_or(0),
                output_tokens: v["eval_count"].as_i64().unwrap_or(0),
                cost_usd: 0.0,
            })
        }
        Kind::Openai | Kind::Openrouter => {
            let mut body = json!({
                "model": model,
                "messages": messages,
                "tools": tools,
                "tool_choice": "auto",
                "stream": true,
                "stream_options": { "include_usage": true },
            });
            if ep.kind == Kind::Openrouter {
                body["usage"] = json!({ "include": true });
            }
            let body = without_empty_tools(body);
            let v = streamed(ep, "/chat/completions", &body, timeout, on_text)?;
            let msg = &v["choices"][0]["message"];
            if msg.is_null() {
                return Err(fail(FailKind::Busy, format!("{}: an empty answer", ep.id)));
            }
            let text = msg["content"].as_str().unwrap_or("").to_string();
            let mut calls: Vec<ToolCall> = msg["tool_calls"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(i, c)| ToolCall {
                            id: c["id"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                                .unwrap_or_else(|| format!("call_{}_{i}", crate::llm::short_id())),
                            name: c["function"]["name"].as_str().unwrap_or("").to_string(),
                            input: arguments(&c["function"]["arguments"]),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let text = if calls.is_empty() {
                let (rest, found) = calls_in_text(&text, known_tools);
                calls = found;
                rest
            } else {
                text
            };
            let usage = &v["usage"];
            Ok(Reply {
                text,
                calls,
                input_tokens: usage["prompt_tokens"].as_i64().unwrap_or(0),
                output_tokens: usage["completion_tokens"].as_i64().unwrap_or(0),
                cost_usd: usage["cost"].as_f64().unwrap_or(0.0),
            })
        }
    }
}

/// Tool arguments arrive as an object (Ollama) or as JSON text (OpenAI).
fn arguments(v: &Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({ "_unparsed": s })),
        Value::Object(_) => v.clone(),
        _ => json!({}),
    }
}

/// Small models often write a tool call as JSON in their text instead of using the tool interface:
/// `{"name": "Read", "arguments": {"file_path": "a.txt"}}`, sometimes inside a code fence or `<tool_call>` tags.
/// Returns the text without those calls, and the calls (unknown tools included; the agent refuses those).
pub fn calls_in_text(text: &str, _known: &[&str]) -> (String, Vec<ToolCall>) {
    let mut calls = Vec::new();
    let mut keep = String::new();
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut stream = serde_json::Deserializer::from_str(&text[i..]).into_iter::<Value>();
            if let Some(Ok(v)) = stream.next() {
                let end = i + stream.byte_offset();
                let name = v["name"].as_str().unwrap_or("");
                let args = if v["arguments"].is_object() {
                    v["arguments"].clone()
                } else if v["parameters"].is_object() {
                    v["parameters"].clone()
                } else if let Some(s) = v["arguments"].as_str() {
                    serde_json::from_str(s).unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                // Shaped like a tool call: a call, even to a tool that does not exist (the agent then says which
                // tools there are, and the brain counts it as a broken call) rather than a final answer.
                if !name.is_empty() && args.is_object() {
                    calls.push(ToolCall {
                        id: format!("call_{}_{}", crate::llm::short_id(), calls.len()),
                        name: name.to_string(),
                        input: args,
                    });
                    i = end;
                    continue;
                }
            }
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        keep.push(ch);
        i += ch.len_utf8();
    }
    if calls.is_empty() {
        return (text.to_string(), calls);
    }
    // What is left of the wrappers around the calls.
    let mut rest = keep;
    for wrapper in ["<tool_call>", "</tool_call>", "```json", "```"] {
        rest = rest.replace(wrapper, "");
    }
    (rest.trim().to_string(), calls)
}

/// A model a provider offers.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// `endpoint/model`, the id sessions use.
    pub id: String,
    pub endpoint: String,
    pub name: String,
    /// Runs on this machine.
    pub local: bool,
    /// Costs nothing (local, or a free model).
    pub free: bool,
    /// Can use tools (an agent needs them); unknown counts as yes.
    pub tools: bool,
    /// Context size in tokens, when known.
    pub context: u32,
    /// Size on disk in bytes (local models).
    pub size: u64,
}

/// How much more memory (GB) an Ollama model needs to answer now: nothing when it is already loaded, else its size
/// plus room for the conversation (Qanuvi asks for an OLLAMA_CONTEXT-token context). None when Ollama cannot tell.
pub fn ollama_need_gb(ep: &Endpoint, model: &str) -> Option<f64> {
    let loaded = get(ep, "/api/ps").ok()?;
    let is = |m: &Value| m["name"] == model || m["model"] == model;
    if loaded["models"]
        .as_array()
        .is_some_and(|l| l.iter().any(is))
    {
        return Some(0.0);
    }
    let tags = get(ep, "/api/tags").ok()?;
    let size = tags["models"].as_array()?.iter().find(|m| is(m))?["size"].as_f64()?;
    Some(size / 1e9 * 1.1 + 0.6)
}

fn get(ep: &Endpoint, path: &str) -> Result<Value, String> {
    let url = format!("{}{path}", ep.base());
    let mut req = http(Duration::from_secs(20)).get(&url);
    if let Some(k) = ep.key.as_deref().filter(|k| !k.is_empty()) {
        req = req.header("Authorization", &format!("Bearer {k}"));
    }
    let mut res = req
        .call()
        .map_err(|e| format!("{} could not be reached: {e}", ep.id))?;
    let status = res.status().as_u16();
    let text = res
        .body_mut()
        .with_config()
        .limit(30 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "{}: {status} {}",
            ep.id,
            text.chars().take(200).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", ep.id))
}

/// The models a provider offers. OpenRouter's list is limited to free models that can use tools, unless `all`.
pub fn list_models(ep: &Endpoint, all: bool) -> Result<Vec<ModelInfo>, String> {
    ep.check()?;
    // A router running here (OmniRoute) passes requests on to models in the cloud: those are not on this computer.
    let local = ep.is_local() && crate::local_servers::program_for(ep) != Some("omniroute");
    let mut out = Vec::new();
    match ep.kind {
        Kind::Ollama => {
            let v = get(ep, "/api/tags")?;
            for m in v["models"].as_array().cloned().unwrap_or_default() {
                let name = m["name"].as_str().unwrap_or("").to_string();
                if name.is_empty() {
                    continue;
                }
                // Ollama tells which models can use tools.
                let show = post(
                    ep,
                    "/api/show",
                    &json!({ "model": name }),
                    Duration::from_secs(20),
                )
                .ok();
                let tools = show
                    .as_ref()
                    .and_then(|s| s["capabilities"].as_array())
                    .map(|c| c.iter().any(|x| x == "tools"))
                    .unwrap_or(true);
                let context = show
                    .as_ref()
                    .and_then(|s| s["model_info"].as_object())
                    .and_then(|o| {
                        o.iter()
                            .find(|(k, _)| k.ends_with(".context_length"))
                            .and_then(|(_, v)| v.as_u64())
                    })
                    .map(|c| (c as u32).min(OLLAMA_CONTEXT))
                    .unwrap_or(OLLAMA_CONTEXT);
                out.push(ModelInfo {
                    id: format!("{}/{name}", ep.id),
                    endpoint: ep.id.clone(),
                    name,
                    local,
                    free: true,
                    tools,
                    context,
                    size: m["size"].as_u64().unwrap_or(0),
                });
            }
        }
        Kind::Openai | Kind::Openrouter => {
            let v = get(ep, "/models")?;
            // OmniRoute here: only the models it marks available, plus its routes and combos.
            let available = if crate::local_servers::program_for(ep) == Some("omniroute") && !all {
                crate::local_servers::omniroute_available(ep)
            } else {
                None
            };
            for m in v["data"].as_array().cloned().unwrap_or_default() {
                let name = m["id"].as_str().unwrap_or("").to_string();
                if name.is_empty() || !chat_model(&m) {
                    continue;
                }
                if let Some(ok) = &available
                    && !(ok.contains(&name) || m["owned_by"] == "combo" || name.starts_with("auto"))
                {
                    continue;
                }
                let price = |k: &str| {
                    m["pricing"][k]
                        .as_str()
                        .and_then(|s| s.parse::<f64>().ok())
                        .or_else(|| m["pricing"][k].as_f64())
                };
                let free = ep.is_local()
                    || name.ends_with(":free")
                    || (price("prompt") == Some(0.0) && price("completion") == Some(0.0));
                let tools = m["supported_parameters"]
                    .as_array()
                    .map(|p| p.iter().any(|x| x == "tools"))
                    .unwrap_or(true);
                if ep.kind == Kind::Openrouter && !all && !(free && tools) {
                    continue;
                }
                out.push(ModelInfo {
                    id: format!("{}/{name}", ep.id),
                    endpoint: ep.id.clone(),
                    name,
                    local,
                    free,
                    tools,
                    context: m["context_length"].as_u64().unwrap_or(0) as u32,
                    size: 0,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Whether a listed model answers chat (routers such as OmniRoute also list image, audio and embedding models).
pub fn chat_model(m: &Value) -> bool {
    const OTHER: [&str; 10] = [
        "image",
        "embedding",
        "embeddings",
        "audio",
        "tts",
        "stt",
        "video",
        "rerank",
        "moderation",
        "transcription",
    ];
    let kind = m["type"].as_str().unwrap_or("").to_lowercase();
    if OTHER.contains(&kind.as_str()) {
        return false;
    }
    match m["capabilities"].as_array() {
        Some(c) if !c.is_empty() => c
            .iter()
            .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case("chat"))),
        _ => true,
    }
}

/// Checks a provider's key and says so in a sentence. OpenRouter also tells how many free requests are left today.
pub fn check_key(ep: &Endpoint) -> Result<String, String> {
    ep.check()?;
    if ep.kind == Kind::Openrouter {
        if ep.key.as_deref().is_none_or(str::is_empty) {
            return Err("Save the key first.".into());
        }
        let v = get(ep, "/key")?;
        let d = &v["data"];
        let mut out = String::from("The key works.");
        let free = &d["free_model_daily_requests"];
        if let (Some(left), Some(limit)) = (free["remaining"].as_i64(), free["limit"].as_i64()) {
            out.push_str(&format!(" Free requests left today: {left} of {limit}."));
        }
        if d["is_free_tier"].as_bool() == Some(true) {
            out.push_str(
                " No credit bought yet: 50 free requests a day (1,000 after buying $10 once).",
            );
        }
        return Ok(out);
    }
    let n = list_models(ep, true)?.len();
    Ok(format!(
        "It answers: {n} model{} offered.",
        if n == 1 { "" } else { "s" }
    ))
}

/// Downloads a model into Ollama, telling progress as it goes (status, bytes done, bytes in all).
pub fn ollama_pull(
    ep: &Endpoint,
    model: &str,
    mut progress: impl FnMut(&str, u64, u64),
) -> Result<(), String> {
    ep.check()?;
    if ep.kind != Kind::Ollama {
        return Err("Only Ollama downloads models.".into());
    }
    let ok = !model.is_empty()
        && model.len() <= 100
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c));
    if !ok {
        return Err(format!("{model} is not a model name."));
    }
    let url = format!("{}/api/pull", ep.base());
    let mut res = http(Duration::from_secs(6 * 3600))
        .post(&url)
        .header("Content-Type", "application/json")
        .send(json!({"model": model, "stream": true}).to_string())
        .map_err(|e| format!("Ollama could not be reached: {e}"))?;
    let status = res.status().as_u16();
    let reader = std::io::BufReader::new(res.body_mut().as_reader());
    use std::io::BufRead;
    let mut last = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(e) = error_text(&v) {
            return Err(e);
        }
        last = v["status"].as_str().unwrap_or("").to_string();
        progress(
            &last,
            v["completed"].as_u64().unwrap_or(0),
            v["total"].as_u64().unwrap_or(0),
        );
    }
    if !(200..300).contains(&status) {
        return Err(format!("Ollama answered {status}."));
    }
    if last != "success" {
        return Err(format!("The download stopped ({last})."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: &[&str] = &["Read", "Edit", "Bash"];

    #[test]
    fn leaves_out_models_that_do_not_chat() {
        assert!(chat_model(&json!({"id": "antigravity/claude-sonnet-4-6"})));
        assert!(chat_model(
            &json!({"id": "x", "capabilities": ["chat", "tools"]})
        ));
        assert!(!chat_model(
            &json!({"id": "aihorde/AAM XL", "type": "image"})
        ));
        assert!(!chat_model(&json!({"id": "e5", "type": "embedding"})));
        assert!(!chat_model(
            &json!({"id": "flux", "capabilities": ["image"]})
        ));
    }

    #[test]
    fn finds_tool_calls_written_as_text() {
        let (rest, calls) = calls_in_text(
            "Let me look.\n```json\n{\"name\": \"Read\", \"arguments\": {\"file_path\": \"a.txt\"}}\n```",
            KNOWN,
        );
        assert_eq!(rest, "Let me look.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Read");
        assert_eq!(calls[0].input["file_path"], "a.txt");

        let (_, calls) = calls_in_text(
            "<tool_call>{\"name\":\"Bash\",\"parameters\":{\"command\":\"ls\"}}</tool_call>",
            KNOWN,
        );
        assert_eq!(calls[0].input["command"], "ls");

        // Unknown tools and plain JSON in an answer stay text.
        let text = "The config is {\"name\": \"app\"} and {\"a\": 1}. Ünïcode ok.";
        let (rest, calls) = calls_in_text(text, KNOWN);
        assert!(calls.is_empty());
        assert_eq!(rest, text);

        // A call to a tool that does not exist is still a call (refused later), not the answer.
        let (_, calls) = calls_in_text(
            "{\"name\": \"Append\", \"arguments\": {\"file_path\": \"notes.txt\", \"content\": \"x\"}}",
            KNOWN,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Append");
    }

    #[test]
    fn sorts_failures() {
        assert_eq!(classify(429, "slow down").kind, FailKind::Busy);
        assert_eq!(classify(503, "x").kind, FailKind::Busy);
        assert_eq!(classify(500, "x").kind, FailKind::Busy);
        assert_eq!(
            classify(400, "model does not support tools").kind,
            FailKind::Unusable
        );
        assert_eq!(classify(402, "no credits").kind, FailKind::Unusable);
        assert_eq!(classify(404, "not found").kind, FailKind::Unusable);
    }

    #[test]
    fn checks_endpoints() {
        let ep = |url: &str, key: Option<&str>| Endpoint {
            id: "x".into(),
            kind: Kind::Openai,
            url: url.into(),
            key: key.map(str::to_string),
        };
        assert!(ep("http://127.0.0.1:11434", None).check().is_ok());
        assert!(ep("http://localhost:20128/v1", Some("k")).check().is_ok());
        assert!(
            ep("https://openrouter.ai/api/v1", Some("k"))
                .check()
                .is_ok()
        );
        assert!(ep("http://192.168.1.5:11434", None).check().is_ok());
        assert!(ep("http://example.com/v1", Some("k")).check().is_err());
        assert!(ep("ftp://x", None).check().is_err());
        assert!(ep("http://127.0.0.1:11434", None).is_local());
        assert!(ep("http://[::1]:8080/v1", None).is_local());
        assert!(!ep("http://192.168.1.5:11434", None).is_local());
    }

    #[test]
    fn converts_tool_calls_for_ollama() {
        let m = vec![json!({"role": "assistant", "content": "", "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "Read", "arguments": "{\"file_path\":\"a\"}"}}
        ]})];
        let o = to_ollama(&m);
        assert_eq!(
            o[0]["tool_calls"][0]["function"]["arguments"]["file_path"],
            "a"
        );
    }
}
