//! O3 part 2: Qanuvi's compression for Claude Code (and any agent that speaks the Anthropic Messages API). A small
//! HTTP proxy on 127.0.0.1, started the first time a session needs it and ending with the process that started it (so
//! a closed terminal can never break other sessions, unlike a separate proxy). A session gets its address in
//! `ANTHROPIC_BASE_URL`; the real address (the provider, or the user's own proxy) is part of the path, so one proxy
//! serves every session. `POST /v1/messages` (and `count_tokens`) have their `tool_result` blocks compressed by
//! [`crate::compress`]; everything else, and every answer, passes through unchanged and streamed as it comes.
//!
//! Compression is deterministic, so the same conversation always reaches the provider as the same bytes and its prompt
//! cache keeps hitting from request to request.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

/// The default upstream: Anthropic's API.
pub const ANTHROPIC: &str = "https://api.anthropic.com";

static PORT: OnceLock<Option<u16>> = OnceLock::new();

/// The address to give a session (as `ANTHROPIC_BASE_URL`) so its requests go through the compression to `upstream`.
/// Starts the proxy on first use; None when it could not start (the session then goes direct).
pub fn base_url(data_dir: &Path, upstream: &str) -> Option<String> {
    let port = (*PORT.get_or_init(|| start(data_dir.to_path_buf()).ok()))?;
    Some(format!(
        "http://127.0.0.1:{port}/u/{}",
        hex(upstream.trim_end_matches('/'))
    ))
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

/// Where a request goes: the upstream from the path, and the rest of the path. Only http(s) upstreams are taken.
fn route(target: &str) -> Option<(String, String)> {
    let rest = target.strip_prefix("/u/")?;
    let (code, path) = rest.split_once('/').unwrap_or((rest, ""));
    let upstream = unhex(code)?;
    if !(upstream.starts_with("https://") || upstream.starts_with("http://")) {
        return None;
    }
    Some((upstream, format!("/{path}")))
}

fn start(data_dir: PathBuf) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::Builder::new()
        .name("compress-proxy".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let dir = data_dir.clone();
                let _ = std::thread::Builder::new()
                    .name("compress-proxy-conn".into())
                    .spawn(move || {
                        let _ = serve(conn, &dir);
                    });
            }
        })?;
    Ok(port)
}

/// One client connection, request after request (keep-alive).
fn serve(conn: TcpStream, data_dir: &Path) -> std::io::Result<()> {
    let mut out = conn.try_clone()?;
    let mut input = BufReader::new(conn);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
            return Ok(());
        };
        let (method, target) = (method.to_string(), target.to_string());
        let mut headers: Vec<(String, String)> = Vec::new();
        loop {
            let mut h = String::new();
            if input.read_line(&mut h)? == 0 {
                return Ok(());
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        let mut body = Vec::new();
        if header("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            read_chunked(&mut input, &mut body)?;
        } else if let Some(n) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
            body.resize(n, 0);
            input.read_exact(&mut body)?;
        }
        let close = header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        let Some((upstream, path)) = route(&target) else {
            write_simple(&mut out, 400, "Not a Qanuvi compression address.")?;
            return Ok(());
        };
        if method == "POST" && path.starts_with("/v1/messages") {
            body = rewrite(&body, data_dir);
        }
        forward(
            &agent,
            &mut out,
            &method,
            &format!("{upstream}{path}"),
            &headers,
            body,
        )?;
        if close {
            return Ok(());
        }
    }
}

fn read_chunked(input: &mut impl BufRead, body: &mut Vec<u8>) -> std::io::Result<()> {
    loop {
        let mut size = String::new();
        input.read_line(&mut size)?;
        let n =
            usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
        if n == 0 {
            let mut end = String::new();
            input.read_line(&mut end)?;
            return Ok(());
        }
        let start = body.len();
        body.resize(start + n, 0);
        input.read_exact(&mut body[start..])?;
        let mut crlf = [0u8; 2];
        input.read_exact(&mut crlf)?;
    }
}

fn write_simple(out: &mut impl Write, status: u16, text: &str) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status} X\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    )?;
    out.flush()
}

/// Sends the request on and streams the answer back as it comes (server-sent events included), chunked.
fn forward(
    agent: &ureq::Agent,
    out: &mut impl Write,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> std::io::Result<()> {
    const HOP: [&str; 6] = [
        "host",
        "content-length",
        "connection",
        "transfer-encoding",
        "accept-encoding",
        "keep-alive",
    ];
    let mut req = ureq::http::Request::builder().method(method).uri(url);
    for (k, v) in headers {
        if !HOP.iter().any(|h| k.eq_ignore_ascii_case(h)) {
            req = req.header(k, v);
        }
    }
    req = req.header("accept-encoding", "identity");
    let Ok(req) = req.body(body) else {
        return write_simple(out, 400, "The request could not be passed on.");
    };
    let resp = match agent.run(req) {
        Ok(r) => r,
        Err(e) => {
            return write_simple(
                out,
                502,
                &format!("Qanuvi's compression proxy could not reach {url}: {e}"),
            );
        }
    };
    let status = resp.status();
    write!(
        out,
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )?;
    for (k, v) in resp.headers() {
        let name = k.as_str();
        if [
            "content-length",
            "transfer-encoding",
            "connection",
            "content-encoding",
        ]
        .contains(&name)
        {
            continue;
        }
        out.write_all(name.as_bytes())?;
        out.write_all(b": ")?;
        out.write_all(v.as_bytes())?;
        out.write_all(b"\r\n")?;
    }
    out.write_all(b"transfer-encoding: chunked\r\nconnection: keep-alive\r\n\r\n")?;
    out.flush()?;
    let mut reader = resp.into_body().into_reader();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        write!(out, "{n:x}\r\n")?;
        out.write_all(&buf[..n])?;
        out.write_all(b"\r\n")?;
        out.flush()?;
    }
    out.write_all(b"0\r\n\r\n")?;
    out.flush()
}

/// A Messages request with its tool results compressed (originals kept for `qanuvi_retrieve`). Anything that is not
/// such a request is sent on as it came.
pub fn rewrite(body: &[u8], data_dir: &Path) -> Vec<u8> {
    let Ok(mut v) = serde_json::from_slice::<Value>(body) else {
        return body.to_vec();
    };
    let store = crate::compress::Store::new(data_dir);
    if !compress_messages(&mut v, Some(&store)) {
        return body.to_vec();
    }
    serde_json::to_vec(&v).unwrap_or_else(|_| body.to_vec())
}

/// Compresses the `tool_result` blocks of a Messages request in place; says whether anything changed.
pub fn compress_messages(v: &mut Value, store: Option<&crate::compress::Store>) -> bool {
    let Some(messages) = v.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };
    // Which tool each result answers, for the router (a Read keeps code whole).
    let mut tools: HashMap<String, (String, String, String)> = HashMap::new();
    for m in messages.iter() {
        for block in m["content"].as_array().into_iter().flatten() {
            if block["type"] == "tool_use"
                && let Some(id) = block["id"].as_str()
            {
                let input = &block["input"];
                tools.insert(
                    id.to_string(),
                    (
                        block["name"].as_str().unwrap_or("").to_string(),
                        input["command"].as_str().unwrap_or("").to_string(),
                        input["file_path"].as_str().unwrap_or("").to_string(),
                    ),
                );
            }
        }
    }
    let mut changed = false;
    for m in messages.iter_mut() {
        let Some(blocks) = m.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in blocks.iter_mut() {
            if block["type"] != "tool_result" {
                continue;
            }
            let (tool, command, path) = block["tool_use_id"]
                .as_str()
                .and_then(|id| tools.get(id))
                .cloned()
                .unwrap_or_default();
            // Qanuvi's own retrieval answers are what was asked for: never cut again.
            if tool.ends_with(crate::compress::RETRIEVE_TOOL) {
                continue;
            }
            let hint = crate::compress::Hint {
                tool: &tool,
                command: &command,
                path: &path,
            };
            let content = &mut block["content"];
            if let Some(text) = content.as_str() {
                let c = crate::compress::compress(text, &hint, store);
                if c.after < c.before {
                    *content = Value::String(c.text);
                    changed = true;
                }
            } else if let Some(parts) = content.as_array_mut() {
                for p in parts.iter_mut() {
                    if p["type"] == "text"
                        && let Some(text) = p["text"].as_str()
                    {
                        let c = crate::compress::compress(text, &hint, store);
                        if c.after < c.before {
                            p["text"] = Value::String(c.text);
                            changed = true;
                        }
                    }
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upstream_travels_in_the_path() {
        let url = format!("http://127.0.0.1:1/u/{}", hex("https://api.anthropic.com"));
        let target = format!(
            "{}/v1/messages?beta=true",
            url.trim_start_matches("http://127.0.0.1:1")
        );
        assert_eq!(
            route(&target),
            Some((
                "https://api.anthropic.com".into(),
                "/v1/messages?beta=true".into()
            ))
        );
        assert_eq!(route("/v1/messages"), None);
        assert_eq!(route(&format!("/u/{}/x", hex("file:///etc"))), None);
    }

    #[test]
    fn tool_results_are_compressed_and_code_reads_are_not() {
        let log: String = (0..400)
            .map(|i| format!("12:00:{i:03} INFO step {i} ok\n"))
            .collect();
        let code = "fn main() {}\n".repeat(400);
        let mut v = serde_json::json!({"model": "m", "messages": [
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "a", "name": "Bash", "input": {"command": "cat log"}},
                {"type": "tool_use", "id": "b", "name": "Read", "input": {"file_path": "/p/main.rs"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "a", "content": log},
                {"type": "tool_result", "tool_use_id": "b", "content": [{"type": "text", "text": code}]}
            ]}
        ]});
        assert!(compress_messages(&mut v, None));
        let out = v["messages"][1]["content"][0]["content"].as_str().unwrap();
        assert!(out.len() * 5 < log.len(), "{}", out.len());
        assert_eq!(v["messages"][1]["content"][1]["content"][0]["text"], code);
        // The same request again gives the same bytes (the prompt cache keeps hitting).
        let mut again: Value = serde_json::from_str(&v.to_string()).unwrap();
        compress_messages(&mut again, None);
        assert_eq!(again, v);
        // Not a Messages request: sent as it came.
        assert_eq!(rewrite(b"not json", Path::new(".")), b"not json".to_vec());
    }

    /// The proxy end to end on this machine: a fake upstream answers a streamed reply; the tool result reached it
    /// compressed, and the reply came back whole.
    #[test]
    fn passes_a_request_through_compressed_and_streams_the_answer() {
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let up_port = upstream.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            let (conn, _) = upstream.accept().unwrap();
            let mut r = BufReader::new(conn.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l.trim().is_empty() {
                    break;
                }
                if let Some(n) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = n.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            *seen2.lock().unwrap() = String::from_utf8(body).unwrap();
            let mut w = conn;
            let reply = "event: message_start\ndata: {\"ok\":true}\n\n";
            write!(w, "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{reply}", reply.len()).unwrap();
        });
        let d = tempfile::tempdir().unwrap();
        let base = base_url(d.path(), &format!("http://127.0.0.1:{up_port}")).unwrap();
        let log: String = (0..400)
            .map(|i| format!("12:00:{i:03} INFO step {i} ok\n"))
            .collect();
        let req = serde_json::json!({"model": "m", "messages": [
            {"role": "assistant", "content": [{"type": "tool_use", "id": "a", "name": "Bash", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "a", "content": log}]}
        ]});
        let resp = ureq::post(&format!("{base}/v1/messages"))
            .header("content-type", "application/json")
            .send(req.to_string())
            .unwrap();
        let text = resp.into_body().read_to_string().unwrap();
        assert!(text.contains("event: message_start"), "{text}");
        let got: Value = serde_json::from_str(&seen.lock().unwrap()).unwrap();
        let sent = got["messages"][1]["content"][0]["content"]
            .as_str()
            .unwrap();
        assert!(sent.len() * 5 < log.len());
        assert!(
            sent.contains(crate::compress::RETRIEVE_TOOL),
            "originals kept: the note offers retrieval"
        );
    }
}
