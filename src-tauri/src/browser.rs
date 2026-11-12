//! The test browser (Milestone 15): a headless Chromium-family browser (Chrome, Edge, Chromium or Brave, whichever is
//! installed) driven over the Chrome DevTools Protocol. It runs with its own empty profile, so it never sees the
//! user's sign-ins, and it is started only when a check needs it. Answers are short text; screenshots only on request.

use base64::Engine;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

const PAGE_JS: &str = include_str!("page.js");
const KEEP_LOG: usize = 50;
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;

/// Where to find a browser: `QANUVI_BROWSER`, else the usual install places of Chrome, Edge, Chromium and Brave.
pub fn find_browser() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("QANUVI_BROWSER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    candidates().into_iter().find(|p| p.is_file())
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        let rel = [
            r"Google\Chrome\Application\chrome.exe",
            r"Microsoft\Edge\Application\msedge.exe",
            r"Chromium\Application\chrome.exe",
            r"BraveSoftware\Brave-Browser\Application\brave.exe",
        ];
        for r in rel {
            for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
                if let Ok(base) = std::env::var(var) {
                    out.push(PathBuf::from(base).join(r));
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        for app in [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Chromium.app/Contents/MacOS/Chromium",
            "Brave Browser.app/Contents/MacOS/Brave Browser",
        ] {
            out.push(PathBuf::from("/Applications").join(app));
        }
    } else {
        let names = [
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "brave-browser",
        ];
        let path = std::env::var_os("PATH").unwrap_or_default();
        for name in names {
            for dir in std::env::split_paths(&path) {
                out.push(dir.join(name));
            }
        }
    }
    out
}

/// What went wrong on the page since it was opened: console errors, uncaught exceptions and failed requests.
#[derive(Default, Debug, Clone)]
pub struct Problems {
    pub console: Vec<String>,
    pub failed: Vec<String>,
}

impl Problems {
    fn push(list: &mut Vec<String>, line: String) {
        if list.last() == Some(&line) {
            return;
        }
        list.push(line);
        if list.len() > KEEP_LOG {
            list.remove(0);
        }
    }

    pub fn count(&self) -> usize {
        self.console.len() + self.failed.len()
    }

    /// The problems as short lines (at most `max`), newest last.
    pub fn lines(&self, max: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .console
            .iter()
            .map(|c| format!("console: {c}"))
            .chain(self.failed.iter().map(|f| format!("request: {f}")))
            .collect();
        if out.len() > max {
            let more = out.len() - max;
            out.truncate(max);
            out.push(format!("… and {more} more"));
        }
        out
    }
}

/// One open browser with one page.
pub struct Browser {
    child: Child,
    /// Ends the browser with this process, however it ends (Windows).
    _job: Job,
    profile: PathBuf,
    ws: WebSocket<TcpStream>,
    next_id: u64,
    main_frame: String,
    /// Requests in flight: id → (started, url, method).
    pending: HashMap<String, (Instant, String, String)>,
    pub problems: Problems,
    /// The main document's last HTTP status.
    pub status: Option<u16>,
    /// Alerts and confirms the page showed (answered with OK so the page does not hang).
    pub dialogs: Vec<String>,
    pub viewport: (u32, u32),
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

/// A plain HTTP GET to the browser's local DevTools address.
fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    s.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    // The browser keeps the connection open, so read until the body has its Content-Length.
    let mut data = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = s.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data);
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len = head.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| v.trim().parse::<usize>().ok())?
            });
            if len.is_some_and(|len| body.len() >= len) {
                return Ok(body.to_string());
            }
        }
    }
    let text = String::from_utf8_lossy(&data);
    Ok(text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default())
}

impl Browser {
    /// Starts a headless browser with a new empty profile and connects to its page.
    pub fn launch() -> Result<Self, String> {
        let exe = find_browser().ok_or(
            "No Chrome, Edge, Chromium or Brave browser was found on this machine. Install one, or set QANUVI_BROWSER to its program file.",
        )?;
        let profile = std::env::temp_dir().join(format!(
            "qanuvi-browser-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(&exe);
        cmd.args([
            "--headless=new",
            "--remote-debugging-port=0",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-component-update",
            "--disable-sync",
            "--mute-audio",
            "--hide-scrollbars",
            // Light: no GPU process, one page process, and none of the browser's background features.
            "--disable-gpu",
            "--renderer-process-limit=1",
            "--disable-site-isolation-trials",
            "--disable-features=site-per-process,IsolateOrigins,Translate,OptimizationHints,MediaRouter,DialMediaRouteProvider,AutofillServerCommunication,CertificateTransparencyComponentUpdater,InterestFeedContentSuggestions,BackForwardCache",
            "--disable-default-apps",
            "--disable-component-extensions-with-background-pages",
            "--enable-features=NetworkServiceInProcess2",
            "--disable-breakpad",
            "--disable-crash-reporter",
            "--no-pings",
            "--metrics-recording-only",
            "--password-store=basic",
        ])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--window-size={WIDTH},{HEIGHT}"))
        .arg("about:blank")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        no_window(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| format!("The browser ({}) could not start: {e}", exe.display()))?;
        let job = Job::for_child(&child);
        // From here on, dropping the guard stops the browser again if connecting fails.
        let mut guard = Guard {
            child: Some(child),
            profile: profile.clone(),
        };

        let port_file = profile.join("DevToolsActivePort");
        let deadline = Instant::now() + Duration::from_secs(20);
        let port: u16 = loop {
            if let Ok(t) = std::fs::read_to_string(&port_file)
                && let Some(p) = t.lines().next().and_then(|l| l.trim().parse().ok())
            {
                break p;
            }
            if Instant::now() > deadline {
                return Err("The browser started but did not open its DevTools port.".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let ws_url = loop {
            let list: Value =
                serde_json::from_str(&http_get(port, "/json/list").unwrap_or_default())
                    .unwrap_or(Value::Null);
            let page = list.as_array().and_then(|a| {
                a.iter()
                    .find(|t| t["type"] == "page")
                    .and_then(|t| t["webSocketDebuggerUrl"].as_str().map(String::from))
            });
            if let Some(u) = page {
                break u;
            }
            if Instant::now() > deadline {
                return Err("The browser did not open a page.".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        stream.set_nodelay(true).ok();
        let (ws, _) = tungstenite::client(ws_url.as_str(), stream)
            .map_err(|e| format!("Could not connect to the browser: {e}"))?;
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok();

        let child = guard.child.take().expect("child is set");
        let mut b = Browser {
            child,
            _job: job,
            profile,
            ws,
            next_id: 0,
            main_frame: String::new(),
            pending: HashMap::new(),
            problems: Problems::default(),
            status: None,
            dialogs: Vec::new(),
            viewport: (WIDTH, HEIGHT),
        };
        drop(guard);
        for domain in [
            "Page.enable",
            "Runtime.enable",
            "Network.enable",
            "Log.enable",
        ] {
            b.call(domain, json!({}))?;
        }
        let tree = b.call("Page.getFrameTree", json!({}))?;
        b.main_frame = tree["frameTree"]["frame"]["id"]
            .as_str()
            .unwrap_or("")
            .to_string();
        b.set_viewport(WIDTH, HEIGHT)?;
        Ok(b)
    }

    fn send(&mut self, method: &str, params: Value) -> Result<u64, String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut sent = self.ws.send(Message::text(
            json!({"id": id, "method": method, "params": params}).to_string(),
        ));
        // Interrupted by a signal: the message is already queued, so only the flush is tried again.
        for _ in 0..20 {
            match sent {
                Err(tungstenite::Error::Io(ref e))
                    if e.kind() == std::io::ErrorKind::Interrupted =>
                {
                    sent = self.ws.flush();
                }
                _ => break,
            }
        }
        sent.map_err(|e| format!("The browser connection closed: {e}"))?;
        Ok(id)
    }

    /// The next message from the browser, or None after a short wait. A read that a signal interrupted (EINTR on
    /// Linux, seen while other tests start processes) is only a wait too; the websocket keeps any partial frame.
    fn read(&mut self) -> Result<Option<Value>, String> {
        match self.ws.read() {
            Ok(Message::Text(t)) => Ok(serde_json::from_str(t.as_str()).ok()),
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
            Err(e) => Err(format!("The browser connection closed: {e}")),
        }
    }

    /// Sends a DevTools command and waits for its answer, noting the events that arrive meanwhile.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.send(method, params)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if Instant::now() > deadline {
                return Err(format!(
                    "The browser did not answer {method} in 30 seconds."
                ));
            }
            let Some(msg) = self.read()? else { continue };
            if msg["id"].as_u64() == Some(id) {
                if let Some(e) = msg.get("error") {
                    return Err(e["message"].as_str().unwrap_or("Browser error").to_string());
                }
                return Ok(msg["result"].clone());
            }
            if msg.get("method").is_some() {
                self.on_event(&msg);
            }
        }
    }

    fn on_event(&mut self, msg: &Value) {
        let p = &msg["params"];
        match msg["method"].as_str().unwrap_or("") {
            "Runtime.consoleAPICalled"
                if matches!(p["type"].as_str(), Some("error" | "assert")) =>
            {
                let text = p["args"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|v| {
                                v["value"]
                                    .as_str()
                                    .map(String::from)
                                    .or_else(|| v["description"].as_str().map(String::from))
                                    .unwrap_or_else(|| v["value"].to_string())
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                Problems::push(&mut self.problems.console, short(&text, 300));
            }
            "Runtime.exceptionThrown" => {
                let d = &p["exceptionDetails"];
                let text = d["exception"]["description"]
                    .as_str()
                    .or(d["text"].as_str())
                    .unwrap_or("Uncaught exception");
                let at = d["url"]
                    .as_str()
                    .filter(|u| !u.is_empty())
                    .map(|u| format!(" ({}:{})", u, d["lineNumber"].as_u64().unwrap_or(0) + 1));
                let first = text.lines().next().unwrap_or(text);
                Problems::push(
                    &mut self.problems.console,
                    short(&format!("{first}{}", at.unwrap_or_default()), 300),
                );
            }
            "Log.entryAdded"
                if p["entry"]["level"] == "error" && p["entry"]["source"] != "network" =>
            {
                Problems::push(
                    &mut self.problems.console,
                    short(p["entry"]["text"].as_str().unwrap_or(""), 300),
                );
            }
            "Network.requestWillBeSent" => {
                let id = p["requestId"].as_str().unwrap_or("").to_string();
                let url = p["request"]["url"].as_str().unwrap_or("").to_string();
                if !url.starts_with("data:") {
                    let method = p["request"]["method"].as_str().unwrap_or("GET").to_string();
                    self.pending.insert(id, (Instant::now(), url, method));
                }
            }
            "Network.responseReceived" => {
                let status = p["response"]["status"].as_u64().unwrap_or(0) as u16;
                if p["type"] == "Document"
                    && p["frameId"].as_str() == Some(self.main_frame.as_str())
                {
                    self.status = Some(status);
                }
                if status >= 400 {
                    let url = p["response"]["url"].as_str().unwrap_or("");
                    let method = self
                        .pending
                        .get(p["requestId"].as_str().unwrap_or(""))
                        .map(|r| r.2.clone())
                        .unwrap_or_else(|| "GET".into());
                    Problems::push(
                        &mut self.problems.failed,
                        format!("{status} {method} {}", short(url, 200)),
                    );
                }
            }
            "Network.loadingFinished" => {
                self.pending.remove(p["requestId"].as_str().unwrap_or(""));
            }
            "Network.loadingFailed" => {
                if let Some((_, url, method)) =
                    self.pending.remove(p["requestId"].as_str().unwrap_or(""))
                    && !p["canceled"].as_bool().unwrap_or(false)
                {
                    let why = p["errorText"].as_str().unwrap_or("failed");
                    Problems::push(
                        &mut self.problems.failed,
                        format!("{why} {method} {}", short(&url, 200)),
                    );
                }
            }
            "Page.javascriptDialogOpening" => {
                let kind = p["type"].as_str().unwrap_or("alert");
                self.dialogs.push(format!(
                    "{kind}: {}",
                    short(p["message"].as_str().unwrap_or(""), 200)
                ));
                // Answered right away (the answer to this command is ignored), so the page does not hang.
                let _ = self.send("Page.handleJavaScriptDialog", json!({"accept": true}));
            }
            _ => {}
        }
    }

    /// Waits until the page has loaded and the network has been quiet for half a second (long-running requests such
    /// as live updates are ignored after 3 seconds), or until `max` has passed.
    pub fn settle(&mut self, max: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut quiet_since = Instant::now();
        let mut last_ready = Instant::now() - Duration::from_secs(1);
        let mut ready = false;
        while start.elapsed() < max {
            if let Some(msg) = self.read()?
                && msg.get("method").is_some()
            {
                self.on_event(&msg);
            }
            let busy = self
                .pending
                .values()
                .any(|(t, _, _)| t.elapsed() < Duration::from_secs(3));
            if busy {
                quiet_since = Instant::now();
            }
            if last_ready.elapsed() > Duration::from_millis(200) {
                last_ready = Instant::now();
                ready = self.page("ready", Value::Null, 0)?.as_str() == Some("complete");
            }
            if ready && quiet_since.elapsed() >= Duration::from_millis(500) {
                break;
            }
        }
        Ok(())
    }

    /// Runs Qanuvi's page helper (page.js) in the page.
    pub fn page(&mut self, mode: &str, arg: Value, max: usize) -> Result<Value, String> {
        let js = PAGE_JS.trim().trim_end_matches(';');
        // Between two pages (or on a slow machine) the document may not exist yet: wait for it, up to 5 s.
        let expr = format!(
            "(async () => {{ for (let i = 0; i < 100 && (!document.documentElement || document.readyState === 'loading'); i++) await new Promise((r) => setTimeout(r, 50)); return ({js})({}, {}, {max}); }})()",
            json!(mode),
            arg
        );
        self.eval(&expr)
    }

    /// Evaluates JavaScript in the page and answers its value (waiting for a promise).
    pub fn eval(&mut self, expression: &str) -> Result<Value, String> {
        let r = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true, "userGesture": true}),
        )?;
        if let Some(d) = r.get("exceptionDetails") {
            let text = d["exception"]["description"]
                .as_str()
                .or(d["text"].as_str())
                .unwrap_or("The script failed.");
            return Err(short(text, 500));
        }
        Ok(r["result"]["value"].clone())
    }

    /// Opens an address and waits for the page to settle. Clears what was noted about the previous page.
    pub fn open(&mut self, url: &str) -> Result<(), String> {
        self.problems = Problems::default();
        self.dialogs.clear();
        self.status = None;
        let r = self.call("Page.navigate", json!({"url": url}))?;
        if let Some(e) = r["errorText"].as_str().filter(|e| !e.is_empty()) {
            return Err(format!("{url} could not be opened: {e}"));
        }
        self.settle(Duration::from_secs(15))
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) -> Result<(), String> {
        let (w, h) = (width.clamp(240, 3840), height.clamp(240, 4000));
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({"width": w, "height": h, "deviceScaleFactor": 1, "mobile": w < 600}),
        )?;
        self.viewport = (w, h);
        Ok(())
    }

    /// Clicks at a point in the page like a mouse would.
    pub fn click_at(&mut self, x: f64, y: f64) -> Result<(), String> {
        self.call(
            "Input.dispatchMouseEvent",
            json!({"type": "mouseMoved", "x": x, "y": y}),
        )?;
        for kind in ["mousePressed", "mouseReleased"] {
            self.call(
                "Input.dispatchMouseEvent",
                json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": 1}),
            )?;
        }
        Ok(())
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), String> {
        self.call("Input.insertText", json!({"text": text}))
            .map(|_| ())
    }

    pub fn press_enter(&mut self) -> Result<(), String> {
        for kind in ["keyDown", "keyUp"] {
            let mut p =
                json!({"type": kind, "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13});
            if kind == "keyDown" {
                p["text"] = json!("\r");
            }
            self.call("Input.dispatchKeyEvent", p)?;
        }
        Ok(())
    }

    /// A JPEG of the visible page, the whole page or one area (page coordinates), as base64.
    pub fn screenshot(
        &mut self,
        full_page: bool,
        clip: Option<(f64, f64, f64, f64)>,
    ) -> Result<String, String> {
        let mut p = json!({"format": "jpeg", "quality": 70, "captureBeyondViewport": full_page || clip.is_some()});
        if let Some((x, y, w, h)) = clip {
            p["clip"] =
                json!({"x": x, "y": y, "width": w.max(1.0), "height": h.max(1.0), "scale": 1});
        } else if full_page {
            let size = self.eval(
                "[document.documentElement.scrollWidth, document.documentElement.scrollHeight]",
            )?;
            let w = size[0].as_f64().unwrap_or(WIDTH as f64);
            let h = size[1].as_f64().unwrap_or(HEIGHT as f64).min(8000.0);
            p["clip"] = json!({"x": 0, "y": 0, "width": w, "height": h, "scale": 1});
        }
        let r = self.call("Page.captureScreenshot", p)?;
        r["data"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "The browser gave no screenshot.".into())
    }
}

/// A Windows job object that ends the browser and everything it started when this process ends, even when it is
/// killed or crashes (the system closes the handle then). Elsewhere it does nothing yet.
pub(crate) struct Job(#[cfg(windows)] Option<windows_sys::Win32::Foundation::HANDLE>);

// The handle is only closed on drop; the job object itself may be used from any thread.
unsafe impl Send for Job {}

impl Job {
    #[cfg(windows)]
    pub(crate) fn for_child(child: &Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        // SAFETY: plain Win32 calls on a new job object and the child's own process handle.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Job(None);
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0
                && AssignProcessToJobObject(job, child.as_raw_handle() as _) != 0;
            if !ok {
                windows_sys::Win32::Foundation::CloseHandle(job);
                return Job(None);
            }
            Job(Some(job))
        }
    }

    #[cfg(not(windows))]
    pub(crate) fn for_child(_child: &Child) -> Self {
        Job()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(h) = self.0.take() {
            // SAFETY: the handle came from CreateJobObjectW and is closed once.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(h);
            }
        }
    }
}

/// Stops a browser that was started but not handed over yet (when connecting to it fails).
struct Guard {
    child: Option<Child>,
    profile: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            stop(&mut c, &self.profile);
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.send("Browser.close", json!({}));
        stop(&mut self.child, &self.profile);
    }
}

/// Stops the browser and everything it started, then removes its profile.
fn stop(child: &mut Child, profile: &Path) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if let Ok(None) = child.try_wait() {
        if cfg!(windows) {
            let mut k = Command::new("taskkill");
            k.args(["/T", "/F", "/PID", &child.id().to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            no_window(&mut k);
            let _ = k.status();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    // The browser can hold files in its profile for a moment after it ends.
    for _ in 0..10 {
        if std::fs::remove_dir_all(profile).is_ok() || !profile.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// At most `max` characters, on one line.
pub fn short(text: &str, max: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > max {
        format!("{}…", one.chars().take(max).collect::<String>())
    } else {
        one
    }
}

/// Lines in `after` that were not in `before`: what a click or a form changed on the page.
pub fn new_lines(before: &str, after: &str, max_lines: usize) -> Vec<String> {
    let seen: std::collections::HashSet<&str> = before.lines().collect();
    let mut out = Vec::new();
    let mut shown = std::collections::HashSet::new();
    for l in after.lines() {
        if !seen.contains(l) && shown.insert(l) {
            out.push(l.to_string());
            if out.len() == max_lines {
                break;
            }
        }
    }
    out
}

/// Saves a base64 JPEG to a file (for the preview tab to show).
pub fn save_jpeg(base64_data: &str, file: &Path) -> Result<(), String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .map_err(|e| e.to_string())?;
    std::fs::write(file, bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_keep_the_newest_and_skip_repeats() {
        let mut p = Problems::default();
        for i in 0..60 {
            Problems::push(&mut p.console, format!("e{i}"));
            Problems::push(&mut p.console, format!("e{i}"));
        }
        assert_eq!(p.console.len(), KEEP_LOG);
        assert_eq!(p.console[0], "e10");
        Problems::push(&mut p.failed, "404 GET /x".into());
        let lines = p.lines(3);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[3], "… and 48 more");
        assert_eq!(p.count(), 51);
    }

    #[test]
    fn new_lines_are_what_appeared() {
        let before = "Cart\nEmpty\nTotal 0";
        let after = "Cart\n1 item\nTotal 10\n1 item";
        assert_eq!(new_lines(before, after, 10), vec!["1 item", "Total 10"]);
        assert_eq!(new_lines(before, after, 1), vec!["1 item"]);
    }

    #[test]
    fn short_keeps_one_line() {
        assert_eq!(short("a\n  b   c", 10), "a b c");
        assert_eq!(short("abcdef", 3), "abc…");
    }
}
