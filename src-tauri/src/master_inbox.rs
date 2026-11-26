// The master's Inbox (24z R2d part 3): a run on the master asks its questions in the master's own Inbox, so the
// desktop reads and answers that list with the same inbox_* commands its own window uses. A connection that wants
// the changes calls `inbox_watch` with a stream and gets `{"change": …, "item": …}` on it, the same payload as the
// desktop's "qanuvi-inbox" event, until it goes away. The first answer anywhere still closes a question everywhere.

use super::{Master, Streams, opt, s, to};
use crate::inbox;
use crate::sink::Sink;
use serde_json::{Value, json};
use std::sync::Mutex;

/// The connections that follow the master's Inbox; one that no longer listens is dropped.
#[derive(Default)]
pub struct InboxWatchers(Mutex<Vec<Sink<Value>>>);

impl InboxWatchers {
    pub fn add(&self, sink: Sink<Value>) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(sink);
    }

    pub fn send(&self, v: &Value) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|sink| sink.send(v.clone()).is_ok());
    }

    /// Sends to every follower and says how many still listen.
    pub fn reach(&self, v: &Value) -> usize {
        let mut all = self.0.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|sink| sink.send(v.clone()).is_ok());
        all.len()
    }
}

impl Master {
    fn inbox_changed(&self, change: &str, item: Option<&inbox::InboxItem>) {
        self.inbox_watchers
            .send(&json!({"change": change, "item": item}));
    }

    /// The `inbox_*` commands.
    pub(super) fn inbox(&self, cmd: &str, args: &Value, stream: Streams) -> Result<Value, String> {
        let db = &self.db;
        match cmd {
            "inbox_watch" => {
                if let Some(name) = opt(args, "onEvent").filter(|n| !n.is_empty()) {
                    self.inbox_watchers.add(stream(&name));
                }
                to(inbox::list(db))
            }
            "inbox_list" => to(inbox::list(db)),
            "inbox_add" => {
                // A note the window writes about this machine's own work (unsaved text kept here whose file no
                // longer opens), so the Inbox lists it under this machine, as if the master had written it.
                let item: inbox::NewItem = serde_json::from_value(args["item"].clone())
                    .map_err(|e| format!("Not an Inbox item: {e}"))?;
                let (added, new) = inbox::add_new(db, &item)?;
                self.inbox_watchers
                    .send(&json!({"change": "add", "item": added, "new": new}));
                to(Ok(added))
            }
            "inbox_answer" => {
                // Who answered: the desktop says which of its windows (or later "phone").
                let by = opt(args, "by")
                    .filter(|b| !b.is_empty())
                    .unwrap_or_else(|| "desktop".into());
                let answer = args["answer"].clone();
                let done = match (opt(args, "id"), opt(args, "key")) {
                    (Some(id), _) => inbox::answer(db, &id, &answer, &by)?,
                    (None, Some(key)) => inbox::answer_key(db, &key, &answer, &by)?,
                    _ => return Err("Which question?".into()),
                };
                if let Some(item) = &done {
                    self.inbox_changed("answer", Some(item));
                    self.answered(item);
                }
                to(Ok(done))
            }
            "inbox_close" => {
                let id = s(args, "id");
                let closed = inbox::close(db, &id)?;
                if closed {
                    self.inbox_changed("close", inbox::get(db, &id)?.as_ref());
                }
                to(Ok(closed))
            }
            "inbox_resolve" => {
                let gone = inbox::resolve(db, &s(args, "key"))?;
                if gone {
                    self.inbox_changed("remove", None);
                }
                to(Ok(gone))
            }
            "inbox_mark_read" => {
                // No id: every item read but open questions (as on the desktop).
                let changed = inbox::mark_read(db, opt(args, "id").as_deref())?;
                if changed {
                    self.inbox_changed("read", None);
                }
                to(Ok(changed))
            }
            "inbox_remove" => {
                let gone = inbox::remove(db, &s(args, "id"))?;
                if gone {
                    self.inbox_changed("remove", None);
                }
                to(Ok(gone))
            }
            "inbox_clear" => {
                let gone = inbox::clear(db)?;
                if gone {
                    self.inbox_changed("remove", None);
                }
                to(Ok(gone))
            }
            // A window's old list (kept in its own storage before 24u part 4), moved in once.
            "inbox_import" => {
                let items: Vec<inbox::OldItem> =
                    serde_json::from_value(args["items"].clone()).map_err(|e| e.to_string())?;
                let n = inbox::import(db, &items)?;
                if n > 0 {
                    self.inbox_changed("add", None);
                }
                to(Ok(n))
            }
            _ => Err(format!("This master does not know the command {cmd}.")),
        }
    }

    /// An answered question that changes how its project works (where reviews get changes from) is applied here, as
    /// on the desktop. A held step's answer needs nothing more: the run reads it from the Inbox.
    fn answered(&self, item: &inbox::InboxItem) {
        if self.home_install_answered(item) {
            return;
        }
        let Some(pid) = item
            .source
            .as_ref()
            .and_then(|s| s.get("projectId"))
            .and_then(Value::as_str)
        else {
            return;
        };
        if let Ok(env) = self.run_env(pid) {
            crate::plan::run::on_answer(&env, item);
        }
    }
}
