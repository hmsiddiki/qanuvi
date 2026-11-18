//! Debug adapters (15i part 3): the same debuggers VS Code uses, spoken to over the Debug Adapter Protocol. Nothing is
//! bundled. An adapter is installed on demand, after the user's yes, into Qanuvi's data folder. It runs only while
//! debugging: it starts with a debug session, runs at below-normal priority, ends with Qanuvi (a job object on
//! Windows), and stops when its session, window or project closes.
//!
//! The window speaks the protocol itself (src/lib/dap). This side only frames the messages, the same
//! `Content-Length` framing as the language servers. An adapter talks either over its standard input and output, or
//! as a small server on a local port (js-debug): then the window may open more connections to it, one per child
//! session (js-debug asks for one with its `startDebugging` request).
//!
//! Migration M1d: on a shown master the adapter is found, installed and run on that machine (a [`Host`]), through a
//! program pipe there. A server adapter listens on the master's own localhost; its port is reached through a tunnel
//! on this computer (`pipes::forward`). PHP's Xdebug calls the adapter back on the master's own localhost, so that
//! needs nothing more. The master knows nothing about debugging: it only runs a program and links a port.

use crate::lsp::{Host, On, Received, frame, read_message};
use crate::pipes::Link;
use crate::sink::Sink;
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// How Qanuvi talks to an adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transport {
    /// Its standard input and output.
    Stdio,
    /// It listens on a local port; every connection is one debug session.
    Server,
}

/// How a release asset is packed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Archive {
    TarGz,
    /// A zip file (a VS Code extension's .vsix).
    Zip,
}

impl Archive {
    fn name(self) -> &'static str {
        match self {
            Archive::TarGz => "tar.gz",
            Archive::Zip => "zip",
        }
    }
}

/// A debug adapter Qanuvi knows how to install and start.
pub struct Spec {
    pub id: &'static str,
    pub label: &'static str,
    /// The `type` values of launch.json it serves.
    pub types: &'static [&'static str],
    /// The GitHub repository whose latest release holds it, and the start and end of the asset's name.
    pub repo: &'static str,
    pub asset: (&'static str, &'static str),
    pub archive: Archive,
    /// The download's size, in words.
    pub size: &'static str,
    /// The script node runs, below the adapter's folder (forward slashes).
    pub entry: &'static str,
    pub transport: Transport,
    pub need_gb: f64,
    /// The oldest Node.js major version it runs on; 0 when any does (no check).
    pub min_node: u32,
}

pub const SPECS: &[Spec] = &[
    Spec {
        id: "node",
        label: "JavaScript debugger (js-debug)",
        types: &["node", "pwa-node", "node-terminal"],
        repo: "microsoft/vscode-js-debug",
        asset: ("js-debug-dap-", ".tar.gz"),
        archive: Archive::TarGz,
        size: "about 10 MB",
        entry: "js-debug/src/dapDebugServer.js",
        transport: Transport::Server,
        need_gb: 0.3,
        // Its releases use JavaScript that Node.js 18 cannot read (a RegExp `v` flag): it stops at once there.
        min_node: 20,
    },
    // The adapter of VS Code's "PHP Debug" extension, made by the Xdebug project. It speaks to Xdebug inside PHP.
    Spec {
        id: "php",
        label: "PHP debugger (PHP Debug, for Xdebug)",
        types: &["php"],
        repo: "xdebug/vscode-php-debug",
        asset: ("php-debug-", ".vsix"),
        archive: Archive::Zip,
        size: "about 2 MB",
        entry: "extension/out/phpDebug.js",
        transport: Transport::Stdio,
        need_gb: 0.15,
        min_node: 0,
    },
];

pub fn spec(id: &str) -> Result<&'static Spec, String> {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Qanuvi has no debugger called {id}."))
}

/// The adapter's folder below the data folder (forward slashes, as the master's data-folder commands take it).
fn own_rel(spec: &Spec) -> String {
    format!("debug-adapters/{}", spec.id)
}

/// The adapter's script on that machine, when it is installed.
fn own_entry(host: &dyn Host, spec: &Spec) -> Option<String> {
    let mut parts = vec!["debug-adapters", spec.id];
    parts.extend(spec.entry.split('/'));
    let p = host.join(&host.data_dir(), &parts);
    host.exists(&p).then_some(p)
}

fn needs_node(place: &str) -> String {
    if place == "this computer" {
        "This debugger runs on Node.js; install Node.js from nodejs.org first.".to_string()
    } else {
        format!("This debugger runs on Node.js; install Node.js from nodejs.org on {place} first.")
    }
}

fn node_too_old(spec: &Spec, place: &str, version: &str) -> String {
    let there = if place == "this computer" {
        String::new()
    } else {
        format!(" on {place}")
    };
    format!(
        "This debugger needs Node.js {} or newer; {place} has {version}. Install a newer Node.js from nodejs.org{there} \
         first.",
        spec.min_node
    )
}

/// `node --version`'s answer ("v18.20.4") as its major number and the version without the "v".
fn parse_node_version(text: &str) -> Option<(u32, String)> {
    let line = text.lines().map(str::trim).find(|l| l.starts_with('v'))?;
    let full = line.trim_start_matches('v');
    let major = full.split('.').next()?.parse().ok()?;
    Some((major, full.to_string()))
}

/// Node.js's version on that machine; None when it does not answer (then nothing is refused).
fn node_version(host: &dyn Host, node: &str) -> Option<(u32, String)> {
    let said = Arc::new(Mutex::new(String::new()));
    let s = said.clone();
    let live = Sink::new(move |line: String| {
        let mut t = s.lock().unwrap();
        t.push_str(&line);
        t.push('\n');
        true
    });
    host.run(node, &["--version".to_string()], None, 30, Some(live))
        .ok()?;
    parse_node_version(&said.lock().unwrap())
}

/// The Node.js that runs this debugger on that machine, or why there is none (missing, or older than it needs).
fn usable_node(host: &dyn Host, spec: &Spec) -> Result<String, String> {
    let node = host.find("node").ok_or_else(|| needs_node(host.place()))?;
    if spec.min_node > 0
        && let Some((major, full)) = node_version(host, &node)
        && major < spec.min_node
    {
        return Err(node_too_old(spec, host.place(), &full));
    }
    Ok(node)
}

/// What an adapter that ended too soon said: its error line (Node's "SyntaxError: ..."), else its last line.
/// Minified source lines that Node prints around an error are left out.
fn adapter_said(lines: &[String]) -> Option<String> {
    let kept: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.len() <= 300 && !l.starts_with("at "))
        .collect();
    kept.iter()
        .find(|l| l.contains("Error"))
        .or_else(|| kept.last())
        .map(|l| l.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub id: &'static str,
    pub label: &'static str,
    pub types: &'static [&'static str],
    /// Installed by Qanuvi.
    pub installed: bool,
    /// The installed version (the release's tag), when known.
    pub version: String,
    /// What installing does, in words; empty when it cannot be installed there.
    pub install: String,
    /// Why it cannot be installed or run there (no Node.js).
    pub missing: String,
}

pub fn status(host: &dyn Host, spec: &'static Spec) -> Status {
    let installed = own_entry(host, spec).is_some();
    let version = host
        .read_small(&format!("{}/version.txt", own_rel(spec)))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let problem = usable_node(host, spec).err();
    let place = host.place();
    Status {
        id: spec.id,
        label: spec.label,
        types: spec.types,
        installed,
        version,
        install: if problem.is_none() {
            format!(
                "Downloads its latest release from GitHub (github.com/{}, {}) into Qanuvi's data folder on {place}; \
                 nothing is installed for the whole machine.",
                spec.repo, spec.size
            )
        } else {
            String::new()
        },
        missing: problem.unwrap_or_default(),
    }
}

pub fn statuses(host: &dyn Host) -> Vec<Status> {
    SPECS.iter().map(|s| status(host, s)).collect()
}

/// Picks the asset of a release (GitHub's JSON) whose name starts and ends as the spec says.
fn pick_asset(release: &serde_json::Value, spec: &Spec) -> Option<(String, String, String)> {
    let tag = release["tag_name"].as_str()?.to_string();
    release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        (name.starts_with(spec.asset.0) && name.ends_with(spec.asset.1)).then(|| {
            (
                tag.clone(),
                name.to_string(),
                a["browser_download_url"].as_str().unwrap_or("").to_string(),
            )
        })
    })
}

/// The newest release's tag, asset name and download address (asked from this computer).
fn latest(spec: &Spec) -> Result<(String, String, String), String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build(),
    );
    let api = format!("https://api.github.com/repos/{}/releases/latest", spec.repo);
    let mut res = agent
        .get(&api)
        .header("User-Agent", "Qanuvi")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GitHub could not be reached: {e}"))?;
    if res.status().as_u16() >= 400 {
        return Err(format!(
            "GitHub answered {} for {}'s latest release.",
            res.status().as_u16(),
            spec.repo
        ));
    }
    let text = res
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("GitHub's answer could not be read: {e}"))?;
    let release: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("GitHub's answer is not JSON: {e}"))?;
    let (tag, name, url) = pick_asset(&release, spec).ok_or_else(|| {
        format!(
            "The latest release of {} has no {}…{} file.",
            spec.repo, spec.asset.0, spec.asset.1
        )
    })?;
    if !url.starts_with("https://github.com/") {
        return Err(format!("{name} comes from an unexpected address: {url}"));
    }
    Ok((tag, name, url))
}

/// Installs an adapter on that machine (blocking; the download takes a few seconds). Only called after the user said
/// yes. The release is looked up from here; the machine downloads and unpacks it into its own data folder.
pub fn install(host: &dyn Host, spec: &'static Spec) -> Result<Status, String> {
    usable_node(host, spec)?;
    let (tag, name, url) = latest(spec)?;
    host.fetch(&url, spec.archive.name(), &own_rel(spec), spec.entry, &tag)
        .map_err(|e| format!("Installing {name} failed: {e}"))?;
    Ok(status(host, spec))
}

/// PHP on that machine and the Xdebug it can load, for the PHP debugger.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PhpInfo {
    /// The php program's full path; empty when none was found.
    pub php: String,
    pub version: String,
    /// "NTS x64" and the like: an Xdebug file must match it.
    pub build: String,
    /// The Xdebug version a debug run gets; empty when it gets none.
    pub xdebug: String,
    /// "ini" when PHP's own settings load Xdebug, "file" when Qanuvi loads the chosen file for debug runs only.
    pub via: String,
    /// What is wrong, in words.
    pub note: String,
}

const PHP_MARK: &str = "@@qanuvi@@";

/// Asks a php program for its version, build and Xdebug (with an extra Xdebug file loaded, when given).
fn ask_php(
    host: &dyn Host,
    php: &str,
    xdebug_file: Option<&str>,
) -> Option<(String, String, String)> {
    let mut args: Vec<String> = Vec::new();
    if let Some(f) = xdebug_file {
        args.push("-d".into());
        args.push(format!("zend_extension={f}"));
    }
    for a in ["-d", "xdebug.log_level=0", "-d", "xdebug.mode=off", "-r"] {
        args.push(a.into());
    }
    args.push(format!(
        "echo '{PHP_MARK}', PHP_VERSION, '|', (phpversion('xdebug') ?: ''), '|', (PHP_ZTS ? 'TS' : 'NTS'), ' ', \
         (PHP_INT_SIZE * 8 == 64 ? 'x64' : 'x86'), PHP_EOL;"
    ));
    let found: Arc<Mutex<Option<String>>> = Arc::default();
    let f = found.clone();
    let lines = Sink::new(move |l: String| {
        if let Some((_, r)) = l.split_once(PHP_MARK) {
            *f.lock().unwrap() = Some(r.to_string());
        }
        true
    });
    host.run(php, &args, None, 30, Some(lines)).ok()?;
    let line = found.lock().unwrap().take()?;
    parse_php_line(&line)
}

fn parse_php_line(line: &str) -> Option<(String, String, String)> {
    let mut parts = line.trim().splitn(3, '|');
    let version = parts.next()?.trim().to_string();
    let xdebug = parts.next()?.trim().to_string();
    let build = parts.next()?.trim().to_string();
    (!version.is_empty()).then_some((version, xdebug, build))
}

fn minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Finds PHP and its Xdebug on that machine. `xdebug_file` is the Xdebug the user chose in that machine's Settings
/// ("" for none); it is loaded only for debug runs, so PHP's own settings (php.ini) stay as they are.
pub fn php_info(host: &dyn Host, xdebug_file: &str) -> PhpInfo {
    let place = host.place();
    let Some(php) = host.find("php") else {
        return PhpInfo {
            note: format!(
                "PHP is not on the PATH of {place}. Install PHP (php.net) or add its folder to PATH."
            ),
            ..Default::default()
        };
    };
    let mut info = PhpInfo {
        php: php.clone(),
        ..Default::default()
    };
    let Some((version, xdebug, build)) = ask_php(host, &php, None) else {
        info.note = format!("{} did not answer.", info.php);
        return info;
    };
    info.version = version;
    info.build = build;
    if !xdebug.is_empty() {
        info.xdebug = xdebug;
        info.via = "ini".into();
        return info;
    }
    let file = xdebug_file.trim();
    if file.is_empty() {
        info.note = format!(
            "PHP {} has no Xdebug. Download the Xdebug file for PHP {} {} from xdebug.org and choose it in \
             Settings > Languages; Qanuvi loads it for debug runs only.",
            info.version,
            minor(&info.version),
            info.build
        );
        return info;
    }
    if !host.exists(file) {
        info.note = format!("The Xdebug file {file} does not exist on {place}.");
        return info;
    }
    match ask_php(host, &php, Some(file)) {
        Some((_, x, _)) if !x.is_empty() => {
            info.xdebug = x;
            info.via = "file".into();
        }
        _ => {
            info.note = format!(
                "PHP {} could not load {file}: the Xdebug file must be made for PHP {} {}.",
                info.version,
                minor(&info.version),
                info.build
            );
        }
    }
    info
}

/// What an adapter sends to the window.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DapEvent {
    /// One whole message on one connection (0 is the first).
    Message { conn: u32, text: String },
    /// A line the adapter wrote to its own output (kept for its log).
    Log { text: String },
    /// A connection closed (its session ended).
    Closed { conn: u32 },
    /// The adapter ended.
    Exit { code: Option<i32> },
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Writes into a program pipe on a master; each write numbered, since the master may get them out of order.
struct PipeWriter {
    link: Arc<dyn Link>,
    id: String,
    seq: AtomicU64,
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.seq.fetch_add(1, Ordering::SeqCst);
        self.link.tell(
            "pipe_send",
            json!({"id": self.id, "seq": n, "data": B64.encode(buf)}),
        );
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A program pipe on a master (This PC's own too, M5c); a server adapter's port comes through `forward`.
struct Proc {
    link: Arc<dyn Link>,
    pipe: String,
    _forward: Option<crate::pipes::Forward>,
}

struct Running {
    project: String,
    owner: String,
    /// The local port a server adapter is reached on (on a master: the tunnel's).
    port: Option<u16>,
    conns: HashMap<u32, Writer>,
    next: u32,
    sink: Sink<DapEvent>,
    proc: Proc,
}

impl Running {
    fn end(self) {
        let Proc { link, pipe, .. } = self.proc;
        link.tell("pipe_stop", json!({"id": pipe}));
    }
}

/// The running debug adapters of this Qanuvi, by key.
#[derive(Default, Clone)]
pub struct Adapters {
    map: Arc<Mutex<HashMap<String, Running>>>,
}

/// Reads a connection's messages into the sink until it closes.
fn read_into(
    map: Arc<Mutex<HashMap<String, Running>>>,
    key: String,
    conn: u32,
    r: impl Read + Send + 'static,
    sink: Sink<DapEvent>,
) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        while let Some(text) = read_message(&mut r) {
            if sink.send(DapEvent::Message { conn, text }).is_err() {
                break;
            }
        }
        if let Some(run) = map.lock().unwrap().get_mut(&key) {
            run.conns.remove(&conn);
        }
        let _ = sink.send(DapEvent::Closed { conn });
    });
}

fn connect(port: u16, wait: Duration) -> Result<TcpStream, String> {
    let until = Instant::now() + wait;
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) if Instant::now() >= until => {
                return Err(format!("The debugger did not open its port: {e}"));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The port in js-debug's "Debug server listening at 127.0.0.1:PORT" line.
fn listening_port(line: &str) -> Option<u16> {
    let rest = line.split("listening at").nth(1)?.trim();
    rest.rsplit(':').next()?.trim().parse().ok()
}

/// Splits a stream of bytes into lines (a program's output arriving in pieces).
#[derive(Default)]
struct Lines(Vec<u8>);

impl Lines {
    fn push(&mut self, bytes: &[u8], mut each: impl FnMut(String)) {
        for &b in bytes {
            if b == b'\n' || b == b'\r' {
                if !self.0.is_empty() {
                    each(String::from_utf8_lossy(&self.0).into_owned());
                    self.0.clear();
                }
            } else {
                self.0.push(b);
            }
        }
    }
}

impl Adapters {
    /// Starts an adapter for a project on a shown master (migration M1d), through a program pipe there. `project` is
    /// the master's own id for it. A server adapter picks a free port on the master and says which; this computer
    /// reaches it through a tunnel.
    pub fn start_on(
        &self,
        host: On,
        project: &str,
        owner: &str,
        id: &str,
        sink: Sink<DapEvent>,
    ) -> Result<String, String> {
        let spec = spec(id)?;
        let link = host.link();
        let entry = own_entry(&host, spec)
            .ok_or_else(|| format!("{} is not installed on {}.", spec.label, host.place()))?;
        let node = usable_node(&host, spec)?;
        if let Some(room) = host.room() {
            crate::machine::fits(spec.label, spec.need_gb, room)
                .map_err(|e| format!("On {}: {e}", host.place()))?;
        }
        let key = format!("dap-{}", uuid::Uuid::new_v4());
        let server = spec.transport == Transport::Server;
        let mut args = vec![entry];
        if server {
            // Port 0: the adapter takes any free port of the master and prints it.
            args.push("0".into());
            args.push("127.0.0.1".into());
        }

        // The pipe's stream: a stdio adapter's output is its messages; a server adapter's is its log, with the port.
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let tx = Mutex::new(Some(tx));
        let (port_tx, port_rx) = mpsc::channel::<u16>();
        let port_tx = Mutex::new(Some(port_tx));
        let lines = Mutex::new(Lines::default());
        // Its last output lines: when it ends before it is ready, the message says why (finding q1).
        let said = Arc::new(Mutex::new(Vec::<String>::new()));
        let heard = said.clone();
        let keep = move |text: &str| {
            let mut h = heard.lock().unwrap();
            h.extend(text.lines().map(String::from));
            let over = h.len().saturating_sub(60);
            h.drain(..over);
        };
        let (log, map, k) = (sink.clone(), self.map.clone(), key.clone());
        let events = Sink::new(move |e: Value| match e["kind"].as_str() {
            Some("out") => {
                let bytes = B64
                    .decode(e["data"].as_str().unwrap_or(""))
                    .unwrap_or_default();
                if !server {
                    return tx
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|t| t.send(bytes).is_ok());
                }
                lines.lock().unwrap().push(&bytes, |line| {
                    if let Some(p) = listening_port(&line)
                        && let Some(t) = port_tx.lock().unwrap().take()
                    {
                        let _ = t.send(p);
                    }
                    keep(&line);
                    let _ = log.send(DapEvent::Log { text: line });
                });
                true
            }
            Some("err") => {
                let text = e["text"].as_str().unwrap_or("").to_string();
                keep(&text);
                log.send(DapEvent::Log { text }).is_ok()
            }
            Some("exit") => {
                tx.lock().unwrap().take();
                port_tx.lock().unwrap().take();
                // Stopped from here already, or ended by itself: either way the window hears once.
                if map.lock().unwrap().remove(&k).is_some() {
                    let code = e["code"].as_i64().map(|c| c as i32);
                    let _ = log.send(DapEvent::Exit { code });
                }
                false
            }
            _ => true,
        });
        let pipe = link.call(
            "pipe_start",
            json!({"projectId": project, "program": node, "args": args}),
            Some(events),
        )?;
        let pipe = pipe.as_str().unwrap_or("").to_string();
        let stop_pipe = |link: &Arc<dyn Link>| link.tell("pipe_stop", json!({"id": pipe}));

        let mut conns: HashMap<u32, Writer> = HashMap::new();
        let (first, port, forward): (
            Box<dyn Read + Send>,
            Option<u16>,
            Option<crate::pipes::Forward>,
        ) = if server {
            let remote = match port_rx.recv_timeout(Duration::from_secs(20)) {
                Ok(p) => p,
                Err(why) => {
                    stop_pipe(&link);
                    let first = match why {
                        mpsc::RecvTimeoutError::Disconnected => {
                            format!("{} ended before it was ready.", spec.label)
                        }
                        mpsc::RecvTimeoutError::Timeout => {
                            format!("{} did not say which port it listens on.", spec.label)
                        }
                    };
                    return Err(match adapter_said(&said.lock().unwrap()) {
                        Some(s) => format!("{first} It said: {s}"),
                        None => first,
                    });
                }
            };
            let fwd = match crate::pipes::forward(link.clone(), remote) {
                Ok(f) => f,
                Err(e) => {
                    stop_pipe(&link);
                    return Err(e);
                }
            };
            let stream = match connect(fwd.local, Duration::from_secs(15)) {
                Ok(s) => s,
                Err(e) => {
                    stop_pipe(&link);
                    return Err(e);
                }
            };
            let w = stream.try_clone().map_err(|e| e.to_string())?;
            conns.insert(0, Arc::new(Mutex::new(Box::new(w))));
            let local = fwd.local;
            (Box::new(stream), Some(local), Some(fwd))
        } else {
            let w = PipeWriter {
                link: link.clone(),
                id: pipe.clone(),
                seq: AtomicU64::new(0),
            };
            conns.insert(0, Arc::new(Mutex::new(Box::new(w))));
            (Box::new(Received::new(rx)), None, None)
        };
        self.map.lock().unwrap().insert(
            key.clone(),
            Running {
                project: project.into(),
                owner: owner.into(),
                port,
                conns,
                next: 1,
                sink: sink.clone(),
                proc: Proc {
                    link,
                    pipe,
                    _forward: forward,
                },
            },
        );
        read_into(self.map.clone(), key.clone(), 0, first, sink);
        Ok(key)
    }

    /// Opens one more connection to a server adapter (a child session); answers its number.
    pub fn connect(&self, key: &str) -> Result<u32, String> {
        let (port, sink) = {
            let map = self.map.lock().unwrap();
            let run = map.get(key).ok_or("That debugger is not running.")?;
            (
                run.port.ok_or("This debugger has only one session.")?,
                run.sink.clone(),
            )
        };
        let stream = connect(port, Duration::from_secs(5))?;
        let w = stream.try_clone().map_err(|e| e.to_string())?;
        let conn = {
            let mut map = self.map.lock().unwrap();
            let run = map.get_mut(key).ok_or("That debugger is not running.")?;
            let n = run.next;
            run.next += 1;
            run.conns.insert(n, Arc::new(Mutex::new(Box::new(w))));
            n
        };
        read_into(self.map.clone(), key.to_string(), conn, stream, sink);
        Ok(conn)
    }

    pub fn send(&self, key: &str, conn: u32, text: &str) -> Result<(), String> {
        let w = self
            .map
            .lock()
            .unwrap()
            .get(key)
            .and_then(|r| r.conns.get(&conn).cloned())
            .ok_or("That debug session is not running.")?;
        let mut w = w.lock().unwrap();
        w.write_all(&frame(text))
            .and_then(|_| w.flush())
            .map_err(|e| format!("The debugger stopped: {e}"))
    }

    pub fn stop(&self, key: &str) {
        let run = self.map.lock().unwrap().remove(key);
        if let Some(run) = run {
            let sink = run.sink.clone();
            run.end();
            let _ = sink.send(DapEvent::Exit { code: None });
        }
    }

    fn stop_where(&self, f: impl Fn(&Running) -> bool) {
        let keys: Vec<String> = {
            let map = self.map.lock().unwrap();
            map.iter()
                .filter(|(_, r)| f(r))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for k in keys {
            self.stop(&k);
        }
    }

    pub fn stop_owner(&self, owner: &str) {
        self.stop_where(|r| r.owner == owner);
    }

    pub fn stop_project(&self, project: &str) {
        self.stop_where(|r| r.project == project);
    }

    pub fn stop_all(&self) {
        self.stop_where(|_| true);
    }

    pub fn running(&self) -> usize {
        self.map.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A test program with no console window.
    fn hide(cmd: &mut Command) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        #[cfg(not(windows))]
        let _ = cmd;
    }

    fn free_port() -> Result<u16, String> {
        let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
    }

    use crate::lsp::Here;

    #[test]
    fn knows_its_adapters() {
        assert!(spec("node").is_ok() && spec("cobol").is_err());
        for s in SPECS {
            let st = status(&Here, s);
            assert!(!st.install.is_empty() || !st.missing.is_empty(), "{}", s.id);
        }
    }

    #[test]
    fn picks_the_release_asset() {
        let release = serde_json::json!({
            "tag_name": "v1.2.3",
            "assets": [
                {"name": "js-debug-v1.2.3.vsix", "browser_download_url": "https://github.com/x/a.vsix"},
                {"name": "js-debug-dap-v1.2.3.tar.gz", "browser_download_url": "https://github.com/x/b.tar.gz"}
            ]
        });
        let (tag, name, url) = pick_asset(&release, &SPECS[0]).unwrap();
        assert_eq!(
            (tag.as_str(), name.as_str()),
            ("v1.2.3", "js-debug-dap-v1.2.3.tar.gz")
        );
        assert_eq!(url, "https://github.com/x/b.tar.gz");
        assert!(
            pick_asset(
                &serde_json::json!({"tag_name": "v1", "assets": []}),
                &SPECS[0]
            )
            .is_none()
        );
    }

    #[test]
    fn picks_the_php_debug_asset() {
        let release = serde_json::json!({
            "tag_name": "v1.40.2",
            "assets": [{"name": "php-debug-1.40.2.vsix", "browser_download_url": "https://github.com/x/p.vsix"}]
        });
        let php = spec("php").unwrap();
        assert_eq!(php.archive, Archive::Zip);
        assert_eq!(
            pick_asset(&release, php).unwrap().1,
            "php-debug-1.40.2.vsix"
        );
    }

    #[test]
    fn unpacks_a_zip_and_swaps_it_in() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("php");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("old.txt"), "old").unwrap();
        let fresh = dir.path().join("php.new");
        std::fs::create_dir_all(&fresh).unwrap();
        let archive = fresh.join("download.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            w.start_file(
                "extension/out/phpDebug.js",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            w.write_all(b"// adapter").unwrap();
            w.finish().unwrap();
        }
        crate::pipes::host::place(&archive, "zip", &target, "extension/out/phpDebug.js", "v9")
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("extension/out/phpDebug.js")).unwrap(),
            "// adapter"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("version.txt")).unwrap(),
            "v9"
        );
        assert!(!target.join("old.txt").exists() && !fresh.exists());
    }

    #[test]
    fn keeps_the_old_copy_when_the_download_lacks_the_adapter() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("php");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("old.txt"), "old").unwrap();
        let fresh = dir.path().join("php.new");
        std::fs::create_dir_all(&fresh).unwrap();
        let archive = fresh.join("download.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
            w.start_file("other.js", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.finish().unwrap();
        }
        assert!(
            crate::pipes::host::place(&archive, "zip", &target, "extension/out/phpDebug.js", "v9")
                .is_err()
        );
        assert!(target.join("old.txt").exists());
    }

    #[test]
    fn reads_php_answers() {
        assert_eq!(
            parse_php_line("8.2.3|3.5.3|NTS x64"),
            Some(("8.2.3".into(), "3.5.3".into(), "NTS x64".into()))
        );
        assert_eq!(
            parse_php_line("8.3.0||TS x64").map(|p| p.1),
            Some(String::new())
        );
        assert_eq!(parse_php_line("|x|y"), None);
    }

    #[test]
    fn reads_the_port_a_server_adapter_took() {
        assert_eq!(
            listening_port("Debug server listening at 127.0.0.1:53123"),
            Some(53123)
        );
        assert_eq!(
            listening_port("Debug server listening at ::1:4711"),
            Some(4711)
        );
        assert_eq!(listening_port("something else"), None);
        let mut lines = Lines::default();
        let mut got = Vec::new();
        lines.push(b"Debug server list", |l| got.push(l));
        lines.push(b"ening at 127.0.0.1:5000\r\nnext", |l| got.push(l));
        assert_eq!(
            got,
            vec!["Debug server listening at 127.0.0.1:5000".to_string()]
        );
    }

    #[test]
    fn knows_a_node_too_old_for_a_debugger() {
        assert_eq!(
            parse_node_version("v18.20.4\n"),
            Some((18, "18.20.4".to_string()))
        );
        assert_eq!(
            parse_node_version("v22.11.0"),
            Some((22, "22.11.0".to_string()))
        );
        assert_eq!(parse_node_version("node: not found"), None);
        let js = spec("node").unwrap();
        assert_eq!(js.min_node, 20);
        assert_eq!(spec("php").unwrap().min_node, 0);
        assert_eq!(
            node_too_old(js, "basevm378", "18.20.4"),
            "This debugger needs Node.js 20 or newer; basevm378 has 18.20.4. Install a newer Node.js from \
             nodejs.org on basevm378 first."
        );
        assert!(node_too_old(js, "this computer", "18.0.0").ends_with("from nodejs.org first."));
    }

    #[test]
    fn says_why_an_adapter_ended_too_soon() {
        // What Node 18 printed when js-debug 1.140 stopped at once (its minified source line shortened here).
        let long = "x".repeat(400);
        let out: Vec<String> = [
            "/root/.local/share/qanuvi/debug-adapters/node/js-debug/src/dapDebugServer.js:91",
            long.as_str(),
            "",
            "SyntaxError: Option 'input': Invalid flags supplied to RegExp constructor 'v'",
            "    at new RegExp (<anonymous>)",
            "    at Re (/root/js-debug/src/dapDebugServer.js:87:13113)",
            "",
            "Node.js v18.20.4",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            adapter_said(&out).as_deref(),
            Some("SyntaxError: Option 'input': Invalid flags supplied to RegExp constructor 'v'")
        );
        assert_eq!(
            adapter_said(&["listening soon".into(), "bye".into()]).as_deref(),
            Some("bye")
        );
        assert_eq!(adapter_said(&[]), None);
    }

    #[test]
    fn talks_to_a_server_adapter() {
        // A stand-in server adapter: listens on the port it is given and echoes each connection.
        let Some(node) = crate::local_servers::find("node") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("echo.js");
        std::fs::write(
            &script,
            "require('net').createServer(s => s.pipe(s)).listen(+process.argv[2], '127.0.0.1');",
        )
        .unwrap();
        let port = free_port().unwrap();
        let mut cmd = Command::new(node);
        cmd.arg(&script).arg(port.to_string());
        hide(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let mut s = connect(port, Duration::from_secs(10)).unwrap();
        s.write_all(&frame(r#"{"seq":1}"#)).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        assert_eq!(read_message(&mut r).as_deref(), Some(r#"{"seq":1}"#));
        let _ = child.kill();
        let _ = child.wait();
    }
}
