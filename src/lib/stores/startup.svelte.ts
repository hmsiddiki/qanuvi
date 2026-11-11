/**
 * Start with Qanuvi: the commands in Settings, Start-up run while Qanuvi is open (a Headroom proxy, for example).
 * Rust starts them hidden and keeps their state and output; this store starts them once and follows their state.
 */
import { invoke } from "@tauri-apps/api/core";
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
  #timer: ReturnType<typeof setInterval> | null = null;

  /** At Qanuvi's start (once per window; Rust starts each command only once). */
  async start(): Promise<void> {
    if (this.#timer) return;
    await this.apply();
    this.#timer = setInterval(() => this.refresh().catch(() => {}), 3000);
  }

  /** Starts what is enabled and not running (after a change in Settings too). */
  async apply(): Promise<void> {
    const items = settings.value.startup.filter((i) => i.enabled);
    this.statuses = await invoke<StartupStatus[]>("startup_start", { items }).catch(() => this.statuses);
  }

  async refresh(): Promise<void> {
    this.statuses = await invoke<StartupStatus[]>("startup_status");
  }

  async restart(item: StartupItem): Promise<void> {
    this.statuses = await invoke<StartupStatus[]>("startup_restart", { item });
  }

  async stop(id: string): Promise<void> {
    this.statuses = await invoke<StartupStatus[]>("startup_stop", { id });
  }

  status(id: string): StartupStatus | undefined {
    return this.statuses.find((s) => s.id === id);
  }
}

export const startup = new StartupStore();
