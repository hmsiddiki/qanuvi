/**
 * The decider's model picks for a plan, made before a run ("Pick models" in the Plan view) and kept on this computer,
 * not in the plan file. Only the owner starts it; a run uses a pick's model while it still fits: same agent, same
 * step title. A model the owner set on the step (its Model: line) is never replaced.
 */
export interface StepPick {
  title: string;
  worker: "free" | "paid";
  model?: string;
  why: string;
}

export interface SavedPicks {
  at: number;
  providerId: string;
  freeFirst: boolean;
  /** By step number (or line, for a step without one). */
  picks: Record<string, StepPick>;
}

const key = (projectId: string, file: string) => `qanuvi.picks.${projectId}.${file}`;

export function loadPicks(store: Pick<Storage, "getItem">, projectId: string, file: string): SavedPicks | null {
  try {
    const v = JSON.parse(store.getItem(key(projectId, file)) ?? "null") as SavedPicks | null;
    return v && typeof v.picks === "object" ? v : null;
  } catch {
    return null;
  }
}

export function savePicks(store: Pick<Storage, "setItem">, projectId: string, file: string, picks: SavedPicks): void {
  store.setItem(key(projectId, file), JSON.stringify(picks));
}

/** The saved pick for a step, while it still fits this run; null when the step changed or the run differs. */
export function pickFor(
  saved: SavedPicks | null,
  step: { number: string; title: string },
  providerId: string,
): StepPick | null {
  if (!saved || saved.providerId !== providerId) return null;
  const p = saved.picks[step.number];
  if (p && p.title === step.title) return p;
  // A step without a number is kept by its line, and lines move when a run writes into the plan ("Done" lines):
  // the same title, on one pick only, is still that step.
  const same = Object.values(saved.picks).filter((x) => x.title === step.title);
  return same.length === 1 ? same[0] : null;
}
