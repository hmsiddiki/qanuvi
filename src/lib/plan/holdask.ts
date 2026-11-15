/**
 * A held plan step as an Inbox question (24u part 4): its key, its form and what its answer runs with. Plain
 * functions, so the runner, the Inbox and the tests share them.
 */

/** "keep" / "qanuvi": the answer to "git cannot see some of the plan's files" (which changes the reviews use). */
export type AnswerChoice = "go" | "skip" | "decide" | "keep" | "qanuvi";

export interface InboxAnswer {
  choice: AnswerChoice;
  text?: string;
}

/** "Let Qanuvi decide" as the held step is told it. */
export const DECIDE_TEXT =
  "Decide this yourself: choose what fits the plan and the project best, take the safer way when unsure, and say in your summary what you chose and why.";

/** What a held step runs with for an answer: "decide" goes ahead with the deciding note. */
export function holdReply(answer: InboxAnswer): { choice: "go" | "skip"; text: string } {
  const text = answer.text?.trim() ?? "";
  if (answer.choice === "skip") return { choice: "skip", text };
  return { choice: "go", text: answer.choice === "decide" ? [DECIDE_TEXT, text].filter(Boolean).join("\n") : text };
}
