import { planData } from "./planData.svelte";
import type { DecidedForYou } from "./preflight";

/**
 * What a plan keeps between runs (24u part 2): the holds from the check before a run and the owner's answers to held
 * steps, so a run started again later holds the same steps (saying who set the hold) and runs the answered ones with
 * the owner's answer. Entries go once their step is done or gone from the plan.
 */

export interface KeptHold {
  /** The step's number, else its title. */
  step: string;
  /** What the hold is about (the check's risk or question). */
  why: string;
  /** "you": the owner chose Wait; "default": Qanuvi's default (a risky step left as it was, a question left open). */
  by: "you" | "default";
  at: number;
}

export interface KeptAnswer {
  step: string;
  choice: "go" | "skip";
  text: string;
  at: number;
}

/**
 * A section's closing step ran while some of its steps were set aside: it runs once more after they are done, in
 * this run or a later one. `late`: those done since, not yet seen by the closing step.
 */
export interface KeptClosing {
  closing: string;
  waiting: string[];
  late: string[];
}

/** A choice Qanuvi made for the owner in the check (24u part 3); `changed`: the owner's later answer. */
export interface KeptDecided extends DecidedForYou {
  changed?: string;
  at: number;
}

/**
 * The model the owner chose for a step in Qanuvi (R2e i): kept in the plan's own data, not written into the plan
 * file (an agent reading the file took "- Model:" lines for model attribution in the project).
 */
export interface KeptModel {
  /** The step's number, else its title. */
  step: string;
  title: string;
  model: string;
}

/** A step the check before a run went through: its key and its text's fingerprint then. */
export interface KeptChecked {
  step: string;
  sig: string;
}

export interface PlanState {
  holds: KeptHold[];
  answers: KeptAnswer[];
  closings: KeptClosing[];
  decided: KeptDecided[];
  /** Steps that count as done only after the decider's review and passing tests (24u part 3). */
  checks: { step: string; why: string }[];
  models: KeptModel[];
  /**
   * The steps the check before a run already went through, with a fingerprint of their text then: a run started
   * again checks only steps that are new or changed since (owner, 2026-11-21).
   */
  checked: KeptChecked[];
  /** The owner folded "Decided for you" when it held this many choices: it stays folded until a new one comes. */
  decidedFolded?: number;
}

export const emptyPlanState = (): PlanState => ({
  holds: [],
  answers: [],
  closings: [],
  decided: [],
  checks: [],
  models: [],
  checked: [],
});

/** Lines a run itself writes under a step (the check's order, notes on work done): they do not change the step. */
const RUN_LINE = /^[-*]?\s*(needs|skipped|done|progress|model|blocked|note)\b[^:]{0,40}:/i;

/**
 * A fingerprint of a step's text (its title and the detail lines the plan's author wrote): a changed step is checked
 * again; lines the run adds (Needs, Progress, Done ...) do not count.
 */
export function stepSig(s: { title: string; details: string[] }): string {
  const own = s.details.map((l) => l.trim()).filter((l) => l && !RUN_LINE.test(l));
  const text = [s.title.trim(), ...own].join("\n");
  let h = 5381;
  for (let i = 0; i < text.length; i++) h = ((h << 5) + h + text.charCodeAt(i)) >>> 0;
  return `${text.length.toString(36)}-${h.toString(36)}`;
}

/** The steps the check went through before, as they were then (new and changed steps are not among them). */
export function alreadyChecked<T extends { number: string | null; title: string; details: string[] }>(
  state: PlanState,
  steps: T[],
): { checked: T[]; fresh: T[] } {
  const seen = new Set(state.checked.map((c) => `${c.step}\u0000${c.sig}`));
  const checked: T[] = [];
  const fresh: T[] = [];
  for (const s of steps) (seen.has(`${stepKey(s)}\u0000${stepSig(s)}`) ? checked : fresh).push(s);
  return { checked, fresh };
}

/** The check went through these steps: kept, so a later start does not ask about them again. */
export function keepChecked(
  state: PlanState,
  steps: { number: string | null; title: string; details: string[] }[],
): PlanState {
  const keys = new Set(steps.map(stepKey));
  const others = state.checked.filter((c) => !keys.has(c.step));
  const now = steps.map((s) => ({ step: stepKey(s), sig: stepSig(s) }));
  return { ...state, checked: [...others, ...now].slice(-500) };
}

/** A step's key in the plan state: its number, else its title. */
export const stepKey = (s: { number: string | null; title: string }): string => s.number ?? s.title;

/** The model the owner chose for the step in Qanuvi, if any. */
export function keptModel(state: PlanState, s: { number: string | null; title: string }): string | null {
  return state.models.find((m) => m.step === stepKey(s))?.model ?? null;
}

/** Sets (or with null, clears) the model the owner chose for the step. */
export function setKeptModel(
  state: PlanState,
  s: { number: string | null; title: string },
  model: string | null,
): PlanState {
  const key = stepKey(s);
  const others = state.models.filter((m) => m.step !== key);
  const value = model?.trim();
  return { ...state, models: value ? [...others, { step: key, title: s.title, model: value }] : others };
}

/**
 * Whether "Decided for you" shows open: while a choice is unchanged, unless the owner folded it; a choice added after
 * the fold opens it again (a new choice is worth a look), a step change or handover never does.
 */
export function decidedOpen(state: PlanState): boolean {
  if (!state.decided.some((d) => !d.changed)) return false;
  return state.decidedFolded === undefined || state.decided.length > state.decidedFolded;
}

/** The owner opened or folded "Decided for you": kept with the plan. */
export function foldDecided(state: PlanState, open: boolean): PlanState {
  const { decidedFolded: _, ...rest } = state;
  return open ? rest : { ...rest, decidedFolded: state.decided.length };
}

function planStateKey(projectId: string, file: string): string {
  return `qanuvi.planstate.${projectId}.${file}`;
}

export function loadPlanState(projectId: string, file: string): PlanState {
  try {
    const v = JSON.parse(planData.getItem(planStateKey(projectId, file)) ?? "null") as Partial<PlanState> | null;
    return {
      holds: Array.isArray(v?.holds) ? v.holds.filter((h) => typeof h?.step === "string") : [],
      answers: Array.isArray(v?.answers) ? v.answers.filter((a) => typeof a?.step === "string") : [],
      closings: Array.isArray(v?.closings)
        ? v.closings.filter((c) => typeof c?.closing === "string" && Array.isArray(c.waiting) && Array.isArray(c.late))
        : [],
      decided: Array.isArray(v?.decided)
        ? v.decided.filter((d) => typeof d?.step === "string" && typeof d.answer === "string")
        : [],
      checks: Array.isArray(v?.checks) ? v.checks.filter((c) => typeof c?.step === "string") : [],
      models: Array.isArray(v?.models)
        ? v.models.filter((m) => typeof m?.step === "string" && typeof m.model === "string" && m.model)
        : [],
      checked: Array.isArray(v?.checked)
        ? v.checked.filter((c) => typeof c?.step === "string" && typeof c.sig === "string")
        : [],
      ...(typeof v?.decidedFolded === "number" ? { decidedFolded: v.decidedFolded } : {}),
    };
  } catch {
    return emptyPlanState();
  }
}

export function savePlanState(projectId: string, file: string, state: PlanState): void {
  try {
    const key = planStateKey(projectId, file);
    if (
      !state.holds.length &&
      !state.answers.length &&
      !state.closings.length &&
      !state.decided.length &&
      !state.checks.length &&
      !state.models?.length &&
      !state.checked?.length
    )
      planData.removeItem(key);
    else planData.setItem(key, JSON.stringify(state));
  } catch {
    // A help only: without it a later run asks again.
  }
}

/** The hold's line, saying who set it. */
export function holdWording(h: Pick<KeptHold, "why" | "by">): string {
  // Qanuvi's own hold says just what is needed (f3: "Qanuvi's default: it needs ..." read oddly).
  return h.by === "you" ? `you asked to see it first (${h.why})` : h.why;
}

/**
 * New holds from a check replace the kept ones of the same steps. A step the owner already answered is not held
 * again: the answer stands.
 */
export function keepHolds(state: PlanState, holds: Omit<KeptHold, "at">[], now = Date.now()): PlanState {
  const answered = new Set(state.answers.map((a) => a.step));
  const fresh = holds.filter((h) => !answered.has(h.step));
  const steps = new Set(fresh.map((h) => h.step));
  return {
    ...state,
    holds: [...state.holds.filter((h) => !steps.has(h.step)), ...fresh.map((h) => ({ ...h, at: now }))],
  };
}

/** The owner answered a held step: the answer is kept and the step's hold goes. */
export function keepAnswer(
  state: PlanState,
  step: string,
  choice: "go" | "skip",
  text: string,
  now = Date.now(),
): PlanState {
  return {
    ...state,
    holds: state.holds.filter((h) => h.step !== step),
    answers: [...state.answers.filter((a) => a.step !== step), { step, choice, text, at: now }],
  };
}

/**
 * A check's decided-for-you choices and extra-check steps: they replace the kept ones of the same steps. A decision
 * the owner already changed stays changed.
 */
export function keepDecided(
  state: PlanState,
  decided: DecidedForYou[],
  checks: { step: string; why: string }[],
  cleared: string[] = [],
  now = Date.now(),
): PlanState {
  const touched = new Set([...decided.map((d) => d.step), ...checks.map((c) => c.step), ...cleared]);
  const changed = state.decided.filter((d) => d.changed);
  const fresh = decided
    .filter((d) => !changed.some((c) => c.step === d.step && c.what === d.what))
    .map((d) => ({ ...d, at: now }));
  return {
    ...state,
    decided: [...state.decided.filter((d) => d.changed || !touched.has(d.step)), ...fresh],
    checks: [...state.checks.filter((c) => !touched.has(c.step)), ...checks],
  };
}

/** The owner changed a decided-for-you choice: kept, so later runs and the panel show their answer. */
export function changeDecided(state: PlanState, step: string, what: string, answer: string): PlanState {
  return {
    ...state,
    decided: state.decided.map((d) => (d.step === step && d.what === what ? { ...d, changed: answer } : d)),
  };
}
