/**
 * A plan run as the window sees it: the run record's types and the helpers the Run panel shows with (totals, savings,
 * step ranges). The run itself, with its decisions, is in the Rust core (src-tauri/src/plan/); stores/runner.svelte.ts
 * only starts it and follows its updates.
 */
import type { AgentModel } from "$lib/agents/providers";
import { FINISHER } from "./finishing";

export interface RunOptions {
  /** Line of the first step to run; earlier steps are skipped. */
  fromLine: number;
  /** Run only that one step. */
  onlyOne: boolean;
  /** Permission mode for the run's sessions. */
  mode: string;
  /** Stop when the run has cost this much (US dollars); 0 means no limit. */
  budgetUsd: number;
  /** Failed attempts on one model before the step moves to a stronger model. */
  attemptsPerModel: number;
  /** Sessions without MCP servers and with only the file and command tools: cheaper, and cached between steps. */
  lean: boolean;
  /** Give each step's session Qanuvi's code-index tools. */
  indexTools: boolean;
  /** Work packets: hand each step the code it mentions, and let Qanuvi run the tests and send back only failures. */
  packets: boolean;
  /** When a step fails even on the strongest model, ask the planner once to rewrite it into smaller steps. */
  replan: boolean;
  /**
   * Free models first: each step (money and security work excepted) first goes to Qanuvi's own agent with the free and
   * local models Qanuvi core picks; the paid agent takes over only when that attempt does not finish the step.
   */
  freeFirst?: boolean;
  /** Which steps by number, for example "2-6, 10-15"; empty: all open steps from `fromLine` (24f). */
  ranges?: string;
  /** The agent the run uses; missing: the one Qanuvi picks (a paid agent on top, else the chosen one). */
  providerId?: string;
  /** One model for every step, whatever the steps' own Model lines say; empty or missing: as the plan says. */
  model?: string;
  /** A step that keeps failing moves to a stronger model; missing: yes. */
  strongerOnFail?: boolean;
  /** The Team mode for this run (the same choices as Settings, Team); missing: the Settings choice. */
  teamMode?: import("$lib/core/decider").TeamMode;
  /** A step may cost at most this much (US dollars) and run this many minutes; 0 or missing: no limit. */
  stepCostLimit?: number;
  stepMinutes?: number;
  /** When the paid agent hits its usage limit: wait for the reset and go on, or let free models work until then. */
  onLimit?: "wait" | "free";
  /** Steps the run pauses before (risky ones the owner wants to see, or a question left open), with why (24i). */
  waitBefore?: { step: string; why: string }[];
  /**
   * Steps that go ahead with extra checks (24u part 3): the decider reviews the step's diff and the full tests must
   * pass before it counts as done, whoever did the work.
   */
  extraChecks?: { step: string; why: string }[];
  /** When every step of the plan is done, its private details (logins) are deleted; missing: yes. */
  forgetPrivateWhenDone?: boolean;
  /** Steps in a row on the same model and in the same section continue one session; missing: yes. */
  carryOn?: boolean;
  /** A session also goes on into the next section (same model, still under the limit); missing: yes. */
  carryAcrossSections?: boolean;
  /** The full test suite after every step, or once at the end of each section (steps test what they changed). */
  fullChecks?: "step" | "section";
  /** Past this many tokens of context the next step starts a new session (with the run's notes). */
  carryLimit?: number;
  /** Past this many tokens of context a step in the middle of its work hands over to a new session; 0 = never. */
  handoverLimit?: number;
  /** Who works: the Team settings, the paid agent only, or free models first (then the paid agent). */
  team?: RunTeam;
  /** When the run stops for the user (24f); missing: "decisions", as before. */
  pause?: PauseMode;
  /** At the end of a run that got done: offer the finishing pass, or run it by itself (24o); missing: offer it. */
  finishPass?: import("./finishing").FinishAuto;
}

/**
 * When a run stops for the user (24f):
 * - "never": runs to the end; a blocked step, or one no model could finish, is skipped with its reason.
 * - "decisions": stops at a blocked step or one no model could finish.
 * - "step": stops after every step too.
 */
export type PauseMode = "never" | "decisions" | "step";

/** The "model" of an attempt on Qanuvi's own agent (Qanuvi core picks among the free and local models). */
export const FREE_MODEL = "qanuvi:auto";

export function isFree(model: string): boolean {
  return model.startsWith("qanuvi:");
}

export interface Attempt {
  model: string;
  sessionId: string;
  outcome: "done" | "open" | "blocked" | "error" | "stopped";
  costUsd: number;
  inputTokens: number;
  outputTokens: number;
  seconds: number;
  note: string;
  /** How often Qanuvi ran the tests in this attempt (work packets). */
  testRuns?: number;
  /** Whether the last test run passed; null when Qanuvi did not run them. */
  testsPassed?: boolean | null;
  /** What its tool calls were for: reading, changing files, running commands. */
  tools?: import("./costs").ToolKinds;
  /** The last test failures sent to the agent (for the planner when the step is rewritten). */
  failure?: string;
}

export interface StepRun {
  line: number;
  number: string | null;
  title: string;
  attempts: Attempt[];
  /**
   * held: set aside, waiting for the owner's answer (24u); set-aside: waits for a held step (`waitsFor`). Both come
   * back as "waiting" once the owner answers.
   */
  status: "waiting" | "running" | "done" | "blocked" | "failed" | "skipped" | "held" | "set-aside";
  /** Why a held step waits for the owner: a hold from the check before the run, blocked, or not finished. */
  held?: { kind: import("./setaside").HeldKind; why: string; card?: import("./holdask").HoldCard };
  /** n1: what the owner was asked about this step and what they said, in order. */
  talk?: { asked: string; answer: string }[];
  /** The held step (number, else title) a set-aside step needs, and why it needs it. */
  waitsFor?: { step: string; why: string };
  /** The owner's answer to a held step: the next try is told it. */
  answer?: string;
  /** Tries before this index were before the owner's answer: the model ladder starts again after it. */
  retryFrom?: number;
  /** A closing step opened again: the set-aside steps that finished after it first ran (24u). */
  rerunAfter?: string[];
  /** Kind of work, for learned routing (Milestone 13). */
  jobType?: import("./routing").JobType;
  /** Why the step started on its model. */
  choice?: string;
  /** The planner rewrote this step into the steps named here. */
  replacedBy?: string;
  /** This step is one of the planner's replacements for the step with this number (or title). */
  replaces?: string;
  /** Files the step changed (modified while it ran). */
  changed?: string[];
  /** Why the run skipped, or could not finish, the step (24f). */
  reason?: string;
}

export interface PlanRun {
  id: string;
  projectId: string;
  file: string;
  provider: string;
  options: RunOptions;
  steps: StepRun[];
  status: "running" | "done" | "stopped" | "failed" | "needs-you" | "paused";
  message: string;
  /** The run stopped because it reached its own budget (this many USD): the plan says so and offers Raise budget. */
  budgetReached?: number;
  /** Why a run that was going stopped with its master (M5b): "shutdown", "crash", "update", "quit", ... */
  stoppedBy?: string;
  startedAt: number;
  endedAt: number | null;
  /** Paused in the middle of a step: its session, so Resume goes on in it (also after a restart). */
  pausedSession?: { sessionId: string; model: string; section: string; number: string | null };
  /** What the decider decided in this run, with its reasons (24b). */
  decisions?: import("$lib/core/decider").DecisionRecord[];
  /** What the owner said during the run and what steps left for later ones: every later step gets them. */
  notes?: string[];
  /** The decider's own calls (assignment, reviews): they count in the run's cost (24e). */
  deciderCalls?: { model: string; costUsd: number }[];
  /** Qanuvi's snapshot of the project when the run started (its id and followed folders), kept with the record so
   * the finishing pass still sees only the run's own changes after a restart. */
  snapshot?: { id: string; also: string[] };
  /** The finishing pass over the whole run (24o), once it ran or runs. */
  finish?: import("./finishing").FinishPass;
}

export type RunTeam = "settings" | "paid" | "free" | "free-only";

/**
 * Roughly how much one model on every step costs against the plan's own models (by output price; tokens per step
 * taken as alike). Null when a price is unknown.
 */
export function costRatio(models: AgentModel[], stepModels: string[], one: string): number | null {
  const out = (id: string) => models.find((m) => m.id === id)?.price?.[1];
  const target = out(one);
  if (target === undefined || stepModels.length === 0) return null;
  let own = 0;
  for (const id of stepModels) {
    const p = out(id);
    if (p === undefined) return null;
    own += p;
  }
  return own > 0 ? (target * stepModels.length) / own : null;
}

/** The run lines' key for the decider's calls on a model. */
export const DECIDER = "decider:";

/** Reads step ranges such as "2-6, 10-15, 20". Unreadable parts are reported, not guessed. */
export function parseRanges(text: string | undefined): { ranges: [number, number][]; error: string } {
  const ranges: [number, number][] = [];
  const bad: string[] = [];
  for (const part of (text ?? "")
    .split(/[,;]+/)
    .map((p) => p.trim())
    .filter(Boolean)) {
    const m = /^(\d+)\s*(?:-|–|to)\s*(\d+)$/i.exec(part) ?? /^(\d+)$/.exec(part);
    if (!m) {
      bad.push(part);
      continue;
    }
    const a = Number(m[1]);
    const b = Number(m[2] ?? m[1]);
    ranges.push([Math.min(a, b), Math.max(a, b)]);
  }
  return { ranges, error: bad.length ? `Not a step range: ${bad.join(", ")}` : "" };
}

/** Whether a step number (such as "12" or "12b") falls in the ranges. Steps without a number never do. */
export function inRanges(number: string | null, ranges: [number, number][]): boolean {
  const n = number ? parseInt(number, 10) : NaN;
  return !Number.isNaN(n) && ranges.some(([a, b]) => n >= a && n <= b);
}

/** Totals for a run, per model and overall. */
export function runTotals(run: Pick<PlanRun, "steps"> & Pick<Partial<PlanRun>, "deciderCalls" | "finish">) {
  const byModel = new Map<string, { attempts: number; costUsd: number; inputTokens: number; outputTokens: number }>();
  let costUsd = 0;
  let inputTokens = 0;
  let outputTokens = 0;
  let seconds = 0;
  for (const s of run.steps)
    for (const a of s.attempts) {
      costUsd += a.costUsd;
      inputTokens += a.inputTokens;
      outputTokens += a.outputTokens;
      seconds += a.seconds;
      const m = byModel.get(a.model) ?? { attempts: 0, costUsd: 0, inputTokens: 0, outputTokens: 0 };
      m.attempts++;
      m.costUsd += a.costUsd;
      m.inputTokens += a.inputTokens;
      m.outputTokens += a.outputTokens;
      byModel.set(a.model, m);
    }
  for (const c of run.deciderCalls ?? []) {
    costUsd += c.costUsd;
    const key = DECIDER + c.model;
    const m = byModel.get(key) ?? { attempts: 0, costUsd: 0, inputTokens: 0, outputTokens: 0 };
    m.attempts++;
    m.costUsd += c.costUsd;
    byModel.set(key, m);
  }
  // The finishing pass (24o) counts in the run's cost, on its own line.
  if (run.finish) {
    costUsd += run.finish.costUsd;
    seconds += run.finish.seconds;
    const key = FINISHER + run.finish.model;
    const m = byModel.get(key) ?? { attempts: 0, costUsd: 0, inputTokens: 0, outputTokens: 0 };
    m.attempts++;
    m.costUsd += run.finish.costUsd;
    byModel.set(key, m);
  }
  return { costUsd, inputTokens, outputTokens, seconds, byModel };
}

/**
 * What free models saved in a run: the steps they finished, and what those would have cost on the paid agent going by
 * the user's own earlier runs (average cost of a finished step of the same kind; this run's paid steps when there is no
 * history). estimateUsd is null when nothing paid has been measured yet.
 */
export function freeSavings(
  run: Pick<PlanRun, "steps">,
  paidPerStep: Partial<Record<string, number>>,
): { steps: number; estimateUsd: number | null } {
  let steps = 0;
  let estimate = 0;
  let known = true;
  // This run's own paid finished steps, as a fallback measure.
  const paidDone = run.steps.flatMap((s) => s.attempts.filter((a) => a.outcome === "done" && !isFree(a.model)));
  const runAverage = paidDone.length ? paidDone.reduce((t, a) => t + a.costUsd, 0) / paidDone.length : null;
  for (const s of run.steps) {
    const done = s.attempts.find((a) => a.outcome === "done");
    if (!done || !isFree(done.model)) continue;
    steps++;
    const per = paidPerStep[s.jobType ?? "feature"] ?? runAverage;
    if (per === null || per === undefined) known = false;
    else estimate += per;
  }
  return { steps, estimateUsd: steps && known ? estimate : steps ? null : 0 };
}

/** Average cost of a finished step per kind of job on paid models, from what earlier runs learned. */
export function paidPerStep(stats: Map<string, { tries: number; successes: number; costUsd: number }>) {
  const sums: Record<string, { cost: number; done: number }> = {};
  for (const [key, s] of stats) {
    const [type, model] = key.split("|");
    if (isFree(model) || !s.successes) continue;
    const t = (sums[type] ??= { cost: 0, done: 0 });
    t.cost += s.costUsd;
    t.done += s.successes;
  }
  return Object.fromEntries(Object.entries(sums).map(([k, v]) => [k, v.cost / v.done]));
}
