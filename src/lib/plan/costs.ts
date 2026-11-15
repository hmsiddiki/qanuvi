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
