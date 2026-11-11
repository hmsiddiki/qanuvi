//! Qanuvi's own brain for its agent (Milestone 24): which model answers a turn. The user lists the models they want
//! (local ones, free ones from OpenRouter or OmniRoute, any OpenAI-compatible API) in their preferred order; the brain
//! then sends light jobs (questions, explanations) to a model on this machine first, skips models that are rate
//! limited or down for a while, moves models that keep breaking tool calls down, and falls back to the next model when
//! one fails, so a turn finishes on whatever works.

use crate::llm::{Endpoint, FailKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A model the user offered the brain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// `endpoint/model`.
    pub id: String,
    #[serde(default)]
    pub local: bool,
    #[serde(default)]
    pub free: bool,
    /// Context size in tokens, 0 when unknown.
    #[serde(default)]
    pub context: u32,
}

/// Everything the agent needs to reach models: the providers (with keys, filled in by the app) and the models in the
/// user's order.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Setup {
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub models: Vec<Candidate>,
}

impl Setup {
    pub fn check(&self) -> Result<(), String> {
        for e in &self.endpoints {
            e.check()?;
        }
        if self.models.is_empty() {
            return Err(
                "Add a model first (Agents → Qanuvi agent): a local one through Ollama, or a free one through OpenRouter."
                    .into(),
            );
        }
        for m in &self.models {
            if self.endpoint_of(&m.id).is_none() {
                return Err(format!("{} belongs to no provider that is set up.", m.id));
            }
        }
        Ok(())
    }

    /// The provider a model id belongs to, and the model's own name there.
    pub fn endpoint_of(&self, id: &str) -> Option<(&Endpoint, String)> {
        let (ep, name) = id.split_once('/')?;
        self.endpoints
            .iter()
            .find(|e| e.id == ep)
            .map(|e| (e, name.to_string()))
    }

    pub fn candidate(&self, id: &str) -> Option<&Candidate> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// Fills in the providers' keys from where this machine keeps them (the credential store on a desktop, environment
/// variables on a server). Keys sent along with the setup are dropped: they never travel through the window.
pub fn with_keys(setup: &str, key_of: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut s: Setup =
        serde_json::from_str(setup).map_err(|e| format!("The model setup is not valid: {e}"))?;
    for e in &mut s.endpoints {
        e.key = key_of(&e.id).filter(|k| !k.trim().is_empty());
    }
    s.check()?;
    serde_json::to_string(&s).map_err(|e| e.to_string())
}

/// A key for a provider from the environment: `QANUVI_KEY_OPENROUTER` for the provider `openrouter`.
pub fn env_key(endpoint: &str) -> Option<String> {
    std::env::var(format!(
        "QANUVI_KEY_{}",
        endpoint.to_uppercase().replace('-', "_")
    ))
    .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// A question or an explanation: a small local model is usually enough.
    Light,
    /// Changing code: the best model the user has.
    Code,
}

/// What kind of job a message is, from its words.
pub fn job_of(text: &str) -> Job {
    let t = text.to_lowercase();
    let change = regex::Regex::new(
        r"\b(fix|add|implement|create|write|change|update|refactor|rename|remove|delete|build|make|move|replace|convert|generate|install|upgrade|migrate|edit|modify|improve|optimi[sz]e|test)\w*\b",
    )
    .map(|r| r.is_match(&t))
    .unwrap_or(true);
    if change {
        return Job::Code;
    }
    let question = t.trim_end().ends_with('?')
        || regex::Regex::new(
            r"^\s*(what|why|how|where|which|who|when|explain|summari[sz]e|describe|list|show|tell|is|are|does|do|can)\b",
        )
        .map(|r| r.is_match(&t))
        .unwrap_or(false);
    if question { Job::Light } else { Job::Code }
}

/// What the brain remembers about a model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRecord {
    pub turns: u32,
    pub finished: u32,
    /// Tool calls the model got wrong (unknown tool, unreadable arguments).
    pub broken_calls: u32,
    pub calls: u32,
    /// Not to be asked again before this time (seconds since 1970) after a rate limit or an outage.
    pub rest_until: i64,
    pub last_problem: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Records {
    #[serde(default)]
    pub models: HashMap<String, ModelRecord>,
    /// What this process changed since it last saved (counts to add, rests to set). Several agent sessions share
    /// brain.json, so a save adds these to what is on disk instead of writing this copy over the others' work.
    #[serde(skip)]
    pending: HashMap<String, ModelRecord>,
}

impl Records {
    fn file(data: &Path) -> PathBuf {
        data.join("brain.json")
    }

    pub fn load(data: &Path) -> Records {
        std::fs::read_to_string(Self::file(data))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Adds this process's changes to brain.json (under a short lock), and takes in what other sessions saved.
    pub fn save(&mut self, data: &Path) {
        let _ = std::fs::create_dir_all(data);
        let _lock = FileLock::take(&Self::file(data).with_extension("lock"));
        let mut disk = Self::load(data);
        for (id, d) in self.pending.drain() {
            let r = disk.models.entry(id).or_default();
            r.turns += d.turns;
            r.finished += d.finished;
            r.calls += d.calls;
            r.broken_calls += d.broken_calls;
            if d.rest_until > r.rest_until {
                r.rest_until = d.rest_until;
                r.last_problem = d.last_problem;
            }
        }
        if let Ok(s) = serde_json::to_string_pretty(&disk) {
            let tmp = Self::file(data).with_extension("tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, Self::file(data));
            }
        }
        self.models = disk.models;
    }

    /// A model failed: rest it for a while when it was busy or down.
    pub fn failed(&mut self, id: &str, kind: FailKind, reason: &str, now: i64) {
        let until = now
            + match kind {
                FailKind::Busy => 60,
                FailKind::Down => 120,
                FailKind::Unusable => 30 * 60,
            };
        let problem: String = reason.chars().take(200).collect();
        for r in [&mut self.models, &mut self.pending].map(|m| m.entry(id.to_string()).or_default())
        {
            r.rest_until = until;
            r.last_problem = problem.clone();
        }
    }

    pub fn turn(&mut self, id: &str, finished: bool, calls: u32, broken: u32) {
        for r in [&mut self.models, &mut self.pending].map(|m| m.entry(id.to_string()).or_default())
        {
            r.turns += 1;
            if finished {
                r.finished += 1;
            }
            r.calls += calls;
            r.broken_calls += broken;
        }
    }

    /// Models that break more than a third of their tool calls (after enough of them) are trusted less with code.
    fn unreliable(&self, id: &str) -> bool {
        self.models
            .get(id)
            .is_some_and(|r| r.calls >= 6 && r.broken_calls * 3 > r.calls)
    }

    fn resting(&self, id: &str, now: i64) -> bool {
        self.models.get(id).is_some_and(|r| r.rest_until > now)
    }
}

pub const AUTO: &str = "auto";

/// The models to try for a turn, best first. "auto": Qanuvi core decides, and the others follow as fallbacks (resting
/// ones last). A model the user chose is the only one: no rotation and no handover, whatever else is set (the owner,
/// 2026-11-11) — if it cannot answer, the user is told why.
pub fn order(setup: &Setup, chosen: &str, job: Job, records: &Records, now: i64) -> Vec<String> {
    if chosen != AUTO {
        return vec![chosen.to_string()];
    }
    let mut list: Vec<&Candidate> = setup.models.iter().collect();
    // Stable sorts: the user's order stays within each group.
    list.sort_by_key(|c| records.unreliable(&c.id) && job == Job::Code);
    if job == Job::Light {
        list.sort_by_key(|c| !c.local);
    }
    list.sort_by_key(|c| records.resting(&c.id, now));
    list.into_iter().map(|c| c.id.clone()).collect()
}

/// One answer from Qanuvi core's own models for a decision or a review (no tools): the models in `setup` are asked in
/// their order, one after another, until one answers. Answers the model that answered and its text.
pub fn decide(setup: &Setup, system: &str, prompt: &str) -> Result<(String, String), String> {
    let messages = [
        serde_json::json!({"role": "system", "content": system}),
        serde_json::json!({"role": "user", "content": prompt}),
    ];
    let mut problems = Vec::new();
    for c in &setup.models {
        let Some((ep, name)) = setup.endpoint_of(&c.id) else {
            continue;
        };
        match crate::llm::chat(
            ep,
            &name,
            &messages,
            &serde_json::json!([]),
            &[],
            &mut |_| {},
        ) {
            Ok(r) if !r.text.trim().is_empty() => return Ok((c.id.clone(), r.text)),
            Ok(_) => problems.push(format!("{}: an empty answer", c.id)),
            Err(f) => problems.push(format!("{}: {}", c.id, f.reason)),
        }
    }
    Err(if problems.is_empty() {
        "No model to decide with.".into()
    } else {
        format!("No model could decide. {}", problems.join(" · "))
    })
}

/// A lock file held while brain.json is read and written (at most about two seconds' wait; a lock left by a process
/// that died is taken over after ten seconds).
struct FileLock(PathBuf);

impl FileLock {
    fn take(path: &Path) -> Option<FileLock> {
        let start = std::time::Instant::now();
        loop {
            if std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .is_ok()
            {
                return Some(FileLock(path.to_path_buf()));
            }
            let stale = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > std::time::Duration::from_secs(10));
            if stale {
                let _ = std::fs::remove_file(path);
                continue;
            }
            if start.elapsed() > std::time::Duration::from_secs(2) {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Kind;

    #[test]
    fn a_chosen_model_is_the_only_one() {
        let s = setup();
        let r = Records::default();
        assert_eq!(
            order(
                &s,
                "openrouter/deepseek/deepseek-chat:free",
                Job::Code,
                &r,
                0
            ),
            ["openrouter/deepseek/deepseek-chat:free"]
        );
        assert!(
            order(&s, AUTO, Job::Code, &r, 0).len() > 1,
            "auto keeps its fallbacks"
        );
    }

    #[test]
    fn sessions_saving_together_add_up_instead_of_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Records::load(dir.path());
        let mut b = Records::load(dir.path());
        a.turn("m", true, 3, 0);
        b.turn("m", true, 2, 1);
        b.failed("n", FailKind::Busy, "rate limited", 100);
        a.save(dir.path());
        b.save(dir.path());
        let disk = Records::load(dir.path());
        assert_eq!(disk.models["m"].turns, 2);
        assert_eq!(disk.models["m"].calls, 5);
        assert_eq!(disk.models["m"].broken_calls, 1);
        assert_eq!(disk.models["n"].rest_until, 160);
        // A second save adds nothing twice; a and b now also see each other's work.
        a.save(dir.path());
        assert_eq!(Records::load(dir.path()).models["m"].turns, 2);
        assert_eq!(a.models["m"].turns, 2);
        assert!(!dir.path().join("brain.lock").exists());
        // Many threads at once.
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let p = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    let mut r = Records::load(&p);
                    r.turn("t", true, 1, 0);
                    r.save(&p);
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(Records::load(dir.path()).models["t"].turns, 8);
    }

    fn setup() -> Setup {
        let ep = |id: &str, url: &str| Endpoint {
            id: id.into(),
            kind: Kind::Openai,
            url: url.into(),
            key: None,
        };
        let c = |id: &str, local: bool| Candidate {
            id: id.into(),
            local,
            free: true,
            context: 0,
        };
        Setup {
            endpoints: vec![
                ep("openrouter", "https://openrouter.ai/api/v1"),
                ep("ollama", "http://127.0.0.1:11434"),
            ],
            models: vec![
                c("openrouter/qwen/qwen3-coder:free", false),
                c("ollama/qwen2.5-coder:3b", true),
                c("openrouter/deepseek/deepseek-chat:free", false),
            ],
        }
    }

    #[test]
    fn tells_questions_from_changes() {
        assert_eq!(job_of("What does the cart module do?"), Job::Light);
        assert_eq!(job_of("explain this function"), Job::Light);
        assert_eq!(job_of("Fix the failing test in cart.rs"), Job::Code);
        assert_eq!(job_of("How do I add a coupon field?"), Job::Code);
        assert_eq!(job_of("the price is wrong"), Job::Code);
    }

    #[test]
    fn sends_light_jobs_to_this_machine_first() {
        let s = setup();
        let r = Records::default();
        assert_eq!(
            order(&s, AUTO, Job::Light, &r, 0)[0],
            "ollama/qwen2.5-coder:3b"
        );
        assert_eq!(
            order(&s, AUTO, Job::Code, &r, 0)[0],
            "openrouter/qwen/qwen3-coder:free"
        );
    }

    #[test]
    fn rests_busy_models_and_distrusts_broken_ones() {
        let s = setup();
        let mut r = Records::default();
        r.failed(
            "openrouter/qwen/qwen3-coder:free",
            FailKind::Busy,
            "429",
            100,
        );
        let o = order(&s, AUTO, Job::Code, &r, 120);
        assert_eq!(o[0], "ollama/qwen2.5-coder:3b");
        assert_eq!(o[2], "openrouter/qwen/qwen3-coder:free");
        // After its rest it is first again.
        assert_eq!(
            order(&s, AUTO, Job::Code, &r, 200)[0],
            "openrouter/qwen/qwen3-coder:free"
        );

        let mut r = Records::default();
        r.turn("openrouter/qwen/qwen3-coder:free", true, 6, 4);
        assert_eq!(
            order(&s, AUTO, Job::Code, &r, 0).last().unwrap(),
            "openrouter/qwen/qwen3-coder:free"
        );
    }

    #[test]
    fn checks_the_setup() {
        let mut s = setup();
        assert!(s.check().is_ok());
        s.models.push(Candidate {
            id: "nowhere/x".into(),
            local: false,
            free: true,
            context: 0,
        });
        assert!(s.check().is_err());
        assert!(Setup::default().check().is_err());
        let (ep, name) = setup()
            .endpoint_of("openrouter/qwen/qwen3-coder:free")
            .map(|(e, n)| (e.id.clone(), n))
            .unwrap();
        assert_eq!(
            (ep.as_str(), name.as_str()),
            ("openrouter", "qwen/qwen3-coder:free")
        );
    }
}
