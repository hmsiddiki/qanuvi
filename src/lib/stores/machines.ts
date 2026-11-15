/**
 * Puts a machine on screen (24z3): calls without an id go to it (remote.ts), and the sidebar's projects and the
 * Settings page (Models included) become its own. Part B's switcher calls this; until then the Servers screen does.
 */
import { showMachine } from "$lib/remote";
import { brain } from "./brain.svelte";
import { projects } from "./projects.svelte";
import { settings } from "./settings.svelte";
import { guides } from "./guides.svelte";
import { usage } from "./usage.svelte";
import { startup } from "./startup.svelte";
import { claudeUpdate } from "./claudeUpdate.svelte";

/** Shows a machine: null is This PC. */
export async function show(machine: string | null): Promise<void> {
  showMachine(machine);
  // The status bar, the setup guide's machine facts, Start-up and Claude Code's version are the new machine's.
  usage.switched();
  guides.switched();
  startup.switched();
  claudeUpdate.switched();
  await Promise.all([projects.load(), settings.loadMachine()]);
  // The Models page: its keys and records are the machine's, read once its provider list is in.
  await brain.switched();
}
