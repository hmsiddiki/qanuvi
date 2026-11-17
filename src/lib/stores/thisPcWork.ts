/**
 * What works on This PC's master now (migration M5a): agent sessions working and plan runs going. Since the window is
 * a viewer, they go on when it closes and stop when the master does (an update, Quit, a restart), so both say so.
 */
import { callThisPc } from "$lib/remote";

export interface Work {
  agents: number;
  runs: number;
}

export async function thisPcWork(): Promise<Work> {
  const [agents, runs] = await Promise.all([
    callThisPc<number>("agents_working"),
    callThisPc<string[]>("plan_run_active"),
  ]);
  return { agents: Number(agents) || 0, runs: Array.isArray(runs) ? runs.length : 0 };
}

/** The work in words ("2 agent sessions and 1 plan run"), or null when nothing works (master.rs work_text). */
export function workText(w: Work): string | null {
  const part = (n: number, one: string) => (n <= 0 ? null : n === 1 ? `1 ${one}` : `${n} ${one}s`);
  const parts = [part(w.agents, "agent session"), part(w.runs, "plan run")].filter((p): p is string => !!p);
  return parts.length ? parts.join(" and ") : null;
}

/** "keeps" for one thing, "keep" for more. */
export function keepVerb(w: Work): string {
  return Math.max(0, w.agents) + Math.max(0, w.runs) === 1 ? "keeps" : "keep";
}
