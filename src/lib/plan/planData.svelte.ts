/**
 * What a plan keeps between runs (24z R2a): the decider's model picks, the plan state, the plan's notes and its run
 * settings. Kept in the core's database (plan_data), so a run going on in the core reads and writes the same; the
 * window keeps a copy that reads at once and writes through. It answers the same keys localStorage had, so the
 * callers stay as they were; the first start brings the old localStorage values in and removes them.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type PlanDataKind = "picks" | "state" | "notes" | "run";

interface Entry {
  project_id: string;
  file: string;
  kind: PlanDataKind;
  value: unknown;
}

/** The localStorage key prefixes of each kind (the keys stay the window's names for them). */
const PREFIXES: [string, PlanDataKind][] = [
  ["qanuvi.picks.", "picks"],
  ["qanuvi.planstate.", "state"],
  ["qanuvi.plannotes.", "notes"],
  ["qanuvi.run.", "run"],
];

const keyOf = (e: Pick<Entry, "project_id" | "file" | "kind">): string =>
  `${PREFIXES.find(([, k]) => k === e.kind)![0]}${e.project_id}.${e.file}`;

/** A key's project, file and kind; project ids have no dots, file paths may. */
export function parsePlanDataKey(key: string): Omit<Entry, "value"> | null {
  for (const [prefix, kind] of PREFIXES) {
    if (!key.startsWith(prefix)) continue;
    const rest = key.slice(prefix.length);
    const dot = rest.indexOf(".");
    if (dot <= 0 || dot === rest.length - 1) return null;
    return { project_id: rest.slice(0, dot), file: rest.slice(dot + 1), kind };
  }
  return null;
}

class PlanData {
  /** Goes up when a value changes here or anywhere else (another window, the runner in the core). */
  changed = $state(0);
  #values = new Map<string, string>();
  #started = false;

  getItem(key: string): string | null {
    return this.#values.get(key) ?? null;
  }

  setItem(key: string, text: string): void {
    const at = parsePlanDataKey(key);
    if (!at) return;
    let value: unknown;
    try {
      value = JSON.parse(text);
    } catch {
      return;
    }
    this.#values.set(key, text);
    this.changed++;
    this.#send(at, value);
  }

  removeItem(key: string): void {
    const at = parsePlanDataKey(key);
    if (!at) return;
    this.#values.delete(key);
    this.changed++;
    this.#send(at, null);
  }

  #send(at: Omit<Entry, "value">, value: unknown): void {
    const key = keyOf(at);
    this.#pending.set(key, (this.#pending.get(key) ?? 0) + 1);
    Promise.resolve(invoke("plan_data_set", { projectId: at.project_id, file: at.file, kind: at.kind, value })).catch(
      () => {
        // No echo comes for it; the copy here holds it for this window and the next change writes again.
        this.#echoed(key);
      },
    );
  }

  /** Writes of this window whose echo has not come back yet, per key: an older echo must not undo a newer write. */
  #pending = new Map<string, number>();

  #echoed(key: string): boolean {
    const n = this.#pending.get(key) ?? 0;
    if (n <= 0) return false;
    if (n === 1) this.#pending.delete(key);
    else this.#pending.set(key, n - 1);
    return true;
  }

  /** Loads the kept values, brings the old localStorage ones in once and follows changes made anywhere. */
  async start(): Promise<void> {
    if (this.#started) return;
    this.#started = true;
    // The old values first, so nothing reads empty while the database answers.
    const old: Entry[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      const at = key ? parsePlanDataKey(key) : null;
      if (!key || !at) continue;
      try {
        const text = localStorage.getItem(key) ?? "null";
        old.push({ ...at, value: JSON.parse(text) });
        this.#values.set(key, text);
      } catch {
        // A broken old value is not worth keeping.
      }
    }
    if (old.length) this.changed++;
    await listen<Entry>("qanuvi-plan-data", (e) => this.#take(e.payload));
    try {
      if (old.length) await invoke("plan_data_import", { entries: old });
      for (const e of old) localStorage.removeItem(keyOf(e));
      for (const e of await invoke<Entry[]>("plan_data_list")) if (!this.#pending.has(keyOf(e))) this.#take(e, false);
    } catch {
      // The old copies stay in localStorage for the next start.
    }
  }

  #take(e: Entry, echo = true): void {
    const key = keyOf(e);
    // This window's own write coming back: the copy here is already as new or newer.
    if (echo && this.#echoed(key)) return;
    if (e.value === null || e.value === undefined) this.#values.delete(key);
    else this.#values.set(key, JSON.stringify(e.value));
    this.changed++;
  }
}

export const planData = new PlanData();
