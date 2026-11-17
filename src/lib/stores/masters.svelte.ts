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
import { show, showOffline } from "./machines";
import { switcher } from "./switcher.svelte";
import { projects } from "./projects.svelte";
import { runner } from "./runner.svelte";
import { terminals } from "./terminals.svelte";
import { settings, type MasterSettings } from "./settings.svelte";
import { nameProblem, pickColor, uniqueName } from "$lib/machineNames";

/** Waits between tries to reach a lost machine (seconds); the last one repeats. */
export const RETRY_WAITS = [2, 4, 8, 15, 30];
/** How long a lost machine's screen stays on, read-only, before it is wiped to "not connected" (ms). */
export const LOST_FOR = 2 * 60_000;

/** The wait before try number `tries` + 1 (ms). */
export function retryWait(tries: number): number {
  return RETRY_WAITS[Math.min(tries, RETRY_WAITS.length - 1)] * 1000;
}

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
  /** This PC's server list could not be read or kept (said on the Servers screen). */
  keepError = $state("");
  #listening = false;
  /** Changes to the kept list go to This PC one after another, in the order they were made. */
  #keeping: Promise<unknown> = Promise.resolve();
  #retryTimer: ReturnType<typeof setTimeout> | null = null;

  get list(): MasterSettings[] {
    return settings.value.masters;
  }

  /**
   * Reads This PC's server list from its database (migration M3a). The first start moves the window's old list in,
   * once only; until it answers (or when it cannot), the window's copy is used.
   */
  async load(): Promise<void> {
    try {
      const kept = await invoke<{ imported: boolean; servers: MasterSettings[] }>("servers_list");
      const list = kept.imported
        ? kept.servers
        : await invoke<MasterSettings[]>("servers_import", { servers: settings.value.masters });
      settings.set("masters", list);
      this.keepError = "";
    } catch (e) {
      this.keepError = `This PC's server list could not be read: ${errorText(e)}`;
    }
  }

  /** Sends one change of the list to This PC's database. */
  #keep(cmd: "servers_save" | "servers_remove", args: Record<string, unknown>): void {
    this.#keeping = this.#keeping
      .then(() => invoke(cmd, args))
      .then(() => {
        this.keepError = "";
      })
      .catch((e) => {
        this.keepError = `This PC could not keep the change to its server list: ${errorText(e)}`;
      });
  }

  /** Waits until the list changes made so far reached This PC (tests). */
  kept(): Promise<unknown> {
    return this.#keeping;
  }

  /** Takes a machine out of the kept list (the window's and This PC's). */
  #drop(id: string): void {
    settings.set(
      "masters",
      this.list.filter((x) => x.id !== id),
    );
    this.#keep("servers_remove", { id });
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
      const said = "The connection ended. Work on the server goes on; connect again to see it.";
      // The machine on screen: it stays, read-only, while it reconnects (24z3 Part E).
      if (shownMachine() === id && !switcher.held) this.#lose(id, said);
      else this.#dropped(id, said);
    });
  }

  #dropped(id: string, error?: string) {
    // A machine held on screen (lost or not connected, Part E) stays shown; its calls fail, never run on This PC.
    const held = switcher.held?.id === id;
    forgetMaster(id, held);
    // The machine on screen went away: This PC is shown again (forgetMaster already sends calls there).
    if (!held && (switcher.shown === id || projects.machine === id || settings.machine === id)) void show(null);
    // Its terminals ended with the connection: their tabs go, and its projects open new ones when shown again.
    terminals.forgetMachine(id);
    const { [id]: _gone, ...counts } = this.projectCount;
    this.projectCount = counts;
    inbox.dropMaster(id);
    runner.dropMaster(id);
    this.#set(id, error ? { state: "error", error } : { state: "off" });
  }

  /**
   * The shown machine's connection ended by itself (Part E): its screen stays, read-only under an overlay, and it is
   * tried again with growing waits. Back: the overlay goes. Not back within LOST_FOR: the screen is wiped to
   * "not connected" with Reconnect.
   */
  #lose(id: string, error: string): void {
    switcher.held = { id, mode: "lost", error: "", tries: 0, nextAt: 0, since: Date.now() };
    // Nothing typed goes into a screen that cannot save it.
    (document.activeElement as HTMLElement | null)?.blur?.();
    this.#dropped(id, error);
    this.#planRetry();
  }

  #planRetry(): void {
    if (this.#retryTimer) clearTimeout(this.#retryTimer);
    this.#retryTimer = null;
    const h = switcher.held;
    if (!h || h.mode !== "lost") return;
    const wait = retryWait(h.tries);
    switcher.held = { ...h, nextAt: Date.now() + wait };
    this.#retryTimer = setTimeout(() => void this.retryNow(), wait);
  }

  /**
   * Tries the held machine again now ("Try now" on the overlay, Reconnect on the "not connected" screen). Answers
   * whether it is back; back, its screen is read again from it.
   */
  async retryNow(): Promise<boolean> {
    if (this.#retryTimer) clearTimeout(this.#retryTimer);
    this.#retryTimer = null;
    const h = switcher.held;
    if (!h) return false;
    const m = this.list.find((x) => x.id === h.id);
    if (!m) {
      switcher.held = null;
      await show(null);
      return false;
    }
    switcher.held = { ...h, nextAt: 0 };
    await this.connect(m);
    // Another machine picked meanwhile, or removed: nothing more to do here.
    const now = switcher.held as typeof h | null;
    if (!now || now.id !== h.id) return this.statusOf(h.id).state === "on";
    if (this.statusOf(h.id).state === "on") {
      // Calls go to it again; its whole screen is read again (a wiped one gets its drafts back with its projects).
      await show(h.id);
      return true;
    }
    const error = this.statusOf(h.id).error ?? "No answer.";
    const next = { ...now, error, tries: now.tries + 1 };
    if (next.mode === "lost" && Date.now() - next.since >= LOST_FOR) {
      switcher.held = next;
      await showOffline(h.id, error);
    } else {
      switcher.held = next;
      this.#planRetry();
    }
    return false;
  }

  /**
   * Shows a machine that did not answer when picked (Part E): "not connected" with Reconnect, never the screen of the
   * machine before taken for it.
   */
  async showUnreachable(id: string): Promise<void> {
    if (this.#retryTimer) clearTimeout(this.#retryTimer);
    this.#retryTimer = null;
    await showOffline(id, this.statusOf(id).error ?? "Not connected.");
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
   * The name a machine without one gets: its computer name, made unique. The test master on this PC (`local`) has this
   * PC's computer name, so it gets "(test)" after it ("DESKTOP-R8G3PAK (test)").
   */
  #ownName(m: MasterSettings, host: string): string {
    return uniqueName(m.target === "local" && !m.relay ? `${host} (test)` : host, this.namesBesides(m.id));
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
    if (!m.name && info.host) next.name = this.#ownName(m, info.host);
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
    this.#drop(m.id);
  }

  /** Stops trying a held machine; the next screen shown replaces its cover. */
  #letGo(): void {
    if (this.#retryTimer) clearTimeout(this.#retryTimer);
    this.#retryTimer = null;
    switcher.held = null;
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

  /**
   * Changes a server on its card: the name (same rules as rename; empty: its computer name) and, for SSH, the
   * address, the program and the key file. Changing how it is reached connects again with the new settings; when
   * that turns out to be another machine already in the list, the change is refused and the old settings stay.
   * Its colour, "Connect when Qanuvi starts" and its place in the list stay. Answers an error text ("" when saved).
   */
  async edit(id: string, change: { name: string; target: string; program: string; identity: string }): Promise<string> {
    const m = this.list.find((x) => x.id === id);
    if (!m) return "That machine is not in the list.";
    const name = change.name.trim();
    const problem = name ? nameProblem(name, this.namesBesides(id)) : "";
    if (problem) return problem;
    const reach = m.relay
      ? { target: m.target, program: m.program, identity: m.identity }
      : {
          target: change.target.trim(),
          program: change.program.trim() || "qanuvi-master",
          identity: change.identity.trim(),
        };
    if (!reach.target)
      return "Give the server's SSH address, for example deploy@203.0.113.5 or an alias from your SSH config.";
    const moved = reach.target !== m.target || reach.program !== m.program || reach.identity !== m.identity;
    const next: MasterSettings = { ...m, ...reach, name };
    if (!moved) {
      // An emptied name: the computer name it already gave, if it is connected (else at its next hello).
      const host = this.statusOf(id).info?.host;
      if (!name && host) next.name = this.#ownName(next, host);
      if (next.name !== m.name) this.save(next);
      return "";
    }
    const wasOn = this.statusOf(id).state === "on";
    const wasShown = shownMachine() === id;
    await this.disconnect(id);
    this.save(next);
    const twin = await this.connect(next);
    if (twin) {
      // Another machine in the list: back to the old settings (and its connection, when it had one).
      this.save(m);
      if (wasOn) await this.connect(m);
      else this.#set(id, { state: "off" });
      return `${twin} The change was not saved.`;
    }
    if (wasShown && this.statusOf(id).state === "on") await show(id);
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
    // Disconnected on purpose (or removed): a held screen of it goes, This PC is shown.
    if (switcher.held?.id === id) this.#letGo();
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
    // The cleaned copy the window now holds is the one kept.
    const clean = this.list.find((x) => x.id === m.id);
    if (clean) this.#keep("servers_save", { server: clean });
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
    // Its Inbox items were kept as "not connected"; a removed machine's leave (they stay on the machine).
    inbox.forgetMaster(id);
    this.#drop(id);
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
