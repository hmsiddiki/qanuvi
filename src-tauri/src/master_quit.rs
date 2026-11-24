// t1: the tray's "Quit Qanuvi" question, asked in Qanuvi's own dialog instead of a system box (the master has no
// window). With a window open, the app shows it there (brought forward); with none, the app starts a small window of
// its own for it (`--quit-question <id>`) that answers and ends. The answer comes back as `app_quit_answer`. No answer
// (the window closed, the question window ended) keeps everything running. While one question is up, Quit again only
// brings it forward.

use super::{Master, b};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// What the Quit question can answer. s10 (owner, 2026-11-25): with plan runs going, "Go on next time" quits too and
/// the runs go on by themselves at the next start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuitChoice {
    /// "Keep running": nothing stops.
    Keep,
    /// "Quit": everything stops; a run waits for Resume.
    Quit,
    /// "Go on next time": everything stops; the runs start again by themselves when Qanuvi starts next.
    Later,
}

/// What the app.s answer says: Some(choice) from the dialog, None when the app has no window to ask in.
type Answer = Option<QuitChoice>;

/// The question up now, if any.
#[derive(Default)]
pub struct QuitQuestion {
    up: Mutex<Option<Up>>,
    next: AtomicU64,
}

struct Up {
    id: u64,
    text: String,
    /// The question offers "Go on next time" (plan runs are going).
    later: bool,
    tx: mpsc::Sender<Answer>,
}

impl QuitQuestion {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Up>> {
        self.up.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Master {
    /// Asks "Quit anyway?" in Qanuvi's own dialog and waits for the answer: true quits. A question already up is
    /// brought forward instead, and this Quit says Keep (the first one goes on with the answer). `later` offers "Go on
    /// next time" beside Quit.
    pub(super) fn ask_quit(&self, text: &str, later: bool) -> QuitChoice {
        let rx = {
            let mut up = self.quit_question.lock();
            if let Some(u) = up.as_ref() {
                let _ = self.app_links.reach(
                    &json!({"do": "ask-quit", "id": u.id, "text": u.text, "later": u.later}),
                );
                return QuitChoice::Keep;
            }
            let id = self.quit_question.next.fetch_add(1, Ordering::SeqCst) + 1;
            let (tx, rx) = mpsc::channel();
            *up = Some(Up {
                id,
                text: text.to_string(),
                later,
                tx,
            });
            rx
        };
        let choice = self.quit_answer_wait(text, later, &rx);
        *self.quit_question.lock() = None;
        choice
    }

    fn quit_answer_wait(&self, text: &str, later: bool, rx: &mpsc::Receiver<Answer>) -> QuitChoice {
        let id = self.quit_question.lock().as_ref().map_or(0, |u| u.id);
        if self
            .app_links
            .reach(&json!({"do": "ask-quit", "id": id, "text": text, "later": later}))
            > 0
        {
            loop {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Some(choice)) => return choice,
                    // The app has no window to ask in: the question gets its own below.
                    Ok(None) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        // The app went (its windows closed) with no answer: everything keeps running.
                        if self.app_links.reach(&json!({"do": "wait"})) == 0 {
                            return QuitChoice::Keep;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return QuitChoice::Keep,
                }
            }
        }
        let Some(mut window) = start_question_window(id) else {
            // The system box asks yes or no only: yes quits and the runs wait for Resume.
            return if super::confirm_quit(text) {
                QuitChoice::Quit
            } else {
                QuitChoice::Keep
            };
        };
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(answer) => {
                    // The question window ends by itself once it has answered.
                    let _ = window.wait();
                    return answer.unwrap_or(QuitChoice::Keep);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !matches!(window.try_wait(), Ok(None)) {
                        // Closed without an answer (or the answer came just as it ended).
                        return rx.try_recv().ok().flatten().unwrap_or(QuitChoice::Keep);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return QuitChoice::Keep,
            }
        }
    }

    /// `app_quit_question`: the question up now ({id, text, later}), or null; the question window reads it.
    pub(super) fn quit_question_now(&self) -> Value {
        match self.quit_question.lock().as_ref() {
            Some(u) => json!({"id": u.id, "text": u.text, "later": u.later}),
            None => Value::Null,
        }
    }

    /// `app_quit_answer` {id, yes, later, noWindow}: the app's answer to the question up now. One for another question
    /// (an old one) is ignored.
    pub(super) fn quit_question_answer(&self, args: &Value) -> Value {
        let up = self.quit_question.lock();
        let Some(u) = up.as_ref().filter(|u| Some(u.id) == args["id"].as_u64()) else {
            return json!(false);
        };
        let answer = if b(args, "noWindow") {
            None
        } else if b(args, "later") && u.later {
            Some(QuitChoice::Later)
        } else if b(args, "yes") {
            Some(QuitChoice::Quit)
        } else {
            Some(QuitChoice::Keep)
        };
        json!(u.tx.send(answer).is_ok())
    }
}

/// The app's flag for the small question window (t1).
pub const QUESTION_FLAG: &str = "--quit-question";

/// Starts the app as the question window alone; None when it cannot start (the system box asks then).
fn start_question_window(id: u64) -> Option<std::process::Child> {
    let exe = crate::sign_in_start::app_exe()
        .map_err(|e| eprintln!("{e}"))
        .ok()?;
    std::process::Command::new(exe)
        .args([QUESTION_FLAG, &id.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| eprintln!("Qanuvi's Quit question could not open: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::Sink;
    use std::sync::Arc;

    /// A window's link: what the master sends it comes out of the receiver.
    fn linked(m: &Master) -> mpsc::Receiver<Value> {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        m.app_links.add(Sink::new(move |v: Value| {
            tx.lock().unwrap().send(v).is_ok()
        }));
        rx
    }

    fn asked(rx: &mpsc::Receiver<Value>) -> Value {
        loop {
            let v = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("no question came");
            if v["do"] == "ask-quit" {
                return v;
            }
        }
    }

    #[test]
    fn the_windows_answer_decides_and_quit_again_only_brings_it_forward() {
        let data = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        let rx = linked(&m);
        let asker = {
            let m = m.clone();
            std::thread::spawn(move || m.ask_quit("Quitting Qanuvi stops 1 plan run.", false))
        };
        let q = asked(&rx);
        assert_eq!(q["text"], "Quitting Qanuvi stops 1 plan run.");
        assert_eq!(m.quit_question_now()["id"], q["id"]);
        // Quit again while it is up: the same question once more, and this Quit says no.
        assert_eq!(m.ask_quit("other", false), QuitChoice::Keep);
        assert_eq!(asked(&rx)["id"], q["id"]);
        // An answer to another question is ignored.
        let other = q["id"].as_u64().unwrap() + 7;
        assert_eq!(
            m.quit_question_answer(&json!({"id": other, "yes": true})),
            json!(false)
        );
        assert_eq!(
            m.quit_question_answer(&json!({"id": q["id"], "yes": true})),
            json!(true)
        );
        assert_eq!(asker.join().unwrap(), QuitChoice::Quit);
        assert_eq!(m.quit_question_now(), Value::Null);
        // The next one asks anew, and "Keep running" keeps it all.
        let asker = {
            let m = m.clone();
            std::thread::spawn(move || m.ask_quit("again", false))
        };
        let q2 = asked(&rx);
        assert_ne!(q2["id"], q["id"]);
        m.quit_question_answer(&json!({"id": q2["id"], "yes": false}));
        assert_eq!(asker.join().unwrap(), QuitChoice::Keep);
    }

    #[test]
    fn a_window_that_goes_without_answering_keeps_everything_running() {
        let data = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        let rx = linked(&m);
        let asker = {
            let m = m.clone();
            std::thread::spawn(move || m.ask_quit("text", false))
        };
        asked(&rx);
        drop(rx);
        assert_eq!(asker.join().unwrap(), QuitChoice::Keep);
    }

    #[test]
    fn go_on_next_time_is_an_answer_only_when_the_question_offers_it() {
        let data = tempfile::tempdir().unwrap();
        let m = Arc::new(Master::open(data.path()).unwrap());
        let rx = linked(&m);
        let asker = {
            let m = m.clone();
            std::thread::spawn(move || m.ask_quit("Quitting Qanuvi stops 1 plan run.", true))
        };
        let q = asked(&rx);
        assert_eq!(q["later"], true);
        assert_eq!(m.quit_question_now()["later"], true);
        m.quit_question_answer(&json!({"id": q["id"], "yes": true, "later": true}));
        assert_eq!(asker.join().unwrap(), QuitChoice::Later);
        // Not offered (only agent sessions work): a "later" answer is a plain Quit.
        let asker = {
            let m = m.clone();
            std::thread::spawn(move || m.ask_quit("Quitting Qanuvi stops 1 agent session.", false))
        };
        let q = asked(&rx);
        assert_eq!(q["later"], false);
        m.quit_question_answer(&json!({"id": q["id"], "yes": true, "later": true}));
        assert_eq!(asker.join().unwrap(), QuitChoice::Quit);
    }
}
