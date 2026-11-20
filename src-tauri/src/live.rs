//! The Preview's live picture of an agent's test browser (LM1). The agent's browser tools write the page's DevTools
//! address into the session's state file; while a window watches, the master opens a second DevTools connection to
//! that page and streams its screencast. The agent's own connection is not touched, so it carries on headless and
//! never waits for the watcher. One picture is on its way at a time: the next is sent only after the window says it
//! has the last one (`ack`), so a slow link skips pictures instead of piling them up. The window's acks also keep the
//! stream alive; without one for a while (the window closed, the link dropped) the stream stops by itself.

use crate::sink::Sink;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::TcpStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

/// Without a word from the window for this long, the stream ends.
const QUIET: Duration = Duration::from_secs(20);

struct Watch {
    stop: AtomicBool,
    /// The window has the last picture (or none was sent yet).
    ready: AtomicBool,
    heard: Mutex<Instant>,
}

/// The live pictures being watched on this master.
#[derive(Default)]
pub struct Lives(Mutex<HashMap<String, Arc<Watch>>>);

/// The page's DevTools address from a session's state file, if its browser was opened.
pub fn devtools_address(state_file: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(state_file).ok()?).ok()?;
    v["cdp"]
        .as_str()
        .filter(|a| local_address(a))
        .map(String::from)
}

/// Only the machine's own browser: ws://127.0.0.1:<port>/devtools/page/<id>.
fn local_address(a: &str) -> bool {
    let rest = a
        .strip_prefix("ws://127.0.0.1:")
        .or_else(|| a.strip_prefix("ws://localhost:"));
    rest.is_some_and(|r| {
        let (port, path) = r.split_once('/').unwrap_or((r, ""));
        port.parse::<u16>().is_ok() && path.starts_with("devtools/page/")
    })
}

fn connect(address: &str) -> Result<WebSocket<TcpStream>, String> {
    const CLOSED: &str = "The agent's test browser is not open right now.";
    // local_address() made sure it is ws://127.0.0.1:<port>/… or ws://localhost:<port>/….
    let port: u16 = address
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .and_then(|h| h.rsplit(':').next())
        .and_then(|p| p.parse().ok())
        .ok_or(CLOSED)?;
    let stream = TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(3),
    )
    .map_err(|_| CLOSED.to_string())?;
    stream.set_nodelay(true).ok();
    let (ws, _) = tungstenite::client(address, stream).map_err(|_| CLOSED.to_string())?;
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();
    Ok(ws)
}

fn send(ws: &mut WebSocket<TcpStream>, id: u64, method: &str, params: Value) -> bool {
    ws.send(Message::text(
        json!({"id": id, "method": method, "params": params}).to_string(),
    ))
    .is_ok()
}

/// Waits a moment for one message: Ok(None) when none came, Err when the connection closed.
fn read(ws: &mut WebSocket<TcpStream>) -> Result<Option<Value>, ()> {
    match ws.read() {
        Ok(Message::Text(t)) => Ok(serde_json::from_str(t.as_str()).ok()),
        Ok(Message::Close(_)) => Err(()),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(()),
    }
}

impl Lives {
    /// Starts streaming a session's browser to `sink`: `{frame, width, height}` events, and `{ended: why}` once at
    /// the end. Gives back the stream's id for `ack` and `stop`.
    pub fn start(self: &Arc<Self>, state_file: &Path, sink: Sink<Value>) -> Result<String, String> {
        let address = devtools_address(state_file)
            .ok_or("The agent's test browser has not opened a page yet.")?;
        let mut ws = connect(&address)?;
        if !send(
            &mut ws,
            1,
            "Page.startScreencast",
            json!({"format": "jpeg", "quality": 60, "maxWidth": 1600, "maxHeight": 1600}),
        ) {
            return Err("The agent's test browser is not open right now.".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let watch = Arc::new(Watch {
            stop: AtomicBool::new(false),
            ready: AtomicBool::new(true),
            heard: Mutex::new(Instant::now()),
        });
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), watch.clone());
        let (lives, key) = (self.clone(), id.clone());
        std::thread::spawn(move || {
            let why = stream(&mut ws, &watch, &sink);
            send(&mut ws, 2, "Page.stopScreencast", json!({}));
            let _ = ws.close(None);
            let _ = sink.send(json!({"ended": why}));
            lives
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&key);
        });
        Ok(id)
    }

    /// The window has the last picture (and is still watching).
    pub fn ack(&self, id: &str) -> Result<(), String> {
        let lives = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let w = lives.get(id).ok_or("This live picture has ended.")?;
        *w.heard.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        w.ready.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn stop(&self, id: &str) {
        if let Some(w) = self.0.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
            w.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn count(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Runs one stream until it should end, and says why.
fn stream(ws: &mut WebSocket<TcpStream>, watch: &Watch, sink: &Sink<Value>) -> String {
    let mut next_id = 10;
    // The browser's newest picture not yet passed on, and its screencast session (acked once passed on, so the
    // browser makes no new picture while one waits).
    let mut held: Option<(Value, u64)> = None;
    loop {
        if watch.stop.load(Ordering::SeqCst) {
            return "stopped".into();
        }
        if watch
            .heard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed()
            > QUIET
        {
            return "The window stopped watching.".into();
        }
        if held.is_some() && watch.ready.load(Ordering::SeqCst) {
            let (frame, session) = held.take().expect("held");
            watch.ready.store(false, Ordering::SeqCst);
            if sink.send(frame).is_err() {
                return "The window stopped watching.".into();
            }
            next_id += 1;
            if !send(
                ws,
                next_id,
                "Page.screencastFrameAck",
                json!({"sessionId": session}),
            ) {
                return "The agent's test browser closed.".into();
            }
        }
        match read(ws) {
            Err(()) => return "The agent's test browser closed.".into(),
            Ok(None) => {}
            Ok(Some(msg)) => match msg["method"].as_str() {
                Some("Page.screencastFrame") => {
                    let p = &msg["params"];
                    let m = &p["metadata"];
                    held = Some((
                        json!({
                            "frame": p["data"],
                            "width": m["deviceWidth"],
                            "height": m["deviceHeight"],
                        }),
                        p["sessionId"].as_u64().unwrap_or(0),
                    ));
                }
                Some("Inspector.detached" | "Target.targetDestroyed") => {
                    return "The agent's test browser closed.".into();
                }
                _ => {
                    if msg["id"] == 1 && msg.get("error").is_some() {
                        return format!(
                            "The agent's test browser cannot show a live picture: {}",
                            msg["error"]["message"].as_str().unwrap_or("")
                        );
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_machine_s_own_browser_is_reached() {
        assert!(local_address("ws://127.0.0.1:51234/devtools/page/ABC"));
        assert!(local_address("ws://localhost:9222/devtools/page/ABC"));
        assert!(!local_address("ws://10.0.0.5:9222/devtools/page/ABC"));
        assert!(!local_address("ws://127.0.0.1:9222/devtools/browser/ABC"));
        assert!(!local_address(
            "ws://127.0.0.1.evil.com:9222/devtools/page/ABC"
        ));
        assert!(!local_address("http://127.0.0.1:9222/devtools/page/ABC"));
    }

    #[test]
    fn a_session_without_a_browser_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let lives = Arc::new(Lives::default());
        let e = lives
            .start(&dir.path().join("none.json"), Sink::new(|_| true))
            .unwrap_err();
        assert!(e.contains("has not opened a page yet"), "{e}");
        std::fs::write(
            dir.path().join("s.json"),
            r#"{"cdp": "ws://127.0.0.1:1/devtools/page/X"}"#,
        )
        .unwrap();
        let e = lives
            .start(&dir.path().join("s.json"), Sink::new(|_| true))
            .unwrap_err();
        assert!(e.contains("not open right now"), "{e}");
        assert!(lives.ack("nope").is_err());
    }

    /// Streams a real browser's page when one is installed (skipped otherwise): pictures come one at a time, only
    /// after each ack, the agent's own connection keeps working meanwhile, and stop ends the stream.
    #[test]
    fn streams_a_real_browser_one_picture_at_a_time() {
        if crate::browser::find_browser().is_none() {
            eprintln!("no browser installed; skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            r#"<!doctype html><title>Live</title><h1 id="n">0</h1>
<script>let i=0;setInterval(()=>{document.getElementById('n').textContent=++i},50)</script>"#,
        )
        .unwrap();
        let state = dir.path().join("state").join("s.json");
        let mut tools = crate::browser_mcp::BrowserTools::new(dir.path(), Some(state.clone()));
        let open =
            crate::mcp::Tools::call(&mut tools, "browser_open", &json!({"url": "index.html"}));
        assert_ne!(open["isError"], true, "{open}");
        assert!(devtools_address(&state).is_some());

        let (tx, rx) = std::sync::mpsc::channel::<Value>();
        let lives = Arc::new(Lives::default());
        let id = lives
            .start(&state, Sink::new(move |e| tx.send(e).is_ok()))
            .unwrap();
        let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            first["frame"].as_str().is_some_and(|f| f.len() > 100),
            "{first}"
        );
        assert!(first["width"].as_f64().unwrap_or(0.0) > 0.0, "{first}");
        // No ack yet: no second picture, although the page changes all the time.
        assert!(rx.recv_timeout(Duration::from_millis(700)).is_err());
        // The agent's connection works while the picture is watched.
        let read = crate::mcp::Tools::call(&mut tools, "browser_read", &json!({}));
        assert_ne!(read["isError"], true, "{read}");
        lives.ack(&id).unwrap();
        let second = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(second["frame"].is_string(), "{second}");
        lives.stop(&id);
        let end = loop {
            let e = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if e.get("ended").is_some() {
                break e;
            }
        };
        assert_eq!(end["ended"], "stopped");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(lives.count(), 0);
    }
}
