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

export interface PlanState {
  holds: KeptHold[];
  answers: KeptAnswer[];
}

export const emptyPlanState = (): PlanState => ({ holds: [], answers: [] });

function planStateKey(projectId: string, file: string): string {
  return `qanuvi.planstate.${projectId}.${file}`;
}

export function loadPlanState(projectId: string, file: string): PlanState {
  try {
    const v = JSON.parse(localStorage.getItem(planStateKey(projectId, file)) ?? "null") as Partial<PlanState> | null;
    return {
      holds: Array.isArray(v?.holds) ? v.holds.filter((h) => typeof h?.step === "string") : [],
      answers: Array.isArray(v?.answers) ? v.answers.filter((a) => typeof a?.step === "string") : [],
    };
  } catch {
    return emptyPlanState();
  }
}

export function savePlanState(projectId: string, file: string, state: PlanState): void {
  try {
    const key = planStateKey(projectId, file);
    if (!state.holds.length && !state.answers.length) localStorage.removeItem(key);
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
    holds: [...state.holds.filter((h) => !steps.has(h.step)), ...fresh.map((h) => ({ ...h, at: now }))],
    answers: state.answers,
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
    holds: state.holds.filter((h) => h.step !== step),
    answers: [...state.answers.filter((a) => a.step !== step), { step, choice, text, at: now }],
  };
}

/** Drops what belongs to steps no longer open (done, or gone from the plan). */
export function pruneState(state: PlanState, open: Set<string>): PlanState {
  return {
    holds: state.holds.filter((h) => open.has(h.step)),
    answers: state.answers.filter((a) => open.has(a.step)),
  };
}
