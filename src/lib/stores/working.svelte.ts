/**
 * The machine switcher's working icon (24z3 step 4): how many agents work now on each machine, chats and plan steps
 * alike, one number per machine (never its chats). This PC and every connected master are asked every few seconds;
 * the call is a single count, so a hidden machine still costs next to nothing.
 */
import { callOn } from "$lib/remote";
import { masters } from "./masters.svelte";

/** How often the counts are read (ms). */
export const WORKING_EVERY = 6000;

/** The hover text for a machine's count ("" when nothing works there). */
export function workingText(n: number | undefined): string {
  if (!n || n < 1) return "";
  return n === 1 ? "1 agent working" : `${n} agents working`;
}

class WorkingStore {
  /** Agents working per machine: "" is This PC, else a master's id. Missing: not known (older master, offline). */
  counts = $state<Record<string, number>>({});
  #timer: ReturnType<typeof setInterval> | null = null;

  start(): void {
    if (this.#timer) return;
    void this.refresh();
    this.#timer = setInterval(() => void this.refresh(), WORKING_EVERY);
  }

  stop(): void {
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
  }

  /** Reads the counts now (the switcher's list also asks when it opens). */
  async refresh(): Promise<void> {
    const ids: (string | null)[] = [null];
    ids.push(...masters.onKeys);
    const read = await Promise.all(
      ids.map((id) =>
        callOn<number>(id, "agents_working", {})
          .then((n) => [id ?? "", typeof n === "number" ? n : 0] as const)
          // An older master does not know the command, a dropped one does not answer: no icon for it.
          .catch(() => null),
      ),
    );
    const next: Record<string, number> = {};
    for (const r of read) if (r) next[r[0]] = r[1];
    this.counts = next;
  }

  /** Agents working on a machine (null: This PC). */
  of(machine: string | null): number {
    return this.counts[machine ?? ""] ?? 0;
  }
}

export const working = new WorkingStore();
