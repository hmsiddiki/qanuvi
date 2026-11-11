import { CLAUDE_CODE, type AgentProvider } from "$lib/agents/providers";

/** The request Qanuvi prepares when the user asks the agent to write a task list Qanuvi can read. */
export function taskListPrompt(file = "DEVELOPMENT-TASKLIST.md", provider: AgentProvider = CLAUDE_CODE): string {
  const MODELS = provider.models;
  const models = MODELS.map((m) => m.id).join(", ");
  return [
    `Write a task list for this project in ${file}, in this format so Qanuvi can show and run it:`,
    "",
    "## Section name",
    "",
    "- [ ] **1. Step title** (optional short note)",
    "  - What the step does, one point per line.",
    "  - Validate: how to check it is done.",
    `  - Model: ${MODELS[1]?.id ?? "model-id"}`,
    "",
    `For each step choose the model that fits the work (${models}): the strongest for money, security and hard ` +
      "design, a mid model for normal coding, the smallest for small edits and docs. If the file already exists, " +
      "keep what is there and add to it. Ask me first if the goal is not clear from the project's files.",
    "",
    "Goal: ",
  ].join("\n");
}
