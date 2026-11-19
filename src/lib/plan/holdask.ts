/**
 * A held plan step as an Inbox question (24u part 4): its key, its form and what its answer runs with. Plain
 * functions, so the runner, the Inbox and the tests share them.
 */

/** "keep" / "qanuvi": the answer to "git cannot see some of the plan's files" (which changes the reviews use). */
export type AnswerChoice = "go" | "skip" | "decide" | "keep" | "qanuvi" | "end";

/**
 * n1: a blocked or unfinished step's question laid out so it stands on its own (the master writes it): what happened,
 * why only the owner can act, the exact change wanted, a snippet to copy, and what Qanuvi can do itself.
 */
export interface HoldCard {
  headline: string;
  happened?: string;
  whyYou?: string;
  wanted?: string;
  snippet?: { label: string; text: string };
  canDo?: string;
}

export interface InboxAnswer {
  choice: AnswerChoice;
  text?: string;
}

/** n1: the answer box of a blocked or unfinished step (the master words its Inbox form the same). */
export const ANSWER_HINT =
  'Your answer, in your own words: "do it yourself", "done, try again", "I don\'t understand"…';

/** "Let Qanuvi decide" as the held step is told it. */
export const DECIDE_TEXT =
  "Decide this yourself: choose what fits the plan and the project best, take the safer way when unsure, and say in your summary what you chose and why.";

/** What a held step runs with for an answer: "decide" goes ahead with the deciding note. */
export function holdReply(answer: InboxAnswer): { choice: "go" | "skip"; text: string } {
  const text = answer.text?.trim() ?? "";
  if (answer.choice === "skip") return { choice: "skip", text };
  return { choice: "go", text: answer.choice === "decide" ? [DECIDE_TEXT, text].filter(Boolean).join("\n") : text };
}
