/**
 * Routes the window's calls (Milestone 16, 24z3 Part A). The window shows one machine at a time: This PC or a paired
 * master. A call about a project, agent session or terminal goes to the machine that made it; any other call goes to
 * the machine on screen. Calls about this window or this install (its sign-in, its list of masters, its banners) stay
 * on This PC. A master answers the same commands with the same arguments, so the rest of the interface does not need
 * to know; a command a master does not answer yet ends in `NotOnMachine`, never on This PC instead.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

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

export function forgetMaster(master: string): void {
  for (const [id, m] of owners) if (m === master) owners.delete(id);
  if (shown === master) shown = null;
}

/** Shows a machine: later calls without an id go to it (null: This PC). */
export function showMachine(master: string | null): void {
  shown = master;
}

export function shownMachine(): string | null {
  return shown;
}

export function nameMachines(names: (master: string) => string): void {
  nameOf = names;
}

/**
 * Commands about this window or this install, never sent to a machine (24z3 Part A map, group 3): the app itself,
 * the switcher's list of masters, the account sign-in, desktop banners, the preview pane, a file picked on this
 * screen, one-time moves out of old window storage, and stopping this window's own terminals and servers.
 */
const WINDOW_ONLY = new Set([
  "app_info",
  "app_restart",
  "data_test_build",
  "remote_call",
  "remote_connect",
  "remote_disconnect",
  "remote_list",
  "remote_pair",
  "remote_unpair",
  "account_cancel",
  "account_me",
  "account_sign_in",
  "account_sign_out",
  "notify_item",
  "notify_take_start",
  "notify_test",
  "inspector_close",
  "inspector_page_text",
  "inspector_pick",
  "inspector_scroll",
  "inspector_show",
  "attachment_file",
  "knowledge_fetch",
  "inbox_import",
  "plan_data_import",
  "term_kill_window",
  "lsp_stop_window",
  "dap_stop_window",
]);

export function windowOnly(cmd: string): boolean {
  return WINDOW_ONLY.has(cmd);
}

/** Commands whose `id` argument is a project or a terminal. */
const ID_KEYED = /^(projects_(remove|update|touch|redetect)|term_(write|resize|kill))$/;

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
  const master = machineFor(cmd, args);
  return master ? callMaster<T>(master, cmd, args ?? {}) : invoke<T>(cmd, args);
}

/** A command the machine does not answer yet (24z3 Part A): said on screen, never done on This PC instead. */
export class NotOnMachine extends Error {
  constructor(
    readonly master: string,
    readonly cmd: string,
  ) {
    super(`${nameOf(master)} cannot do this yet (${cmd}).`);
    this.name = "NotOnMachine";
  }
}

/** The master's own reply for a command it does not know (master.rs, master_inbox.rs, master_runs.rs). */
const UNKNOWN = /^This master does not know the command [a-z_0-9]+\.$/;

/** Sends a call to a master. A `Channel` in `onEvent` receives what it streams (terminal output, agent events). */
export async function callMaster<T>(master: string, cmd: string, args: Record<string, unknown>): Promise<T> {
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
  remember(master, cmd, result);
  return result;
}

/** Notes the ids a master made, so later calls about them go to it. */
export function remember(master: string, cmd: string, result: unknown): void {
  const idOf = (v: unknown) => (v && typeof v === "object" ? (v as { id?: unknown }).id : undefined);
  if (cmd === "term_spawn" && typeof result === "string") own(result, master);
  else if (Array.isArray(result) && (cmd === "agent_sessions" || cmd === "projects_list")) {
    for (const item of result) if (typeof idOf(item) === "string") own(idOf(item) as string, master);
  } else if (typeof idOf(result) === "string" && /^(agent_start|projects_(add|update|redetect))$/.test(cmd)) {
    own(idOf(result) as string, master);
  }
}
