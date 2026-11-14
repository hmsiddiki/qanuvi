//! Projects: folders the user works on, their detected type and their run, test and preview settings.

use crate::store::{Db, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub run_command: String,
    pub test_command: String,
    pub preview_url: String,
    /// Where plan steps' changes come from for reviews: "" or "qanuvi" (Qanuvi's own snapshots, git or not), or
    /// "git" (each file's repository, against its last commit; files git cannot see still use the snapshots).
    pub changes_from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    /// Main type, for the icon: laravel, qlopy, php, flutter, android, dotnet, python, node, rust or folder.
    pub kind: String,
    /// Everything that was recognised, for example ["laravel", "php", "node"].
    pub tags: Vec<String>,
    pub settings: ProjectSettings,
    pub added_at: i64,
    pub last_opened_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub kind: String,
    pub tags: Vec<String>,
    pub settings: ProjectSettings,
}

fn has(dir: &Path, name: &str) -> bool {
    dir.join(name).exists()
}

fn has_ext(dir: &Path, ext: &str) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case(ext))
            })
        })
        .unwrap_or(false)
}

/// Recognises a project from the files in its top folder. Order matters: the first match is the main type.
pub fn detect(dir: &Path) -> Detection {
    let mut tags: Vec<&str> = Vec::new();
    let qlopy =
        has(dir, "includes") && has(dir, "content") && has(dir, "admin") && has(dir, "index.php");
    let laravel = has(dir, "artisan") && has(dir, "composer.json");
    let flutter = has(dir, "pubspec.yaml");
    let android = !flutter
        && (has(dir, "build.gradle")
            || has(dir, "build.gradle.kts")
            || has(dir, "settings.gradle")
            || has(dir, "settings.gradle.kts"));
    let dotnet = has_ext(dir, "csproj") || has_ext(dir, "sln");
    let python = has(dir, "pyproject.toml") || has(dir, "requirements.txt") || has(dir, "setup.py");
    let node = has(dir, "package.json");
    let rust = has(dir, "Cargo.toml");
    let php = qlopy || laravel || has(dir, "composer.json") || has_ext(dir, "php");

    for (on, tag) in [
        (qlopy, "qlopy"),
        (laravel, "laravel"),
        (flutter, "flutter"),
        (android, "android"),
        (dotnet, "dotnet"),
        (python, "python"),
        (rust, "rust"),
        (node, "node"),
        (php, "php"),
    ] {
        if on {
            tags.push(tag);
        }
    }
    let kind = tags.first().copied().unwrap_or("folder").to_string();

    let mut settings = ProjectSettings::default();
    match kind.as_str() {
        "laravel" => {
            settings.run_command = "php artisan serve".into();
            settings.test_command = "php artisan test".into();
            settings.preview_url = "http://127.0.0.1:8000".into();
        }
        "flutter" => {
            settings.run_command = "flutter run".into();
            settings.test_command = "flutter test".into();
        }
        "android" => {
            settings.run_command = "./gradlew installDebug".into();
            settings.test_command = "./gradlew test".into();
        }
        "dotnet" => {
            settings.run_command = "dotnet run".into();
            settings.test_command = "dotnet test".into();
        }
        "python" => settings.test_command = "pytest".into(),
        "rust" => {
            settings.run_command = "cargo run".into();
            settings.test_command = "cargo test".into();
        }
        "node" => {
            settings.run_command = "npm run dev".into();
            settings.test_command = "npm test".into();
        }
        _ => {}
    }
    if settings.test_command.is_empty() && dir.join("tests").join("run.php").exists() {
        settings.test_command = "php tests/run.php".into();
    }

    Detection {
        kind,
        tags: tags.into_iter().map(String::from).collect(),
        settings,
    }
}

/// A path as the user would write it: Windows' `\\?\` prefix from canonicalising is removed.
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => rest.to_string(),
        _ => s,
    }
}

pub fn canonical_dir(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path.trim());
    if path.trim().is_empty() {
        return Err("Choose a folder.".into());
    }
    let canon = p
        .canonicalize()
        .map_err(|_| format!("The folder {path} cannot be found."))?;
    if !canon.is_dir() {
        return Err(format!("{path} is not a folder."));
    }
    Ok(PathBuf::from(display_path(&canon)))
}

fn from_row(row: &Row) -> rusqlite::Result<Project> {
    let tags: String = row.get("tags")?;
    let settings: String = row.get("settings")?;
    Ok(Project {
        id: row.get("id")?,
        name: row.get("name")?,
        path: row.get("path")?,
        kind: row.get("kind")?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        settings: serde_json::from_str(&settings).unwrap_or_default(),
        added_at: row.get("added_at")?,
        last_opened_at: row.get("last_opened_at")?,
    })
}

pub fn list(db: &Db) -> Result<Vec<Project>, String> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT * FROM projects ORDER BY last_opened_at DESC, name")
        .map_err(|e| e.to_string())?;

    stmt.query_map([], from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn get(db: &Db, id: &str) -> Result<Project, String> {
    db.conn()
        .query_row("SELECT * FROM projects WHERE id = ?1", [id], from_row)
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That project is no longer in the list.".to_string())
}

/// Adds a folder, or returns the existing project if the folder is already in the list.
pub fn add(db: &Db, path: &str) -> Result<Project, String> {
    let dir = canonical_dir(path)?;
    let path_str = display_path(&dir);
    let existing = db
        .conn()
        .query_row(
            "SELECT * FROM projects WHERE path = ?1 COLLATE NOCASE",
            [&path_str],
            from_row,
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(project) = existing {
        return Ok(project);
    }
    let found = detect(&dir);
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path_str.clone());
    let project = Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: path_str,
        kind: found.kind,
        tags: found.tags,
        settings: found.settings,
        added_at: now(),
        last_opened_at: now(),
    };
    db.conn()
        .execute(
            "INSERT INTO projects (id, name, path, kind, tags, settings, added_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                project.id,
                project.name,
                project.path,
                project.kind,
                serde_json::to_string(&project.tags).unwrap_or_default(),
                serde_json::to_string(&project.settings).unwrap_or_default(),
                project.added_at,
                project.last_opened_at
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(project)
}

/// Removes a project from the list. Its files are never touched.
pub fn remove(db: &Db, id: &str) -> Result<(), String> {
    db.conn()
        .execute("DELETE FROM projects WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update(
    db: &Db,
    id: &str,
    name: &str,
    settings: &ProjectSettings,
) -> Result<Project, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err("A project name needs 1 to 80 characters.".into());
    }
    let changed = db
        .conn()
        .execute(
            "UPDATE projects SET name = ?2, settings = ?3 WHERE id = ?1",
            params![
                id,
                name,
                serde_json::to_string(settings).unwrap_or_default()
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err("That project is no longer in the list.".into());
    }
    get(db, id)
}

pub fn touch(db: &Db, id: &str) -> Result<(), String> {
    db.conn()
        .execute(
            "UPDATE projects SET last_opened_at = ?2 WHERE id = ?1",
            params![id, now()],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Looks again at the folder's files (after the user added a framework, for example). Settings are kept.
pub fn redetect(db: &Db, id: &str) -> Result<Project, String> {
    let project = get(db, id)?;
    let found = detect(Path::new(&project.path));
    db.conn()
        .execute(
            "UPDATE projects SET kind = ?2, tags = ?3 WHERE id = ?1",
            params![
                id,
                found.kind,
                serde_json::to_string(&found.tags).unwrap_or_default()
            ],
        )
        .map_err(|e| e.to_string())?;
    get(db, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn folder(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            if f.ends_with('/') {
                fs::create_dir_all(&p).unwrap();
            } else {
                if let Some(parent) = p.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&p, "").unwrap();
            }
        }
        dir
    }

    #[test]
    fn recognises_each_project_type() {
        let cases: &[(&[&str], &str)] = &[
            (&["artisan", "composer.json", "package.json"], "laravel"),
            (&["includes/", "content/", "admin/", "index.php"], "qlopy"),
            (&["pubspec.yaml", "android/"], "flutter"),
            (&["settings.gradle.kts", "app/"], "android"),
            (&["Shop.csproj"], "dotnet"),
            (&["pyproject.toml"], "python"),
            (&["Cargo.toml"], "rust"),
            (&["package.json"], "node"),
            (&["plugin.php"], "php"),
            (&["notes.txt"], "folder"),
        ];
        for (files, kind) in cases {
            let dir = folder(files);
            assert_eq!(detect(dir.path()).kind, *kind, "files {files:?}");
        }
    }

    #[test]
    fn keeps_every_tag_and_fills_default_commands() {
        let dir = folder(&["artisan", "composer.json", "package.json"]);
        let found = detect(dir.path());
        assert_eq!(found.tags, vec!["laravel", "node", "php"]);
        assert_eq!(found.settings.run_command, "php artisan serve");
        assert_eq!(found.settings.preview_url, "http://127.0.0.1:8000");
    }

    #[test]
    fn a_flutter_project_is_not_also_android() {
        let dir = folder(&["pubspec.yaml", "build.gradle"]);
        assert_eq!(detect(dir.path()).tags, vec!["flutter"]);
    }

    #[test]
    fn finds_the_php_test_runner_used_by_qcommerce() {
        let dir = folder(&["plugin.php", "tests/run.php"]);
        assert_eq!(
            detect(dir.path()).settings.test_command,
            "php tests/run.php"
        );
    }

    #[test]
    fn add_list_update_and_remove_without_touching_files() {
        let db = Db::open_in_memory().unwrap();
        let dir = folder(&["package.json", "src/"]);
        let path = dir.path().to_string_lossy().to_string();

        let added = add(&db, &path).unwrap();
        assert_eq!(added.kind, "node");
        assert_eq!(
            add(&db, &path).unwrap().id,
            added.id,
            "adding the same folder twice keeps one entry"
        );
        assert_eq!(list(&db).unwrap().len(), 1);

        let settings = ProjectSettings {
            run_command: "npm start".into(),
            ..added.settings.clone()
        };
        let updated = update(&db, &added.id, "  Shop  ", &settings).unwrap();
        assert_eq!(updated.name, "Shop");
        assert_eq!(updated.settings.run_command, "npm start");
        assert!(update(&db, &added.id, "   ", &settings).is_err());

        remove(&db, &added.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(dir.path().join("package.json").exists(), "files stay");
    }

    #[test]
    fn refuses_missing_folders_and_files() {
        let db = Db::open_in_memory().unwrap();
        assert!(add(&db, "Z:/no/such/folder/anywhere").is_err());
        let dir = folder(&["file.txt"]);
        assert!(add(&db, &dir.path().join("file.txt").to_string_lossy()).is_err());
        assert!(add(&db, "  ").is_err());
    }

    #[test]
    fn display_path_drops_the_windows_verbatim_prefix() {
        assert_eq!(
            display_path(Path::new(r"\\?\E:\Projects\qanuvi")),
            r"E:\Projects\qanuvi"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share")),
            r"\\?\UNC\server\share"
        );
        assert_eq!(display_path(Path::new("/home/me/app")), "/home/me/app");
    }
}
