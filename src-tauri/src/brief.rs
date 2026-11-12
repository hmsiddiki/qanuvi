//! Qanuvi introduces itself (24g): a short brief every agent gets with its instructions, whatever model it runs on,
//! so "make a plan" gives a Qanuvi plan straight away instead of a search through Qanuvi's own code. Kept short and
//! the same for every session, so it is read from the cache after the first time.

/// What Qanuvi is and how its plans work, as the agent's instructions.
pub const BRIEF: &str = "\
You are working inside Qanuvi, a code editor that runs agents and plans. Know this without searching for it:

Plans (task lists). When the user asks for a plan, a task list or steps, write a Qanuvi plan: a Markdown file in the \
project named like PLAN.md, DEVELOPMENT-TASKLIST.md, TODO.md, ROADMAP.md or <NAME>-PLAN.md (any .md file whose name \
contains plan, task list, todo or roadmap). If the project already has one, add to it and keep what is there. Qanuvi \
shows the file in its Plan view and runs it step by step, each step in its own agent session. Format, exactly:

## Section title

- [ ] **1. Short step title** (optional short note)
  - What to do, concretely: files, functions, behaviour. One point per line.
  - Validate: how to check it is done (a test, a command, what to look at).
  - Needs: 3, 4b, docs/OTHER-PLAN.md#7
  - Model: <a model id this agent offers>

Status marks: [ ] not started, [-] in progress, [x] done, [!] blocked. Step numbers are unique in the file and may \
have a letter (12, 12a, 12b). Needs (optional) lists steps that must be done first; Qanuvi skips a step whose needs \
are not done and writes the reason on a '- Skipped:' line. Finished sections may have been moved to an archive \
next to the plan (PLAN.archive.md for PLAN.md); their steps count as done, so new numbers must not reuse them. Model (optional) picks the model for that step: the \
strongest for money, security and hard design, a mid one for normal coding, the smallest for docs and small edits. \
Make each step small enough for one focused session, leaving the project working, with its tests in the same step. \
Write the plan; do not start doing the steps unless the user asks. Do not read Qanuvi's source to learn this.";

/// The brief, or nothing for lean sessions (plan steps get their own exact instructions).
pub fn for_session(lean: bool) -> Option<&'static str> {
    (!lean).then_some(BRIEF)
}

/// The brief as a command-line argument for `program`: on one line for a .cmd or .bat wrapper (an npm install of an
/// agent), whose command line cannot carry line breaks.
pub fn as_arg(brief: &str, program: &std::path::Path) -> String {
    let batch = program
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if batch {
        brief.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        brief.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brief_is_short_and_shows_the_format_the_parser_reads() {
        // About 600 tokens at most: cheap for free models too.
        assert!(BRIEF.len() < 2400, "{}", BRIEF.len());
        for part in [
            "- [ ] **1. ",
            "  - Validate:",
            "  - Needs:",
            "  - Model:",
            "## ",
        ] {
            assert!(BRIEF.contains(part), "{part}");
        }
        assert!(
            !BRIEF.contains('"'),
            "no double quotes: a .cmd wrapper's command line must carry it"
        );
        assert!(for_session(true).is_none());
        let flat = as_arg(BRIEF, std::path::Path::new("C:/npm/claude.CMD"));
        assert!(!flat.contains('\n') && flat.contains("- [ ] **1. Short step title**"));
        assert_eq!(
            as_arg(BRIEF, std::path::Path::new("C:/x/claude.exe")),
            BRIEF
        );
        assert_eq!(for_session(false), Some(BRIEF));
    }
}
