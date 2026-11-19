/**
 * Qanuvi's total memory and processor use, for the status bar (1f). Two halves since the migration (u2): the shown
 * master's (its core and watcher, and every process the core started), and the screen's (this window and its
 * WebView2 processes), which the window measures itself, so any master version works. This PC shown: the total of
 * both. A server shown: that master's, with the screen in the tooltip only (the screen always runs on this PC).
 * A switch clears the master's half at once, and an answer from the machine before is dropped (24z3 D2).
 */
import { invoke } from "@tauri-apps/api/core";
import { routed, shownMachine, shownPlace } from "$lib/remote";

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
  /** The master program's own processes (core and watcher); missing from a master before u2. */
  own?: number;
}

/** Memory in people's units. */
export function bytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  return `${Math.round(n / 1024 ** 2)} MB`;
}

function partLines(parts: UsagePart[]): string[] {
  return parts.map((p) => `${p.name}${p.count > 1 ? ` ×${p.count}` : ""}: ${bytes(p.memory)}`);
}

/** The split's first part: the master program itself, and what it started when it says (a master since u2). */
function masterSplit(m: Usage): string[] {
  if (m.own === undefined) return [`master ${bytes(m.memory)}`];
  return [`master ${bytes(m.own)}`, `agents, shells, language servers ${bytes(Math.max(0, m.memory - m.own))}`];
}

/** What the status bar shows: `master` the shown master's half, `screen` this window's, `thisPc` which is shown. */
export function shown(
  master: Usage | null,
  screen: Usage | null,
  thisPc: boolean,
  place: string,
): { text: string; title: string } | null {
  if (!master) return null;
  if (thisPc) {
    const memory = master.memory + (screen?.memory ?? 0);
    const cpu = master.cpu + (screen?.cpu ?? 0);
    const processes = master.processes + (screen?.processes ?? 0);
    const split = masterSplit(master);
    if (screen) split.splice(1, 0, `screen ${bytes(screen.memory)}`);
    // One line per program: the master's link helpers and the screen's are both qanuvi-master.
    const byName = new Map<string, UsagePart>();
    for (const p of [...master.parts, ...(screen?.parts ?? [])]) {
      const had = byName.get(p.name);
      byName.set(p.name, had ? { name: p.name, count: had.count + p.count, memory: had.memory + p.memory } : p);
    }
    const parts = [...byName.values()].sort((a, b) => b.memory - a.memory);
    return {
      text: `${bytes(memory)} · ${Math.min(100, cpu).toFixed(0)}%`,
      title: [`Qanuvi in all on ${place} (${processes} processes):`, split.join(" · "), ...partLines(parts)].join("\n"),
    };
  }
  const split = masterSplit(master);
  return {
    text: `${bytes(master.memory)} · ${master.cpu.toFixed(0)}%`,
    title: [
      `Qanuvi in all on ${place} (${master.processes} processes):`,
      split.join(" · "),
      ...partLines(master.parts),
      ...(screen ? [`On this computer, the screen: ${bytes(screen.memory)}`] : []),
    ].join("\n"),
  };
}

class UsageStore {
  /** The shown master's half. */
  now = $state<Usage | null>(null);
  /** This window's half (always this PC's). */
  screen = $state<Usage | null>(null);
  /** Whose the master's half is, for the tooltip: "this computer" or the master's name. */
  place = $state("this computer");
  /** Whether that half is This PC's (the screen is then added in). */
  thisPc = $state(true);
  #timer: ReturnType<typeof setInterval> | null = null;
  #asks = 0;
  #screenAsks = 0;

  /** The status bar's text and tooltip, or null while the master's half is not known. */
  get bar(): { text: string; title: string } | null {
    return shown(this.now, this.screen, this.thisPc, this.place);
  }

  start(): void {
    if (this.#timer) return;
    void this.look();
    this.#timer = setInterval(() => void this.look(), 5000);
  }

  async look(): Promise<void> {
    void this.lookScreen();
    const ask = ++this.#asks;
    const machine = shownMachine();
    try {
      const u = await routed<Usage | null>("qanuvi_usage");
      if (ask === this.#asks && machine === shownMachine()) {
        this.now = u;
        this.place = shownPlace();
        this.thisPc = machine === null;
      }
    } catch {
      // A master that cannot tell (an older one) shows nothing, never another machine's numbers.
      if (ask === this.#asks && machine === shownMachine()) this.now = null;
    }
  }

  /** The window's own half, measured here whatever master is shown. */
  async lookScreen(): Promise<void> {
    const ask = ++this.#screenAsks;
    try {
      const s = await invoke<Usage | null>("screen_usage");
      if (ask === this.#screenAsks) this.screen = s ?? null;
    } catch {
      if (ask === this.#screenAsks) this.screen = null;
    }
  }

  /** Another machine is shown (machines.ts). */
  switched(): void {
    this.now = null;
    if (this.#timer) void this.look();
  }
}

export const usage = new UsageStore();
