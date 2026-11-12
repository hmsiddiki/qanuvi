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

const READ_TOOLS = /^(Read|Grep|Glob|LS|mcp__qanuvi__)/;
const CHANGE_TOOLS = /^(Edit|Write|MultiEdit|NotebookEdit)$/;
const READING_COMMAND = /^(read|find|list|show|check|look|see|search|count|inspect|view|grep|cat|sed|head|tail|ls)\b/i;

/** What an attempt's tool calls were for, from its session lines. */
export function toolKinds(lines: string[]): ToolKinds {
  const k: ToolKinds = { read: 0, change: 0, run: 0 };
  for (const l of lines) {
    if (!l.includes('"tool_use"')) continue;
    let m: {
      type?: string;
      message?: { content?: { type?: string; name?: string; input?: Record<string, unknown> }[] };
    };
    try {
      m = JSON.parse(l);
    } catch {
      continue;
    }
    if (m.type !== "assistant") continue;
    for (const b of m.message?.content ?? []) {
      if (b.type !== "tool_use" || !b.name) continue;
      if (CHANGE_TOOLS.test(b.name)) k.change++;
      else if (READ_TOOLS.test(b.name)) k.read++;
      else if (b.name === "Bash" || b.name === "PowerShell") {
        const what = String(b.input?.description ?? b.input?.command ?? "").replace(/^cd\s+\S+\s*&&\s*/, "");
        if (READING_COMMAND.test(what.trim())) k.read++;
        else k.run++;
      } else k.run++;
    }
  }
  return k;
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

/** Whether a step went over its limits: why, or null. */
export function overLimit(
  spentUsd: number,
  minutes: number,
  limits: { usd?: number; minutes?: number },
): string | null {
  if (limits.usd && limits.usd > 0 && spentUsd > limits.usd)
    return `it cost $${spentUsd.toFixed(2)}, over the limit of $${limits.usd.toFixed(2)} per step`;
  if (limits.minutes && limits.minutes > 0 && minutes > limits.minutes)
    return `it ran ${Math.round(minutes)} minutes, over the limit of ${limits.minutes} per step`;
  return null;
}
