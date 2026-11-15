/**
 * Start with Qanuvi: the commands in Settings, Start-up run while Qanuvi is open (a Headroom proxy, for example).
 * Each machine keeps its own list and its core starts it (24z3 D4: This PC's app at launch, a master in serve, with
 * no window needed). This store shows the shown machine's state, starts what was turned on, and follows it.
 */
import { routed as invoke, shownMachine } from "$lib/remote";
import { errorText } from "$lib/api";
import { settings, type StartupItem } from "./settings.svelte";

export interface StartupStatus {
  id: string;
  name: string;
  state: "starting" | "running" | "already running" | "failed" | "stopped" | "off";
  detail: string;
  output: string;
}

class StartupStore {
  statuses = $state<StartupStatus[]>([]);
  /** The shown machine could not answer (an older master, or the connection): said on the page. */
  problem = $state("");
  #timer: ReturnType<typeof setInterval> | null = null;

  /** At Qanuvi's start (once per window; Rust starts each command only once). */
  async start(): Promise<void> {
    if (this.#timer) return;
    await this.apply();
    this.#timer = setInterval(() => void this.refresh(), 3000);
  }

  /** Starts what is enabled and not running (after a change in Settings too). */
  apply(): Promise<void> {
    const items = settings.value.startup.filter((i) => i.enabled);
    return this.#ask("startup_start", { items });
  }

  refresh(): Promise<void> {
    return this.#ask("startup_status");
  }

  restart(item: StartupItem): Promise<void> {
    return this.#ask("startup_restart", { item });
  }

  stop(id: string): Promise<void> {
    return this.#ask("startup_stop", { id });
  }

  status(id: string): StartupStatus | undefined {
    return this.statuses.find((s) => s.id === id);
  }

  /** Another machine is shown (machines.ts): never the one before's commands. */
  switched(): void {
    this.statuses = [];
    this.problem = "";
    if (this.#timer) void this.refresh();
  }

  /** Asks the shown machine; an answer that comes after a switch is dropped. */
  async #ask(cmd: string, args?: Record<string, unknown>): Promise<void> {
    const machine = shownMachine();
    try {
      const got = await invoke<StartupStatus[]>(cmd, args);
      if (machine !== shownMachine()) return;
      this.statuses = got;
      this.problem = "";
    } catch (e) {
      if (machine === shownMachine()) this.problem = errorText(e);
    }
  }
}

export const startup = new StartupStore();
