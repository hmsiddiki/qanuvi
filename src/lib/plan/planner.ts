/**
 * Planner and workers (Milestone 14): a strong model turns a request into task-list steps; Qanuvi checks them, numbers
 * them and writes them into the file itself (the planner works read-only). When a step fails even on the strongest
 * model, the planner is asked once to rewrite it into smaller steps. Plain functions; the store is planner.svelte.ts.
 */
import type { AgentModel } from "$lib/agents/providers";
import { parsePlan, splitLines, type Step } from "./tasklist";

const FENCE_RE = /```(?:tasklist|markdown|md)?\s*\n([\s\S]*?)```/i;

export function plannerPrompt(request: string, file: string, models: AgentModel[]): string {
  const ids = models.map((m) => m.id).join(", ");
  return [
    "Plan the following work for this project. You are the planner: read the code as much as you need (read-only),",
    "then answer with the plan only, as task-list steps inside one ```tasklist block. Do not change any file.",
    "",
    "Request:",
    request.trim(),
    "",
    "Format of each step (Qanuvi reads exactly this):",
    "```tasklist",
    "- [ ] **1. Short step title**",
    "  - What to do, concretely: files, functions, behaviour. One point per line.",
    "  - Validate: how to check it is done (a test, a command, what to look at).",
    "  - Model: <model id>",
    "```",
    "",
    "Rules:",
    "- 2 to 10 steps, in order; each small enough for one focused session, and each leaves the project working.",
    `- Choose a model per step from: ${ids} (strongest first). Use the strongest for money, security and hard design; a mid model for normal coding; the smallest for docs and small edits.`,
    "- Number the steps from 1; Qanuvi renumbers them to follow the steps already in " + file + ".",
    "- Put tests in the same step as the code they test.",
  ].join("\n");
}

/** The steps block in a planner's answer: the ```tasklist (or markdown) block, else the whole answer. */
export function extractSteps(answer: string): { block: string; steps: Step[] } {
  const m = FENCE_RE.exec(answer);
  const block = (m ? m[1] : answer).replace(/\r\n/g, "\n").trim();
  // Only the step lines and their details: drop headings and other text around them.
  const lines = block.split("\n");
  const kept: string[] = [];
  let inStep = false;
  for (const l of lines) {
    if (/^ ?- \[[ xX!-]\] /.test(l)) {
      inStep = true;
      kept.push(l.replace(/^ ?- \[[xX!-]\] /, "- [ ] "));
    } else if (inStep && (/^\s{2,}\S/.test(l) || !l.trim())) kept.push(l);
    else inStep = false;
  }
  const clean = kept.join("\n").trim();
  return { block: clean, steps: parsePlan(clean).steps };
}

/** The highest whole step number in a task list (12 for "12" or "12b"), 0 when there is none. */
export function lastNumber(text: string): number {
  let max = 0;
  for (const s of parsePlan(text).steps) {
    const n = Number.parseInt(s.number ?? "", 10);
    if (Number.isFinite(n)) max = Math.max(max, n);
  }
  return max;
}

/** Gives the steps of a block new numbers: `start`, `start + 1`, … or, with a base like "12", 12a, 12b, …. */
export function renumber(block: string, start: number | string): string {
  let i = 0;
  const label = () => (typeof start === "number" ? String(start + i++) : `${start}${String.fromCharCode(97 + i++)}`);
  return block
    .split("\n")
    .map((l) => {
      const m = /^(- \[ \] )(?:\*\*(?:\d+[a-z]?\.\s+)?(.+?)\*\*(.*)|(.+))$/.exec(l);
      if (!m) return l;
      const title = (m[2] ?? m[4]).trim();
      return `${m[1]}**${label()}. ${title}**${m[3] ?? ""}`;
    })
    .join("\n");
}

/**
 * Takes the planner's "- Model:" lines out of a numbered block (R2e i): the plan file gets the steps without them, and
 * each suggestion is kept in the plan's own data instead (an agent reading the file took them for model attribution).
 */
export function takeModels(block: string): {
  block: string;
  models: { number: string | null; title: string; model: string }[];
} {
  const lines = block.split(/\r?\n/);
  const models: { number: string | null; title: string; model: string }[] = [];
  const drop = new Set<number>();
  for (const s of parsePlan(block).steps) {
    if (!s.model) continue;
    models.push({ number: s.number, title: s.title, model: s.model });
    for (let i = s.line + 1; i < s.end && i < lines.length; i++) if (/^\s+- Model:/i.test(lines[i])) drop.add(i);
  }
  return { block: lines.filter((_, i) => !drop.has(i)).join("\n"), models };
}

/** Adds a block of steps as a new section at the end of a task list, keeping its line endings. */
export function appendSection(text: string, title: string, block: string): string {
  const { eol } = splitLines(text);
  const heading = `## ${title.replace(/\s+/g, " ").trim().slice(0, 80) || "Planned work"}`;
  const body = [heading, "", block.trim(), ""].join("\n").replace(/\n/g, eol);
  const base = text.replace(/[\r\n\s]*$/, "");
  return base ? `${base}${eol}${eol}${body}` : body;
}

/** A short section title from a request: its first sentence, cut at a word boundary to at most 60 characters. */
export function sectionTitleFrom(request: string): string {
  const first = (request.split("\n").find((l) => l.trim()) ?? "").trim();
  const sentence = first.split(/(?<=[.!?])\s/)[0].replace(/[.!?]+$/, "");
  if (sentence.length <= 60) return sentence || "Planned work";
  const cut = sentence.slice(0, 60);
  const space = cut.lastIndexOf(" ");
  return `${(space > 30 ? cut.slice(0, space) : cut).replace(/[,;:]$/, "")}…`;
}

/** The text blocks of one agent line of type "assistant" ("" for any other line). */
export function assistantText(msg: { type?: unknown; message?: { content?: unknown } }): string {
  if (msg.type !== "assistant" || !Array.isArray(msg.message?.content)) return "";
  return (msg.message.content as { type?: unknown; text?: unknown }[])
    .filter((b) => b.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("");
}

/**
 * The answer a turn gave: its result, unless a proxy that compresses context (Headroom) left a placeholder in it
 * ("<<ccr:…>>"); then the agent's last text, which holds the real words (PLAN-F 97a-2: the review's reason was only a
 * placeholder, so nobody saw why the step was turned down).
 */
export function finalText(result: string, lastAssistant: string): string {
  return result.includes("<<ccr:") && lastAssistant.trim() ? lastAssistant : result;
}
