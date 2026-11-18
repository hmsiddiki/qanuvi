/**
 * Servers running Qanuvi without a window (Milestone 16): connecting over SSH or the relay, and what happens when a
 * connection ends. The calls themselves are routed in remote.ts; the sidebar lists only the machine on screen
 * (24z3 Part A2), so a master's projects show once it is shown.
 *
 * M5c part 2: This PC's master holds This PC's server connections (it connects the ones marked to connect at start
 * itself, window or not); the window asks it to connect, follows its `remote_watch` stream for their comings and
 * goings, and its calls to them go through it (remote.rs `through`). Their keys stay their ids. Only the test master
 * next to the app ("local") and remote access ("self") stay the window's own links.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorText, type Project } from "$lib/api";
import {
  THIS_PC,
  callMaster,
  callThisPc,
  disown,
  forgetMaster,
  linkEnded,
  machineName,
  nameMachines,
  own,
  shownMachine,
} from "$lib/remote";
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
  /** It passes calls on to its own servers (migration M3c); an older master does not. */
  forwards?: boolean;
}

export interface MasterStatus {
  state: "off" | "connecting" | "on" | "error";
  info?: MasterInfo;
  error?: string;
  /** A machine reached by another way already (its key): picking this one shows that one. */
  twin?: string;
}

/** What a master says on its `remote_watch` stream when one of its servers comes or goes. */
export interface WatchEvent {
  connected?: string;
  info?: MasterInfo;
  closed?: string;
  error?: string;
  removed?: boolean;
}

/**
 * Machines reached through others (migration M3c): a server of a connected master is a machine of its own in the
 * window, keyed by the path of server ids ("vps1" is one of This PC's servers, "vps1/vps2" one of vps1's). Calls to it
 * go along that path (remote.rs); at most three machines below the window.
 */
export const MAX_DEPTH = 3;

/** The key of the machine a nested one is a server of (null: one of This PC's). */
export function parentOf(key: string): string | null {
  const i = key.lastIndexOf("/");
  return i < 0 ? null : key.slice(0, i);
}

/** How many machines its path holds (1: one of This PC's servers). */
export function depthOf(key: string): number {
  return key.split("/").length;
}

/** The id its parent keeps it under. */
function serverIdOf(key: string): string {
  return key.slice(key.lastIndexOf("/") + 1);
}

/** A machine in the switcher's list: its settings (the id is its key) and how deep it is. */
export interface TreeRow {
  m: MasterSettings;
  depth: number;
}

/**
 * A machine the window links to itself (M5c part 2): the test master next to the app and remote access. Every other
 * of This PC's servers is held by This PC's master.
 */
export function appHeld(m: Pick<MasterSettings, "target" | "relay">): boolean {
  return !m.relay && (m.target.trim() === "local" || m.target.trim() === "self");
}

/** What a server's line says when This PC's master, which held its connection, stopped. */
export const THIS_PC_STOPPED =
  "Qanuvi's background part on this computer stopped, and this connection ended with it. It connects again when it is back.";

/** A machine's name for the screen: as typed, or until it has said its computer name, its address. */
export function displayName(m: MasterSettings): string {
  return m.name || (m.relay ? "A computer" : m.target);
}

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
  /** Each connected master's own server list (migration M3c), by its key. */
  nested = $state<Record<string, MasterSettings[]>>({});
  /** The reading of a master's server list under way, by its key (a window opening on a nested machine waits). */
  #following = new Map<string, Promise<void>>();
  /** Nested machines whose arrival (projects, Inbox, runs) is being read, so it is read once. */
  #arriving = new Set<string>();

  get list(): MasterSettings[] {
    return settings.value.masters;
  }

  /** A machine by its key: one of This PC's servers, or a server of a connected master (its id is the key). */
  find(key: string): MasterSettings | undefined {
    const parent = parentOf(key);
    if (!parent) return this.list.find((x) => x.id === key);
    const m = this.nested[parent]?.find((x) => x.id === serverIdOf(key));
    // A server its master has not named shows its computer name once connected.
    return m ? { ...m, id: key, name: m.name || this.status[key]?.info?.host || "" } : undefined;
  }

  /** Every machine in the switcher's order: This PC's servers, each followed by the ones reached through it. */
  get tree(): TreeRow[] {
    const rows: TreeRow[] = [];
    const add = (key: string, m: MasterSettings, depth: number) => {
      rows.push({ m, depth });
      // A row that is the same computer as another shows its servers under that one only.
      if (depth >= MAX_DEPTH || this.status[key]?.twin) return;
      for (const child of this.nested[key] ?? []) {
        const k = `${key}/${child.id}`;
        const found = this.find(k);
        if (found) add(k, found, depth + 1);
      }
    };
    for (const m of this.list) add(m.id, m, 1);
    return rows;
  }

  /** The keys of every machine connected now (This PC's servers and the ones reached through them). */
  get onKeys(): string[] {
    return Object.keys(this.status).filter((k) => this.status[k].state === "on");
  }

  /** The way to a machine, for sentences and hover: "VPS › build-box". */
  route(key: string): string {
    const parts = key.split("/");
    return parts.map((_, i) => machineName(parts.slice(0, i + 1).join("/"))).join(" › ");
  }

  /**
   * Reads a connected master's own server list and follows it (migration M3c): its servers show under it, each
   * with how it stands, and come and go as that master connects to them. Only a master that passes calls on, and
   * not below the deepest level.
   */
  #follow(key: string, info: MasterInfo): Promise<void> {
    if (!info.forwards || depthOf(key) >= MAX_DEPTH) {
      // A list known from before (it was updated to an older build): nothing shows under it now.
      if (this.nested[key]) {
        const { [key]: _gone, ...lists } = this.nested;
        this.nested = lists;
      }
      return Promise.resolve();
    }
    const run = (async () => {
      try {
        await this.#readNested(key);
        const ch = new Channel<WatchEvent>();
        ch.onmessage = (e) => void this.#watched(key, e);
        const states = await callMaster<Record<string, MasterStatus>>(key, "remote_watch", { onEvent: ch });
        if (this.statusOf(key).state !== "on") return;
        await Promise.all(Object.entries(states).map(([id, st]) => this.#nestedState(`${key}/${id}`, st)));
      } catch {
        // A master with no list of its own (an older one): nothing shows under it.
      }
    })();
    this.#following.set(key, run);
    return run;
  }

  async #readNested(key: string): Promise<void> {
    const kept = await callMaster<{ servers: MasterSettings[] }>(key, "servers_list", {});
    if (this.statusOf(key).state === "on") this.nested = { ...this.nested, [key]: kept.servers };
  }

  /** A master's list changed on its Servers screen (a server added, renamed, coloured): read again. */
  nestedChanged(key: string): void {
    if (this.statusOf(key).state === "on" && this.nested[key]) void this.#readNested(key).catch(() => {});
  }

  /** One of a followed master's servers came or went. */
  async #watched(parent: string, e: WatchEvent): Promise<void> {
    if (this.statusOf(parent).state !== "on") return;
    if (e.connected && e.info) {
      await this.#readNested(parent).catch(() => {});
      await this.#nestedState(`${parent}/${e.connected}`, { state: "on", info: e.info });
    } else if (e.closed) {
      const key = `${parent}/${e.closed}`;
      if (e.removed) return this.#removedNested(key);
      const now = this.statusOf(key);
      if (!e.error) {
        // Disconnected on purpose (here or from another window): it is off.
        if (now.state === "on") this.#dropped(key);
        else this.#set(key, { state: "off" });
      } else if (now.state === "on" && shownMachine() === key && !switcher.held) this.#lose(key, e.error);
      else if (now.state === "on") this.#dropped(key, e.error);
      else if (now.state !== "connecting") this.#set(key, { state: "error", error: e.error });
    }
  }

  /** How a nested machine stands, as its master says: one that came on is read as This PC's servers are. */
  async #nestedState(key: string, st: MasterStatus): Promise<void> {
    if (st.state !== "on" || !st.info) {
      // On with no hello yet (finding n3): still connecting; it comes on with its hello, through #arrived.
      const now: MasterStatus = st.state === "on" ? { state: "connecting" } : st;
      if (this.statusOf(key).state !== "on") this.#set(key, now);
      return;
    }
    if (this.statusOf(key).state === "on" || this.#arriving.has(key)) return;
    this.#arriving.add(key);
    try {
      await this.#arrived(key, st.info);
    } catch (e) {
      this.#dropped(key, errorText(e));
    } finally {
      this.#arriving.delete(key);
    }
  }

  /** Its master took it out of its list: it goes from the switcher, and a screen showing it says why. */
  async #removedNested(key: string): Promise<void> {
    const parent = parentOf(key) ?? "";
    const said = `It was taken out of the server list of ${machineName(parent)}.`;
    const wasShown = shownMachine() === key || switcher.shown === key;
    this.nested = { ...this.nested, [parent]: (this.nested[parent] ?? []).filter((x) => x.id !== serverIdOf(key)) };
    if (switcher.held?.id === key) this.#letGo();
    this.#dropped(key, said);
    inbox.forgetMaster(key);
    const { [key]: _gone, ...rest } = this.status;
    this.status = rest;
    if (wasShown) await showOffline(key, said);
  }

  /**
   * The machine by its key, with every machine on its path connected and their lists read (a window opening on a
   * nested machine). Undefined: it is not in the list it should be in.
   */
  async reach(key: string): Promise<MasterSettings | undefined> {
    const parent = parentOf(key);
    if (!parent) return this.find(key);
    const p = await this.reach(parent);
    if (!p) return undefined;
    if (this.statusOf(parent).state !== "on") await this.connect(p);
    await this.#following.get(parent);
    return this.find(key);
  }

  /**
   * Reads This PC's server list from its database (migration M3a). The first start moves the window's old list in,
   * once only; until it answers (or when it cannot), the window's copy is used.
   */
  async load(): Promise<void> {
    try {
      const kept = await callThisPc<{ imported: boolean; servers: MasterSettings[] }>("servers_list");
      const list = kept.imported
        ? kept.servers
        : await callThisPc<MasterSettings[]>("servers_import", { servers: settings.value.masters });
      settings.set("masters", list);
      this.keepError = "";
    } catch (e) {
      this.keepError = `This PC's server list could not be read: ${errorText(e)}`;
    }
  }

  /** Sends one change of the list to This PC's database. */
  #keep(cmd: "servers_save" | "servers_remove", args: Record<string, unknown>): void {
    this.#keeping = this.#keeping
      .then(() => callThisPc(cmd, args))
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
      // This PC's own link: the app makes it again on the next call. The server connections its master held ended
      // with it (M5c part 2).
      if (id === THIS_PC) return this.#thisPcEnded();
      if (this.statusOf(id).state !== "on") return;
      invoke("remote_disconnect", { id }).catch(() => {});
      const said = "The connection ended. Work on the server goes on; connect again to see it.";
      // The machine on screen: it stays, read-only, while it reconnects (24z3 Part E).
      if (shownMachine() === id && !switcher.held) this.#lose(id, said);
      else this.#dropped(id, said);
    });
  }

  #dropped(id: string, error?: string) {
    // The machines reached through it go with it. Ended by itself (an error): a shown one is held and tried again
    // through it, as a lost one of This PC's is. Disconnected on purpose: they are off, and a shown one goes too.
    // Its last known list stays (finding n4), so they show off under it and can be reached again through it.
    const below = `${id}/`;
    const lostWith = error ? `The connection to ${machineName(id)} ended.` : undefined;
    for (const k of Object.keys(this.status)) {
      if (!k.startsWith(below) || depthOf(k) !== depthOf(id) + 1) continue;
      const on = this.status[k].state === "on" || this.status[k].state === "connecting";
      if (lostWith && this.status[k].state === "on" && shownMachine() === k && !switcher.held) this.#lose(k, lostWith);
      else if (on) this.#dropped(k, lostWith);
      else if (!this.status[k].twin) this.#set(k, { state: "off" });
    }
    this.#following.delete(id);
    // A nested machine's connection is its master's, and one of This PC's servers is This PC's master's (M5c part 2);
    // the app only lets go of its port links to it.
    if (!this.#heldHere(id)) invoke("remote_disconnect", { id }).catch(() => {});
    // Its link ended by itself: whatever waits on it (a terminal's shell) is told, as the app tells of its own.
    if (error && !this.#heldHere(id)) linkEnded(id);
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
    const m = this.find(h.id);
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
    if (parentOf(m.id)) return this.#connectNested(m);
    this.#set(m.id, { state: "connecting" });
    try {
      const target = m.relay ? "relay" : m.target;
      // M5c part 2: This PC's master connects (or answers the connection it holds); only "local" and "self" are the
      // app's own links.
      const info = appHeld(m)
        ? await invoke<MasterInfo>("remote_connect", {
            id: m.id,
            target,
            program: m.program,
            identity: m.identity || null,
            relay: m.relay ?? null,
          })
        : await callThisPc<MasterInfo | null>("remote_connect", { ...m, target });
      // Its hello is not in yet (an older master answering a connect under way): it comes on through the follow.
      if (!info) return "";
      // The same computer added twice (another address, or paired as well as reached over SSH).
      const twin = this.twinOf(m.id, info.machineId);
      if (twin) {
        await this.#letConnectionGo(m.id);
        const said = `This is the same computer as ${displayName(twin)}.`;
        if (fresh) {
          await this.#forget(m);
          return `${said} It was not added again.`;
        }
        this.#dropped(m.id, `${said} Remove one of the two.`);
        return said;
      }
      this.#fillIn(m.id, info);
      // Known from now on, so a nested row of the same computer arriving meanwhile sees it (finding n1).
      this.#set(m.id, { state: "connecting", info });
      await this.#arrived(m.id, info);
      return "";
    } catch (e) {
      void this.#letConnectionGo(m.id);
      this.#dropped(m.id, errorText(e));
      return "";
    }
  }

  /** Whether the app holds the link to this machine itself ("local", "self"; M5c part 2). */
  #heldHere(id: string): boolean {
    if (parentOf(id)) return false;
    const m = this.list.find((x) => x.id === id);
    return !!m && appHeld(m);
  }

  /** The follow of This PC's master's server connections under way (one at a time). */
  #followingThisPc: Promise<void> | null = null;

  /**
   * Follows This PC's master's connections to This PC's servers (M5c part 2): how each stands now, then each coming
   * and going. Asked again after This PC's own link ended (its master restarted), a few times until it answers.
   */
  followThisPc(): Promise<void> {
    if (this.#followingThisPc) return this.#followingThisPc;
    const run = (async () => {
      for (let tries = 0; tries < 6; tries++) {
        try {
          const ch = new Channel<WatchEvent>();
          ch.onmessage = (e) => void this.#thisPcWatched(e);
          const states = await callThisPc<Record<string, MasterStatus>>("remote_watch", { onEvent: ch });
          await Promise.all(Object.entries(states).map(([id, st]) => this.#thisPcState(id, st)));
          return;
        } catch {
          // Its master is starting again (or an older one without servers): asked again shortly.
          await new Promise((r) => setTimeout(r, 2000));
        }
      }
    })().finally(() => {
      this.#followingThisPc = null;
    });
    this.#followingThisPc = run;
    return run;
  }

  /** How one of This PC's servers stands, as This PC's master says (its follow just started). */
  async #thisPcState(id: string, st: MasterStatus): Promise<void> {
    const m = this.list.find((x) => x.id === id);
    if (!m || appHeld(m)) return;
    const now = this.statusOf(id).state;
    if ((st.state === "on" || st.state === "connecting") && now !== "on" && now !== "connecting") {
      // Connected by it already (at its start, or for another window): this window joins it.
      await this.connect(m);
    } else if (st.state === "error" && now !== "on" && now !== "connecting" && !switcher.held) {
      this.#set(id, { state: "error", error: st.error });
    }
  }

  /** One of This PC's servers came or went, as This PC's master says. */
  async #thisPcWatched(e: WatchEvent): Promise<void> {
    const id = e.connected ?? e.closed;
    if (!id) return;
    let m = this.list.find((x) => x.id === id);
    if (!m && e.connected) {
      // Added from another window: the list is read again.
      await this.load();
      m = this.list.find((x) => x.id === id);
    }
    if (!m || appHeld(m)) return;
    const now = this.statusOf(id);
    if (e.connected) {
      if (now.state !== "on" && now.state !== "connecting") await this.connect(m);
      return;
    }
    if (e.removed) {
      // Taken out from another window: it goes here too.
      settings.set(
        "masters",
        this.list.filter((x) => x.id !== id),
      );
      if (switcher.held?.id === id) this.#letGo();
      this.#dropped(id);
      inbox.forgetMaster(id);
      const { [id]: _gone, ...rest } = this.status;
      this.status = rest;
      return;
    }
    if (!e.error) {
      // Disconnected on purpose (here or from another window): it is off.
      if (now.state === "on") this.#dropped(id);
      else if (now.state !== "connecting") this.#set(id, { state: "off" });
    } else if (now.state === "on" && shownMachine() === id && !switcher.held) this.#lose(id, e.error);
    else if (now.state === "on") this.#dropped(id, e.error);
    else if (now.state !== "connecting") this.#set(id, { state: "error", error: e.error });
  }

  /**
   * This PC's own link ended (its master stopped or restarted, M5c part 2): the server connections it held ended
   * with it. A shown one is held and tried again; the others say why. The follow is asked for again.
   */
  #thisPcEnded(): void {
    for (const m of this.list) {
      if (appHeld(m)) continue;
      const st = this.statusOf(m.id).state;
      if (st !== "on" && st !== "connecting") continue;
      if (st === "on" && shownMachine() === m.id && !switcher.held) this.#lose(m.id, THIS_PC_STOPPED);
      else this.#dropped(m.id, THIS_PC_STOPPED);
    }
    setTimeout(() => void this.followThisPc(), 1000);
  }

  /**
   * A machine answered (one of This PC's servers, or one reached through them): its projects, plans' data, Inbox
   * and runs are read as for any machine, and its own servers are followed.
   */
  async #arrived(key: string, info: MasterInfo): Promise<void> {
    // The same computer twice (finding n1) is shown once: a nested row gives way to any other way to it, and This
    // PC's own server takes over from a nested row that came first.
    if (parentOf(key)) {
      const twin = this.#twinOf(key, info);
      if (twin) return this.#becomeTwin(key, twin);
    } else if (info.machineId) {
      for (const k of this.onKeys)
        if (parentOf(k) && this.status[k].info?.machineId === info.machineId) this.#becomeTwin(k, key);
    }
    // Listing them notes which ids are the master's (remote.ts), so runs and Inbox questions about them go there.
    const list = await callMaster<Project[]>(key, "projects_list", {});
    // Projects this computer has itself keep working here, not through the server.
    for (const p of list) if (projects.isLocal(p.id)) disown(p.id);
    const theirs = list.filter((p) => !projects.isLocal(p.id));
    // A way to the same computer came while its projects were read: they stay that one's.
    const late = parentOf(key) ? this.#twinOf(key, info) : undefined;
    if (late)
      return this.#becomeTwin(
        key,
        late,
        theirs.map((p) => p.id),
      );
    this.projectCount = { ...this.projectCount, [key]: theirs.length };
    this.#set(key, { state: "on", info });
    // A project window opened for one of its projects shows this machine.
    const want = projects.startWith;
    if (want && !projects.list.some((p) => p.id === want) && theirs.some((p) => p.id === want)) void show(key);
    else if (projects.machine === key) void projects.load();
    // Its plans' kept data (holds, picks, run settings) lives there, where its runner reads it.
    void planData.loadMaster(key);
    // Questions a run on the server asks wait in its own Inbox: shown here with this computer's.
    void inbox.followMaster(key);
    // A run going on there (it goes on when no computer is connected) is shown.
    void runner.masterConnected(key);
    // The machines it connects to itself show under it (migration M3c).
    void this.#follow(key, info);
  }

  /** Another way to the same computer, connected or answering now (not a row that itself leads elsewhere). */
  #twinOf(key: string, info: MasterInfo): string | undefined {
    if (!info.machineId) return undefined;
    return Object.keys(this.status).find((k) => {
      const st = this.status[k];
      return (
        k !== key &&
        !st.twin &&
        (st.state === "on" || st.state === "connecting") &&
        st.info?.machineId === info.machineId
      );
    });
  }

  /**
   * A nested row turns out to be a computer reached another way: it lets go of what it followed (its projects
   * `ids` go back to that way) and leads there; shown, that way is shown instead.
   */
  #becomeTwin(key: string, twin: string, ids: string[] = []): void {
    const wasShown = shownMachine() === key;
    this.#dropped(key);
    for (const id of ids) own(id, twin);
    this.#set(key, { state: "error", error: `This is the same computer as ${machineName(twin)}.`, twin });
    if (wasShown) void show(twin);
  }

  /**
   * Connects a nested machine: its master connects to it (or answers the connection it has), after the machines on
   * its path are connected themselves. Answers "" (its state says how it went), as for This PC's servers.
   */
  async #connectNested(m: MasterSettings): Promise<string> {
    const key = m.id;
    const parent = parentOf(key) ?? "";
    this.#set(key, { state: "connecting" });
    try {
      if (this.statusOf(parent).state !== "on") {
        const p = await this.reach(parent);
        if (p && this.statusOf(parent).state !== "on") await this.connect(p);
      }
      if (this.statusOf(parent).state !== "on") throw new Error(`${this.route(parent)} is not connected.`);
      const server = this.nested[parent]?.find((x) => x.id === serverIdOf(key));
      if (!server) throw new Error(`It is not in the server list of ${machineName(parent)} any more.`);
      const info = await callMaster<MasterInfo>(parent, "remote_connect", { ...server });
      if (this.statusOf(key).state !== "on") {
        // Its state is set again by what it turns out to be (a twin, or arrived).
        this.#set(key, { state: "off" });
        await this.#nestedState(key, { state: "on", info });
      }
    } catch (e) {
      this.#dropped(key, errorText(e));
    }
    return "";
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
    const m = this.find(id);
    if (!m) return "That machine is not in the list.";
    const wasShown = shownMachine() === id;
    try {
      await callMaster(id, "master_restart", {});
    } catch (e) {
      return errorText(e);
    }
    await this.#letConnectionGo(id);
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

  /**
   * Ends the connection to a machine: for a nested one its master's, for one of This PC's servers This PC's
   * master's (M5c part 2), else the app's own; the app's port links to it go in every case.
   */
  async #letConnectionGo(id: string): Promise<void> {
    const parent = parentOf(id);
    if (parent) await callMaster(parent, "remote_disconnect", { id: serverIdOf(id) }).catch(() => {});
    else if (!this.#heldHere(id)) await callThisPc("remote_disconnect", { id }).catch(() => {});
    await invoke("remote_disconnect", { id }).catch(() => {});
  }

  async disconnect(id: string): Promise<void> {
    await this.#letConnectionGo(id);
    // Disconnected on purpose (or removed): a held screen of it goes, This PC is shown.
    if (switcher.held?.id === id) this.#letGo();
    this.#dropped(id);
  }

  /**
   * Connects the servers marked to connect at start-up, and the ones the app is already connected to (a new window
   * joins the connections of the others). Resolves when all have answered.
   */
  async autoConnect(): Promise<void> {
    await this.#listen();
    // M5c part 2: This PC's master holds This PC's server connections; the app only its own ("local", "self").
    const [mine, held] = await Promise.all([
      invoke<string[]>("remote_list").catch(() => [] as string[]),
      callThisPc<string[]>("remote_list").catch(() => [] as string[]),
    ]);
    const open = [...mine, ...held];
    await Promise.all(this.list.filter((m) => m.autoConnect || open.includes(m.id)).map((m) => this.connect(m)));
    // Then each coming and going of the ones its master holds.
    void this.followThisPc();
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

// Messages from the router name the machine as the owner named it ("This cannot be done on VPS yet").
nameMachines((id) => {
  const m = masters.find(id);
  // A machine reached through others that is not known any more is never named by its path of ids (finding n5).
  return m ? displayName(m) : id.includes("/") ? "that machine" : id;
});
