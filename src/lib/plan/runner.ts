/**
 * Running a task list step by step, each step on its own model. Plain functions only (the decisions), so they can be
 * tested; the run itself is in stores/runner.svelte.ts.
 */
import type { AgentModel } from "$lib/agents/providers";
import { parsePlan, type Step } from "./tasklist";

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
  /** When the run stops for the user (24f); missing: "decisions", as before. */
  pause?: PauseMode;
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
  /** The last test failures sent to the agent (for the planner when the step is rewritten). */
  failure?: string;
}

export interface StepRun {
  line: number;
  number: string | null;
  title: string;
  attempts: Attempt[];
  status: "waiting" | "running" | "done" | "blocked" | "failed" | "skipped";
  /** Kind of work, for learned routing (Milestone 13). */
  jobType?: import("./routing").JobType;
  /** Why the step started on its model. */
  choice?: string;
  /** The planner rewrote this step into the steps named here. */
  replacedBy?: string;
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
  status: "running" | "done" | "stopped" | "failed" | "needs-you";
  message: string;
  startedAt: number;
  endedAt: number | null;
  /** What the decider decided in this run, with its reasons (24b). */
  decisions?: import("$lib/core/decider").DecisionRecord[];
  /** The decider's own calls (assignment, reviews): they count in the run's cost (24e). */
  deciderCalls?: { model: string; costUsd: number }[];
}

/** The run lines' key for the decider's calls on a model. */
export const DECIDER = "decider:";

/** The steps a run covers: open steps (not done, not blocked) from the chosen one on. */
export function stepsToRun(steps: Step[], fromLine: number, onlyOne: boolean, ranges: [number, number][] = []): Step[] {
  const open = steps.filter((s) => s.status !== "x" && s.status !== "!");
  if (ranges.length && !onlyOne) return open.filter((s) => inRanges(s.number, ranges));
  const from = open.filter((s) => s.line >= fromLine);
  return onlyOne ? from.filter((s) => s.line === fromLine).slice(0, 1) : from;
}

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

/**
 * The first thing a step needs that is not done, as a reason to skip it; null when all is done (24f). `done` holds
 * this plan's finished step numbers; `others` the finished numbers of other plans by file (missing: not read).
 */
export function unmetNeed(
  needs: string[] | undefined,
  done: Set<string>,
  others: Map<string, Set<string>> = new Map(),
): string | null {
  for (const need of needs ?? []) {
    const hash = need.lastIndexOf("#");
    if (hash > 0) {
      const file = need.slice(0, hash);
      const num = need.slice(hash + 1);
      const there = others.get(file);
      if (!there) return `needs step ${num} of ${file}, which could not be read`;
      if (!there.has(num)) return `needs step ${num} of ${file}, which is not done`;
    } else if (!done.has(need)) return `needs step ${need}, which is not done`;
  }
  return null;
}

/** The end-of-run report (24f): what was done, and what was skipped or left, with why. */
export function runSummary(run: Pick<PlanRun, "steps">): string {
  const done = run.steps.filter((s) => s.status === "done").length;
  const left = run.steps.filter((s) => s.status === "skipped" || s.status === "blocked" || s.status === "failed");
  const name = (s: StepRun) => (s.number ? `step ${s.number}` : s.title);
  const parts = [`${done} done`];
  for (const kind of ["skipped", "blocked", "failed"] as const) {
    const these = left.filter((s) => s.status === kind);
    if (these.length)
      parts.push(
        `${these.length} ${kind}: ${these.map((s) => `${name(s)}${s.reason ? ` (${s.reason})` : ""}`).join("; ")}`,
      );
  }
  return parts.join(" · ");
}

/** The model a step starts on: its own "- Model:" line, else the agent's default. */
export function startModel(step: Pick<Step, "model">, defaultModel: string): string {
  return step.model?.trim() || defaultModel;
}

/**
 * The model for the next attempt. Models are listed strongest first. After `perModel` failed attempts on a model the
 * step moves one model up; null means the strongest model has failed too, so the run needs the user.
 */
export function nextModel(
  models: AgentModel[],
  attempts: Pick<Attempt, "model" | "outcome">[],
  first: string,
  perModel: number,
): string | null {
  const failed = attempts.filter((a) => a.outcome === "open" || a.outcome === "error");
  if (failed.length === 0) return first;
  const current = failed[failed.length - 1].model;
  const onCurrent = failed.filter((a) => a.model === current).length;
  if (onCurrent < Math.max(1, perModel)) return current;
  const index = models.findIndex((m) => m.id === current);
  if (index <= 0) return null; // already the strongest (or a model outside the list): ask the user
  return models[index - 1].id;
}

/** What the task list says about a step after an attempt. The step is found by its number and title, not its line. */
export function stepOutcome(
  text: string,
  step: Pick<Step, "number" | "title">,
): "done" | "blocked" | "open" | "missing" {
  const found = parsePlan(text).steps.find((s) => s.number === step.number && s.title === step.title);
  if (!found) return "missing";
  if (found.status === "x") return "done";
  if (found.status === "!") return "blocked";
  return "open";
}

/** The request for one step: only what the step needs, not the whole history. */
export function stepPrompt(file: string, step: Step, done: { number: string | null; title: string }[]): string {
  const label = step.number ? `${step.number}. ${step.title}` : step.title;
  const lines = [
    `You are working through the task list ${file} in this project, one step at a time. Your step:`,
    "",
    `**${label}**${step.note ? ` ${step.note}` : ""}`,
  ];
  if (step.details.length) lines.push(...step.details.map((d) => `  ${d}`));
  if (done.length) {
    lines.push("", "Already done in this run (for context only):");
    for (const d of done.slice(-10)) lines.push(`- ${d.number ? `${d.number}. ` : ""}${d.title}`);
  }
  lines.push(
    "",
    "Rules:",
    "- Work only on this step.",
    `- When the step is complete and checked, change its line in ${file} to "- [x]" and add a short "  - Done: ..." line under it saying what you did and how you checked it.`,
    `- If you cannot finish it, leave it open and say why in your last message. If only the user can unblock it, mark it "- [!]" and add the reason under it.`,
  );
  return lines.join("\n");
}

/** Totals for a run, per model and overall. */
export function runTotals(run: Pick<PlanRun, "steps"> & Pick<Partial<PlanRun>, "deciderCalls">) {
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
  return { costUsd, inputTokens, outputTokens, seconds, byModel };
}

/**
 * The model for a step's next attempt when free models go first: the free agent (as often as one model gets tries),
 * then the paid agent from `paidFirst` up, as without free models. Null: even the strongest paid model failed.
 */
export function nextModelFreeFirst(
  models: AgentModel[],
  attempts: Pick<Attempt, "model" | "outcome">[],
  start: string,
  paidFirst: string,
  perModel: number,
): string | null {
  const failed = attempts.filter((a) => a.outcome === "open" || a.outcome === "error");
  if (failed.length === 0) return start;
  const paid = attempts.filter((a) => !isFree(a.model));
  if (!paid.length) {
    const onFree = failed.filter((a) => isFree(a.model)).length;
    return onFree < Math.max(1, perModel) ? start : paidFirst;
  }
  return nextModel(models, paid, paidFirst, perModel);
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
