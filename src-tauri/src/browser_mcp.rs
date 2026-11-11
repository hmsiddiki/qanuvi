//! Qanuvi's test browser offered to agents as MCP tools (`qanuvi --mcp-browser <project> [state file]`), so an agent
//! checks pages in Qanuvi's browser instead of starting Playwright or its own Chrome. The browser starts on the first
//! tool call and stops when the session ends. Answers are short text; a screenshot is sent only when asked for.

use crate::browser::{Browser, new_lines, save_jpeg, short};
use crate::files;
use crate::mcp::{Tools, text_result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SERVER_NAME: &str = "qanuvi-browser";
const OPEN_TEXT: usize = 1500;
const READ_TEXT: usize = 4000;
const MAX_TEXT: usize = 20000;

pub struct BrowserTools {
    root: PathBuf,
    /// Where the page the agent is on is written, for the preview tab to follow.
    state: Option<PathBuf>,
    browser: Option<Browser>,
}

fn target_props() -> Value {
    json!({
        "ref": {"type": "string", "description": "The element's number from browser_fields or browser_links, like 12."},
        "selector": {"type": "string", "description": "A CSS selector."},
        "text": {"type": "string", "description": "Visible text or label of the element."}
    })
}

fn tools() -> Value {
    let mut click_props = target_props();
    click_props["wait_for"] = json!({"type": "string", "description": "Optional text to wait for after the click (up to 10 s)."});
    let mut type_props = target_props();
    type_props["value"] = json!({"type": "string", "description": "What to type (for a select: the option's text or value)."});
    type_props["clear"] =
        json!({"type": "boolean", "description": "Empty the field first (default true)."});
    type_props["submit"] = json!({"type": "boolean", "description": "Press Enter afterwards."});
    let mut shot_props = target_props();
    shot_props["full_page"] = json!({"type": "boolean"});
    json!([
        {
            "name": "browser_open",
            "description": "Open an address (http, https, localhost:port, or a file in the project) in Qanuvi's headless test browser and wait for it to load. Answers the title, HTTP status, counts of links/fields/buttons, headings, the start of the visible text and any errors.",
            "inputSchema": {"type": "object", "properties": {
                "url": {"type": "string"},
                "width": {"type": "integer", "description": "Viewport width (default 1280; under 600 is treated as a phone)."},
                "height": {"type": "integer"}
            }, "required": ["url"]}
        },
        {
            "name": "browser_read",
            "description": "The visible text of the page, or of one element.",
            "inputSchema": {"type": "object", "properties": {
                "selector": {"type": "string"},
                "max_chars": {"type": "integer", "description": "Default 4000, at most 20000."}
            }}
        },
        {
            "name": "browser_links",
            "description": "Visible links as [number] text → href.",
            "inputSchema": {"type": "object", "properties": {"max": {"type": "integer"}}}
        },
        {
            "name": "browser_fields",
            "description": "Visible form fields and buttons as [number] kind name label value, to click or type into by number.",
            "inputSchema": {"type": "object", "properties": {"max": {"type": "integer"}}}
        },
        {
            "name": "browser_click",
            "description": "Click an element (by number, selector or text) like a mouse. Answers what changed: address, new text on the page, new errors.",
            "inputSchema": {"type": "object", "properties": click_props}
        },
        {
            "name": "browser_type",
            "description": "Type into a field (or choose a select option), optionally pressing Enter. Answers what changed.",
            "inputSchema": {"type": "object", "properties": type_props, "required": ["value"]}
        },
        {
            "name": "browser_wait_for",
            "description": "Wait until text (or an element) appears on the page, or disappears with gone=true.",
            "inputSchema": {"type": "object", "properties": {
                "text": {"type": "string"},
                "selector": {"type": "string"},
                "gone": {"type": "boolean"},
                "seconds": {"type": "integer", "description": "Default 10, at most 30."}
            }}
        },
        {
            "name": "browser_errors",
            "description": "Console errors, uncaught exceptions, failed requests (4xx/5xx or network errors) and dialogs since the page was opened.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "browser_screenshot",
            "description": "A picture of the page (visible part, full page, or one element). Costs far more than text: use it only when the question is about looks or layout.",
            "inputSchema": {"type": "object", "properties": shot_props}
        },
        {
            "name": "browser_eval",
            "description": "Evaluate a JavaScript expression in the page and answer its value as JSON (promises are awaited).",
            "inputSchema": {"type": "object", "properties": {"expression": {"type": "string"}}, "required": ["expression"]}
        }
    ])
}

/// The address to open: web addresses as they are, `localhost:3000` as http, other text as a file in the project.
pub fn resolve_url(root: &Path, input: &str) -> Result<String, String> {
    let u = input.trim();
    if u.is_empty() {
        return Err("Give an address to open.".into());
    }
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower == "about:blank" {
        return Ok(u.to_string());
    }
    if lower.starts_with("localhost")
        || lower.starts_with("127.0.0.1")
        || lower.starts_with("[::1]")
    {
        return Ok(format!("http://{u}"));
    }
    if lower.contains("://") || lower.starts_with("javascript:") || lower.starts_with("data:") {
        return Err(format!(
            "Only http and https addresses, or files in the project, can be opened: {u}"
        ));
    }
    // A full path is fine when it is inside the project.
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let given = Path::new(u);
    let rel = if given.is_absolute() {
        let full = given
            .canonicalize()
            .map_err(|_| format!("{u} is not a file in the project."))?;
        let rel = full
            .strip_prefix(&canon_root)
            .map_err(|_| format!("{u} is outside the project."))?;
        rel.to_string_lossy().replace('\\', "/")
    } else {
        u.to_string()
    };
    let path = files::resolve(root, &rel)?;
    if !path.is_file() {
        return Err(format!("{u} is not a file in the project."));
    }
    let full = path.canonicalize().unwrap_or(path);
    let text = full.to_string_lossy().replace('\\', "/");
    let text = text.trim_start_matches("//?/");
    Ok(format!("file:///{}", text.trim_start_matches('/')))
}

fn target(args: &Value) -> Value {
    let mut t = json!({});
    for k in ["ref", "selector", "text"] {
        if let Some(v) = args[k].as_str().filter(|v| !v.trim().is_empty()) {
            t[k] = json!(v.trim());
        } else if let Some(n) = args[k].as_u64() {
            t[k] = json!(n.to_string());
        }
    }
    t
}

fn has_target(t: &Value) -> bool {
    t.as_object().is_some_and(|o| !o.is_empty())
}

impl BrowserTools {
    pub fn new(root: &Path, state: Option<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            state,
            browser: None,
        }
    }

    /// The file address of the project folder, ending in "/" (file:///E:/Sites/shop/).
    fn file_prefix(&self) -> String {
        let full = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let text = full.to_string_lossy().replace('\\', "/");
        let text = text
            .trim_start_matches("//?/")
            .trim_start_matches('/')
            .trim_end_matches('/');
        format!("file:///{text}/")
    }

    fn browser(&mut self) -> Result<&mut Browser, String> {
        if self.browser.is_none() {
            self.browser = Some(Browser::launch()?);
        }
        Ok(self.browser.as_mut().expect("just set"))
    }

    /// Writes the page the agent is on (and the last screenshot) for the preview tab.
    fn note_state(&mut self, shot: Option<&str>) {
        let Some(file) = self.state.clone() else {
            return;
        };
        let Some(b) = self.browser.as_mut() else {
            return;
        };
        let Ok(v) = b.eval("[location.href, document.title]") else {
            return;
        };
        let mut has_shot = file.with_extension("jpg").exists();
        if let Some(data) = shot {
            has_shot = save_jpeg(data, &file.with_extension("jpg")).is_ok();
        }
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let state = json!({"url": v[0], "title": v[1], "updated": millis, "shot": has_shot, "shot_updated": shot.map(|_| millis)});
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Keep the time of the last screenshot when this update has none.
        let mut state = state;
        if shot.is_none()
            && let Ok(old) = std::fs::read_to_string(&file)
            && let Ok(old) = serde_json::from_str::<Value>(&old)
        {
            state["shot_updated"] = old["shot_updated"].clone();
        }
        let _ = std::fs::write(&file, state.to_string());
    }

    fn problems_since(b: &Browser, console: usize, failed: usize) -> Vec<String> {
        let mut out: Vec<String> = b
            .problems
            .console
            .iter()
            .skip(console)
            .map(|c| format!("console: {c}"))
            .collect();
        out.extend(
            b.problems
                .failed
                .iter()
                .skip(failed)
                .map(|f| format!("request: {f}")),
        );
        out.truncate(8);
        out
    }

    /// Runs an action (click, typing) and answers what it changed on the page.
    fn act(
        &mut self,
        what: impl FnOnce(&mut Browser) -> Result<String, String>,
        wait_for: Option<String>,
    ) -> Result<String, String> {
        let b = self.browser()?;
        let before = b.page("text", Value::Null, MAX_TEXT)?;
        let (c0, f0, d0) = (
            b.problems.console.len(),
            b.problems.failed.len(),
            b.dialogs.len(),
        );
        let done = what(b)?;
        std::thread::sleep(Duration::from_millis(100));
        b.settle(Duration::from_secs(10))?;
        if let Some(t) = wait_for.filter(|t| !t.trim().is_empty()) {
            wait(b, Some(&t), None, false, 10)?;
        }
        let after = b.page("text", Value::Null, MAX_TEXT)?;
        let mut out = vec![done];
        let (u0, u1) = (
            before["url"].as_str().unwrap_or(""),
            after["url"].as_str().unwrap_or(""),
        );
        if u0 != u1 {
            out.push(format!(
                "Now on: {} — {}{}",
                short(after["title"].as_str().unwrap_or(""), 80),
                u1,
                b.status.map(|s| format!(" (HTTP {s})")).unwrap_or_default()
            ));
        }
        let added = new_lines(
            before["text"].as_str().unwrap_or(""),
            after["text"].as_str().unwrap_or(""),
            20,
        );
        if added.is_empty() {
            out.push("No new text on the page.".into());
        } else {
            out.push("New on the page:".into());
            out.extend(added.iter().map(|l| format!("  {}", short(l, 160))));
        }
        for d in b.dialogs.iter().skip(d0) {
            out.push(format!("Dialog (answered OK): {d}"));
        }
        let problems = Self::problems_since(b, c0, f0);
        if !problems.is_empty() {
            out.push("New problems:".into());
            out.extend(problems.iter().map(|p| format!("  {p}")));
        }
        Ok(out.join("\n"))
    }

    fn tool(&mut self, name: &str, args: &Value) -> Result<String, String> {
        let s = |k: &str| args[k].as_str().unwrap_or("").to_string();
        let n = |k: &str, d: u64, max: u64| args[k].as_u64().unwrap_or(d).clamp(1, max) as usize;
        match name {
            "browser_open" => {
                let url = resolve_url(&self.root, &s("url"))?;
                let size = (args["width"].as_u64(), args["height"].as_u64());
                let b = self.browser()?;
                if size.0.is_some() || size.1.is_some() {
                    let w = size.0.unwrap_or(b.viewport.0 as u64) as u32;
                    let h = size
                        .1
                        .unwrap_or(if w < 600 { 844 } else { b.viewport.1 as u64 })
                        as u32;
                    b.set_viewport(w, h)?;
                }
                b.open(&url)?;
                let p = b.page("summary", Value::Null, OPEN_TEXT)?;
                let mut out = vec![
                    format!(
                        "{} — {}",
                        short(p["title"].as_str().unwrap_or("(no title)"), 100),
                        p["url"].as_str().unwrap_or(&url)
                    ),
                    format!(
                        "{}{} links, {} fields, {} buttons · {}×{} · {} problems",
                        b.status.map(|s| format!("HTTP {s} · ")).unwrap_or_default(),
                        p["links"],
                        p["fields"],
                        p["buttons"],
                        b.viewport.0,
                        b.viewport.1,
                        b.problems.count()
                    ),
                ];
                out.extend(b.problems.lines(5).iter().map(|l| format!("  {l}")));
                out.extend(
                    b.dialogs
                        .iter()
                        .map(|d| format!("Dialog (answered OK): {d}")),
                );
                if let Some(h) = p["headings"].as_array().filter(|h| !h.is_empty()) {
                    out.push(format!(
                        "Headings: {}",
                        h.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ));
                }
                out.push("Text:".into());
                out.push(p["text"].as_str().unwrap_or("").to_string());
                self.note_state(None);
                Ok(out.join("\n"))
            }
            "browser_read" => {
                let max = n("max_chars", READ_TEXT as u64, MAX_TEXT as u64);
                let sel = s("selector");
                let arg = if sel.trim().is_empty() {
                    Value::Null
                } else {
                    json!({"selector": sel})
                };
                let r = self.browser()?.page("text", arg, max)?;
                if let Some(e) = r["error"].as_str() {
                    return Err(e.into());
                }
                let text = r["text"].as_str().unwrap_or("");
                Ok(if text.is_empty() {
                    "(no visible text)".into()
                } else {
                    text.to_string()
                })
            }
            "browser_links" | "browser_fields" => {
                let mode = if name == "browser_links" {
                    "links"
                } else {
                    "fields"
                };
                let max = n("max", 60, 300);
                let r = self.browser()?.page(mode, Value::Null, max)?;
                let lines: Vec<String> = r
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(if lines.is_empty() {
                    format!(
                        "No visible {}.",
                        if mode == "links" {
                            "links"
                        } else {
                            "fields or buttons"
                        }
                    )
                } else {
                    lines.join("\n")
                })
            }
            "browser_click" => {
                let t = target(args);
                if !has_target(&t) {
                    return Err("Say which element: ref, selector or text.".into());
                }
                let wait_for = Some(s("wait_for"));
                self.act(
                    move |b| {
                        let r = b.page("target", t, 0)?;
                        if let Some(e) = r["error"].as_str() {
                            return Err(e.into());
                        }
                        let (x, y) = (
                            r["x"].as_f64().unwrap_or(0.0),
                            r["y"].as_f64().unwrap_or(0.0),
                        );
                        let mut done =
                            format!("Clicked {}.", r["what"].as_str().unwrap_or("the element"));
                        if r["disabled"].as_bool().unwrap_or(false) {
                            done.push_str(" (It is disabled.)");
                        }
                        if let Some(c) = r["covered"].as_str().filter(|c| !c.is_empty()) {
                            done.push_str(&format!(
                                " Note: {c} is on top of it, so that got the click."
                            ));
                        }
                        b.click_at(x, y)?;
                        Ok(done)
                    },
                    wait_for,
                )
            }
            "browser_type" => {
                let mut t = target(args);
                if !has_target(&t) {
                    return Err("Say which field: ref, selector or text (its label).".into());
                }
                let value = s("value");
                t["value"] = json!(value);
                t["clear"] = json!(args["clear"].as_bool().unwrap_or(true));
                let submit = args["submit"].as_bool().unwrap_or(false);
                self.act(
                    move |b| {
                        let r = b.page("fill", t, 0)?;
                        if let Some(e) = r["error"].as_str() {
                            return Err(e.into());
                        }
                        let what = r["what"].as_str().unwrap_or("the field").to_string();
                        if r["done"].as_bool() == Some(true) {
                            return Ok(format!("Chose \"{value}\" in {what}."));
                        }
                        b.insert_text(&value)?;
                        if submit {
                            b.press_enter()?;
                        }
                        Ok(format!(
                            "Typed into {what}{}.",
                            if submit { " and pressed Enter" } else { "" }
                        ))
                    },
                    None,
                )
            }
            "browser_wait_for" => {
                let text = s("text");
                let sel = s("selector");
                if text.trim().is_empty() && sel.trim().is_empty() {
                    return Err("Give text or a selector to wait for.".into());
                }
                let secs = n("seconds", 10, 30) as u64;
                let gone = args["gone"].as_bool().unwrap_or(false);
                let b = self.browser()?;
                wait(
                    b,
                    Some(text.as_str()).filter(|t| !t.trim().is_empty()),
                    Some(sel.as_str()).filter(|t| !t.trim().is_empty()),
                    gone,
                    secs,
                )
            }
            "browser_errors" => {
                let b = self.browser()?;
                let mut out = b.problems.lines(30);
                out.extend(b.dialogs.iter().map(|d| format!("dialog: {d}")));
                Ok(if out.is_empty() {
                    "No problems since the page was opened.".into()
                } else {
                    out.join("\n")
                })
            }
            "browser_screenshot" => {
                // Handled in `call` (it answers an image); never reached.
                Err("Internal: screenshot".into())
            }
            "browser_eval" => {
                let expr = s("expression");
                if expr.trim().is_empty() {
                    return Err("Give an expression.".into());
                }
                let v = self.browser()?.eval(&expr)?;
                let text = if v.is_null() {
                    "undefined or null".to_string()
                } else {
                    v.to_string()
                };
                let mut text: String = text.chars().take(4000).collect();
                if text.len() >= 4000 {
                    text.push_str(" …");
                }
                Ok(text)
            }
            _ => Err(format!("Unknown tool {name}.")),
        }
    }

    fn screenshot(&mut self, args: &Value) -> Result<(String, String), String> {
        let t = target(args);
        let full = args["full_page"].as_bool().unwrap_or(false);
        let b = self.browser()?;
        let clip = if has_target(&t) {
            let r = b.page("rect", t, 0)?;
            if let Some(e) = r["error"].as_str() {
                return Err(e.into());
            }
            let f = |k: &str| r[k].as_f64().unwrap_or(0.0);
            Some((f("x"), f("y"), f("width"), f("height")))
        } else {
            None
        };
        let data = b.screenshot(full, clip)?;
        let v = b.eval("location.href").unwrap_or(Value::Null);
        let what = format!(
            "Screenshot of {}{}",
            v.as_str().unwrap_or("the page"),
            if clip.is_some() {
                " (one element)"
            } else if full {
                " (full page)"
            } else {
                ""
            }
        );
        self.note_state(Some(&data));
        Ok((data, what))
    }
}

/// Waits for text or an element to appear (or go away).
fn wait(
    b: &mut Browser,
    text: Option<&str>,
    selector: Option<&str>,
    gone: bool,
    secs: u64,
) -> Result<String, String> {
    let check = format!(
        "(() => {{ const t = {}; const s = {}; const has = (t ? (document.body && document.body.innerText.includes(t)) : true) && (s ? !!document.querySelector(s) : true); return has; }})()",
        json!(text),
        json!(selector)
    );
    let what = text
        .map(|t| format!("\"{t}\""))
        .or(selector.map(String::from))
        .unwrap_or_default();
    let start = std::time::Instant::now();
    loop {
        let has = b.eval(&check)?.as_bool().unwrap_or(false);
        if has != gone {
            return Ok(format!(
                "{what} {} after {:.1} s.",
                if gone { "is gone" } else { "is on the page" },
                start.elapsed().as_secs_f64()
            ));
        }
        if start.elapsed() > Duration::from_secs(secs) {
            return Err(format!(
                "{what} {} after {secs} s.",
                if gone {
                    "is still there"
                } else {
                    "did not appear"
                }
            ));
        }
        b.settle(Duration::from_millis(250))?;
    }
}

impl Tools for BrowserTools {
    fn name(&self) -> &'static str {
        SERVER_NAME
    }
    fn instructions(&self) -> &'static str {
        "Qanuvi's headless test browser, with its own empty profile. Use it instead of starting Playwright or another browser to check pages: open, then read text, list fields, click and type by number, and check errors. Ask for a screenshot only when text cannot answer the question."
    }
    fn list(&self) -> Value {
        tools()
    }
    fn call(&mut self, name: &str, args: &Value) -> Value {
        let result = if name == "browser_screenshot" {
            match self.screenshot(args) {
                Ok((data, what)) => {
                    return json!({"content": [
                        {"type": "image", "data": data, "mimeType": "image/jpeg"},
                        {"type": "text", "text": what}
                    ]});
                }
                Err(e) => Err(e),
            }
        } else {
            // Before the first page, say so instead of answering about an empty page.
            let needs_page = !matches!(name, "browser_open" | "browser_errors");
            if needs_page && self.browser.is_none() {
                return text_result(Err("No page is open yet: use browser_open first.".into()));
            }
            self.tool(name, args)
        };
        // A browser that closed or crashed is started again on the next call.
        if let Err(e) = &result
            && e.contains("connection closed")
        {
            self.browser = None;
        }
        // Files of the project are named by their path in the project, which keeps answers short.
        let prefix = self.file_prefix();
        let result = match result {
            Ok(t) if !prefix.is_empty() => Ok(t.replace(&prefix, "")),
            Err(e) if !prefix.is_empty() => Err(e.replace(&prefix, "")),
            other => other,
        };
        text_result(result)
    }
}

/// Runs the browser tools until stdin closes; the browser stops with them.
pub fn serve(root: &Path, state: Option<PathBuf>) {
    let mut tools = BrowserTools::new(root, state);
    crate::mcp::serve(&mut tools);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_web_addresses_or_project_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<h1>Hi</h1>").unwrap();
        let root = dir.path();
        assert_eq!(
            resolve_url(root, "https://example.com/a").unwrap(),
            "https://example.com/a"
        );
        assert_eq!(
            resolve_url(root, "localhost:5173/cart").unwrap(),
            "http://localhost:5173/cart"
        );
        let file = resolve_url(root, "index.html").unwrap();
        assert!(
            file.starts_with("file:///") && file.ends_with("/index.html"),
            "{file}"
        );
        assert!(!file.contains('\\'), "{file}");
        let full = dir.path().join("index.html");
        assert_eq!(resolve_url(root, &full.to_string_lossy()).unwrap(), file);
        assert!(resolve_url(root, "javascript:alert(1)").is_err());
        assert!(resolve_url(root, "chrome://settings").is_err());
        assert!(resolve_url(root, "../outside.html").is_err());
        assert!(resolve_url(root, "missing.html").is_err());
        assert!(resolve_url(root, " ").is_err());
    }

    #[test]
    fn lists_the_browser_tools() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = BrowserTools::new(dir.path(), None);
        let list = crate::mcp::answer(
            &mut t,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        )
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 10);
        assert!(names.iter().all(|n| n.starts_with("browser_")));
        // Tools that need an element refuse without one, before any browser is started.
        let r = t.call("browser_click", &json!({}));
        assert_eq!(r["isError"], true);
        assert!(t.browser.is_none());
    }

    /// Drives a real browser when one is installed (skipped otherwise).
    #[test]
    fn checks_a_page_in_a_real_browser() {
        if crate::browser::find_browser().is_none() {
            eprintln!("no browser installed; skipped");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("index.html"),
            r#"<!doctype html><title>Shop</title><h1>Cart</h1><p id="msg">Empty</p>
<label>Coupon <input name="coupon"></label>
<button onclick="document.getElementById('msg').textContent='Added: '+document.querySelector('[name=coupon]').value">Add</button>
<a href="other.html">Other page</a>
<script>console.error('Broken widget'); fetch('missing.json');</script>"#,
        )
        .unwrap();
        let state = dir.path().join("state").join("s.json");
        let mut t = BrowserTools::new(dir.path(), Some(state.clone()));
        let text = |r: Value| r["content"][0]["text"].as_str().unwrap_or("").to_string();

        let open = text(t.call("browser_open", &json!({"url": "index.html"})));
        assert!(
            open.starts_with(
                "Shop — index.html
"
            ),
            "{open}"
        );
        assert!(!open.contains("file:///"), "{open}");
        assert!(open.contains("1 links, 1 fields, 1 buttons"), "{open}");
        assert!(open.contains("Broken widget"), "{open}");
        assert!(open.contains("Headings: h1: Cart"), "{open}");

        let fields = text(t.call("browser_fields", &json!({})));
        assert!(
            fields.contains("input text name=\"coupon\" label=\"Coupon\""),
            "{fields}"
        );
        let typed = text(t.call(
            "browser_type",
            &json!({"text": "Coupon", "value": "SAVE10"}),
        ));
        assert!(typed.starts_with("Typed into"), "{typed}");
        let clicked = text(t.call("browser_click", &json!({"text": "Add"})));
        assert!(clicked.contains("Added: SAVE10"), "{clicked}");

        let errors = text(t.call("browser_errors", &json!({})));
        assert!(errors.contains("console: Broken widget"), "{errors}");

        let shot = t.call("browser_screenshot", &json!({}));
        assert_eq!(shot["content"][0]["type"], "image");
        assert!(state.with_extension("jpg").exists());
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
        assert_eq!(saved["title"], "Shop");
        assert_eq!(saved["shot"], true);

        assert_eq!(
            text(t.call("browser_eval", &json!({"expression": "1 + 2"}))),
            "3"
        );
        let missing = t.call("browser_click", &json!({"ref": "999"}));
        assert_eq!(missing["isError"], true);
    }
}
