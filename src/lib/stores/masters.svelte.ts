/**
 * Servers running Qanuvi without a window (Milestone 16): connecting over SSH or the relay, and what happens when a
 * connection ends. The calls themselves are routed in remote.ts; the sidebar lists only the machine on screen
 * (24z3 Part A2), so a master's projects show once it is shown.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorText, type Project } from "$lib/api";
import { callMaster, disown, forgetMaster, nameMachines, own, shownMachine } from "$lib/remote";
import { planData } from "$lib/plan/planData.svelte";
import { inbox } from "./inbox.svelte";
import { show } from "./machines";
import { projects } from "./projects.svelte";
import { runner } from "./runner.svelte";
import { settings, type MasterSettings } from "./settings.svelte";
import { nameProblem, pickColor, uniqueName } from "$lib/machineNames";

export interface MasterInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
  host: string;
  dataDir: string;
  /** The machine's lasting id (kept in its data folder); an older master has none. */
  machineId?: string;
}

export interface MasterStatus {
  state: "off" | "connecting" | "on" | "error";
  info?: MasterInfo;
  error?: string;
}

/** A machine's name for the screen: as typed, or until it has said its computer name, its address. */
export function displayName(m: MasterSettings): string {
  return m.name || (m.relay ? "A computer" : m.target);
}

// Messages from the router name the machine as the owner named it ("This cannot be done on VPS yet").
nameMachines((id) => {
  const m = settings.value.masters.find((x) => x.id === id);
  return m ? displayName(m) : id;
});

class MastersStore {
  status = $state<Record<string, MasterStatus>>({});
  /** How many projects each connected master has (the Servers screen), whichever machine is shown. */
  projectCount = $state<Record<string, number>>({});
  #listening = false;

  get list(): MasterSettings[] {
    return settings.value.masters;
  }

  statusOf(id: string): MasterStatus {
    return this.status[id] ?? { state: "off" };
  }

  #set(id: string, s: MasterStatus) {
    this.status = { ...this.status, [id]: s };
  }

  async #listen() {
    if (this.#listening) return;
    this.#listening = true;
    // A connection that ended by itself (network gone, server restarted).
    await listen<{ id: string }>("remote-closed", (e) => {
      const id = e.payload.id;
      if (this.statusOf(id).state !== "on") return;
      invoke("remote_disconnect", { id }).catch(() => {});
      this.#dropped(id, "The connection ended. Work on the server goes on; connect again to see it.");
    });
  }

  #dropped(id: string, error?: string) {
    forgetMaster(id);
    // The machine on screen went away: This PC is shown again (forgetMaster already sends calls there).
    if (projects.machine === id || settings.machine === id) void show(null);
    const { [id]: _gone, ...counts } = this.projectCount;
    this.projectCount = counts;
    inbox.dropMaster(id);
    runner.dropMaster(id);
    this.#set(id, error ? { state: "error", error } : { state: "off" });
  }

  /** The other machines' names, as shown (for the name rules). */
  namesBesides(id: string): string[] {
    return this.list.filter((x) => x.id !== id).map(displayName);
  }

  /** Another machine in the list that is the same computer (the same lasting id), if any. */
  twinOf(id: string, machineId?: string): MasterSettings | undefined {
    return machineId ? this.list.find((x) => x.id !== id && x.machineId === machineId) : undefined;
  }

  /**
   * What the first hello fills in: the lasting id, the computer name when the owner gave none, and a colour.
   * The newest copy in the list is used, so a rename made while connecting is kept.
   */
  #fillIn(id: string, info: MasterInfo): void {
    const m = this.list.find((x) => x.id === id);
    if (!m) return;
    const next = { ...m };
    if (info.machineId && m.machineId !== info.machineId) next.machineId = info.machineId;
    if (!m.name && info.host) next.name = uniqueName(info.host, this.namesBesides(id));
    if (!m.color) next.color = pickColor(this.list.filter((x) => x.id !== id).map((x) => x.color));
    if (next.machineId !== m.machineId || next.name !== m.name || next.color !== m.color) this.save(next);
  }

  /**
   * Connects. `fresh` is a machine just added: when it turns out to be a computer already in the list, it is taken
   * out again. Answers an error text for a fresh one ("" when it was added).
   */
  async connect(m: MasterSettings, fresh = false): Promise<string> {
    await this.#listen();
    this.#set(m.id, { state: "connecting" });
    try {
      const info = await invoke<MasterInfo>("remote_connect", {
        id: m.id,
        target: m.relay ? "relay" : m.target,
        program: m.program,
        identity: m.identity || null,
        relay: m.relay ?? null,
      });
      // The same computer added twice (another address, or paired as well as reached over SSH).
      const twin = this.twinOf(m.id, info.machineId);
      if (twin) {
        await invoke("remote_disconnect", { id: m.id }).catch(() => {});
        const said = `This is the same computer as ${displayName(twin)}.`;
        if (fresh) {
          await this.#forget(m);
          return `${said} It was not added again.`;
        }
        this.#dropped(m.id, `${said} Remove one of the two.`);
        return said;
      }
      this.#fillIn(m.id, info);
      // Listing them notes which ids are the master's (remote.ts), so runs and Inbox questions about them go there.
      const list = await callMaster<Project[]>(m.id, "projects_list", {});
      // Projects this computer has itself keep working here, not through the server.
      for (const p of list) if (projects.isLocal(p.id)) disown(p.id);
      const theirs = list.filter((p) => !projects.isLocal(p.id));
      this.projectCount = { ...this.projectCount, [m.id]: theirs.length };
      this.#set(m.id, { state: "on", info });
      // A project window opened for one of its projects shows this machine.
      const want = projects.startWith;
      if (want && !projects.list.some((p) => p.id === want) && theirs.some((p) => p.id === want)) void show(m.id);
      else if (projects.machine === m.id) void projects.load();
      // Its plans' kept data (holds, picks, run settings) lives there, where its runner reads it.
      void planData.loadMaster(m.id);
      // Questions a run on the server asks wait in its own Inbox: shown here with this computer's.
      void inbox.followMaster(m.id);
      // A run going on there (it goes on when no computer is connected) is shown.
      void runner.masterConnected(m.id);
      return "";
    } catch (e) {
      invoke("remote_disconnect", { id: m.id }).catch(() => {});
      this.#dropped(m.id, errorText(e));
      return "";
    }
  }

  /** Takes a machine out of the list (its relay pairing on this device too). */
  async #forget(m: MasterSettings): Promise<void> {
    if (m.relay) await invoke("remote_unpair", { pairing: m.relay.pairing }).catch(() => {});
    this.#dropped(m.id);
    const { [m.id]: _gone, ...rest } = this.status;
    this.status = rest;
    settings.set(
      "masters",
      this.list.filter((x) => x.id !== m.id),
    );
  }

  /** Adds a server and connects to it. Answers an error text ("" when it was added). */
  async add(m: MasterSettings): Promise<string> {
    const problem = m.name ? nameProblem(m.name, this.namesBesides(m.id)) : "";
    if (problem) return problem;
    this.save(m);
    return this.connect(m, true);
  }

  /** Renames a machine (its card on the Servers screen). Answers an error text ("" when renamed). */
  rename(id: string, name: string): string {
    const m = this.list.find((x) => x.id === id);
    if (!m) return "That machine is not in the list.";
    const n = name.trim();
    const problem = nameProblem(n, this.namesBesides(id));
    if (problem) return problem;
    if (n !== m.name) this.save({ ...m, name: n });
    return "";
  }

  setColor(id: string, color: string): void {
    const m = this.list.find((x) => x.id === id);
    if (m && m.color !== color) this.save({ ...m, color });
  }

  /**
   * Restarts a master's service (24z3 D3, a restore booked in its Backups): it answers, starts a new service that
   * waits for it to end, and ends. The window lets the old connection go, connects again and shows the machine again
   * when it was shown. Answers an error text, empty when it is back.
   */
  async restart(id: string): Promise<string> {
    const m = this.list.find((x) => x.id === id);
    if (!m) return "That machine is not in the list.";
    const wasShown = shownMachine() === id;
    try {
      await callMaster(id, "master_restart", {});
    } catch (e) {
      return errorText(e);
    }
    await invoke("remote_disconnect", { id }).catch(() => {});
    this.#dropped(id);
    this.#set(id, { state: "connecting" });
    // The new service waits for the old one to end, then applies the restore before it answers.
    for (let i = 0; i < 4; i++) {
      await new Promise((r) => setTimeout(r, i === 0 ? 1500 : 3000));
      await this.connect(m);
      if (this.statusOf(id).state === "on") {
        if (wasShown) await show(id);
        return "";
      }
    }
    return this.statusOf(id).error ?? `No answer from ${displayName(m)} after the restart.`;
  }

  async disconnect(id: string): Promise<void> {
    await invoke("remote_disconnect", { id }).catch(() => {});
    this.#dropped(id);
  }

  /**
   * Connects the servers marked to connect at start-up, and the ones the app is already connected to (a new window
   * joins the connections of the others). Resolves when all have answered.
   */
  async autoConnect(): Promise<void> {
    const open = await invoke<string[]>("remote_list").catch(() => [] as string[]);
    await Promise.all(this.list.filter((m) => m.autoConnect || open.includes(m.id)).map((m) => this.connect(m)));
  }

  /** Keeps a machine's settings; a machine already in the list keeps its place in it. */
  save(m: MasterSettings): void {
    const known = this.list.some((x) => x.id === m.id);
    settings.set("masters", known ? this.list.map((x) => (x.id === m.id ? m : x)) : [...this.list, m]);
  }

  /** Pairs with a computer showing a pairing code (through the relay) and connects to it. */
  async pair(relay: string, code: string, myName: string): Promise<string> {
    const mine = myName.trim();
    const problem = mine ? nameProblem(mine, [], true) : "";
    if (problem) return `This device's name: ${problem.charAt(0).toLowerCase()}${problem.slice(1)}`;
    try {
      const p = await invoke<{ relay: string; masterId: string; masterName: string; pairing: string }>("remote_pair", {
        relay: relay.trim(),
        code: code.trim(),
        name: mine || "A device",
      });
      const m: MasterSettings = {
        id: crypto.randomUUID(),
        // The name the computer gives itself, made unique; none: its computer name from the first hello.
        name: uniqueName(p.masterName ?? "", this.list.map(displayName)),
        target: "relay",
        program: "",
        identity: "",
        autoConnect: true,
        relay: { url: p.relay, masterId: p.masterId, pairing: p.pairing },
      };
      this.save(m);
      return await this.connect(m, true);
    } catch (e) {
      return errorText(e);
    }
  }

  async remove(id: string): Promise<void> {
    const m = this.list.find((x) => x.id === id);
    if (m?.relay) await invoke("remote_unpair", { pairing: m.relay.pairing }).catch(() => {});
    await this.disconnect(id);
    settings.set(
      "masters",
      this.list.filter((x) => x.id !== id),
    );
  }

  /** Adds a folder on the server as a project; it opens when that machine is on screen. */
  async addFolder(id: string, path: string): Promise<string> {
    try {
      const p = await callMaster<Project>(id, "projects_add", { path: path.trim() });
      own(p.id, id);
      this.projectCount = { ...this.projectCount, [id]: (this.projectCount[id] ?? 0) + 1 };
      if (projects.machine === id) {
        await projects.load();
        projects.select(p.id);
      }
      return "";
    } catch (e) {
      return errorText(e);
    }
  }
}

export const masters = new MastersStore();
