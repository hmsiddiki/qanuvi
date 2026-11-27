use super::*;

fn store() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::new(d.path());
    (d, s)
}

fn build_log() -> String {
    let mut s = String::new();
    for i in 0..400 {
        s.push_str(&format!(
            "2026-11-27T08:{:02}:{:02} INFO worker {i} handled request in {} ms\n",
            i / 60,
            i % 60,
            i % 17
        ));
        if i == 250 {
            s.push_str("2026-11-27T08:04:10 FATAL database connection refused: port 5432\n");
        }
    }
    s
}

#[test]
fn small_output_and_code_are_never_touched() {
    let small = "a\nb\nc\n";
    assert_eq!(compress(small, &Hint::default(), None).text, small);
    let code = "fn main() {}\n".repeat(400);
    let c = compress(
        &code,
        &Hint {
            tool: "Read",
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.kind, Kind::Code);
    assert_eq!(c.text, code);
    assert_eq!(c.id, None);
}

#[test]
fn a_log_keeps_the_fatal_line_whole_and_says_repeats_once() {
    let (_d, s) = store();
    let log = build_log();
    let c = compress(
        &log,
        &Hint {
            tool: "Bash",
            command: "cat app.log",
            ..Default::default()
        },
        Some(&s),
    );
    assert_eq!(c.kind, Kind::Log);
    assert!(
        c.text
            .contains("2026-11-27T08:04:10 FATAL database connection refused: port 5432")
    );
    assert!(c.after * 5 < c.before, "{} -> {}", c.before, c.after);
    let id = c.id.clone().unwrap();
    assert!(c.text.contains(&format!("{RETRIEVE_TOOL} id \"{id}\"")));
    // The original comes back whole, or only the lines asked for.
    assert_eq!(s.get(&id, None).unwrap(), log);
    let only = s.get(&id, Some("fatal")).unwrap();
    assert!(only.starts_with("252: 2026-11-27T08:04:10 FATAL"), "{only}");
}

#[test]
fn the_same_text_always_gives_the_same_result() {
    let log = build_log();
    let a = compress(&log, &Hint::default(), None);
    let b = compress(&log, &Hint::default(), None);
    assert_eq!(a, b);
    assert!(
        !a.text.contains(RETRIEVE_TOOL),
        "no store: no promise of retrieval"
    );
}

#[test]
fn json_arrays_keep_ends_errors_and_outliers() {
    let mut items = Vec::new();
    for i in 0..200 {
        let status = if i == 120 { "error" } else { "ok" };
        let ms = if i == 77 { 9_000 } else { 100 + (i % 7) };
        items.push(
            serde_json::json!({"id": i, "status": status, "ms": ms, "name": format!("item-{i}")}),
        );
    }
    let text = serde_json::to_string_pretty(&items).unwrap();
    let c = compress(&text, &Hint::default(), None);
    assert_eq!(c.kind, Kind::Json);
    assert!(c.after * 10 < c.before, "{} -> {}", c.before, c.after);
    for must in [
        "\"id\":0",
        "\"id\":199",
        "\"id\":120",
        "\"id\":77",
        "similar items left out",
    ] {
        assert!(c.text.contains(must), "missing {must}");
    }
}

#[test]
fn search_results_name_every_file_and_count_the_rest() {
    let mut text = String::new();
    for f in 0..30 {
        for n in 0..20 {
            text.push_str(&format!(
                "src/mod{f}.rs:{}:    let value = compute(x, {n});\n",
                n * 3 + 1
            ));
        }
    }
    let c = compress(
        &text,
        &Hint {
            tool: "Grep",
            ..Default::default()
        },
        None,
    );
    assert_eq!(c.kind, Kind::Search);
    for f in 0..30 {
        assert!(
            c.text.contains(&format!("src/mod{f}.rs:1:")),
            "file {f} named"
        );
    }
    assert!(c.text.contains("src/mod7.rs: [... 15 more matches ...]"));
    assert!(c.text.contains("600 matches in 30 files"));
}

#[test]
fn diffs_keep_changes_and_cut_context() {
    let mut text = String::from(
        "diff --git a/x.rs b/x.rs\nindex 1..2 100644\n--- a/x.rs\n+++ b/x.rs\n@@ -1,300 +1,300 @@\n",
    );
    for i in 0..300 {
        if i == 150 {
            text.push_str("-    old line\n+    new line\n");
        } else {
            text.push_str(&format!("     context line {i}\n"));
        }
    }
    let c = compress(&text, &Hint::default(), None);
    assert_eq!(c.kind, Kind::Diff);
    assert!(c.text.contains("-    old line\n+    new line"));
    assert!(c.text.contains("+++ b/x.rs"));
    assert!(c.after * 5 < c.before);
}

#[test]
fn html_becomes_plain_text() {
    let mut text = String::from(
        "<!DOCTYPE html><html><head><style>.a{color:red}</style><script>var x=1;</script></head><body>",
    );
    for i in 0..100 {
        text.push_str(&format!(
            "<div class=\"row\"><span>Row {i}</span> &amp; more</div>\n"
        ));
    }
    text.push_str("</body></html>");
    let c = compress(&text, &Hint::default(), None);
    assert_eq!(c.kind, Kind::Html);
    assert!(c.text.contains("Row 42 & more"));
    assert!(
        !c.text.contains("<div") && !c.text.contains("var x=1") && !c.text.contains("color:red")
    );
}

#[test]
fn retrieval_ids_are_checked_and_old_ones_said_plainly() {
    let (_d, s) = store();
    assert!(s.get("../etc", None).unwrap_err().contains("is not a"));
    assert!(
        s.get("abcdef", None)
            .unwrap_err()
            .contains("kept for a day")
    );
}

#[test]
fn search_lines_are_told_from_log_lines_and_windows_paths_work() {
    assert!(looks_like_match("src/a.rs:12:    let x = 1;"));
    assert!(looks_like_match(r"C:\p\a.rs:12:let x"));
    assert_eq!(match_file(r"C:\p\a.rs:12:let x"), r"C:\p\a.rs");
    assert!(!looks_like_match("2026-11-27T08:00:00 INFO started"));
    assert!(!looks_like_match("Note: 12: something"));
}

/// 24w: the measurement against Headroom. Runs only on request, on a folder of real tool outputs:
/// QANUVI_O3_CORPUS=<folder> cargo test --lib measure_corpus -- --ignored --nocapture
#[test]
#[ignore]
fn measure_corpus() {
    let dir = std::env::var("QANUVI_O3_CORPUS").expect("QANUVI_O3_CORPUS");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    for p in names {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let t = std::time::Instant::now();
        let c = compress(
            &text,
            &Hint {
                tool: "Bash",
                ..Default::default()
            },
            None,
        );
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:<18} {:?} {} -> {} ({:.0}% saved, {ms:.1} ms)",
            p.file_name().unwrap().to_string_lossy(),
            c.kind,
            c.before,
            c.after,
            100.0 * (1.0 - c.after as f64 / c.before.max(1) as f64)
        );
        std::fs::write(p.with_extension("qanuvi.out"), &c.text).ok();
    }
}

/// Owner 2026-11-27: off until proven; Free models covers Qanuvi's own agent only, All models Claude Code too.
#[test]
fn the_built_in_compression_is_off_until_chosen() {
    let set = |v: Option<&str>| {
        let mut m = serde_json::Map::new();
        if let Some(v) = v {
            m.insert(SETTING.into(), Value::String(v.into()));
        }
        m
    };
    assert_eq!(mode_of(&set(None)), Mode::Off);
    assert!(!own_on(&set(None)));
    assert_eq!(mode_of(&set(Some("free"))), Mode::Free);
    assert!(own_on(&set(Some("free"))));
    assert_eq!(mode_of(&set(Some("all"))), Mode::All);
    // 0.1.50's values: "builtin" was every model; Automatic, Headroom and Off are off now.
    assert_eq!(mode_of(&set(Some("builtin"))), Mode::All);
    for old in ["", "headroom", "off"] {
        assert_eq!(mode_of(&set(Some(old))), Mode::Off, "{old}");
    }
    assert!(
        looks_like_headroom("http://127.0.0.1:8787")
            && !looks_like_headroom("https://api.anthropic.com")
    );
}
