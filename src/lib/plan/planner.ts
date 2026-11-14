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

export function replanPrompt(file: string, step: Step, failure: string, models: AgentModel[]): string {
  const ids = models.map((m) => m.id).join(", ");
  return [
    `A step in ${file} was not finished, even by the strongest model. Rewrite it into 2 to 5 smaller steps that together do the same work, using what went wrong. Read the code as you need (read-only); answer with the steps only, inside one \`\`\`tasklist block, in the same format as the task list (with Validate and Model lines; models: ${ids}).`,
    "",
    "The step:",
    `**${step.number ? `${step.number}. ` : ""}${step.title}**${step.note ? ` ${step.note}` : ""}`,
    ...step.details.map((d) => `  ${d}`),
    "",
    "What went wrong:",
    failure.trim() || "(no details)",
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
 * Numbers for `count` replacement steps of the step `number` that no step in the plan or its archive has (PLAN-F,
 * 2026-11-14: 97a's replacements were numbered 97a–97d next to the real 97b–97d and everything waited on 97a).
 * "12" → 12a, 12b, … (letters in use skipped; past z, 12-1, 12-2, …); "97a" → 97a-1, 97a-2, …; "97a-2" (itself a
 * replacement) → the next free 97a-n; no number → after the highest whole number.
 */
export function freeLabels(text: string, archive: string, number: string | null, count: number): string[] {
  const used = new Set(
    [...parsePlan(text).steps, ...parsePlan(archive).steps].map((s) => s.number).filter(Boolean) as string[],
  );
  const out: string[] = [];
  const take = (l: string) => {
    if (used.has(l) || out.includes(l)) return;
    out.push(l);
  };
  if (!number) {
    let n = Math.max(lastNumber(text), lastNumber(archive)) + 1;
    while (out.length < count) take(String(n++));
    return out;
  }
  if (/^\d+$/.test(number)) {
    for (let c = 0; c < 26 && out.length < count; c++) take(`${number}${String.fromCharCode(97 + c)}`);
    for (let k = 1; out.length < count; k++) take(`${number}-${k}`);
    return out;
  }
  const base = number.replace(/-\d+$/, "");
  for (let k = 1; out.length < count; k++) take(`${base}-${k}`);
  return out;
}

/** Gives the steps of a block the given numbers in order, dropping any number or label a planner wrote before a
 * title ("97a1. Make …", "97a-2a. Check …" → "Make …", "Check …"). Steps past the end of `labels` keep no number.
 * The block's Needs lines follow: a planner's own label there becomes the step's new number (PLAN-F 97a: "Needs:
 * 97a-2a" was left pointing at no step, and every step after it was skipped). */
export function renumberWith(block: string, labels: string[]): string {
  let i = 0;
  const renamed = new Map<string, string>();
  // Every leading label goes ("97b. 97a2. Schema" → "Schema"); each one is remembered for the Needs lines.
  const strip = (t: string, label: string | undefined) => {
    let s = t.trim();
    for (let prev = ""; prev !== s;) {
      prev = s;
      const m = /^(\d+[a-z0-9]*(?:-\d+[a-z0-9]*)*)\.\s+/.exec(s);
      if (m) {
        if (label && !renamed.has(m[1])) renamed.set(m[1], label);
        s = s.slice(m[0].length);
      }
    }
    return s.trim();
  };
  const lines = block.split("\n").map((l) => {
    const m = /^(- \[ \] )(?:\*\*(.+?)\*\*(.*)|(.+))$/.exec(l);
    if (!m) return l;
    const label = labels[i++];
    const title = strip((m[2] ?? m[4]).trim(), label);
    return `${m[1]}**${label ? `${label}. ` : ""}${title}**${m[3] ?? ""}`;
  });
  return lines
    .map((l) => {
      const m = /^(\s*- Needs:\s*)(.*)$/.exec(l);
      if (!m || !renamed.size) return l;
      const needs = m[2].split(",").map((n) => {
        const t = n.trim();
        return renamed.get(t) ?? t;
      });
      return `${m[1]}${needs.join(", ")}`;
    })
    .join("\n");
}

/** Adds a block of steps as a new section at the end of a task list, keeping its line endings. */
export function appendSection(text: string, title: string, block: string): string {
  const { eol } = splitLines(text);
  const heading = `## ${title.replace(/\s+/g, " ").trim().slice(0, 80) || "Planned work"}`;
  const body = [heading, "", block.trim(), ""].join("\n").replace(/\n/g, eol);
  const base = text.replace(/[\r\n\s]*$/, "");
  return base ? `${base}${eol}${eol}${body}` : body;
}

/**
 * Marks a failed step blocked with a note pointing at its replacement, and puts the replacement steps right after it
 * (after its details). The step is found by number and title, so other edits in the file do not matter.
 */
export function replaceStep(text: string, step: Pick<Step, "number" | "title">, block: string, labels: string): string {
  const plan = parsePlan(text);
  const found = plan.steps.find((s) => s.number === step.number && s.title === step.title);
  if (!found) throw new Error("The step is no longer in the task list.");
  const { lines, eol } = splitLines(text);
  lines[found.line] = lines[found.line].replace(/^( ?- \[)[ xX!-](\])/, "$1!$2");
  const note = `  - Replaced by steps ${labels}, rewritten by the planner after it failed on every model.`;
  const insertAt = found.end;
  lines.splice(insertAt, 0, note, ...block.trim().split("\n"));
  // The note belongs to the failed step: it goes right after its details, before the new steps.
  return lines.join(eol);
}

/** "12a–12c" for a block numbered from base 12 with three steps. */
export function labelRange(block: string): string {
  const nums = parsePlan(block)
    .steps.map((s) => s.number)
    .filter(Boolean) as string[];
  if (nums.length === 0) return "";
  return nums.length === 1 ? nums[0] : `${nums[0]}–${nums[nums.length - 1]}`;
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
