// A master's own server list and its connections to them (migration M3b). Every machine keeps the servers it connects
// to in its own database (servers.rs) and their pairing keys in its own store (pairing_keys.rs); a master answers the
// same servers_* and remote_* commands as the desktop app, so a window shows and changes the list of whichever machine
// it shows. The master connects to its servers itself: the ones marked to connect at start-up when it starts, and
// again every minute while they are away. A connection that wants to know when one of them comes or goes calls
// `remote_watch` with a stream and gets `{"closed": id, "error": …}` or `{"connected": id, "info": …}` on it.

use super::{Master, Streams, b, opt, s, to};
use crate::remote::{OnEnd, Transport};
use serde_json::{Value, json};
use std::sync::Arc;

/// How long a master waits before it tries its away servers again.
pub const RECONNECT_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// Why "local" (a desktop's test master next to the app) and "self" (the desktop itself) are not servers of a master.
fn app_only(target: &str) -> String {
    format!(
        "A master reaches other machines over SSH or through the relay; \"{target}\" works only in the app."
    )
}

/// Commands whose answer is the id of something that ends with the connection that opened it.
const OPENS: [&str; 3] = ["term_spawn", "pipe_start", "port_open"];

/// How a terminal, pipe or port opened on another machine through this one is noted with the asking connection's
/// own (M5c part 2): the machine's path and its id there.
fn forwarded_entry(key: &str, id: &str) -> String {
    format!("forwarded\t{key}\t{id}")
}

/// The machine's path and the id there of an entry noted by `note_forwarded`; none for this machine's own.
pub(super) fn forwarded(entry: &str) -> Option<(&str, &str)> {
    let rest = entry.strip_prefix("forwarded\t")?;
    rest.split_once('\t')
}

/// A server the window reaches itself, not this master: "local" or "self" (kept in This PC's list, M5c part 2).
fn app_held(server: &Value) -> bool {
    matches!(
        server["target"].as_str().unwrap_or("").trim(),
        "local" | "self"
    )
}

/// What a server entry or a remote_connect call says about how to reach it.
fn transport_for(server: &Value) -> Result<Transport, String> {
    let target = server["target"].as_str().unwrap_or("").trim();
    match target {
        "relay" => {
            let r = &server["relay"];
            let pairing = r["pairing"].as_str().unwrap_or("");
            let key = crate::pairing_keys::get(pairing)?;
            Ok(Transport::Relay(crate::relay_client::Paired {
                relay: r["url"].as_str().unwrap_or("").to_string(),
                master_id: r["masterId"].as_str().unwrap_or("").to_string(),
                master_name: String::new(),
                pairing: pairing.to_string(),
                key,
            }))
        }
        // "local" is a desktop's test master next to the app; "self" is the desktop itself. Neither is a machine a
        // master can reach onward.
        "local" | "self" => Err(app_only(target)),
        t => {
            let program = server["program"].as_str().unwrap_or("").trim();
            let identity = server["identity"].as_str();
            Ok(Transport::Process(crate::remote::command_for(
                t,
                if program.is_empty() {
                    "qanuvi-master"
                } else {
                    program
                },
                identity,
                // Never used: "local" is refused above.
                &crate::paths::data_dir().join("local-master"),
            )?))
        }
    }
}

impl Master {
    /// Tells the connections following this master's servers.
    fn servers_changed(&self, v: Value) {
        self.remote_watchers.send(&v);
    }

    /// Each server in the list with how it stands now: on (with its hello), error (with why) or off.
    fn remote_states(&self) -> Result<Value, String> {
        let infos: std::collections::HashMap<String, Value> =
            self.remotes.infos().into_iter().collect();
        let errors = self.remote_errors.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = serde_json::Map::new();
        for server in crate::servers::list(&self.db)? {
            let id = server["id"].as_str().unwrap_or("").to_string();
            let state = match (infos.get(&id), errors.get(&id)) {
                // Its hello is not in yet (finding n3): it is still connecting, not on with nothing known of it.
                (Some(info), _) if info.is_null() => json!({"state": "connecting"}),
                (Some(info), _) => {
                    json!({"state": "on", "info": info, "direct": self.remotes.is_direct(&id)})
                }
                (None, Some(e)) => json!({"state": "error", "error": e}),
                (None, None) => json!({"state": "off"}),
            };
            out.insert(id, state);
        }
        Ok(Value::Object(out))
    }

    /// Holds a server off the minute's retry (disconnected on purpose), or lets it be tried again.
    fn hold_off(&self, id: &str, on: bool) {
        let mut held = self.held_off.lock().unwrap_or_else(|e| e.into_inner());
        if on {
            held.insert(id.to_string());
        } else {
            held.remove(id);
        }
    }

    fn is_held_off(&self, id: &str) -> bool {
        let held = self.held_off.lock().unwrap_or_else(|e| e.into_inner());
        held.contains(id)
    }

    fn note_error(&self, id: &str, error: Option<&str>) {
        let mut errors = self.remote_errors.lock().unwrap_or_else(|e| e.into_inner());
        match error {
            Some(e) => {
                errors.insert(id.to_string(), e.to_string());
            }
            None => {
                errors.remove(id);
            }
        }
    }

    /// One of its connections ended by itself: it is let go, and the followers are told why.
    fn remote_ended(&self, id: &str) {
        self.remotes.disconnect(id);
        let said = "The connection ended. Work there goes on; it is tried again in a minute.";
        self.note_error(id, Some(said));
        self.servers_changed(json!({"closed": id, "error": said}));
    }

    /// Connects to one of its servers (or answers the connection it has) and answers that machine's hello.
    pub(super) fn connect_server(&self, server: &Value) -> Result<Value, String> {
        let id = server["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            return Err("A server needs an id.".into());
        }
        // S3: the window hears when a connection moves to a direct link at home and back to the relay.
        let watcher = self.me.get().cloned().unwrap_or_default();
        self.remotes
            .set_on_road(Arc::new(move |id: &str, direct: bool| {
                if let Some(m) = watcher.upgrade() {
                    m.servers_changed(json!({"road": id, "direct": direct}));
                }
            }));
        let result = transport_for(server).and_then(|transport| {
            let me = self.me.get().cloned().unwrap_or_default();
            let on_end: OnEnd = Arc::new(move |id: &str| {
                if let Some(m) = me.upgrade() {
                    m.remote_ended(id);
                }
            });
            self.remotes.connect(on_end, &id, transport)
        });
        let result = result.and_then(|info| {
            // This machine itself, reached by another address: refused (a loop).
            if info["machineId"].as_str() == Some(self.machine_id.as_str()) {
                self.remotes.disconnect(&id);
                Err("That is this machine itself, so it is not connected to.".to_string())
            } else {
                Ok(info)
            }
        });
        match &result {
            Ok(info) => {
                self.note_error(&id, None);
                self.servers_changed(json!({"connected": id, "info": info}));
            }
            Err(e) => {
                self.note_error(&id, Some(e));
                self.servers_changed(json!({"closed": id, "error": e}));
            }
        }
        result
    }

    /// Connects the servers marked to connect at start-up that are not connected now (at start, then every minute).
    pub(super) fn connect_away_servers(&self) {
        let Ok(list) = crate::servers::list(&self.db) else {
            return;
        };
        let on = self.remotes.connected();
        for server in list {
            let id = server["id"].as_str().unwrap_or("");
            // This PC's test master and remote access stay the window's (M5c part 2): never tried here.
            if self.app_data && app_held(&server) {
                continue;
            }
            if server["autoConnect"] == true && !on.iter().any(|c| c == id) && !self.is_held_off(id)
            {
                let _ = self.connect_server(&server);
            }
        }
    }

    /// Where a `remote_call` goes (migration M3c): to `master`, one of this machine's servers, and from there along
    /// `via`. `seen` holds the lasting ids of the machines the call came through (the window's first); a call that
    /// already passed this machine is a loop, and one whose path would hold more than three machines below the
    /// window is refused, both before anything is sent on.
    fn route_for(&self, args: &Value) -> Result<crate::remote::Routed, String> {
        let list = |v: &Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut seen = list(&args["seen"]);
        if seen.iter().any(|id| id == &self.machine_id) {
            return Err(crate::remote::a_loop());
        }
        seen.push(self.machine_id.clone());
        let via = list(&args["via"]);
        // Below the window: the masters passed (this one included), the next one, and the rest of the path.
        if seen.len() + via.len() > crate::remote::MAX_DEPTH {
            return Err(crate::remote::too_deep());
        }
        let first = s(args, "master");
        if first.contains('/') || via.iter().any(|v| v.contains('/')) {
            return Err("A machine's path holds server ids, not paths.".into());
        }
        let key = std::iter::once(first)
            .chain(via)
            .collect::<Vec<_>>()
            .join("/");
        self.remotes.route(&key, seen)
    }

    /// A call passed on that opened a terminal, pipe or port there (M5c part 2): it belongs to the connection here
    /// that asked, not to this machine's own lasting connection there, so it is noted with that one and closed when
    /// it ends (a window's terminals and language servers on This PC's servers end when the window goes).
    pub(super) fn note_forwarded(
        &self,
        args: &Value,
        out: &Value,
        opened: &std::sync::Mutex<std::collections::HashSet<String>>,
    ) {
        let (Some(id), true) = (out.as_str(), OPENS.contains(&s(args, "cmd").as_str())) else {
            return;
        };
        let via: Vec<String> = args["via"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let key = std::iter::once(s(args, "master"))
            .chain(via)
            .collect::<Vec<_>>()
            .join("/");
        opened
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(forwarded_entry(&key, id));
    }

    /// Closes a terminal, pipe or port opened along `key` for a connection that ended (one already gone is no harm).
    pub(super) fn close_forwarded(&self, key: &str, id: &str) {
        let Ok(route) = self.remotes.route(key, vec![self.machine_id.clone()]) else {
            return;
        };
        let cmd = if crate::pipes::is_pipe(id) {
            "pipe_stop"
        } else {
            "term_kill"
        };
        crate::pipes::Link::tell(&route, cmd, json!({"id": id}));
    }

    /// The `servers_*` and `remote_*` commands.
    pub(super) fn servers(
        &self,
        cmd: &str,
        args: &Value,
        stream: Streams,
    ) -> Result<Value, String> {
        let db = &self.db;
        match cmd {
            "servers_list" => Ok(json!({
                "imported": crate::servers::imported(db)?,
                "servers": crate::servers::list(db)?,
            })),
            "servers_import" => {
                let old = args["servers"].as_array().cloned().unwrap_or_default();
                to(crate::servers::import(db, &old))
            }
            "servers_save" => {
                // Refused before it is kept (finding s1): kept, it would be tried again every minute. This PC's own
                // list keeps them for the window, which reaches them itself (M5c part 2); they are never tried here.
                let target = args["server"]["target"].as_str().unwrap_or("").trim();
                if !self.app_data && app_held(&args["server"]) {
                    return Err(app_only(target));
                }
                to(crate::servers::save(db, &args["server"]))
            }
            "servers_remove" => {
                // Unpaired at once: its connection closes now, and whoever follows this master is told.
                let id = s(args, "id");
                let gone = crate::servers::remove(db, &id)?;
                self.hold_off(&id, false);
                self.remotes.disconnect(&id);
                self.note_error(&id, None);
                self.servers_changed(json!({"closed": id, "removed": true}));
                to(Ok(gone))
            }
            "remote_connect" => {
                // Connected on purpose: a disconnect on purpose before it no longer holds it off.
                self.hold_off(&s(args, "id"), false);
                self.connect_server(args)
            }
            "remote_call" => {
                let route = self.route_for(args)?;
                let events = if b(args, "streams") {
                    opt(args, "onEvent")
                        .filter(|n| !n.is_empty())
                        .map(|n| stream(&n))
                } else {
                    None
                };
                crate::pipes::Link::call(&route, &s(args, "cmd"), args["args"].clone(), events)
            }
            "remote_list" => to(Ok(self.remotes.connected())),
            "remote_states" => self.remote_states(),
            "remote_watch" => {
                if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
                    self.remote_watchers.add(stream(&name));
                }
                self.remote_states()
            }
            "remote_disconnect" => {
                let id = s(args, "id");
                // On purpose (the window's Disconnect, finding z2): it stays off until connected again. A restart's
                // or a twin's disconnect leaves it to the minute's retry.
                if b(args, "held") {
                    self.hold_off(&id, true);
                }
                self.remotes.disconnect(&id);
                self.note_error(&id, None);
                self.servers_changed(json!({"closed": id}));
                Ok(Value::Null)
            }
            "remote_pair" => {
                let name = opt(args, "name")
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(super::hostname);
                let p = crate::relay_client::pair(&s(args, "relay"), &s(args, "code"), &name)?;
                crate::pairing_keys::set(&p.pairing, &p.key)?;
                Ok(
                    json!({"relay": p.relay, "masterId": p.master_id, "masterName": p.master_name, "pairing": p.pairing}),
                )
            }
            "remote_unpair" => to(crate::pairing_keys::delete(&s(args, "pairing"))),
            _ => Err(format!("This master does not know the command {cmd}.")),
        }
    }
}
