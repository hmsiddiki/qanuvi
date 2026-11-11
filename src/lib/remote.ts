/**
 * Routes the window's calls (Milestone 16): calls for a project, agent session or terminal that lives on a master
 * without a window go to that master over its SSH connection; everything else goes to this PC's own core. The master
 * answers the same commands with the same arguments, so the rest of the interface does not need to know.
 */
import { Channel, invoke } from "@tauri-apps/api/core";

/** Which master owns an id (project, agent session or terminal); ids are unique across machines. */
const owners = new Map<string, string>();

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
}

/** Commands whose `id` argument is a project or a terminal. */
const ID_KEYED = /^(projects_(remove|update|touch|redetect)|term_(write|resize|kill))$/;

/** The master a call belongs to, or null for this PC. */
export function routeOf(cmd: string, args?: Record<string, unknown>): string | null {
  if (!args) return null;
  return ownerOf(args.projectId) ?? ownerOf(args.sessionId) ?? (ID_KEYED.test(cmd) ? ownerOf(args.id) : null);
}

export async function routed<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const master = routeOf(cmd, args);
  return master ? callMaster<T>(master, cmd, args ?? {}) : invoke<T>(cmd, args);
}

/** Sends a call to a master. A `Channel` in `onEvent` receives what it streams (terminal output, agent events). */
export async function callMaster<T>(master: string, cmd: string, args: Record<string, unknown>): Promise<T> {
  const { onEvent, ...rest } = args;
  const streams = onEvent instanceof Channel;
  const result = await invoke<T>("remote_call", {
    master,
    cmd,
    args: rest,
    streams,
    onEvent: streams ? onEvent : new Channel(),
  });
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
