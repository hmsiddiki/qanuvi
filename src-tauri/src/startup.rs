//! Start with Qanuvi: commands the owner wants running while Qanuvi is open (a Headroom proxy, for example). Each one
//! starts hidden at start-up; when it has an address to wait for, Qanuvi checks that address, and when the address
//! already answers (the owner started it), it leaves it alone. Only what Qanuvi started is stopped when it closes.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub name: String,
    pub command: String,
    /// host:port that answers once it runs (empty: no check).
    #[serde(default)]
    pub wait_for: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub id: String,
    pub name: String,
    /// "starting", "running", "already running", "failed", "stopped" or "off".
    pub state: String,
    pub detail: String,
    /// The last lines it printed.
    pub output: String,
}

struct Entry {
    child: Option<Child>,
    status: Arc<Mutex<Status>>,
    output: Arc<Mutex<VecDeque<String>>>,
}

#[derive(Default)]
pub struct Startup {
    entries: Mutex<HashMap<String, Entry>>,
}

/// "host:port" from a host:port or an address like http://127.0.0.1:8787/v1.
pub fn host_port(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let rest = t.split("://").nth(1).unwrap_or(t);
    let hostport = rest.split('/').next().unwrap_or(rest);
    let hp = if hostport.contains(':') {
        hostport.to_string()
    } else if t.starts_with("https://") {
        format!("{hostport}:443")
    } else {
        format!("{hostport}:80")
    };
    let hp = hp.replacen("localhost:", "127.0.0.1:", 1);
    hp.rsplit_once(':')
        .filter(|(h, p)| !h.is_empty() && p.parse::<u16>().is_ok())
        .map(|_| hp.clone())
}

/// Whether something answers at host:port now.
pub fn answers(host_port: &str) -> bool {
    host_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(400)).is_ok())
}

/// Waits until the address answers, at most `secs` seconds.
pub fn wait_answers(host_port: &str, secs: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if answers(host_port) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn set(status: &Arc<Mutex<Status>>, state: &str, detail: &str) {
    let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
    s.state = state.into();
    s.detail = detail.into();
}

fn keep_lines(out: Arc<Mutex<VecDeque<String>>>, from: impl std::io::Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in BufReader::new(from).lines().map_while(Result::ok) {
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            o.push_back(line.chars().take(400).collect());
            while o.len() > 200 {
                o.pop_front();
            }
        }
    });
}

impl Startup {
    /// Starts every enabled item that is not running yet (the others keep their state).
    pub fn start_all(&self, items: &[Item]) {
        for item in items {
            let running = self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&item.id)
                .is_some_and(|e| {
                    let s = e
                        .status
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .state
                        .clone();
                    ["starting", "running", "already running"].contains(&s.as_str())
                });
            if !running {
                self.start_one(item);
            }
        }
    }

    pub fn start_one(&self, item: &Item) {
        let status = Arc::new(Mutex::new(Status {
            id: item.id.clone(),
            name: item.name.clone(),
            state: "starting".into(),
            detail: String::new(),
            output: String::new(),
        }));
        let output = Arc::new(Mutex::new(VecDeque::new()));
        let mut entry = Entry {
            child: None,
            status: status.clone(),
            output: output.clone(),
        };
        let wait = host_port(&item.wait_for);
        if !item.enabled {
            set(&status, "off", "");
        } else if wait.as_deref().is_some_and(answers) {
            set(
                &status,
                "already running",
                "It answered already, so Qanuvi left it alone.",
            );
        } else {
            match spawn(&item.command) {
                Err(e) => set(&status, "failed", &e),
                Ok(mut child) => {
                    if let Some(o) = child.stdout.take() {
                        keep_lines(output.clone(), o);
                    }
                    if let Some(e) = child.stderr.take() {
                        keep_lines(output.clone(), e);
                    }
                    entry.child = Some(child);
                    let s = status.clone();
                    let name = item.name.clone();
                    std::thread::spawn(move || match wait {
                        Some(hp) if wait_answers(&hp, 60) => {
                            set(&s, "running", &format!("Answering at {hp}."))
                        }
                        Some(hp) => set(
                            &s,
                            "failed",
                            &format!(
                                "{name} did not answer at {hp} within a minute; its output may say why."
                            ),
                        ),
                        None => {
                            std::thread::sleep(Duration::from_secs(2));
                            set(&s, "running", "");
                        }
                    });
                }
            }
        }
        let old = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(item.id.clone(), entry);
        if let Some(mut old) = old {
            stop_child(&mut old);
        }
    }

    pub fn restart(&self, item: &Item) {
        self.stop(&item.id);
        self.start_one(item);
    }

    pub fn stop(&self, id: &str) {
        if let Some(mut e) = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id)
        {
            stop_child(&mut e);
        }
    }

    pub fn stop_all(&self) {
        let all: Vec<Entry> = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, e)| e)
            .collect();
        for mut e in all {
            stop_child(&mut e);
        }
    }

    pub fn statuses(&self) -> Vec<Status> {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for e in map.values_mut() {
            // A command that ended by itself.
            if let Some(child) = e.child.as_mut()
                && let Ok(Some(code)) = child.try_wait()
            {
                let mut s = e.status.lock().unwrap_or_else(|x| x.into_inner());
                if s.state != "stopped" && s.state != "failed" {
                    s.state = "stopped".into();
                    s.detail = format!("It ended ({code}).");
                }
            }
            let mut s = e.status.lock().unwrap_or_else(|x| x.into_inner()).clone();
            let o = e.output.lock().unwrap_or_else(|x| x.into_inner());
            s.output = o
                .iter()
                .skip(o.len().saturating_sub(60))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            out.push(s);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// Starts the command hidden, with its output kept. The program is found on the PATH; the rest are its arguments.
fn spawn(command: &str) -> Result<Child, String> {
    let words = crate::local_servers::words(command);
    let first = words.first().ok_or("The command is empty.")?;
    let program = if std::path::Path::new(first).is_file() {
        std::path::PathBuf::from(first)
    } else {
        crate::terminal::find_on_path(first)
            .ok_or_else(|| format!("{first} was not found on this computer."))?
    };
    let mut cmd = Command::new(program);
    cmd.args(&words[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map_err(|e| format!("{first} could not start: {e}"))
}

/// Stops what Qanuvi started, with the processes it started in turn (a proxy's workers).
fn stop_child(e: &mut Entry) {
    if let Some(mut child) = e.child.take() {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    set(&e.status, "stopped", "Stopped by Qanuvi.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_addresses_people_write() {
        assert_eq!(
            host_port("127.0.0.1:8787").as_deref(),
            Some("127.0.0.1:8787")
        );
        assert_eq!(
            host_port("http://localhost:8787/v1").as_deref(),
            Some("127.0.0.1:8787")
        );
        assert_eq!(
            host_port("https://example.com/api").as_deref(),
            Some("example.com:443")
        );
        assert_eq!(host_port(""), None);
        assert_eq!(host_port("nonsense:port"), None);
    }

    #[test]
    fn leaves_a_running_server_alone_and_reports_a_missing_program() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let s = Startup::default();
        let item = |id: &str, command: &str, wait: &str| Item {
            id: id.into(),
            name: id.into(),
            command: command.into(),
            wait_for: wait.into(),
            enabled: true,
        };
        s.start_all(&[
            item("a", "no-such-program-xyz --serve", &addr),
            item("b", "no-such-program-xyz --serve", ""),
        ]);
        let st = s.statuses();
        assert_eq!(st[0].state, "already running");
        assert_eq!(st[1].state, "failed");
        assert!(st[1].detail.contains("not found"));
        s.stop_all();
        assert!(s.statuses().is_empty());
    }
}
