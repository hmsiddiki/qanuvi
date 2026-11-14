//! Debug adapters (15i part 3): the same debuggers VS Code uses, spoken to over the Debug Adapter Protocol. Nothing is
//! bundled. An adapter is installed on demand, after the user's yes, into Qanuvi's data folder. It runs only while
//! debugging: it starts with a debug session, runs at below-normal priority, ends with Qanuvi (a job object on
//! Windows), and stops when its session, window or project closes.
//!
//! The window speaks the protocol itself (src/lib/dap). This side only frames the messages, the same
//! `Content-Length` framing as the language servers. An adapter talks either over its standard input and output, or
//! as a small server on a local port (js-debug): then the window may open more connections to it, one per child
//! session (js-debug asks for one with its `startDebugging` request).

use crate::lsp::{frame, read_message};
use crate::sink::Sink;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How Qanuvi talks to an adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Transport {
    /// Its standard input and output.
    Stdio,
    /// It listens on a local port Qanuvi gives it; every connection is one debug session.
    Server,
}

/// How a release asset is packed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Archive {
    TarGz,
    /// A zip file (a VS Code extension's .vsix).
    Zip,
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
    /// The script node runs, below the adapter's folder.
    pub entry: &'static str,
    pub transport: Transport,
    pub need_gb: f64,
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
    },
];

pub fn spec(id: &str) -> Result<&'static Spec, String> {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Qanuvi has no debugger called {id}."))
}

fn own_dir(spec: &Spec) -> PathBuf {
    crate::paths::data_dir()
        .join("debug-adapters")
        .join(spec.id)
}

fn own_entry(spec: &Spec) -> Option<PathBuf> {
    let p = own_dir(spec).join(spec.entry);
    p.is_file().then_some(p)
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
    /// What installing does, in words; empty when it cannot be installed here.
    pub install: String,
    /// Why it cannot be installed or run here (no Node.js).
    pub missing: String,
}

pub fn status(spec: &'static Spec) -> Status {
    let installed = own_entry(spec).is_some();
    let version = std::fs::read_to_string(own_dir(spec).join("version.txt"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let node = crate::local_servers::find("node").is_some();
    Status {
        id: spec.id,
        label: spec.label,
        types: spec.types,
        installed,
        version,
        install: if node {
            format!(
                "Downloads its latest release from GitHub (github.com/{}, {}) into Qanuvi's data folder; nothing \
                 is installed for the whole computer.",
                spec.repo, spec.size
            )
        } else {
            String::new()
        },
        missing: if node {
            String::new()
        } else {
            "This debugger runs on Node.js; install Node.js from nodejs.org first.".to_string()
        },
    }
}

pub fn statuses() -> Vec<Status> {
    SPECS.iter().map(status).collect()
}

fn hide(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
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

/// Installs an adapter (blocking; the download takes a few seconds). Only called after the user said yes.
pub fn install(spec: &'static Spec) -> Result<Status, String> {
    if crate::local_servers::find("node").is_none() {
        return Err("This debugger runs on Node.js; install Node.js from nodejs.org first.".into());
    }
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(180)))
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

    let dir = own_dir(spec);
    let fresh = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh)
        .map_err(|e| format!("Could not make {}: {e}", fresh.display()))?;
    let archive = fresh.join(&name);
    let mut res = agent
        .get(&url)
        .header("User-Agent", "Qanuvi")
        .call()
        .map_err(|e| format!("{name} could not be downloaded: {e}"))?;
    if res.status().as_u16() >= 400 {
        return Err(format!(
            "GitHub answered {} for {name}.",
            res.status().as_u16()
        ));
    }
    {
        let mut out = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
        let mut body = res
            .body_mut()
            .with_config()
            .limit(200 * 1024 * 1024)
            .reader();
        std::io::copy(&mut body, &mut out)
            .map_err(|e| format!("{name} could not be downloaded: {e}"))?;
    }
    unpack(spec.archive, &archive, &fresh).map_err(|e| format!("Unpacking {name} failed: {e}"))?;
    let _ = std::fs::remove_file(&archive);
    if !fresh.join(spec.entry).is_file() {
        return Err(format!(
            "{name} does not hold {}; nothing was changed.",
            spec.entry
        ));
    }
    std::fs::write(fresh.join("version.txt"), &tag).map_err(|e| e.to_string())?;
    // Swap only once the new copy is whole: a failed update keeps the old one.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&fresh, &dir)
        .map_err(|e| format!("Could not move the debugger into place: {e}"))?;
    Ok(status(spec))
}

/// Unpacks a downloaded archive into a folder.
fn unpack(kind: Archive, archive: &std::path::Path, into: &std::path::Path) -> Result<(), String> {
    match kind {
        Archive::TarGz => {
            // The tar that comes with Windows 10+, macOS and Linux.
            let mut cmd = Command::new("tar");
            cmd.arg("-xzf")
                .arg(archive)
                .arg("-C")
                .arg(into)
                .stdin(Stdio::null());
            hide(&mut cmd);
            let out = cmd.output().map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        // Not the system tar: Linux's tar cannot read zip files. Names leaving the folder are refused.
        Archive::Zip => {
            let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            zip.extract(into).map_err(|e| e.to_string())
        }
    }
}

/// PHP on this computer and the Xdebug it can load, for the PHP debugger.
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
fn ask_php(php: &std::path::Path, xdebug_file: Option<&str>) -> Option<(String, String, String)> {
    let mut cmd = Command::new(php);
    if let Some(f) = xdebug_file {
        cmd.arg("-d").arg(format!("zend_extension={f}"));
    }
    cmd.args(["-d", "xdebug.log_level=0", "-d", "xdebug.mode=off", "-r"])
        .arg(format!(
            "echo '{PHP_MARK}', PHP_VERSION, '|', (phpversion('xdebug') ?: ''), '|', (PHP_ZTS ? 'TS' : 'NTS'), ' ', \
             (PHP_INT_SIZE * 8 == 64 ? 'x64' : 'x86'), PHP_EOL;"
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide(&mut cmd);
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find_map(|l| l.split_once(PHP_MARK).map(|(_, r)| r))?;
    parse_php_line(line)
}

fn parse_php_line(line: &str) -> Option<(String, String, String)> {
    let mut parts = line.trim().splitn(3, '|');
    let version = parts.next()?.trim().to_string();
    let xdebug = parts.next()?.trim().to_string();
    let build = parts.next()?.trim().to_string();
    (!version.is_empty()).then_some((version, xdebug, build))
}

/// Finds PHP and its Xdebug. `xdebug_file` is the Xdebug the user chose in Settings ("" for none); it is loaded
/// only for debug runs, so PHP's own settings (php.ini) stay as they are.
pub fn php_info(xdebug_file: &str) -> PhpInfo {
    let Some(php) = crate::local_servers::find("php") else {
        return PhpInfo {
            note: "PHP is not on this computer's PATH. Install PHP (php.net) or add its folder to PATH.".into(),
            ..Default::default()
        };
    };
    let mut info = PhpInfo {
        php: php.display().to_string(),
        ..Default::default()
    };
    let Some((version, xdebug, build)) = ask_php(&php, None) else {
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
            info.version
                .split('.')
                .take(2)
                .collect::<Vec<_>>()
                .join("."),
            info.build
        );
        return info;
    }
    if !std::path::Path::new(file).is_file() {
        info.note = format!("The Xdebug file {file} does not exist.");
        return info;
    }
    match ask_php(&php, Some(file)) {
        Some((_, x, _)) if !x.is_empty() => {
            info.xdebug = x;
            info.via = "file".into();
        }
        _ => {
            info.note = format!(
                "PHP {} could not load {file}: the Xdebug file must be made for PHP {} {}.",
                info.version,
                info.version
                    .split('.')
                    .take(2)
                    .collect::<Vec<_>>()
                    .join("."),
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

struct Running {
    project: String,
    owner: String,
    port: Option<u16>,
    conns: HashMap<u32, Writer>,
    next: u32,
    sink: Sink<DapEvent>,
    child: Child,
    _job: crate::browser::Job,
}

/// The running debug adapters of this Qanuvi, by key.
#[derive(Default, Clone)]
pub struct Adapters {
    map: Arc<Mutex<HashMap<String, Running>>>,
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("No free local port: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
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

impl Adapters {
    /// Starts an adapter for a project. Connection 0 is open when this returns; its messages go to `sink`.
    pub fn start(
        &self,
        project: &str,
        owner: &str,
        root: &std::path::Path,
        id: &str,
        sink: Sink<DapEvent>,
    ) -> Result<String, String> {
        let spec = spec(id)?;
        let entry = own_entry(spec).ok_or_else(|| format!("{} is not installed.", spec.label))?;
        let node = crate::local_servers::find("node")
            .ok_or("This debugger runs on Node.js; install Node.js from nodejs.org first.")?;
        crate::machine::check(spec.label, spec.need_gb)?;
        let port = match spec.transport {
            Transport::Server => Some(free_port()?),
            Transport::Stdio => None,
        };
        let mut cmd = Command::new(&node);
        cmd.arg(&entry);
        if let Some(p) = port {
            cmd.arg(p.to_string()).arg("127.0.0.1");
        }
        cmd.current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide(&mut cmd);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start {}: {e}", spec.label))?;
        let job = crate::browser::Job::for_child(&child);
        let key = format!("dap-{}", uuid::Uuid::new_v4());
        let stdin = child.stdin.take().ok_or("The debugger has no input.")?;
        let stdout = child.stdout.take().ok_or("The debugger has no output.")?;
        if let Some(err) = child.stderr.take() {
            let s = sink.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    if s.send(DapEvent::Log { text: line }).is_err() {
                        break;
                    }
                }
            });
        }

        let mut conns: HashMap<u32, Writer> = HashMap::new();
        let first: Box<dyn Read + Send> = match port {
            None => {
                conns.insert(0, Arc::new(Mutex::new(Box::new(stdin))));
                Box::new(stdout)
            }
            Some(p) => {
                // A server adapter's own output is only its log.
                let s = sink.clone();
                std::thread::spawn(move || {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if s.send(DapEvent::Log { text: line }).is_err() {
                            break;
                        }
                    }
                });
                drop(stdin);
                let stream = match connect(p, Duration::from_secs(15)) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e);
                    }
                };
                let w = stream.try_clone().map_err(|e| e.to_string())?;
                conns.insert(0, Arc::new(Mutex::new(Box::new(w))));
                Box::new(stream)
            }
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
                child,
                _job: job,
            },
        );
        read_into(self.map.clone(), key.clone(), 0, first, sink.clone());

        // Watch for the adapter's end.
        let map = self.map.clone();
        let k = key.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(300));
                let mut guard = map.lock().unwrap();
                let Some(run) = guard.get_mut(&k) else { break };
                if let Ok(Some(st)) = run.child.try_wait() {
                    guard.remove(&k);
                    drop(guard);
                    let _ = sink.send(DapEvent::Exit { code: st.code() });
                    break;
                }
            }
        });
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
        if let Some(mut run) = run {
            let _ = run.child.kill();
            let _ = run.child.wait();
            let _ = run.sink.send(DapEvent::Exit { code: None });
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_its_adapters() {
        assert!(spec("node").is_ok() && spec("cobol").is_err());
        for s in SPECS {
            let st = status(s);
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
    fn unpacks_a_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.vsix");
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
        let into = dir.path().join("out");
        std::fs::create_dir_all(&into).unwrap();
        unpack(Archive::Zip, &archive, &into).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("extension/out/phpDebug.js")).unwrap(),
            "// adapter"
        );
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
