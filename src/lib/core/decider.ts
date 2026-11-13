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
  | "free-decides"
  /** Free models decide and work; the paid agent is not used (its limit is reached, or the owner wants no cost). */
  | "free-only";

export type Priority = "save" | "balanced" | "solve";

export interface TeamSettings {
  mode: TeamMode;
  priority: Priority;
}

export const DEFAULT_TEAM: TeamSettings = { mode: "paid-decides", priority: "balanced" };

export function parseTeam(raw: unknown): TeamSettings {
  const t = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const modes: TeamMode[] = ["paid-decides", "priority", "direct", "free-decides", "free-only"];
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
  /** The paid model the decider picked for the step (pickPrompt), when it named one from the list. */
  model?: string;
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

/** One kind of work's record, for the decider: per model, how it did and what it burned (plan/routing modelProfiles). */
export type Profiles = Record<string, import("$lib/plan/routing").ModelProfile[]>;

function profileLines(profiles: Profiles): string[] {
  const out: string[] = [];
  for (const [type, list] of Object.entries(profiles)) {
    if (!list.length) continue;
    const parts = list.map(
      (p) =>
        `${p.model} ${p.successes}/${p.tries} done, ${Math.round(p.tokensPerSuccess / 1000)}k tokens and $${p.costPerSuccess.toFixed(2)} per finished step${p.heavy ? " HEAVY" : ""}`,
    );
    out.push(`- ${type}: ${parts.join("; ")}`);
  }
  return out;
}

/**
 * The decider's question for a whole plan, one call: the model for each step (and, when free models work first, which
 * steps they take). It sees what each model burned on each kind of work before and keeps heavy models for the steps
 * that really need them.
 */
export function pickPrompt(
  steps: StepBrief[],
  opts: { priority: Priority; freeFirst: boolean; models: { id: string; label: string }[]; profiles: Profiles },
): string {
  const list = steps
    .map(
      (s) =>
        `${s.number}. ${s.title} [${s.type}]${s.details.length ? `\n   ${s.details.slice(0, 4).join("\n   ")}` : ""}`,
    )
    .join("\n");
  const record = profileLines(opts.profiles);
  return [
    "You are the decider in a coding team. Pick, for each step of this plan, the model that does it.",
    ...(opts.freeFirst
      ? [
          'Free models (capable but weaker, may break tool calls) can take routine steps: worker "free". ' +
            LEAN[opts.priority],
        ]
      : []),
    "Paid models, strongest first:",
    ...opts.models.map((m) => `- ${m.id} (${m.label})`),
    "High-quality code comes first, saving tokens second. For each step pick the paid model that will do it well; only among models that would do it equally well, pick the lighter one. Never pick a weaker model to save tokens on a step that needs real judgement (money logic, security, tricky bugs, changes across many files).",
    ...(record.length
      ? [
          "What earlier runs showed, per kind of work (steps done of tried, tokens and cost per finished step). A model marked HEAVY burned far more tokens than another model that also finished that kind of work: pick it only when the step truly needs what it does better, and say why in the reason. A model that often did not finish a kind of work is a quality risk there, whatever it costs.",
          ...record,
        ]
      : []),
    "Money, payments, security and authentication work always gets a paid model.",
    "Answer with JSON only, in this shape:",
    `{"steps":[{"number":"1","worker":"${opts.freeFirst ? "free" : "paid"}","model":"<model id>","why":"a short reason"}]}`,
    "",
    "Steps:",
    list,
  ].join("\n");
}

/**
 * Reads the decider's picks. A model not in the list is dropped (the step then follows the rules); without free models
 * first every step is paid; steps left out get parseAssignment's safe default.
 */
export function parsePicks(
  answer: string,
  steps: StepBrief[],
  opts: { priority: Priority; freeFirst: boolean; modelIds: string[] },
): Assignment[] {
  const v = jsonIn(answer) as { steps?: unknown } | null;
  const models = new Map<string, string>();
  if (v && Array.isArray(v.steps))
    for (const s of v.steps as Record<string, unknown>[]) {
      const number = String(s?.number ?? "").trim();
      const model = String(s?.model ?? "").trim();
      if (number && opts.modelIds.includes(model)) models.set(number, model);
    }
  return parseAssignment(answer, steps, opts.priority).map((a) => {
    const model = models.get(a.number);
    const worker = opts.freeFirst ? a.worker : "paid";
    return { ...a, worker, ...(model ? { model } : {}) };
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
  kind: "assign" | "review" | "chat-model" | "handover" | "set-aside";
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

/** The word the paid agent answers with when the free pick is too weak for the message (24e). */
export const NEEDS_STRONGER = "NEEDS_STRONGER_MODEL";

/**
 * Free decides, paid works, with the paid agent on top (24e): the note added to the message, so the paid agent sees
 * the free model's pick and can overrule it before doing the work.
 */
export const OVERRULE_MARK = "\n\n(Qanuvi: a free model chose";

export function overruleNote(model: string, why: string): string {
  return [
    "",
    "",
    `(Qanuvi: a free model chose ${model} for this message${why ? ` because: ${why}` : ""}. If this message needs a`,
    `stronger model to be done well, answer only "${NEEDS_STRONGER}: <the reason>" and do nothing else.)`,
  ].join("\n");
}

/** What the user wrote, without the note Qanuvi added for the paid agent. */
export function withoutOverruleNote(text: string): string {
  const i = text.indexOf(OVERRULE_MARK);
  return i >= 0 ? text.slice(0, i) : text;
}

/** The paid agent's overrule, if its answer is one: the reason, else null. */
export function overruleIn(answer: string): string | null {
  const m = answer.trim().match(new RegExp(`^${NEEDS_STRONGER}\\s*:?\\s*(.*)`, "s"));
  return m ? m[1].trim() || "no reason given" : null;
}

/** One model up in a list ordered strongest first; the strongest stays itself. */
export function strongerModel(models: { id: string }[], current: string): string {
  const i = models.findIndex((m) => m.id === current);
  return i > 0 ? models[i - 1].id : (models[0]?.id ?? current);
}
