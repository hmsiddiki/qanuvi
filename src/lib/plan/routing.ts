/**
 * Learned routing (Milestone 13): which model a step starts on when the step does not name one. Plain functions; the
 * run uses them in stores/runner.svelte.ts.
 *
 * Order: the step's own "- Model:" line > the project's rules > what earlier runs showed > the agent's default.
 * A monthly budget then may move the choice one model cheaper (never for money and security work).
 */
import type { AgentModel } from "$lib/agents/providers";
import type { PlanRun } from "./runner";

export const JOB_TYPES = ["docs", "tests", "ui", "security", "bugfix", "refactor", "feature"] as const;
export type JobType = (typeof JOB_TYPES)[number];

export const JOB_LABELS: Record<JobType, string> = {
  docs: "Docs and wording",
  tests: "Tests",
  ui: "Interface and styling",
  security: "Money and security",
  bugfix: "Bug fixes",
  refactor: "Refactoring",
  feature: "Features",
};

const RULES: [JobType, RegExp][] = [
  [
    "security",
    /\b(payments?|pay|prices?|pricing|money|refunds?|wallets?|gift ?cards?|coupons?|discounts?|checkout|invoices?|tax(es)?|vat|auth\w*|log ?in|passwords?|tokens?|secrets?|permissions?|security|encrypt\w*|csrf|xss|sql injection|nonces?)\b/i,
  ],
  ["docs", /\b(readme|docs?|documentation|documents?|wording|typos?|comments?|changelog|guides?)\b|\.md\b/i],
  ["tests", /\b(tests?|specs?|coverage|e2e|playwright|vitest|phpunit|pytest|jest)\b/i],
  [
    "ui",
    /\b(ui|css|styles?|styling|restyle|layouts?|buttons?|modals?|components?|svelte|vue|react|themes?|templates?|responsive|icons?)\b/i,
  ],
  ["bugfix", /\b(fix(es|ed)?|bugs?|broken|crash(es)?|errors?|regressions?|wrong|fails?|failing)\b/i],
  [
    "refactor",
    /\b(refactor\w*|rename\w*|move|moving|clean ?up|simplif\w*|split|extract\w*|reorganis\w*|reorganiz\w*)\b/i,
  ],
];

/** The kind of work a step is, from its title and details. Money and security win over everything else. */
export function jobType(text: string): JobType {
  for (const [type, re] of RULES) if (re.test(text)) return type;
  return "feature";
}

export interface ModelStats {
  tries: number;
  successes: number;
  costUsd: number;
}

/** Per job type and model: how many tries, how many succeeded, what they cost — from saved runs. */
export function learn(runs: Pick<PlanRun, "steps">[], typeOf: (title: string) => JobType) {
  const stats = new Map<string, ModelStats>();
  for (const run of runs)
    for (const step of run.steps) {
      const type = step.jobType ?? typeOf(step.title);
      for (const a of step.attempts) {
        if (a.outcome === "stopped") continue; // the user stopped it: says nothing about the model
        const key = `${type}|${a.model}`;
        const s = stats.get(key) ?? { tries: 0, successes: 0, costUsd: 0 };
        s.tries++;
        if (a.outcome === "done") s.successes++;
        s.costUsd += a.costUsd;
        stats.set(key, s);
      }
    }
  return stats;
}

export const MIN_TRIES = 3;
export const MIN_SUCCESS = 0.8;

/**
 * The cheapest model (by cost per success) that succeeded at least 80% of the time on this kind of job, with at least
 * three tries. Null when nothing qualifies yet.
 */
export function learnedModel(stats: Map<string, ModelStats>, type: JobType, models: AgentModel[]) {
  let best: { model: string; perSuccess: number; s: ModelStats } | null = null;
  for (const m of models) {
    const s = stats.get(`${type}|${m.id}`);
    if (!s || s.tries < MIN_TRIES || s.successes / s.tries < MIN_SUCCESS) continue;
    const perSuccess = s.costUsd / Math.max(1, s.successes);
    if (!best || perSuccess < best.perSuccess) best = { model: m.id, perSuccess, s };
  }
  return best;
}

export interface Budget {
  /** Monthly limit in US dollars; 0 means none. */
  monthlyUsd: number;
  /** Spent so far this month by all agent sessions. */
  spentUsd: number;
}

/** "ok", "tight" (80% or more used: prefer cheaper models) or "over" (stop and ask). */
export function budgetState(b: Budget): "ok" | "tight" | "over" {
  if (b.monthlyUsd <= 0) return "ok";
  if (b.spentUsd >= b.monthlyUsd) return "over";
  if (b.spentUsd >= b.monthlyUsd * 0.8) return "tight";
  return "ok";
}

export interface Choice {
  model: string;
  /** Why, in a few words, for the step's chip and the run record. */
  reason: string;
}

/** The model a step starts on, and why. `models` are listed strongest first. */
export function chooseModel(opts: {
  stepModel: string | null;
  type: JobType;
  rules: Partial<Record<JobType, string>>;
  stats: Map<string, ModelStats>;
  models: AgentModel[];
  defaultModel: string;
  budget: Budget;
}): Choice {
  const { stepModel, type, rules, stats, models, defaultModel, budget } = opts;
  if (stepModel?.trim()) return { model: stepModel.trim(), reason: "set on the step" };
  let choice: Choice;
  const rule = rules[type];
  const learned = learnedModel(stats, type, models);
  if (rule) choice = { model: rule, reason: `project rule for ${JOB_LABELS[type].toLowerCase()}` };
  else if (learned)
    choice = {
      model: learned.model,
      reason: `learned: ${learned.s.successes}/${learned.s.tries} done on ${JOB_LABELS[type].toLowerCase()}`,
    };
  else choice = { model: defaultModel, reason: "default model" };

  if (budgetState(budget) === "tight" && type !== "security") {
    const i = models.findIndex((m) => m.id === choice.model);
    if (i >= 0 && i < models.length - 1)
      return { model: models[i + 1].id, reason: `${choice.reason}; one cheaper (monthly budget 80% used)` };
  }
  return choice;
}

/** Start of the current month (seconds since 1970, local time), for "spent this month". */
export function monthStart(now = new Date()): number {
  return Math.floor(new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000);
}
