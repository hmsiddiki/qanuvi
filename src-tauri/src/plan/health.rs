//! Health watch (24y Y7): a run whose proxy or model provider is down pauses, says so once, and goes on by itself
//! when it is back, instead of using up every model's tries on steps that could not work.
//!
//! An outage is read from the agent's own error end only (as usage limits are, `limits::own_error_text`): a server
//! error (500, 502, 503, 504, 529), "overloaded", or the connection failing. What a step's work says or reads never
//! counts. A proxy (the run's own address, or the one Claude Code's settings send it to) is looked at directly: its
//! port must answer.

use super::limits::{NOTICE_MAX, is_limit, own_error_text};
use regex::Regex;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::LazyLock;
use std::time::Duration;

/// How long one look at the proxy waits for its port.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the proxy is looked at again while it is down.
pub const PROXY_POLL: Duration = Duration::from_secs(15);
/// How long the run waits before the step is tried again after the provider failed, by how many outages in a row
/// the step has met: 1, 2, 5, then 10 minutes.
pub fn provider_wait(in_a_row: u32) -> Duration {
    Duration::from_secs(
        60 * match in_a_row {
            0 | 1 => 1,
            2 => 2,
            3 => 5,
            _ => 10,
        },
    )
}
/// A step that met outages for this long is set aside (a "server error" that never goes away is not an outage).
pub const GIVE_UP_AFTER: Duration = Duration::from_secs(3 * 60 * 60);

/// What is down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Down {
    /// The proxy at this address does not answer.
    Proxy(String),
    /// The model provider failed: its own words.
    Provider(String),
}

impl Down {
    /// For the Run panel and the run's notice.
    pub fn says(&self) -> String {
        match self {
            Down::Proxy(url) => format!("the proxy at {} is not answering", short(url)),
            Down::Provider(_) => "the model provider is not answering".into(),
        }
    }
}

/// The address without its scheme and path ("127.0.0.1:8787").
fn short(url: &str) -> String {
    host_port(url)
        .map(|(h, p)| format!("{h}:{p}"))
        .unwrap_or_else(|| url.to_string())
}

/// Whether a text is a provider or connection failure (and not a usage limit, which has its own wait).
pub fn is_outage(text: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?i)\b(500|502|503|504|529)\b|overloaded|internal server error|service unavailable|bad gateway|gateway time-?out|connection (error|refused|reset|closed)|econn(refused|reset|aborted)|etimedout|enotfound|eai_again|fetch failed|socket hang up|unable to connect|could not connect|request timed out|network error",
        )
        .unwrap()
    });
    !is_limit(text) && RE.is_match(text)
}

/// The outage in one of a session's agent lines, or None when the line cannot be one.
pub fn outage_notice(line: &str) -> Option<String> {
    let text = own_error_text(line)?;
    let text = text.trim();
    (text.len() <= NOTICE_MAX && is_outage(text)).then(|| text.to_string())
}

/// Host and port of an address ("http://127.0.0.1:8787/v1" -> 127.0.0.1, 8787; https without a port is 443).
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_lowercase(), r),
        None => ("http".to_string(), url),
    };
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    if authority.is_empty() {
        return None;
    }
    let default = if scheme == "https" { 443 } else { 80 };
    if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']')?;
        let port = after
            .strip_prefix(':')
            .map_or(Some(default), |p| p.parse().ok())?;
        return Some((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((authority.to_string(), default)),
    }
}

/// Whether something answers on the address's port.
pub fn reachable(url: &str) -> bool {
    let Some((host, port)) = host_port(url) else {
        return false;
    };
    let Ok(addrs) = (host.as_str(), port).to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|a| TcpStream::connect_timeout(&a, PROBE_TIMEOUT).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn knows_an_outage_from_the_agents_own_error_end_only() {
        let ends = [
            r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            "API Error: Connection error.",
            "API Error (Request timed out.)",
            "API Error: 503 Service Unavailable",
            "request to http://127.0.0.1:8787/v1/messages failed, reason: connect ECONNREFUSED 127.0.0.1:8787",
        ];
        for e in ends {
            let line = json!({"type": "result", "is_error": true, "result": e}).to_string();
            assert_eq!(outage_notice(&line).as_deref(), Some(e), "{e}");
        }
        let synthetic = json!({"type": "assistant", "isApiErrorMessage": true,
            "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "API Error: 500 Internal server error"}]}})
        .to_string();
        assert!(outage_notice(&synthetic).is_some());
        // Not outages: the work talking about one, a usage limit, a request the provider refused, a normal end.
        let not = [
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Fixed the 503 page and the ECONNREFUSED retry."}]}}).to_string(),
            json!({"type": "result", "is_error": true, "result": "You've hit your session limit · resets 5am"}).to_string(),
            json!({"type": "result", "is_error": true, "result": "API Error: 400 invalid_request_error: prompt is too long"}).to_string(),
            json!({"type": "result", "is_error": false, "result": "Connection error handling added."}).to_string(),
            json!({"type": "result", "is_error": true, "result": "Tests failed."}).to_string(),
        ];
        for l in &not {
            assert_eq!(outage_notice(l), None, "{l}");
        }
        // Plain output of the agent program.
        assert!(outage_notice("Error: fetch failed").is_some());
    }

    #[test]
    fn reads_host_and_port() {
        assert_eq!(
            host_port("http://127.0.0.1:8787/v1"),
            Some(("127.0.0.1".into(), 8787))
        );
        assert_eq!(
            host_port("https://proxy.example.com"),
            Some(("proxy.example.com".into(), 443))
        );
        assert_eq!(host_port("http://[::1]:9000/x"), Some(("::1".into(), 9000)));
        assert_eq!(
            host_port("localhost:20128"),
            Some(("localhost".into(), 20128))
        );
        assert_eq!(host_port("http://"), None);
        assert_eq!(
            Down::Proxy("http://127.0.0.1:8787/v1".into()).says(),
            "the proxy at 127.0.0.1:8787 is not answering"
        );
    }

    #[test]
    fn looks_at_a_port_for_real() {
        let open = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = open.local_addr().unwrap().port();
        assert!(reachable(&format!("http://127.0.0.1:{port}")));
        drop(open);
        // Nothing listens there any more.
        assert!(!reachable(&format!("http://127.0.0.1:{port}")));
    }

    #[test]
    fn waits_longer_the_more_outages_in_a_row() {
        assert_eq!(provider_wait(1), Duration::from_secs(60));
        assert_eq!(provider_wait(3), Duration::from_secs(300));
        assert_eq!(provider_wait(9), Duration::from_secs(600));
    }
}
