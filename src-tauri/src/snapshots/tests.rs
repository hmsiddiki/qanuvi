//! Snapshots in the four project layouts a plan can run in: no git, a folder the project's git ignores, a repository
//! nested inside the project, and one plain repository. Each must show what a piece of work changed.

use super::*;
use std::fs;

fn git_ok() -> bool {
    crate::git::run_for_test(Path::new("."), &["--version"]).is_ok()
}

fn g(dir: &Path, args: &[&str]) {
    crate::git::run_for_test(dir, args).unwrap();
}

fn init(dir: &Path) {
    g(dir, &["init", "-q"]);
    g(dir, &["config", "user.email", "test@example.com"]);
    g(dir, &["config", "user.name", "Test"]);
    g(dir, &["config", "core.autocrlf", "false"]);
}

fn commit_all(dir: &Path) {
    g(dir, &["add", "-A"]);
    g(dir, &["commit", "-q", "-m", "start"]);
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn none(_: &str) -> bool {
    false
}

fn paths_of(c: &Changes) -> Vec<(&str, &str)> {
    c.changes
        .iter()
        .map(|x| (x.path.as_str(), x.kind))
        .collect()
}

#[test]
fn a_project_without_git_shows_added_changed_and_deleted_files() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "a.php", "<?php\necho 1;\n");
    write(r, "gone.txt", "bye\n");
    write(r, "same.txt", "kept\n");
    let t = take(r, &[]).unwrap();
    assert_eq!(t.files, 3);

    write(r, "a.php", "<?php\necho 2;\n");
    write(r, "src/new.php", "<?php\n");
    fs::remove_file(r.join("gone.txt")).unwrap();
    let c = changes(r, &t.id, &[], false, &none).unwrap();
    assert_eq!(
        paths_of(&c),
        [
            ("a.php", "changed"),
            ("gone.txt", "deleted"),
            ("src/new.php", "added")
        ]
    );
    assert!(
        c.changes[0].diff.contains("-echo 1;\n+echo 2;"),
        "{}",
        c.changes[0].diff
    );
    assert!(c.changes[1].diff.contains("-bye"));
    assert!(c.changes[2].diff.contains("+<?php"));
    assert!(c.unseen.is_empty());

    // With git chosen but no repository, the changes still show, and git's blind spot is named.
    let c = changes(r, &t.id, &[], true, &none).unwrap();
    assert_eq!(c.changes.len(), 3);
    assert_eq!(c.unseen, ["a.php", "gone.txt", "src/new.php"]);
}

#[test]
fn nothing_changed_is_empty_and_the_next_snapshot_reads_nothing_new() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "a.txt", "one\n");
    let t = take(r, &[]).unwrap();
    let c = changes(r, &t.id, &[], false, &none).unwrap();
    assert!(c.changes.is_empty());
    assert_eq!(take(r, &[]).unwrap().stored, 0);
}

#[test]
fn skipped_files_and_qanuvis_own_folder_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "PLAN.md", "- [ ] **1. A**\n");
    write(r, "a.txt", "one\n");
    let t = take(r, &[]).unwrap();
    write(r, "PLAN.md", "- [x] **1. A**\n");
    write(r, "a.txt", "two\n");
    write(r, ".qanuvi/plan-logs/1.md", "log\n");
    let c = changes(r, &t.id, &[], false, &|p| p == "PLAN.md").unwrap();
    assert_eq!(paths_of(&c), [("a.txt", "changed")]);
}

#[test]
fn a_folder_the_projects_git_ignores_is_followed_when_named() {
    if !git_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    init(r);
    write(r, ".gitignore", "/plugins/*\n");
    write(r, "index.php", "<?php\n");
    write(r, "plugins/shop/PLAN.md", "- [ ] **1. A**\n");
    write(r, "plugins/shop/src/Cart.php", "<?php\nclass Cart {}\n");
    commit_all(r);

    let t = take(r, &["plugins/shop".into()]).unwrap();
    write(
        r,
        "plugins/shop/src/Cart.php",
        "<?php\nclass Cart { public $contact; }\n",
    );
    let c = changes(r, &t.id, &["plugins/shop".into()], false, &none).unwrap();
    assert_eq!(paths_of(&c), [("plugins/shop/src/Cart.php", "changed")]);
    assert!(
        c.changes[0]
            .diff
            .contains("+class Cart { public $contact; }")
    );

    // Git chosen: the project's repository ignores the folder, so Qanuvi's own snapshot shows it and says so.
    let t = take(r, &["plugins/shop".into()]).unwrap();
    write(r, "plugins/shop/src/Cart.php", "<?php\nclass Cart {}\n");
    let c = changes(r, &t.id, &["plugins/shop".into()], true, &none).unwrap();
    assert_eq!(c.unseen, ["plugins/shop/src/Cart.php"]);
    assert!(
        c.changes[0]
            .diff
            .contains("-class Cart { public $contact; }")
    );
}

#[test]
fn a_repository_nested_in_an_ignored_folder_shows_its_changes_both_ways() {
    if !git_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    init(r);
    write(r, ".gitignore", "/content/plugins/*\n");
    write(r, "index.php", "<?php\n");
    commit_all(r);
    let p = r.join("content/plugins/qcommerce");
    write(
        r,
        "content/plugins/qcommerce/src/Schema.php",
        "<?php\n// v25\n",
    );
    init(&p);
    commit_all(&p);

    // Not named: the nested repository is found by itself (as the file tree finds it).
    let t = take(r, &[]).unwrap();
    write(
        r,
        "content/plugins/qcommerce/src/Schema.php",
        "<?php\n// v26\n$contact_hash = 1;\n",
    );
    let own = changes(r, &t.id, &[], false, &none).unwrap();
    assert_eq!(
        paths_of(&own),
        [("content/plugins/qcommerce/src/Schema.php", "changed")]
    );
    assert!(own.changes[0].diff.contains("+$contact_hash = 1;"));

    // Git chosen: the nested repository's own diff, nothing unseen.
    let t = take(r, &[]).unwrap();
    write(
        r,
        "content/plugins/qcommerce/src/Schema.php",
        "<?php\n// v27\n",
    );
    let via_git = changes(r, &t.id, &[], true, &none).unwrap();
    assert!(via_git.unseen.is_empty());
    assert!(
        via_git.changes[0].diff.contains("diff --git"),
        "{}",
        via_git.changes[0].diff
    );
    assert!(via_git.changes[0].diff.contains("+// v27"));
}

#[test]
fn a_plain_repository_compares_with_the_snapshot_not_with_older_uncommitted_work() {
    if !git_ok() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    init(r);
    write(r, "a.txt", "one\n");
    commit_all(r);
    write(r, "a.txt", "one\ntwo\n"); // earlier work, not committed
    let t = take(r, &[]).unwrap();
    write(r, "a.txt", "one\ntwo\nthree\n");
    let c = changes(r, &t.id, &[], false, &none).unwrap();
    let d = &c.changes[0].diff;
    assert!(d.contains("+three") && !d.contains("+two"), "{d}");
    // Git chosen: against the last commit, as git shows it.
    let t = take(r, &[]).unwrap();
    write(r, "a.txt", "one\ntwo\nthree\nfour\n");
    let c = changes(r, &t.id, &[], true, &none).unwrap();
    assert!(c.changes[0].diff.contains("+two") && c.changes[0].diff.contains("+four"));
}

#[test]
fn binary_and_large_files_are_noticed_but_not_shown() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::write(r.join("logo.png"), [0u8, 1, 2, 3]).unwrap();
    let t = take(r, &[]).unwrap();
    fs::write(r.join("logo.png"), [0u8, 1, 2, 3, 4]).unwrap();
    let c = changes(r, &t.id, &[], false, &none).unwrap();
    assert_eq!(paths_of(&c), [("logo.png", "changed")]);
    assert!(c.changes[0].diff.contains("binary or too large"));
}

#[test]
fn an_unknown_or_unsafe_snapshot_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(changes(dir.path(), "../x", &[], false, &none).is_err());
    assert!(changes(dir.path(), "nothere", &[], false, &none).is_err());
}

#[test]
fn unified_diffs_keep_three_lines_of_context_and_split_far_changes() {
    let a: String = (1..=20).map(|i| format!("l{i}\n")).collect();
    let b = a.replace("l2\n", "L2\n").replace("l18\n", "L18\n");
    let d = unified("f", &a, &b);
    assert!(d.starts_with("--- f (before this step)\n+++ f (after)\n"));
    assert!(
        d.contains("@@ -1,5 +1,5 @@\n l1\n-l2\n+L2\n l3\n l4\n l5\n"),
        "{d}"
    );
    assert!(
        d.contains("@@ -15,6 +15,6 @@\n l15\n l16\n l17\n-l18\n+L18\n l19\n l20\n"),
        "{d}"
    );
    assert_eq!(unified("f", "x\n", "x\n"), "");
    assert!(unified("f", "x\n", "x\r\n").contains("only line endings"));
    // From and to nothing.
    assert!(unified("f", "", "a\nb\n").contains("@@ -1,0 +1,2 @@\n+a\n+b\n"));
    assert!(unified("f", "a\n", "").contains("-a"));
}

#[test]
fn line_ops_match_a_plain_longest_common_subsequence() {
    // Small random texts: the ops must rebuild b from a, with the fewest changes.
    let mut seed = 7u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };
    for _ in 0..300 {
        let a: Vec<String> = (0..next() % 12)
            .map(|_| ["a", "b", "c"][next() % 3].into())
            .collect();
        let b: Vec<String> = (0..next() % 12)
            .map(|_| ["a", "b", "c"][next() % 3].into())
            .collect();
        let (ar, br): (Vec<&str>, Vec<&str>) = (
            a.iter().map(|s| s.as_str()).collect(),
            b.iter().map(|s| s.as_str()).collect(),
        );
        let ops = line_ops(&ar, &br).unwrap();
        let (mut i, mut j, mut rebuilt, mut edits) = (0, 0, Vec::new(), 0);
        for o in &ops {
            match o {
                Op::Same => {
                    assert_eq!(ar[i], br[j]);
                    rebuilt.push(ar[i]);
                    i += 1;
                    j += 1;
                }
                Op::Del => {
                    i += 1;
                    edits += 1;
                }
                Op::Add => {
                    rebuilt.push(br[j]);
                    j += 1;
                    edits += 1;
                }
            }
        }
        assert_eq!((i, j), (ar.len(), br.len()));
        assert_eq!(rebuilt, br);
        // Fewest edits = n + m - 2 * lcs.
        let (n, m) = (ar.len(), br.len());
        let mut l = vec![vec![0usize; m + 1]; n + 1];
        for x in (0..n).rev() {
            for y in (0..m).rev() {
                l[x][y] = if ar[x] == br[y] {
                    l[x + 1][y + 1] + 1
                } else {
                    l[x + 1][y].max(l[x][y + 1])
                };
            }
        }
        assert_eq!(edits, n + m - 2 * l[0][0], "{ar:?} -> {br:?}");
    }
}

/// Measures a real project: QANUVI_MEASURE=<folder> cargo test -- --ignored measure_a_real_project --nocapture
#[test]
#[ignore]
fn measure_a_real_project() {
    let Ok(root) = std::env::var("QANUVI_MEASURE") else {
        return;
    };
    let r = Path::new(&root);
    let first = take(r, &[]).unwrap();
    let second = take(r, &[]).unwrap();
    let c = changes(r, &second.id, &[], false, &none).unwrap();
    println!(
        "first {first:?}\nsecond {second:?}\nchanges after nothing: {}",
        c.changes.len()
    );
}

#[test]
fn a_run_learns_up_front_which_plan_folders_git_cannot_see() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    write(r, "plugins/shop/PLAN.md", "- [ ] **1. A**\n");
    write(r, "plugins/qc/PLAN.md", "- [ ] **1. A**\n");
    write(r, "top.txt", "x\n");
    // No git at all: every folder is out of git's sight.
    let all = vec![
        "plugins/shop".to_string(),
        "plugins/qc".to_string(),
        String::new(),
    ];
    assert_eq!(take(r, &all).unwrap().git_blind, all);
    if !git_ok() {
        return;
    }
    init(r);
    write(r, ".gitignore", "/plugins/*\n");
    commit_all(r);
    init(&r.join("plugins/qc"));
    commit_all(&r.join("plugins/qc"));
    // The ignored folder is blind; the nested repository and the project's top are seen.
    assert_eq!(take(r, &all).unwrap().git_blind, ["plugins/shop"]);
}
