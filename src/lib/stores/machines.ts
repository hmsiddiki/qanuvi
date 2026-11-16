/**
 * Puts a machine on screen (24z3): calls without an id go to it (remote.ts), and the sidebar's projects and the
 * Settings page (Models included) become its own. The sidebar's machine switcher (Part B) picks one with pick().
 */
import { showMachine } from "$lib/remote";
import { brain } from "./brain.svelte";
import { projects } from "./projects.svelte";
import { settings } from "./settings.svelte";
import { guides } from "./guides.svelte";
import { usage } from "./usage.svelte";
import { startup } from "./startup.svelte";
import { claudeUpdate } from "./claudeUpdate.svelte";
import { agent } from "./agent.svelte";
import { omni } from "./omniroute.svelte";
import { memoryDisk } from "./memoryDisk.svelte";
import { remoteAccess } from "./remoteAccess.svelte";
import { memory } from "./memory.svelte";
import { switcher } from "./switcher.svelte";

/** Shows a machine: null is This PC. */
export async function show(machine: string | null): Promise<void> {
  showMachine(machine);
  switcher.shown = machine;
  // The status bar, the setup guide's machine facts, Start-up and Claude Code's version are the new machine's.
  usage.switched();
  guides.switched();
  startup.switched();
  claudeUpdate.switched();
  omni.switched();
  // The Agents page's Claude Code card: never the machine before's path and version while the new one answers.
  agent.info = null;
  await Promise.all([projects.load(), settings.loadMachine()]);
  // The Models page: its keys and records are the machine's, read once its provider list is in.
  await brain.switched();
  // Asked once its settings are in: with no Claude Code there, its own default agent may become Qanuvi's.
  await agent.loadInfo();
  // The status bar's OmniRoute warnings: the new machine's own.
  void omni.look();
  // The memory disk and remote access (24z3 D6): the new machine's own, read once its settings are in.
  void memoryDisk.switched();
  void remoteAccess.switched();
  // Memory entries (24z3 D7): the Memory page and the waiting suggestions are the new machine's.
  memory.touch();
}

/**
 * The owner picks a machine in the switcher: it is shown and remembered for the next start (the main window only).
 * A connection that ends by itself shows This PC but keeps the choice, so the next start comes back to it.
 */
export async function pick(machine: string | null, remember: boolean): Promise<void> {
  if (remember) switcher.remember(machine);
  switcher.switching = machine ?? "";
  try {
    await show(machine);
  } finally {
    switcher.switching = null;
  }
}

/**
 * At start, once the servers marked to connect have answered: the machine picked last is shown again when it is
 * connected; otherwise the window stays on This PC (the choice is kept for a later start).
 */
export async function restoreShown(connected: (id: string) => boolean): Promise<void> {
  const want = switcher.remembered();
  if (want && connected(want) && switcher.shown === null) await pick(want, false);
}
