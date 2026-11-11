/** Claude Code's version against the newest one, and its own update (Settings, Agents; or by itself when chosen). */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { settings } from "./settings.svelte";
import { agent } from "./agent.svelte";

interface Versions {
  installed: string;
  latest: string;
  newer: boolean;
}

class ClaudeUpdateStore {
  versions = $state<Versions | null>(null);
  busy = $state(false);
  note = $state("");
  #started = false;

  async check(): Promise<void> {
    this.versions = await invoke<Versions>("claude_versions").catch(() => null);
  }

  async update(): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    this.note = "Updating Claude Code…";
    try {
      const out = await invoke<string>("claude_update");
      await this.check();
      await agent.loadInfo();
      this.note = this.versions && !this.versions.newer ? `Claude Code is ${this.versions.installed} now.` : out;
    } catch (e) {
      this.note = `The update did not finish: ${errorText(e)}`;
    } finally {
      this.busy = false;
    }
  }

  /** At start: look once; update by itself when the owner chose it and no agent is working. */
  start(): void {
    if (this.#started) return;
    this.#started = true;
    setTimeout(async () => {
      await this.check();
      const working = agent.sessions.some((s) => s.status === "running" || s.status === "waiting");
      if (this.versions?.newer && settings.value.updates.claudeAuto && !working) await this.update();
    }, 20_000);
  }
}

export const claudeUpdate = new ClaudeUpdateStore();
