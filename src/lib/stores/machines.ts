/**
 * Puts a machine on screen (24z3): calls without an id go to it (remote.ts), and the sidebar's projects and the
 * Settings page become its own. Part B's switcher calls this; until then the Servers screen does.
 */
import { showMachine } from "$lib/remote";
import { projects } from "./projects.svelte";
import { settings } from "./settings.svelte";

/** Shows a machine: null is This PC. */
export async function show(machine: string | null): Promise<void> {
  showMachine(machine);
  await Promise.all([projects.load(), settings.loadMachine()]);
}
