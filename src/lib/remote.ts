/**
 * Routes the window's calls (Milestone 16, 24z3 Part A). The window shows one machine at a time: This PC or a paired
 * master. A call about a project, agent session or terminal goes to the machine that made it; any other call goes to
 * the machine on screen. Calls about this window or this install (its sign-in, its list of masters, its banners) stay
 * on This PC. A master answers the same commands with the same arguments, so the rest of the interface does not need
 * to know; a command a master does not answer yet ends in `NotOnMachine`, never on This PC instead.
 *
 * Migration M5a: This PC is a machine like any other. Its calls go to its own master over the "this-computer" link
 * (the app makes it on the first call); only the screen's own commands (WINDOW_ONLY) are answered by the app.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** The link to This PC's own master (desktop.rs THIS_PC); remote access uses it too. */
export const THIS_PC = "this-computer";

/** Which master owns an id (project, agent session or terminal); ids are unique across machines. */
const owners = new Map<string, string>();

/** The machine on screen: null is This PC, otherwise a paired master's id. */
let shown: string | null = null;

/** A machine's name for messages; the masters store sets it. */
let nameOf: (master: string) => string = (master) => master;

export function own(id: string, master: string): void {
  owners.set(id, master);
}

export function ownerOf(id: unknown): string | null {
  return typeof id === "string" ? (owners.get(id) ?? null) : null;
}

/** Calls about this id stay on this computer. */
export function disown(id: string): void {
  owners.delete(id);
}

/**
 * A master's connection ended. Its ids are forgotten; when it was shown, This PC is shown again unless `keepShown`
 * (24z3 Part E: a lost machine stays on screen while it reconnects, and its calls fail rather than run on This PC).
 */
export function forgetMaster(master: string, keepShown = false): void {
  for (const [id, m] of owners) if (m === master) owners.delete(id);
  if (shown === master && !keepShown) shown = null;
}

/** Shows a machine: later calls without an id go to it (null: This PC). */
export function showMachine(master: string | null): void {
  shown = master;
}

export function shownMachine(): string | null {
  return shown;
}

/** Where the shown machine's things are, for sentences: "this computer", or the master's name ("VPS"). */
export function shownPlace(): string {
  return shown ? nameOf(shown) : "this computer";
}

/**
 * A place as a label or heading: "This computer", or the master's name exactly as typed (never capitalised).
 * Sentences are worded so a machine's name never starts one.
 */
export function placeTitle(place: string): string {
  return place === "this computer" ? "This computer" : place;
}

export function shownPlaceTitle(): string {
  return placeTitle(shownPlace());
}

/** A master's name as shown (its id until the masters store names it). */
export function machineName(master: string): string {
  return nameOf(master);
}

export function nameMachines(names: (master: string) => string): void {
  nameOf = names;
}

/**
 * Commands about this window or this install, answered by the app and never sent to a machine (24z3 Part A map,
 * group 3; M5a: exactly master.rs INTERFACE, plus the links to masters): the app itself, the switcher's links, the
 * browser half of a sign-in, desktop notifications, the preview pane's inspector, a file picked on this screen, the
 * data folder and the coding tools.
 */
const WINDOW_ONLY = new Set([
  "app_info",
  "app_restart",
  "data_test_build",
  "startup_path",
  "data_location_move",
  "data_location_cancel",
  "data_location_remove_old",
  "this_pc_master_stop",
  "browser_agent_shot",
  "remote_call",
  "remote_connect",
  "remote_disconnect",
  "remote_list",
  "remote_pair",
  "remote_unpair",
  // The browser half of a sign-in; the machine's own account_* calls go to it (migration M2).
  "account_wait",
  "account_cancel",
  "notify_item",
  "notify_settings",
  "notify_take_start",
  "notify_test",
  "inspector_close",
  "inspector_page_text",
  "inspector_pick",
  "inspector_scroll",
  "inspector_show",
  "attachment_file",
  // Coding tools are the interface's (migration M1); a master's are reached through the app's lsp_* with `master`.
  "lsp_statuses",
  "lsp_install",
  "lsp_start",
  "lsp_send",
  "lsp_stop",
  "lsp_stop_window",
  "dap_statuses",
  "dap_php",
  "dap_install",
  "dap_start",
  "dap_connect",
  "dap_send",
  "dap_stop",
  "dap_stop_window",
  "port_forward",
  "port_unforward",
]);

export function windowOnly(cmd: string): boolean {
  return WINDOW_ONLY.has(cmd);
}

/** Commands whose `id` argument is a project or a terminal. */
const ID_KEYED = /^(projects_(remove|update|touch|redetect)|term_(write|resize|kill|attach|detach))$/;

/** The master that made the call's project, session or terminal, or null when none did. */
export function routeOf(cmd: string, args?: Record<string, unknown>): string | null {
  if (!args) return null;
  return ownerOf(args.projectId) ?? ownerOf(args.sessionId) ?? (ID_KEYED.test(cmd) ? ownerOf(args.id) : null);
}

/** The machine a call goes to: null is This PC. */
export function machineFor(cmd: string, args?: Record<string, unknown>): string | null {
  if (windowOnly(cmd)) return null;
  return routeOf(cmd, args) ?? shown;
}

export async function routed<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (windowOnly(cmd)) return invoke<T>(cmd, args);
  return callOn<T>(machineFor(cmd, args), cmd, args ?? {});
}

/** A call to one machine whatever is shown (24z3 Part B2: a machine being hidden; null is This PC's master). */
export function callOn<T>(machine: string | null, cmd: string, args: Record<string, unknown>): Promise<T> {
  return machine ? callMaster<T>(machine, cmd, args) : callThisPc<T>(cmd, args);
}

/**
 * A call to This PC's master (M5a). Its ids stay This PC's (null owner), as before, so nothing else needs to know the
 * work moved there; a command it does not answer says so as for any machine.
 */
export async function callThisPc<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  return linkCall<T>(THIS_PC, cmd, args);
}

/**
 * Calls `fn` each time This PC's link ended (its master restarted, M5a), a moment later, so a stream it follows is
 * asked for again (the next call makes the link again). The returned promise settles once it listens.
 */
export async function onThisPcLinkEnd(fn: () => void): Promise<void> {
  try {
    await listen<{ id: string }>("remote-closed", (e) => {
      if (e.payload?.id === THIS_PC) setTimeout(fn, 1000);
    });
  } catch {
    // No app events (tests, a browser preview).
  }
}

/** A command the machine does not answer yet (24z3 Part A): said on screen, never done on This PC instead. */
export class NotOnMachine extends Error {
  constructor(
    readonly master: string,
    readonly cmd: string,
  ) {
    super(`This cannot be done on ${master === THIS_PC ? "this computer" : nameOf(master)} yet (${cmd}).`);
    this.name = "NotOnMachine";
  }
}

/** The master's own reply for a command it does not know (master.rs, master_inbox.rs, master_runs.rs). */
const UNKNOWN = /^This master does not know the command [a-z_0-9]+\.$/;

/** Sends a call to a master. A `Channel` in `onEvent` receives what it streams (terminal output, agent events). */
export async function callMaster<T>(master: string, cmd: string, args: Record<string, unknown>): Promise<T> {
  const result = await linkCall<T>(master, cmd, args);
  remember(master, cmd, result);
  return result;
}

/** One call on a link, with what it streams going to the `Channel` in `onEvent`. */
async function linkCall<T>(master: string, cmd: string, args: Record<string, unknown>): Promise<T> {
  const { onEvent, ...rest } = args;
  const streams = onEvent instanceof Channel;
  let result: T;
  try {
    result = await invoke<T>("remote_call", {
      master,
      cmd,
      args: rest,
      streams,
      onEvent: streams ? onEvent : new Channel(),
    });
  } catch (e) {
    throw typeof e === "string" && UNKNOWN.test(e) ? new NotOnMachine(master, cmd) : e;
  }
  return result;
}

/** Notes the ids a master made, so later calls about them go to it. */
export function remember(master: string, cmd: string, result: unknown): void {
  const idOf = (v: unknown) => (v && typeof v === "object" ? (v as { id?: unknown }).id : undefined);
  if ((cmd === "term_spawn" || cmd === "setup_install_start") && typeof result === "string") own(result, master);
  else if (Array.isArray(result) && (cmd === "agent_sessions" || cmd === "projects_list")) {
    for (const item of result) if (typeof idOf(item) === "string") own(idOf(item) as string, master);
  } else if (typeof idOf(result) === "string" && /^(agent_start|projects_(add|update|redetect))$/.test(cmd)) {
    own(idOf(result) as string, master);
  }
}
