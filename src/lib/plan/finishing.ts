/**
 * The finishing pass at the end of a plan run (24o). The steps followed their own instructions; one smarter pass then
 * looks at the whole. Light: the project's full checks and a review of the whole run's diff. Full: also a session
 * that fixes what was found, runs live tests where the project defines them (scratch copies only) and checks the task
 * list's Done notes against what was built. Both end with a short "still open" list. The pass itself runs in the Rust
 * core (src-tauri/src/plan/); this file keeps the types and the estimate the Run panel shows.
 */
import type { PlanRun, StepRun } from "./runner";

export type FinishLevel = "light" | "full";
/** What a run does at its end: offer the pass, or run one by itself (a plan's run setting). */
export type FinishAuto = "ask" | FinishLevel;

export interface FinishPass {
  level: FinishLevel;
  status: "running" | "done" | "failed" | "stopped";
  /** The decider's model the pass ran on. */
  model: string;
  startedAt: number;
  endedAt: number | null;
  costUsd: number;
  seconds: number;
  /** The project's full checks, when it has a test command. */
  checks: { command: string; ok: boolean; summary: string } | null;
  /** What the review found (light), or what the full pass found and did. */
  findings: string[];
  /** Left for the owner. */
  stillOpen: string[];
  /** The full pass's session (to open it). */
  sessionId?: string;
  note?: string;
}

/** The key the pass's cost has in the run's lines per model. */
export const FINISHER = "finish:";

/**
 * Prices to turn the steps' costs into the pass model's: the pass model's price and each model's (any one price per
 * model works, e.g. output per million tokens). A model with no known price keeps its cost as it is.
 */
export interface PassPrices {
  pass: number;
  byModel: Record<string, number>;
}

/** A finished step's cost and time in this run (all its attempts), its cost scaled to the pass model when priced. */
function stepCost(s: StepRun, prices?: PassPrices): { cost: number; seconds: number } {
  return s.attempts.reduce(
    (t, a) => {
      const own = prices?.byModel[a.model];
      const scale = prices && prices.pass > 0 && own && own > 0 ? prices.pass / own : 1;
      return { cost: t.cost + a.costUsd * scale, seconds: t.seconds + a.seconds };
    },
    { cost: 0, seconds: 0 },
  );
}

function median(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

/**
 * About what the pass costs and how long it takes, from the run's own finished steps: a light pass is about half a
 * typical step (one review of the whole diff, plus the checks), a full one about 1.5 typical steps, more for long runs.
 * Cost is null when no finished step was paid for (free steps tell nothing about the paid decider's price). With
 * `prices`, each step's cost is scaled to the pass model's price (cheap steps say little about a pass on a dear model).
 */
export function finishEstimate(
  run: Pick<PlanRun, "steps">,
  level: FinishLevel,
  prices?: PassPrices,
): { costUsd: number | null; seconds: number | null } {
  const done = run.steps.filter((s) => s.status === "done" && s.attempts.length).map((s) => stepCost(s, prices));
  if (!done.length) return { costUsd: null, seconds: null };
  const paid = done.filter((d) => d.cost > 0).map((d) => d.cost);
  const factor = level === "light" ? 0.5 : Math.min(4, 1.5 + 0.1 * done.length);
  return {
    costUsd: paid.length ? median(paid) * factor : null,
    seconds: median(done.map((d) => d.seconds)) * factor,
  };
}

/** Whether a finished run gets the offer: it ended done, some step got done in it, and no pass ran yet. */
export function offersFinish(run: Pick<PlanRun, "status" | "steps"> & { finish?: FinishPass }): boolean {
  return run.status === "done" && !run.finish && run.steps.some((s) => s.status === "done" && s.attempts.length > 0);
}
