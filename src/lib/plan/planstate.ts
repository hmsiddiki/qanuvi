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

export interface PlanState {
  holds: KeptHold[];
  answers: KeptAnswer[];
  closings: KeptClosing[];
  decided: KeptDecided[];
  /** Steps that count as done only after the decider's review and passing tests (24u part 3). */
  checks: { step: string; why: string }[];
}

export const emptyPlanState = (): PlanState => ({ holds: [], answers: [], closings: [], decided: [], checks: [] });

function planStateKey(projectId: string, file: string): string {
  return `qanuvi.planstate.${projectId}.${file}`;
}

export function loadPlanState(projectId: string, file: string): PlanState {
  try {
    const v = JSON.parse(localStorage.getItem(planStateKey(projectId, file)) ?? "null") as Partial<PlanState> | null;
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
      !state.checks.length
    )
      localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(state));
  } catch {
    // A help only: without it a later run asks again.
  }
}

/** The hold's line, saying who set it. */
export function holdWording(h: Pick<KeptHold, "why" | "by">): string {
  return h.by === "you" ? `you asked to see it first (${h.why})` : `Qanuvi's default: ${h.why}`;
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
 * Drops what belongs to steps no longer open (done, or gone from the plan); a closing record stays while its closing
 * step is in the plan and something is still waiting or done late.
 */
export function pruneState(state: PlanState, open: Set<string>, all: Set<string> = open): PlanState {
  return {
    holds: state.holds.filter((h) => open.has(h.step)),
    answers: state.answers.filter((a) => open.has(a.step)),
    // Decided choices stay while their step is in the plan (the owner may still want to see or change them).
    decided: state.decided.filter((d) => all.has(d.step)),
    checks: state.checks.filter((c) => open.has(c.step)),
    closings: state.closings
      .filter((c) => all.has(c.closing))
      // A waited-for step done meanwhile (by hand, or outside a run) counts as late; one gone from the plan drops.
      .map((c) => ({
        ...c,
        waiting: c.waiting.filter((w) => open.has(w)),
        late: [...c.late, ...c.waiting.filter((w) => all.has(w) && !open.has(w) && !c.late.includes(w))],
      }))
      .filter((c) => c.waiting.length || c.late.length),
  };
}

/** A closing step finished: what of its section is still set aside is what it waits for now; late ones are seen. */
export function closingRan(state: PlanState, closing: string, waiting: string[]): PlanState {
  const rest = state.closings.filter((c) => c.closing !== closing);
  return { ...state, closings: waiting.length ? [...rest, { closing, waiting, late: [] }] : rest };
}

/** A step finished: closing steps that ran without it note it as late. */
export function markLate(state: PlanState, step: string): PlanState {
  return {
    ...state,
    closings: state.closings.map((c) =>
      c.waiting.includes(step)
        ? { ...c, waiting: c.waiting.filter((w) => w !== step), late: [...c.late.filter((l) => l !== step), step] }
        : c,
    ),
  };
}

/**
 * The closing steps to run again now: something came in late and nothing they wait for is still pending (once for
 * the whole section, not once per late step). `ending`: the run is about to end, so pending steps no longer count.
 */
export function closingsDue(state: PlanState, pending: (step: string) => boolean, ending: boolean): KeptClosing[] {
  return state.closings.filter((c) => c.late.length && (ending || !c.waiting.some(pending)));
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
