//! An agent's own configuration files (for Claude Code: settings, instructions and MCP servers), opened in Qanuvi's
//! editor. Only the files listed here can be read or written, so this cannot be used to reach other files.

use crate::files::{FileContent, fingerprint, write_path};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigFile {
    pub id: &'static str,
    pub label: &'static str,
    /// "user" (applies everywhere) or "project".
    pub scope: &'static str,
    /// The path as the user would write it, for example `~/.claude/settings.json`.
    pub display: String,
    pub exists: bool,
}

struct Spec {
    id: &'static str,
    label: &'static str,
    scope: &'static str,
    /// Path parts below the home folder (user scope) or the project folder (project scope).
    parts: &'static [&'static str],
}

const CLAUDE_CODE: &[Spec] = &[
    Spec {
        id: "user-settings",
        label: "Settings",
        scope: "user",
        parts: &[".claude", "settings.json"],
    },
    Spec {
        id: "user-instructions",
        label: "Instructions (CLAUDE.md)",
        scope: "user",
        parts: &[".claude", "CLAUDE.md"],
    },
    Spec {
        id: "project-settings",
        label: "Project settings",
        scope: "project",
        parts: &[".claude", "settings.json"],
    },
    Spec {
        id: "project-local-settings",
        label: "Project settings, this machine only",
        scope: "project",
        parts: &[".claude", "settings.local.json"],
    },
    Spec {
        id: "project-instructions",
        label: "Project instructions (CLAUDE.md)",
        scope: "project",
        parts: &["CLAUDE.md"],
    },
    Spec {
        id: "project-mcp",
        label: "Project MCP servers (.mcp.json)",
        scope: "project",
        parts: &[".mcp.json"],
    },
];

/// Qanuvi's own agent reads the project's AGENTS.md (an open convention many agents share).
const QANUVI: &[Spec] = &[Spec {
    id: "project-instructions",
    label: "Project instructions (AGENTS.md)",
    scope: "project",
    parts: &["AGENTS.md"],
}];

fn specs(provider: &str) -> Result<&'static [Spec], String> {
    match provider {
        "claude-code" => Ok(CLAUDE_CODE),
        "qanuvi" => Ok(QANUVI),
        _ => Err(format!("Qanuvi does not know the agent {provider}.")),
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn locate(spec: &Spec, home: &Path, project: Option<&Path>) -> Result<(PathBuf, String), String> {
    let (base, prefix) = match spec.scope {
        "user" => (home.to_path_buf(), "~".to_string()),
        _ => {
            let p = project.ok_or("Choose a project first.")?;
            (
                p.to_path_buf(),
                p.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            )
        }
    };
    let mut path = base;
    for part in spec.parts {
        path.push(part);
    }
    Ok((path, format!("{prefix}/{}", spec.parts.join("/"))))
}

pub fn list(
    provider: &str,
    home: &Path,
    project: Option<&Path>,
) -> Result<Vec<ConfigFile>, String> {
    let mut out = Vec::new();
    for spec in specs(provider)? {
        if spec.scope == "project" && project.is_none() {
            continue;
        }
        let (path, display) = locate(spec, home, project)?;
        out.push(ConfigFile {
            id: spec.id,
            label: spec.label,
            scope: spec.scope,
            display,
            exists: path.is_file(),
        });
    }
    Ok(out)
}

fn find<'a>(provider: &str, id: &str) -> Result<&'a Spec, String> {
    specs(provider)?
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| format!("Unknown configuration file {id}."))
}

/// Reads a config file; a file that does not exist yet reads as empty, so it can be created by saving.
pub fn read(
    provider: &str,
    id: &str,
    home: &Path,
    project: Option<&Path>,
) -> Result<FileContent, String> {
    let (path, display) = locate(find(provider, id)?, home, project)?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    Ok(FileContent {
        path: display,
        text: String::from_utf8_lossy(&bytes).to_string(),
        hash: if bytes.is_empty() && !path.exists() {
            String::new()
        } else {
            fingerprint(&bytes)
        },
        size: bytes.len() as u64,
        binary: false,
        too_large: false,
    })
}

/// Saves a config file. JSON files must be valid JSON (an agent would fail to start otherwise); folders are created.
pub fn write(
    provider: &str,
    id: &str,
    home: &Path,
    project: Option<&Path>,
    text: &str,
    expected_hash: Option<&str>,
) -> Result<String, String> {
    let spec = find(provider, id)?;
    let (path, display) = locate(spec, home, project)?;
    if path.extension().is_some_and(|e| e == "json") && !text.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(text)
            .map_err(|e| format!("{display} is not valid JSON, so it was not saved: {e}"))?;
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    write_path(&path, &display, text, expected_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_user_files_always_and_project_files_with_a_project() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        assert_eq!(list("claude-code", home.path(), None).unwrap().len(), 2);
        let all = list("claude-code", home.path(), Some(project.path())).unwrap();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0].display, "~/.claude/settings.json");
        assert!(all.iter().all(|f| !f.exists));
        assert!(list("someone-else", home.path(), None).is_err());
    }

    #[test]
    fn reads_missing_files_as_empty_and_creates_them_on_save() {
        let home = tempfile::tempdir().unwrap();
        let empty = read("claude-code", "user-settings", home.path(), None).unwrap();
        assert_eq!(empty.text, "");
        let hash = write(
            "claude-code",
            "user-settings",
            home.path(),
            None,
            "{\"env\": {}}",
            Some(&empty.hash),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(home.path().join(".claude/settings.json")).unwrap(),
            "{\"env\": {}}"
        );
        let again = read("claude-code", "user-settings", home.path(), None).unwrap();
        assert_eq!(again.hash, hash);
    }

    #[test]
    fn refuses_invalid_json_unknown_files_and_changes_made_elsewhere() {
        let home = tempfile::tempdir().unwrap();
        let err = write(
            "claude-code",
            "user-settings",
            home.path(),
            None,
            "{ not json",
            None,
        )
        .unwrap_err();
        assert!(err.contains("not valid JSON"));
        assert!(!home.path().join(".claude/settings.json").exists());
        assert!(
            write(
                "claude-code",
                "../../etc/passwd",
                home.path(),
                None,
                "x",
                None
            )
            .is_err()
        );
        assert!(
            read("claude-code", "project-mcp", home.path(), None).is_err(),
            "needs a project"
        );

        let opened = read("claude-code", "user-instructions", home.path(), None).unwrap();
        write(
            "claude-code",
            "user-instructions",
            home.path(),
            None,
            "first",
            Some(&opened.hash),
        )
        .unwrap();
        // Saving again with the fingerprint from before the first save is refused.
        let stale = fingerprint(b"something else");
        assert!(
            write(
                "claude-code",
                "user-instructions",
                home.path(),
                None,
                "second",
                Some(&stale)
            )
            .is_err()
        );
        // Markdown is not checked as JSON.
        write(
            "claude-code",
            "user-instructions",
            home.path(),
            None,
            "{ plain text",
            None,
        )
        .unwrap();
    }
}
