/**
 * A held plan step as an Inbox question (24u part 4): its key, its form and what its answer runs with. Plain
 * functions, so the runner, the Inbox and the tests share them.
 */
import type { HeldKind } from "./setaside";

/** "keep" / "qanuvi": the answer to "git cannot see some of the plan's files" (which changes the reviews use). */
export type AnswerChoice = "go" | "skip" | "decide" | "keep" | "qanuvi";

export interface InboxAnswer {
  choice: AnswerChoice;
  text?: string;
}

/** One question per plan step: a later run holding the same step asks the same question, not a new one. */
export function holdKey(projectId: string, file: string, step: string): string {
  return `hold:${projectId}:${file}:${step}`;
}

/** The hold asks for a login, password or key: such answers are typed in the run panel only. */
export function asksSecret(why: string): boolean {
  return /\b(password|passphrase|log ?in|sign[- ]?in|credential|token|api[- ]?key|secret|ssh key)s?\b/i.test(why);
}

/** "Let Qanuvi decide" as the held step is told it. */
export const DECIDE_TEXT =
  "Decide this yourself: choose what fits the plan and the project best, take the safer way when unsure, and say in your summary what you chose and why.";

/** The answer form of a held step, worded like the run panel's. */
export function holdForm(kind: HeldKind, why: string) {
  const secret = asksSecret(why);
  const go = kind === "hold" ? "Go ahead" : "Try again";
  const choices: { id: AnswerChoice; label: string; hint?: string }[] = [{ id: "go", label: go }];
  if (kind === "hold" && !secret)
    choices.push({ id: "decide", label: "Let Qanuvi decide", hint: "It picks the safer way and says what it chose" });
  choices.push({ id: "skip", label: "Skip this step", hint: "Leave it, and the steps waiting for it" });
  return {
    choices,
    ...(secret
      ? { secret: true }
      : { text: kind === "hold" ? "Your answer (optional)" : "What to do differently (optional)" }),
  };
}

/** What a held step runs with for an answer: "decide" goes ahead with the deciding note. */
export function holdReply(answer: InboxAnswer): { choice: "go" | "skip"; text: string } {
  const text = answer.text?.trim() ?? "";
  if (answer.choice === "skip") return { choice: "skip", text };
  return { choice: "go", text: answer.choice === "decide" ? [DECIDE_TEXT, text].filter(Boolean).join("\n") : text };
}
