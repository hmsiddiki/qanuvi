/**
 * Qanuvi core's decision layer (24b). One model decides — what kind of job, which worker does it, whether a result is
 * good enough — and the others work. Plain functions (prompts, parsing, fallbacks) so they can be tested; the stores
 * call the models.
 *
 * Built to grow: the decider and the workers are described as plain "seats", so later a personal model (25) or another
 * paid agent (GitHub Copilot) takes a seat without the rest changing. Every decision is kept as a record with its
 * reason, which is what a personal model will learn from.
 */
import { jsonIn } from "$lib/agents/guides";

/** How the team works (the owner's design, 2026-11-11). */
export type TeamMode =
  /** The paid model decides; free models work; the paid one does what they could not, or steps in on trouble. */
  | "paid-decides"
  /** Like paid-decides, leaning by `priority`: save tokens / balanced / solve problems first. */
  | "priority"
  /** Only the paid model works; Qanuvi core gives it context. */
  | "direct"
  /** A free model decides what to send to the paid one (which paid model), so it gets fewer, better requests. */
  | "free-decides";

export type Priority = "save" | "balanced" | "solve";

export interface TeamSettings {
  mode: TeamMode;
  priority: Priority;
}

export const DEFAULT_TEAM: TeamSettings = { mode: "paid-decides", priority: "balanced" };

export function parseTeam(raw: unknown): TeamSettings {
  const t = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const modes: TeamMode[] = ["paid-decides", "priority", "direct", "free-decides"];
  const prios: Priority[] = ["save", "balanced", "solve"];
  return {
    mode: modes.includes(t.mode as TeamMode) ? (t.mode as TeamMode) : DEFAULT_TEAM.mode,
    priority: prios.includes(t.priority as Priority) ? (t.priority as Priority) : DEFAULT_TEAM.priority,
  };
}

/** A seat at the table: who decides or works. */
export type Seat =
  { kind: "paid"; agent: string; model: string } | { kind: "free"; models: string[] } | { kind: "none" };

/**
 * Who decides: any paid agent must; with none, Qanuvi core's free models, best first, one after another (the next
 * takes over when one is unavailable). Local models decide last: they are the weakest judges.
 */
export function deciderSeat(
  paid: { agent: string; model: string } | null,
  freeModels: { id: string; local: boolean }[],
): Seat {
  if (paid) return { kind: "paid", ...paid };
  const online = freeModels.filter((m) => !m.local).map((m) => m.id);
  const local = freeModels.filter((m) => m.local).map((m) => m.id);
  const models = [...online, ...local];
  return models.length ? { kind: "free", models } : { kind: "none" };
}

/** Whether free models get work at all in this mode (Direct: only the paid model works). */
export function freeWorks(team: TeamSettings, hasFree: boolean, hasPaid: boolean): boolean {
  if (!hasFree) return false;
  if (!hasPaid) return true;
  return team.mode === "paid-decides" || team.mode === "priority";
}

export interface StepBrief {
  number: string;
  title: string;
  details: string[];
  /** The kind of work (docs, tests, ui, security, bugfix, refactor, feature). */
  type: string;
}

export interface Assignment {
  number: string;
  worker: "free" | "paid";
  why: string;
}

const LEAN: Record<Priority, string> = {
  save: "Save the paid model's tokens: give free models every step they can plausibly do.",
  balanced: "Balance: free models for routine steps, the paid model for steps that need real judgement.",
  solve:
    "Solving the problem comes first: give the paid model every step that is not routine; free models only for plain, low-risk steps.",
};

/** The decider's question for a whole plan, one call: which steps free models can do and which need the paid model. */
export function assignPrompt(steps: StepBrief[], priority: Priority): string {
  const list = steps
    .map(
      (s) =>
        `${s.number}. ${s.title} [${s.type}]${s.details.length ? `\n   ${s.details.slice(0, 4).join("\n   ")}` : ""}`,
    )
    .join("\n");
  return [
    "You are the decider in a coding team. Free models (capable but weaker, may break tool calls) and one paid model (strong) can do each step of this plan.",
    LEAN[priority],
    "Money, payments, security and authentication work always goes to the paid model.",
    "Look at the steps, then answer with JSON only, in this shape:",
    '{"steps":[{"number":"1","worker":"free","why":"a short reason"}]}',
    "",
    "Steps:",
    list,
  ].join("\n");
}

/**
 * Reads the decider's answer. Steps it left out, or answered in a way that cannot be used, get the safe default:
 * security work → paid; otherwise free when saving, paid when solving, by kind when balanced.
 */
export function parseAssignment(answer: string, steps: StepBrief[], priority: Priority): Assignment[] {
  const v = jsonIn(answer) as { steps?: unknown } | null;
  const given = new Map<string, Assignment>();
  if (v && Array.isArray(v.steps))
    for (const s of v.steps as Record<string, unknown>[]) {
      const number = String(s?.number ?? "").trim();
      const worker = s?.worker === "free" || s?.worker === "paid" ? s.worker : null;
      if (number && worker) given.set(number, { number, worker, why: String(s?.why ?? "").slice(0, 200) });
    }
  return steps.map((s) => {
    if (s.type === "security") return { number: s.number, worker: "paid", why: "money or security work" };
    return (
      given.get(s.number) ?? {
        number: s.number,
        worker: fallbackWorker(s, priority),
        why: "Qanuvi core's rule (the decider gave no answer for it)",
      }
    );
  });
}

function fallbackWorker(s: StepBrief, priority: Priority): "free" | "paid" {
  if (priority === "save") return "free";
  if (priority === "solve") return s.type === "docs" ? "free" : "paid";
  return ["docs", "tests", "ui", "refactor"].includes(s.type) ? "free" : "paid";
}

/** The decider's look at a free model's change before it is accepted (quality first). */
export function reviewPrompt(step: { title: string; details: string[] }, diff: string, tests: string): string {
  const shown = diff.length > 12_000 ? `${diff.slice(0, 12_000)}\n… (cut)` : diff;
  return [
    "You are reviewing a change a free model made for one step of a plan. Is it correct and complete for the step,",
    "without breaking anything around it? Be strict but fair; style alone is not a reason to reject.",
    'Answer with JSON only: {"ok": true, "why": "a short reason"} or {"ok": false, "why": "what is wrong"}',
    "",
    `Step: ${step.title}`,
    ...step.details.slice(0, 6).map((d) => `- ${d}`),
    "",
    tests ? `Tests: ${tests}` : "Tests: none were run.",
    "",
    "The change (git diff):",
    shown || "(no change)",
  ].join("\n");
}

/** Reads a review. Anything unreadable counts as not accepted: quality first. */
export function parseReview(answer: string): { ok: boolean; why: string } {
  const v = jsonIn(answer) as { ok?: unknown; why?: unknown } | null;
  if (!v || typeof v.ok !== "boolean") return { ok: false, why: "the review could not be read" };
  return { ok: v.ok, why: String(v.why ?? "").slice(0, 300) };
}

/** Free decides, paid works (chat): which paid model a message needs. */
export function chatModelPrompt(message: string, models: { id: string; label: string }[]): string {
  return [
    "Pick the paid model for this coding request: the cheapest one that will do it well. Stronger models first in the list.",
    models.map((m) => `- ${m.id} (${m.label})`).join("\n"),
    'Answer with JSON only: {"model": "<id>", "why": "a short reason"}',
    "",
    "Request:",
    message.slice(0, 4000),
  ].join("\n");
}

export function parseChatModel(answer: string, models: { id: string }[]): { model: string; why: string } | null {
  const v = jsonIn(answer) as { model?: unknown; why?: unknown } | null;
  const model = typeof v?.model === "string" ? v.model.trim() : "";
  if (!models.some((m) => m.id === model)) return null;
  return { model, why: String(v?.why ?? "").slice(0, 200) };
}

/** A decision kept for later (shown in the run, and what a personal model will learn from). */
export interface DecisionRecord {
  at: number;
  kind: "assign" | "review" | "chat-model" | "handover";
  by: string;
  subject: string;
  decision: string;
  why: string;
}

/**
 * What one step changed in a file: the lines removed (-) and added (+) between two versions, with no context lines.
 * The review sees only this step's work, not earlier steps' changes that are not committed yet. Very long files
 * return null (the caller uses the file's whole diff instead).
 */
export function stepDiff(path: string, before: string, after: string): string | null {
  const a = before.split("\n");
  const b = after.split("\n");
  if (a.length * b.length > 4_000_000) return null;
  // Longest common subsequence, then walk it.
  const n = a.length;
  const m = b.length;
  const lcs: Uint32Array[] = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1));
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--)
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: string[] = [];
  let i = 0;
  let j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && a[i] === b[j]) {
      i++;
      j++;
    } else if (j < m && (i >= n || lcs[i][j + 1] >= lcs[i + 1][j])) out.push(`+${b[j++]}`);
    else out.push(`-${a[i++]}`);
  }
  return out.length ? `--- ${path} (before this step)\n+++ ${path} (after)\n${out.join("\n")}` : "";
}
