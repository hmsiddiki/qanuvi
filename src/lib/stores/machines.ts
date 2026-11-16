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
import { folderPicker } from "./folderPicker.svelte";
import { lsp } from "./lsp.svelte";
import { dap } from "./dap.svelte";

/**
 * The machine leaving the screen is wiped from the window (24z3 Part B2): its open files go (unsaved text kept as
 * drafts on that machine), its terminals keep running there with their screens gone, and its projects' workspaces
 * are given back from scratch on return. A machine whose connection already ended (or one held as lost, Part E) has
 * its drafts held in the window instead.
 */
export async function park(machine: string | null): Promise<void> {
  const reachable = shownMachine() === machine && switcher.held?.id !== machine;
  // The workspaces as they are now are written, and stop following the editor while it empties.
  workspace.flush();
  for (const p of projects.list) workspace.restored.delete(p.id);
  const unsaved = editor.park();
  // Coding tools end when the viewer leaves the machine (migration M1); they start again on its next file.
  lsp.stopMachine(machine);
  dap.stopMachine(machine);
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
  // Another machine shown (or the held one back): no "not connected" screen any more.
  switcher.held = null;
  showMachine(machine);
  switcher.shown = machine;
  // A folder picker open on the machine before closes (it read that machine's folders).
  folderPicker.switched(machine);
  // The shell picker lists the new machine's shells (a Linux server has no Windows PowerShell).
  void terminals.loadShells();
  // The chat pane's "working" was the machine before's turn: the new machine's chat shows its own sessions' state.
  if (from !== machine) agent.busy = false;
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
 * A master that cannot be reached is put on screen as "not connected" (24z3 Part E): the machine before is hidden as
 * when switching, and nothing of the new one shows (never a stale copy) until Reconnect brings it. A machine shown
 * already (lost for too long) has its screen wiped the same way, its unsaved text held in the window.
 */
export async function showOffline(machine: string, error: string): Promise<void> {
  const from = switcher.shown;
  const held = switcher.held;
  // Wiping it: the drafts of a lost machine stay in the window (park sees it held).
  if (from === machine && !held) switcher.held = { id: machine, mode: "lost", error, tries: 0, nextAt: 0, since: 0 };
  void park(from);
  showMachine(machine);
  switcher.shown = machine;
  switcher.held = {
    id: machine,
    mode: "offline",
    error,
    tries: held?.id === machine ? held.tries : 0,
    nextAt: 0,
    since: held?.id === machine ? held.since : Date.now(),
  };
  // Its projects are not known here: an empty sidebar under the cover, and a fresh project change on return.
  projects.list = [];
  projects.currentId = null;
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
