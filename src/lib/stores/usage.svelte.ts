/**
 * Qanuvi's total memory and processor use (itself and every process it started), for the status bar (1f). It is the
 * shown machine's (24z3 D2): a switch clears it at once, and an answer from the machine before is dropped.
 */
import { routed as invoke, shownMachine, shownPlace } from "$lib/remote";

export interface UsagePart {
  name: string;
  count: number;
  memory: number;
}

export interface Usage {
  memory: number;
  cpu: number;
  processes: number;
  parts: UsagePart[];
}

/** Memory in people's units. */
export function bytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  return `${Math.round(n / 1024 ** 2)} MB`;
}

class UsageStore {
  now = $state<Usage | null>(null);
  /** Whose it is, for the tooltip: "This computer" or the master's name. */
  place = $state("this computer");
  #timer: ReturnType<typeof setInterval> | null = null;
  #asks = 0;

  start(): void {
    if (this.#timer) return;
    void this.look();
    this.#timer = setInterval(() => void this.look(), 5000);
  }

  async look(): Promise<void> {
    const ask = ++this.#asks;
    const machine = shownMachine();
    try {
      const u = await invoke<Usage | null>("qanuvi_usage");
      if (ask === this.#asks && machine === shownMachine()) {
        this.now = u;
        this.place = shownPlace();
      }
    } catch {
      // A master that cannot tell (an older one) shows nothing, never another machine's numbers.
      if (ask === this.#asks && machine === shownMachine()) this.now = null;
    }
  }

  /** Another machine is shown (machines.ts). */
  switched(): void {
    this.now = null;
    if (this.#timer) void this.look();
  }
}

export const usage = new UsageStore();
