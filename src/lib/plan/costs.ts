/**
 * Measuring plan runs (the owner's aim: Qanuvi spends less than working by hand): what each attempt spent its tool
 * calls on, and what finished steps cost in this run against the plan's earlier runs.
 */

export interface ToolKinds {
  /** Reading and searching: finding its way. */
  read: number;
  /** Writing and editing files. */
  change: number;
  /** Other commands: tests, builds, scripts. */
  run: number;
}

/** A run record's finished steps and what they cost. */
export function finishedCost(data: { steps?: { status: string; attempts?: { costUsd?: number }[] }[] }): {
  steps: number;
  cost: number;
} {
  let steps = 0;
  let cost = 0;
  for (const s of data.steps ?? []) {
    const c = (s.attempts ?? []).reduce((t, a) => t + (a.costUsd ?? 0), 0);
    cost += c;
    if (s.status === "done") steps++;
  }
  return { steps, cost };
}

/** This plan's earlier runs: cost per finished step, and the plan's whole spend so far. */
export function planHistory(
  records: { id: string; file: string; data: string }[],
  file: string,
  exceptId: string,
): { runs: number; steps: number; cost: number; perStep: number | null } {
  let runs = 0;
  let steps = 0;
  let cost = 0;
  for (const r of records) {
    if (r.file !== file || r.id === exceptId) continue;
    let d: Parameters<typeof finishedCost>[0];
    try {
      d = JSON.parse(r.data);
    } catch {
      continue;
    }
    const f = finishedCost(d);
    runs++;
    steps += f.steps;
    cost += f.cost;
  }
  return { runs, steps, cost, perStep: steps ? cost / steps : null };
}

/**
 * What one finished step usually costs on a model, from what earlier runs learned (keys "jobType|model"): the same
 * kind of job first, else every kind on that model. Failed tries count in (a step pays for them too). Null when the
 * model never finished a step.
 */
export function usualStepCost(
  stats: Map<string, { successes: number; costUsd: number }>,
  model: string,
  type: string,
): number | null {
  const same = stats.get(`${type}|${model}`);
  if (same && same.successes) return same.costUsd / same.successes;
  let cost = 0;
  let done = 0;
  for (const [key, s] of stats) {
    if (key.slice(key.indexOf("|") + 1) !== model) continue;
    cost += s.costUsd;
    done += s.successes;
  }
  return done ? cost / done : null;
}

/** A run budget (0 = none) below what its first step usually costs: the run would stop after that step. */
export function budgetBelowFirstStep(budget: number, usual: number | null): boolean {
  return budget > 0 && usual !== null && usual > budget;
}
