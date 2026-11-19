/**
 * The server list of the master on screen (migration M3b). Every machine keeps the servers it connects to in its own
 * database, with their pairing keys, and connects to them itself; the Servers screen of a shown master reads and
 * changes that machine's list through its servers_* and remote_* commands, never This PC's. This PC's own list stays
 * in masters.svelte.ts (its switcher), which also follows each master's list for the machines nested under it (M3c).
 */
import { Channel } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { callOn, machineName } from "$lib/remote";
import { nameProblem, pickColor, uniqueName } from "$lib/machineNames";
import type { MasterSettings } from "./settings.svelte";
import { displayName, masters, type MasterInfo, type MasterStatus } from "./masters.svelte";

/** What a master says on its `remote_watch` stream when one of its servers comes or goes. */
interface WatchEvent {
  connected?: string;
  info?: MasterInfo;
  closed?: string;
  error?: string;
  removed?: boolean;
}

class MachineServersStore {
  /** The master whose list is read (null: none yet). */
  machine = $state<string | null>(null);
  list = $state<MasterSettings[]>([]);
  status = $state<Record<string, MasterStatus>>({});
  error = $state("");
  /** The connection (its hello) each master's watch was opened on: a new connection needs a new watch. */
  #watched = new Map<string, unknown>();

  statusOf(id: string): MasterStatus {
    return this.status[id] ?? { state: "off" };
  }

  #set(id: string, s: MasterStatus): void {
    this.status = { ...this.status, [id]: s };
  }

  /** The other servers' names, as shown (for the name rules). */
  namesBesides(id: string): string[] {
    return this.list.filter((x) => x.id !== id).map(displayName);
  }

  #on(event: WatchEvent, machine: string): void {
    if (this.machine !== machine) return;
    if (event.connected && event.info) this.#set(event.connected, { state: "on", info: event.info });
    else if (event.closed) {
      if (event.removed) this.list = this.list.filter((x) => x.id !== event.closed);
      else this.#set(event.closed, event.error ? { state: "error", error: event.error } : { state: "off" });
    }
  }

  /**
   * Reads a master's list and how each server stands. `connection` is that master's hello as This PC holds it: the
   * watch is opened once per connection (a reconnect opens a new one).
   */
  async load(machine: string, connection: unknown): Promise<void> {
    if (this.machine !== machine) {
      this.list = [];
      this.status = {};
    }
    this.machine = machine;
    this.error = "";
    try {
      const kept = await callOn<{ servers: MasterSettings[] }>(machine, "servers_list", {});
      let states: Record<string, MasterStatus>;
      if (this.#watched.get(machine) !== connection) {
        const ch = new Channel<WatchEvent>();
        ch.onmessage = (e) => this.#on(e, machine);
        states = await callOn(machine, "remote_watch", { onEvent: ch });
        this.#watched.set(machine, connection);
      } else {
        states = await callOn(machine, "remote_states", {});
      }
      if (this.machine !== machine) return;
      this.list = kept.servers;
      this.status = states;
    } catch (e) {
      if (this.machine === machine)
        this.error = `The server list of ${machineName(machine)} could not be read: ${errorText(e)}`;
    }
  }

  #call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
    if (!this.machine) return Promise.reject("No machine is shown.");
    return callOn<T>(this.machine, cmd, args);
  }

  async #save(m: MasterSettings): Promise<void> {
    await this.#call("servers_save", { server: m });
    const known = this.list.some((x) => x.id === m.id);
    this.list = known ? this.list.map((x) => (x.id === m.id ? m : x)) : [...this.list, m];
    if (this.machine) masters.nestedChanged(this.machine);
  }

  /**
   * Has the shown master connect to one of its servers. `fresh` is one just added: when it turns out to be a computer
   * already in the list, it is taken out again. Answers an error text ("" when connected).
   */
  async connect(m: MasterSettings, fresh = false): Promise<string> {
    this.#set(m.id, { state: "connecting" });
    try {
      const info = await this.#call<MasterInfo>("remote_connect", { ...m });
      const twin = info.machineId ? this.list.find((x) => x.id !== m.id && x.machineId === info.machineId) : undefined;
      if (twin) {
        const said = `This is the same computer as ${displayName(twin)}.`;
        if (fresh) {
          await this.remove(m);
          return `${said} It was not added again.`;
        }
        await this.#call("remote_disconnect", { id: m.id }).catch(() => {});
        this.#set(m.id, { state: "error", error: `${said} Remove one of the two.` });
        return said;
      }
      // The first hello fills in its lasting id, its computer name when none was given, and a colour.
      const now = this.list.find((x) => x.id === m.id) ?? m;
      const next = { ...now };
      if (info.machineId) next.machineId = info.machineId;
      if (!next.name && info.host) next.name = uniqueName(info.host, this.namesBesides(m.id));
      if (!next.color) next.color = pickColor(this.list.filter((x) => x.id !== m.id).map((x) => x.color));
      if (next.machineId !== now.machineId || next.name !== now.name || next.color !== now.color)
        await this.#save(next);
      this.#set(m.id, { state: "on", info });
      return "";
    } catch (e) {
      const error = errorText(e);
      this.#set(m.id, { state: "error", error });
      return fresh ? "" : error;
    }
  }

  async disconnect(id: string): Promise<void> {
    // On purpose: the shown master does not connect it again every minute (finding z2).
    await this.#call("remote_disconnect", { id, held: true }).catch(() => {});
    this.#set(id, { state: "off" });
  }

  /** Adds a server to the shown master's list and has it connect. Answers an error text ("" when added). */
  async add(m: MasterSettings): Promise<string> {
    const problem = m.name ? nameProblem(m.name, this.namesBesides(m.id)) : "";
    if (problem) return problem;
    try {
      await this.#save(m);
    } catch (e) {
      return errorText(e);
    }
    return this.connect(m, true);
  }

  /** Pairs the shown master with a computer showing a pairing code, and has it connect. */
  async pair(relay: string, code: string, theirName: string): Promise<string> {
    const mine = theirName.trim();
    const problem = mine ? nameProblem(mine, [], true) : "";
    if (problem) return `The name shown there: ${problem.charAt(0).toLowerCase()}${problem.slice(1)}`;
    try {
      const p = await this.#call<{ relay: string; masterId: string; masterName: string; pairing: string }>(
        "remote_pair",
        { relay: relay.trim(), code: code.trim(), name: mine },
      );
      const m: MasterSettings = {
        id: crypto.randomUUID(),
        name: uniqueName(p.masterName ?? "", this.list.map(displayName)),
        target: "relay",
        program: "",
        identity: "",
        autoConnect: true,
        relay: { url: p.relay, masterId: p.masterId, pairing: p.pairing },
      };
      await this.#save(m);
      return await this.connect(m, true);
    } catch (e) {
      return errorText(e);
    }
  }

  /** Renames a server (empty: its computer name, when it is connected). Answers an error text. */
  async rename(id: string, name: string): Promise<string> {
    const m = this.list.find((x) => x.id === id);
    if (!m) return "That server is not in the list.";
    let n = name.trim();
    const problem = n ? nameProblem(n, this.namesBesides(id)) : "";
    if (problem) return problem;
    const host = this.statusOf(id).info?.host;
    if (!n && host) n = uniqueName(host, this.namesBesides(id));
    if (n === m.name) return "";
    try {
      await this.#save({ ...m, name: n });
      return "";
    } catch (e) {
      return errorText(e);
    }
  }

  async setColor(id: string, color: string): Promise<void> {
    const m = this.list.find((x) => x.id === id);
    if (m && m.color !== color) await this.#save({ ...m, color }).catch((e) => (this.error = errorText(e)));
  }

  async setAutoConnect(id: string, on: boolean): Promise<void> {
    const m = this.list.find((x) => x.id === id);
    if (m) await this.#save({ ...m, autoConnect: on }).catch((e) => (this.error = errorText(e)));
  }

  /** Takes a server out of the shown master's list; its pairing there and its connection end at once. */
  async remove(m: MasterSettings): Promise<void> {
    try {
      if (m.relay) await this.#call("remote_unpair", { pairing: m.relay.pairing });
      await this.#call("servers_remove", { id: m.id });
      this.list = this.list.filter((x) => x.id !== m.id);
      const { [m.id]: _gone, ...rest } = this.status;
      this.status = rest;
      if (this.machine) masters.nestedChanged(this.machine);
    } catch (e) {
      this.error = errorText(e);
    }
  }
}

export const machineServers = new MachineServersStore();
