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
  /** Tokens read and written over all tries (what a model burns, apart from its price). */
  tokens?: number;
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
        const s = stats.get(key) ?? { tries: 0, successes: 0, costUsd: 0, tokens: 0 };
        s.tries++;
        if (a.outcome === "done") s.successes++;
        s.costUsd += a.costUsd;
        s.tokens = (s.tokens ?? 0) + (a.inputTokens ?? 0) + (a.outputTokens ?? 0);
        stats.set(key, s);
      }
    }
  return stats;
}

export const MIN_TRIES = 3;
export const MIN_SUCCESS = 0.8;

/**
 * How sure the record makes us that a model finishes this kind of work: the lower end of the 95% range around its
 * success rate (Wilson). Many good tries count for more than a few: 33 of 34 is surer than 5 of 5.
 */
export function reliability(s: Pick<ModelStats, "tries" | "successes">): number {
  if (!s.tries) return 0;
  const z = 1.96;
  const n = s.tries;
  const p = s.successes / n;
  const z2 = z * z;
  return (p + z2 / (2 * n) - z * Math.sqrt((p * (1 - p) + z2 / (4 * n)) / n)) / (1 + z2 / n);
}

/**
 * The most reliable model on this kind of job (quality first: code quality before saving), among those that succeeded
 * at least 80% of the time with at least three tries; cost per success only breaks a tie. Null when nothing qualifies.
 */
export function learnedModel(stats: Map<string, ModelStats>, type: JobType, models: AgentModel[]) {
  let best: { model: string; sure: number; perSuccess: number; s: ModelStats } | null = null;
  for (const m of models) {
    const s = stats.get(`${type}|${m.id}`);
    if (!s || s.tries < MIN_TRIES || s.successes / s.tries < MIN_SUCCESS) continue;
    const sure = reliability(s);
    const perSuccess = s.costUsd / Math.max(1, s.successes);
    const better =
      !best || sure > best.sure + 1e-9 || (Math.abs(sure - best.sure) <= 1e-9 && perSuccess < best.perSuccess);
    if (better) best = { model: m.id, sure, perSuccess, s };
  }
  return best;
}

/** How a model did on one kind of work, from saved runs: what the decider weighs when it picks models. */
export interface ModelProfile {
  model: string;
  tries: number;
  successes: number;
  tokensPerSuccess: number;
  costPerSuccess: number;
  /**
   * Burns far more tokens than the lightest model that also finishes this kind of work (1.5× the tokens per finished
   * step, with at least three tries): picked only when a step really needs it. Tokens, not price: a stronger model is
   * not heavy only for costing more per token (quality first).
   */
  heavy: boolean;
}

export const HEAVY_FACTOR = 1.5;

/** Per model, for one kind of work: tries, successes, tokens and cost per finished step, and whether it is heavy. */
export function modelProfiles(stats: Map<string, ModelStats>, type: JobType, models: AgentModel[]): ModelProfile[] {
  const out: ModelProfile[] = [];
  for (const m of models) {
    const s = stats.get(`${type}|${m.id}`);
    if (!s || !s.tries) continue;
    const done = Math.max(1, s.successes);
    out.push({
      model: m.id,
      tries: s.tries,
      successes: s.successes,
      tokensPerSuccess: Math.round((s.tokens ?? 0) / done),
      costPerSuccess: s.costUsd / done,
      heavy: false,
    });
  }
  // Compared only with models that do finish this kind of work (a model that always fails is not "light").
  const proven = out.filter((p) => p.tries >= MIN_TRIES && p.successes / p.tries >= MIN_SUCCESS);
  const minTokens = Math.min(...proven.map((p) => p.tokensPerSuccess).filter((t) => t > 0));
  if (Number.isFinite(minTokens))
    for (const p of out) if (p.tries >= MIN_TRIES) p.heavy = p.tokensPerSuccess >= minTokens * HEAVY_FACTOR;
  return out;
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
  /** The decider's pick for this step (it read the step and the token record), when it made one. */
  picked?: { model: string; why: string } | null;
}): Choice {
  const { stepModel, type, rules, stats, models, defaultModel, budget, picked } = opts;
  if (stepModel?.trim()) return { model: stepModel.trim(), reason: "set on the step" };
  let choice: Choice;
  const rule = rules[type];
  const learned = learnedModel(stats, type, models);
  if (rule) choice = { model: rule, reason: `project rule for ${JOB_LABELS[type].toLowerCase()}` };
  else if (picked && models.some((m) => m.id === picked.model))
    choice = { model: picked.model, reason: `decider: ${picked.why || "picked for this step"}` };
  else if (learned)
    choice = {
      model: learned.model,
      reason: `learned: most reliable on ${JOB_LABELS[type].toLowerCase()} (${learned.s.successes}/${learned.s.tries} done)`,
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
