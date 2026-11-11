//! The preview tab's side of the test browser (Milestone 15): the page the agent is on (written by its browser tools),
//! and an inspector the user points with, to hand the agent a page or an element. The inspector's browser is started
//! on first use and stopped after a few idle minutes, so it costs nothing while it is not used.

use crate::browser::{Browser, short};
use base64::Engine;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IDLE: Duration = Duration::from_secs(180);

struct Slot {
    browser: Browser,
    used: Instant,
}

#[derive(Default, Clone)]
pub struct Inspector {
    slot: Arc<Mutex<Option<Slot>>>,
}

impl Inspector {
    /// Runs something with the inspector's browser, starting it (and its idle watch) when needed.
    fn with<T>(&self, f: impl FnOnce(&mut Browser) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(Slot {
                browser: Browser::launch()?,
                used: Instant::now(),
            });
            let slot = self.slot.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(15));
                    let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
                    match g.as_ref() {
                        None => break,
                        Some(s) if s.used.elapsed() > IDLE => {
                            *g = None;
                            break;
                        }
                        _ => {}
                    }
                }
            });
        }
        let s = guard.as_mut().expect("just set");
        s.used = Instant::now();
        let r = f(&mut s.browser);
        if let Err(e) = &r
            && e.contains("connection closed")
        {
            *guard = None;
        }
        r
    }

    /// Opens an address at the preview's size (if it is not open already) and answers a screenshot.
    pub fn show(&self, url: &str, width: u32, height: u32) -> Result<Value, String> {
        self.with(|b| {
            if b.viewport != (width, height) {
                b.set_viewport(width, height)?;
            }
            let here = b.eval("location.href")?;
            if here.as_str() != Some(url) {
                b.open(url)?;
            }
            shot(b)
        })
    }

    pub fn scroll(&self, dy: f64) -> Result<Value, String> {
        self.with(|b| {
            b.page("scroll", json!({"dy": dy}), 0)?;
            b.settle(Duration::from_millis(400))?;
            shot(b)
        })
    }

    /// The element at a point of the screenshot.
    pub fn pick(&self, x: f64, y: f64) -> Result<Value, String> {
        self.with(|b| {
            let r = b.page("at", json!({"x": x, "y": y}), 0)?;
            if let Some(e) = r["error"].as_str() {
                return Err(e.into());
            }
            Ok(r)
        })
    }

    /// The page as short text for an agent: title, address, status, problems and visible text.
    pub fn page_text(&self, url: &str) -> Result<String, String> {
        self.with(|b| {
            let here = b.eval("location.href")?;
            if here.as_str() != Some(url) {
                b.open(url)?;
            }
            let p = b.page("summary", Value::Null, 4000)?;
            let mut out = vec![format!(
                "Page: {} — {}{}",
                short(p["title"].as_str().unwrap_or(""), 100),
                p["url"].as_str().unwrap_or(url),
                b.status.map(|s| format!(" (HTTP {s})")).unwrap_or_default()
            )];
            if b.problems.count() > 0 {
                out.push("Problems:".into());
                out.extend(b.problems.lines(8).iter().map(|l| format!("  {l}")));
            }
            out.push("Visible text:".into());
            out.push(p["text"].as_str().unwrap_or("").to_string());
            Ok(out.join("\n"))
        })
    }

    pub fn close(&self) {
        *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

fn shot(b: &mut Browser) -> Result<Value, String> {
    let data = b.screenshot(false, None)?;
    let v = b.eval("[location.href, document.title, scrollY]")?;
    Ok(json!({
        "image": format!("data:image/jpeg;base64,{data}"),
        "url": v[0],
        "title": v[1],
        "scroll": v[2],
        "width": b.viewport.0,
        "height": b.viewport.1,
    }))
}

/// What the agent's browser tools last noted for a session (address, title, time), if anything.
pub fn agent_view(state_file: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(state_file).ok()?;
    serde_json::from_str(&text).ok()
}

/// The agent's last screenshot as a data address.
pub fn agent_shot(state_file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(state_file.with_extension("jpg"))
        .map_err(|_| "No screenshot yet.".to_string())?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Text to hand the agent for a picked element.
pub fn element_note(e: &Value) -> String {
    let mut out = vec![format!(
        "Element on {} ({}): {}",
        e["url"].as_str().unwrap_or(""),
        short(e["title"].as_str().unwrap_or(""), 80),
        e["selector"].as_str().unwrap_or("")
    )];
    if let Some(t) = e["text"].as_str().filter(|t| !t.is_empty()) {
        out.push(format!("Text: {t}"));
    }
    out.push(format!("HTML: {}", e["html"].as_str().unwrap_or("")));
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_note_is_short_and_says_where() {
        let e = json!({"url": "http://localhost/cart", "title": "Cart", "selector": "#total > span", "text": "10.00", "html": "<span>10.00</span>"});
        assert_eq!(
            element_note(&e),
            "Element on http://localhost/cart (Cart): #total > span\nText: 10.00\nHTML: <span>10.00</span>"
        );
    }

    #[test]
    fn agent_view_reads_what_the_tools_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("s.json");
        assert!(agent_view(&f).is_none());
        std::fs::write(&f, r#"{"url":"http://x/","title":"X","updated":5}"#).unwrap();
        assert_eq!(agent_view(&f).unwrap()["url"], "http://x/");
        assert!(agent_shot(&f).is_err());
        std::fs::write(f.with_extension("jpg"), [0xFFu8, 0xD8]).unwrap();
        assert_eq!(agent_shot(&f).unwrap(), "data:image/jpeg;base64,/9g=");
    }

    /// Points at an element in a real browser when one is installed (skipped otherwise).
    #[test]
    fn inspector_shows_picks_and_reads_a_page() {
        if crate::browser::find_browser().is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.html");
        std::fs::write(&file, r#"<title>Picked</title><body style="margin:0"><div id="total" style="height:100px"><span>10.00</span></div><p>More</p>"#).unwrap();
        let url = format!(
            "file:///{}",
            file.canonicalize()
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                .trim_start_matches("//?/")
        );
        let i = Inspector::default();
        let shot = i.show(&url, 400, 300).unwrap();
        assert!(
            shot["image"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
        assert_eq!(
            (shot["width"].as_u64(), shot["title"].as_str()),
            (Some(400), Some("Picked"))
        );
        let e = i.pick(5.0, 5.0).unwrap();
        assert_eq!(e["selector"], "#total > span");
        assert!(element_note(&e).contains("Text: 10.00"));
        let text = i.page_text(&url).unwrap();
        assert!(
            text.starts_with("Page: Picked") && text.contains("More"),
            "{text}"
        );
        i.close();
    }
}
