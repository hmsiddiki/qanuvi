/**
 * Puts a machine on screen (24z3): calls without an id go to it (remote.ts), and the sidebar's projects and the
 * Settings page (Models included) become its own. The sidebar's machine switcher (Part B) picks one with pick().
 */
import { showMachine, shownMachine } from "$lib/remote";
import { editor } from "./editor.svelte";
import { terminals } from "./terminals.svelte";
import { workspace } from "./workspace.svelte";
import { drafts } from "./drafts";
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

/**
 * The machine leaving the screen is wiped from the window (24z3 Part B2): its open files go (unsaved text kept as
 * drafts on that machine), its terminals keep running there with their screens gone, and its projects' workspaces
 * are given back from scratch on return. A machine whose connection already ended has its drafts held in the window
 * instead.
 */
export async function park(machine: string | null): Promise<void> {
  const reachable = shownMachine() === machine;
  // The workspaces as they are now are written, and stop following the editor while it empties.
  workspace.flush();
  for (const p of projects.list) workspace.restored.delete(p.id);
  const unsaved = editor.park();
  await Promise.all([drafts.keep(machine, unsaved, reachable), terminals.park(machine, reachable)]);
}

/** Gives back the agent configuration files the shown machine kept unsaved text for (their projects do the rest). */
async function reopenConfigDrafts(machine: string | null): Promise<void> {
  for (const d of await drafts.take(machine, "")) {
    let meta: { provider?: string; id?: string; projectId?: string | null; label?: string } = {};
    try {
      meta = JSON.parse(d.meta || "{}");
    } catch {
      continue;
    }
    if (!meta.provider || !meta.id || switcher.shown !== machine) continue;
    await editor.openConfig(meta.provider, meta.id, meta.label ?? "", meta.projectId ?? null);
    editor.applyDraft(d.path, d.text, d.hash);
  }
}

/** Shows a machine: null is This PC. */
export async function show(machine: string | null): Promise<void> {
  const from = switcher.shown;
  // Its calls name the machine being hidden, so the new one shows without waiting for them.
  if (from !== machine) void park(from);
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
  // Unsaved agent configuration files kept on it (24z3 Part B2); project files come back with their workspace.
  if (from !== machine) void reopenConfigDrafts(machine);
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
