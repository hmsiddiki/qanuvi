/**
 * Claude Code's version against the newest one, and its own update (Settings, Agents), on the shown machine. Keeping
 * it up to date by itself is done by each machine's core from its own settings (24z3 D4: claude_update.rs
 * auto_update), never by the window, so a machine not shown is kept current too.
 */
import { routed as invoke, shownMachine } from "$lib/remote";
import { errorText } from "$lib/api";
import { agent } from "./agent.svelte";

interface Versions {
  installed: string;
  latest: string;
  newer: boolean;
}

/** No update running (null is This PC, so it cannot mean "none"). */
const NONE = Symbol("none");

class ClaudeUpdateStore {
  versions = $state<Versions | null>(null);
  /** An update runs on the shown machine. */
  busy = $state(false);
  note = $state("");
  #started = false;
  /** The machine an update runs on. */
  #on: string | null | typeof NONE = NONE;

  async check(): Promise<void> {
    const machine = shownMachine();
    const got = await invoke<Versions>("claude_versions").catch(() => null);
    if (machine === shownMachine()) this.versions = got;
  }

  async update(): Promise<void> {
    if (this.#on !== NONE) return;
    const machine = shownMachine();
    this.#on = machine;
    this.busy = true;
    this.note = "Updating Claude Code…";
    let note: string;
    try {
      const out = await invoke<string>("claude_update");
      await this.check();
      if (machine === shownMachine()) await agent.loadInfo();
      note = this.versions && !this.versions.newer ? `Claude Code is ${this.versions.installed} now.` : out;
    } catch (e) {
      note = `The update did not finish: ${errorText(e)}`;
    }
    this.#on = NONE;
    // Shown elsewhere meanwhile: that machine's page says nothing about this update.
    if (machine === shownMachine()) {
      this.busy = false;
      this.note = note;
    }
  }

  /** At start: look once, so Settings, Agents shows the versions. */
  start(): void {
    if (this.#started) return;
    this.#started = true;
    setTimeout(() => void this.check(), 20_000);
  }

  /** Another machine is shown (machines.ts): its own versions, asked again. */
  switched(): void {
    this.versions = null;
    this.note = "";
    this.busy = this.#on !== NONE && this.#on === shownMachine();
    if (this.busy) this.note = "Updating Claude Code…";
    if (this.#started) void this.check();
  }
}

export const claudeUpdate = new ClaudeUpdateStore();
