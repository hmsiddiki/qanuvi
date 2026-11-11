//! Model servers that run on this computer (Ollama, OmniRoute): whether one is installed when its address does not
//! answer, and starting it for the user.

use crate::llm::{Endpoint, Kind};
use serde::Serialize;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// What Qanuvi found about a provider's server on this computer.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalServer {
    /// "ollama" or "omniroute".
    pub program: String,
    pub installed: bool,
    /// Where it is installed (empty when not).
    pub path: String,
    /// The command Qanuvi runs to start it (shown to the user).
    pub start: String,
}

/// The program behind a provider, when Qanuvi knows how to find and start it.
pub fn program_for(ep: &Endpoint) -> Option<&'static str> {
    if !ep.is_local() {
        return None;
    }
    if ep.kind == Kind::Ollama {
        Some("ollama")
    } else if ep.id == "omniroute" || ep.url.contains(":20128") {
        Some("omniroute")
    } else {
        None
    }
}

/// The port in an address such as http://localhost:20128/v1.
pub fn port_of(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}

/// Places installers put these programs when they are not on PATH (for example a window started from the desktop
/// with an older PATH).
fn usual_places(program: &str) -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut list = Vec::new();
    if cfg!(windows) {
        if let Some(local) = env("LOCALAPPDATA") {
            list.push(local.join("Programs").join("Ollama").join("ollama.exe"));
        }
        if let Some(roaming) = env("APPDATA") {
            list.push(roaming.join("npm").join(format!("{program}.cmd")));
        }
    } else {
        for dir in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"] {
            list.push(PathBuf::from(dir).join(program));
        }
        if let Some(home) = env("HOME") {
            list.push(home.join(".npm-global").join("bin").join(program));
            list.push(home.join(".local").join("bin").join(program));
        }
    }
    list.into_iter()
        .filter(|p| p.file_stem().is_some_and(|s| s == program))
        .collect()
}

pub(crate) fn find(program: &str) -> Option<PathBuf> {
    crate::terminal::find_on_path(program)
        .or_else(|| usual_places(program).into_iter().find(|p| p.is_file()))
}

fn start_args(program: &str, ep: &Endpoint) -> Vec<String> {
    match program {
        "ollama" => vec!["serve".into()],
        _ => {
            // Not --daemon: that starts the server in a console window of its own. Qanuvi starts it hidden and does
            // not wait for it; it keeps running when Qanuvi closes.
            let mut a: Vec<String> = ["serve", "--no-open", "--no-tray"]
                .map(String::from)
                .to_vec();
            if let Some(port) = port_of(&ep.url) {
                a.extend(["--port".into(), port.to_string()]);
            }
            a
        }
    }
}

/// Splits a start command into words; "double quotes" keep spaces (paths such as "C:\My models\a.gguf").
pub fn words(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// The user's own start command for a server on this computer (llama.cpp, LM Studio, vLLM…), as words. Only for
/// local addresses; Ollama and OmniRoute use Qanuvi's own commands.
fn custom_command(ep: &Endpoint, custom: Option<&str>) -> Option<Vec<String>> {
    if !ep.is_local() || program_for(ep).is_some() {
        return None;
    }
    Some(words(custom?.trim())).filter(|w| !w.is_empty())
}

/// A program named in a start command: a path, or a name found on PATH.
fn resolve(program: &str) -> Option<PathBuf> {
    let p = PathBuf::from(program);
    if p.components().count() > 1 {
        return p.is_file().then_some(p);
    }
    crate::terminal::find_on_path(program)
}

/// Whether the provider's program is installed here: Ollama and OmniRoute, or a server with the user's start command.
pub fn status(ep: &Endpoint, custom: Option<&str>) -> Option<LocalServer> {
    if let Some(w) = custom_command(ep, custom) {
        let path = resolve(&w[0]);
        return Some(LocalServer {
            program: w[0].clone(),
            installed: path.is_some(),
            path: path
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            start: custom.unwrap_or("").trim().to_string(),
        });
    }
    let program = program_for(ep)?;
    let path = find(program);
    Some(LocalServer {
        program: program.into(),
        installed: path.is_some(),
        path: path
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        start: format!("{program} {}", start_args(program, ep).join(" ")),
    })
}

/// Starts the provider's server in the background and waits until its address answers (at most a minute).
pub fn start(ep: &Endpoint, custom: Option<&str>) -> Result<(), String> {
    let (program, path, args) = if let Some(w) = custom_command(ep, custom) {
        let path =
            resolve(&w[0]).ok_or_else(|| format!("{} was not found on this computer.", w[0]))?;
        (w[0].clone(), path, w[1..].to_vec())
    } else {
        let program = program_for(ep).ok_or("Qanuvi does not know how to start this provider.")?;
        let path =
            find(program).ok_or_else(|| format!("{program} is not installed on this computer."))?;
        (program.to_string(), path, start_args(program, ep))
    };
    // The server itself (models load later, each checked then): Ollama ~0.3 GB, OmniRoute ~0.6 GB, others ~1 GB.
    let need = match program.as_str() {
        "ollama" => 0.3,
        "omniroute" => 0.6,
        _ => 1.0,
    };
    crate::machine::check(&program, need)?;
    let mut cmd = Command::new(&path);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A hidden console: the script and the server it starts inherit it, so no window opens.
        cmd.creation_flags(0x0800_0000);
    }
    // The server keeps running when Qanuvi closes, like when the user starts it themselves.
    cmd.spawn()
        .map_err(|e| format!("{program} could not start: {e}"))?;
    let until = Instant::now() + Duration::from_secs(60);
    while Instant::now() < until {
        // Any answer counts, a refused key included (the window then asks for the key).
        let answered = match crate::llm::list_models(ep, false) {
            Ok(_) => true,
            Err(e) => !e.contains("could not be reached"),
        };
        if answered {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(700));
    }
    Err(format!(
        "{program} was started but {} did not answer within a minute. Its own window or log may say why.",
        ep.url
    ))
}

/// The key in OmniRoute's answer to "create an API key" (top level or under data), if there is one.
fn key_in(answer: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(answer.get(answer.find('{')?..)?).ok()?;
    [
        &v["key"],
        &v["data"]["key"],
        &v["apiKey"],
        &v["data"]["apiKey"],
    ]
    .into_iter()
    .find_map(|k| k.as_str().filter(|s| s.len() >= 8).map(str::to_string))
}

/// The models in OmniRoute's own list that it marks available (from providers that are connected and working), as
/// "provider/model". OmniRoute's OpenAI-style list also names models of providers that are not connected.
/// None when it cannot be asked (then nothing is left out).
pub fn omniroute_available(ep: &Endpoint) -> Option<std::collections::HashSet<String>> {
    let path = find("omniroute")?;
    let mut cmd = Command::new(&path);
    cmd.args(["api", "models", "get-api-models", "--output", "json"])
        .stdin(Stdio::null());
    if let Some(port) = port_of(&ep.url) {
        cmd.env("OMNIROUTE_BASE_URL", format!("http://localhost:{port}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    available_in(&String::from_utf8_lossy(&out.stdout))
}

fn available_in(answer: &str) -> Option<std::collections::HashSet<String>> {
    let start = answer.lines().position(|l| l.starts_with('{'))?;
    let json: String = answer.lines().skip(start).collect::<Vec<_>>().join("\n");
    let v: serde_json::Value = serde_json::from_str(&json).ok()?;
    let list = v["models"].as_array()?;
    Some(
        list.iter()
            .filter(|m| m["available"].as_bool() != Some(false))
            .filter_map(|m| m["fullModel"].as_str().map(str::to_string))
            .collect(),
    )
}

/// Asks the OmniRoute running on this computer for a new API key named "Qanuvi", through its own command line (it
/// signs in with this machine's local token). The key goes back to the caller only, to be kept in the credential
/// store; it is never shown.
pub fn omniroute_new_key(ep: &Endpoint) -> Result<String, String> {
    if program_for(ep) != Some("omniroute") {
        return Err("This is not an OmniRoute on this computer.".into());
    }
    let path = find("omniroute").ok_or("OmniRoute is not installed on this computer.")?;
    // The request body goes through a file: quotes in arguments do not survive the .cmd wrapper on Windows.
    let body =
        std::env::temp_dir().join(format!("qanuvi-omniroute-{}.json", crate::llm::short_id()));
    std::fs::write(&body, r#"{"name":"Qanuvi"}"#).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&path);
    cmd.args([
        "api",
        "api-keys",
        "post-api-keys",
        "--output",
        "json",
        "--body",
    ])
    .arg(format!("@{}", body.display()))
    .stdin(Stdio::null());
    if let Some(port) = port_of(&ep.url) {
        cmd.env("OMNIROUTE_BASE_URL", format!("http://localhost:{port}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output();
    let _ = std::fs::remove_file(&body);
    let out = out.map_err(|e| format!("OmniRoute could not be asked: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    key_in(&text).ok_or_else(|| {
        // Never echo the answer itself: it could hold a key. Its first line of errors is enough.
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err
            .lines()
            .map(str::trim)
            .find(|l| l.to_lowercase().contains("error") || l.contains("HTTP"))
            .unwrap_or("no key in its answer");
        format!("OmniRoute did not make a key ({why}). Make one in its dashboard under Endpoints instead.")
    })
}

/// What the setup wizard needs to know about this computer's tools.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub ollama: bool,
    pub node: bool,
    pub npm: bool,
    pub omniroute: bool,
    /// Windows' package manager (installs with Windows' own admin prompt).
    pub winget: bool,
    pub os: String,
}

pub fn tools() -> Tools {
    let on = |p: &str| find(p).is_some() || crate::terminal::find_on_path(p).is_some();
    Tools {
        ollama: on("ollama"),
        node: on("node"),
        npm: on("npm"),
        omniroute: on("omniroute"),
        winget: cfg!(windows) && crate::terminal::find_on_path("winget").is_some(),
        os: std::env::consts::OS.into(),
    }
}

/// Installs a tool the setup wizard needs, at the user's click. Windows: winget (Windows asks for permission itself);
/// OmniRoute: npm. Other systems get the official command to run themselves (Err starting with "command:").
pub fn install(tool: &str) -> Result<(), String> {
    let (program, args): (&str, Vec<&str>) = match (tool, cfg!(windows)) {
        ("ollama", true) => ("winget", vec!["install", "--id", "Ollama.Ollama", "-e"]),
        ("node", true) => ("winget", vec!["install", "--id", "OpenJS.NodeJS.LTS", "-e"]),
        ("ollama", false) => {
            return Err(if cfg!(target_os = "macos") {
                "command:brew install ollama".into()
            } else {
                "command:curl -fsSL https://ollama.com/install.sh | sh".into()
            });
        }
        ("node", false) => return Err("command:open https://nodejs.org".into()),
        ("omniroute", _) => ("npm", vec!["install", "-g", "omniroute"]),
        _ => return Err("Qanuvi does not install that.".into()),
    };
    // Room first: the installers themselves need some memory and the download needs disk.
    crate::machine::check(tool, 0.5)?;
    let path = crate::terminal::find_on_path(program)
        .ok_or_else(|| format!("{program} is not on this computer."))?;
    let mut cmd = Command::new(path);
    cmd.args(&args);
    if program == "winget" {
        // The user pressed Install after reading what it installs: winget's source and package terms are accepted.
        cmd.args([
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--silent",
        ]);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("{program} could not start: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("no reason given");
    Err(format!(
        "{tool} was not installed: {}",
        line.chars().take(200).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: &str, kind: Kind, url: &str) -> Endpoint {
        Endpoint {
            id: id.into(),
            kind,
            url: url.into(),
            key: None,
        }
    }

    #[test]
    fn knows_which_local_providers_it_can_start() {
        assert_eq!(
            program_for(&ep("ollama", Kind::Ollama, "http://127.0.0.1:11434")),
            Some("ollama")
        );
        assert_eq!(
            program_for(&ep("omniroute", Kind::Openai, "http://localhost:20128/v1")),
            Some("omniroute")
        );
        assert_eq!(
            program_for(&ep("router", Kind::Openai, "http://localhost:20128/v1")),
            Some("omniroute")
        );
        assert_eq!(
            program_for(&ep("llamacpp", Kind::Openai, "http://127.0.0.1:8080/v1")),
            None
        );
        assert_eq!(
            program_for(&ep(
                "openrouter",
                Kind::Openrouter,
                "https://openrouter.ai/api/v1"
            )),
            None
        );
        assert_eq!(port_of("http://localhost:20130/v1"), Some(20130));
        assert_eq!(port_of("https://openrouter.ai/api/v1"), None);
        let omni = ep("omniroute", Kind::Openai, "http://localhost:20130/v1");
        assert_eq!(
            start_args("omniroute", &omni),
            ["serve", "--no-open", "--no-tray", "--port", "20130"]
        );
    }

    #[test]
    fn keeps_only_what_omniroute_marks_available() {
        let answer = "Loaded env
{
 \"models\": [{\"fullModel\": \"antigravity/claude-sonnet-4-6\", \"available\": true}, {\"fullModel\": \"antigravity/gemini-4-argon-high\", \"available\": false}]
}";
        let set = available_in(answer).unwrap();
        assert!(set.contains("antigravity/claude-sonnet-4-6"));
        assert!(!set.contains("antigravity/gemini-4-argon-high"));
        assert!(available_in("no json").is_none());
    }

    #[test]
    fn runs_the_users_start_command_for_other_local_servers_only() {
        assert_eq!(
            words(r#"llama-server -m "C:\My models\q.gguf" --port 8080"#),
            [
                "llama-server",
                "-m",
                r"C:\My models\q.gguf",
                "--port",
                "8080"
            ]
        );
        let llama = ep("llamacpp", Kind::Openai, "http://127.0.0.1:8080/v1");
        let cmd = Some("llama-server -m q.gguf");
        assert_eq!(custom_command(&llama, cmd).unwrap()[0], "llama-server");
        assert!(custom_command(&llama, Some("  ")).is_none());
        // Not for servers elsewhere, nor for the ones Qanuvi starts itself.
        assert!(custom_command(&ep("x", Kind::Openai, "https://example.com/v1"), cmd).is_none());
        assert!(
            custom_command(&ep("ollama", Kind::Ollama, "http://127.0.0.1:11434"), cmd).is_none()
        );
        let s = status(&llama, Some("surely-not-a-program-xyz --port 8080")).unwrap();
        assert!(!s.installed);
        assert!(status(&llama, None).is_none());
    }

    #[test]
    fn reads_the_new_key_from_omniroutes_answer() {
        let key = |s: &str| key_in(s);
        assert_eq!(
            key("noise\n{\"id\":\"1\",\"key\":\"sk-abcdef123\"}").as_deref(),
            Some("sk-abcdef123")
        );
        assert_eq!(
            key("{\"data\":{\"key\":\"sk-abcdef123\"}}").as_deref(),
            Some("sk-abcdef123")
        );
        assert_eq!(key("{\"error\":\"no\"}"), None);
        assert_eq!(key("no json at all"), None);
    }
}
