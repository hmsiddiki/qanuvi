//! The request Qanuvi prepares when the user asks the agent to write a task list Qanuvi can read (ported from
//! `src/lib/plan/prompts.ts`).

/// Claude Code's models, strongest first (as in `src/lib/agents/providers.ts`).
pub const CLAUDE_CODE_MODELS: [&str; 4] = [
    "claude-fable-5-2",
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

/// The task-list request for a file, with the agent's models (strongest first) to choose from.
pub fn task_list_prompt(file: &str, models: &[&str]) -> String {
    [
        format!("Write a task list for this project in {file}, in this format so Qanuvi can show and run it:"),
        String::new(),
        "## Section name".into(),
        String::new(),
        "- [ ] **1. Step title** (optional short note)".into(),
        "  - What the step does, one point per line.".into(),
        "  - Validate: how to check it is done.".into(),
        format!("  - Model: {}", models.get(1).copied().unwrap_or("model-id")),
        String::new(),
        format!(
            "For each step choose the model that fits the work ({}): the strongest for money, security and hard \
             design, a mid model for normal coding, the smallest for small edits and docs. If the file already exists, \
             keep what is there and add to it. Ask me first if the goal is not clear from the project's files.",
            models.join(", ")
        ),
        String::new(),
        "Goal: ".into(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_file_and_shows_a_step_format() {
        let p = task_list_prompt("PLAN.md", &CLAUDE_CODE_MODELS);
        assert!(p.contains("PLAN.md"));
        assert!(p.contains("  - Model: claude-opus-5-5"));
    }
}
