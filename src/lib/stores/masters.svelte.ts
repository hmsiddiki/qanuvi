/**
 * Servers running Qanuvi without a window (Milestone 16): connecting over SSH, their projects in the sidebar, and
 * what happens when a connection ends. The calls themselves are routed in remote.ts.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorText, type Project } from "$lib/api";
import { callMaster, disown, forgetMaster, own } from "$lib/remote";
import { projects } from "./projects.svelte";
import { settings, type MasterSettings } from "./settings.svelte";

export interface MasterInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
  host: string;
  dataDir: string;
}

export interface MasterStatus {
  state: "off" | "connecting" | "on" | "error";
  info?: MasterInfo;
  error?: string;
}

class MastersStore {
  status = $state<Record<string, MasterStatus>>({});
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
    projects.dropRemote(id);
    this.#set(id, error ? { state: "error", error } : { state: "off" });
  }

  async connect(m: MasterSettings): Promise<void> {
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
      const list = await callMaster<Project[]>(m.id, "projects_list", {});
      // Projects this computer has itself keep working here, not through the server.
      for (const p of list) if (projects.isLocal(p.id)) disown(p.id);
      projects.addRemote(m.id, list);
      this.#set(m.id, { state: "on", info });
    } catch (e) {
      invoke("remote_disconnect", { id: m.id }).catch(() => {});
      this.#dropped(m.id, errorText(e));
    }
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

  save(m: MasterSettings): void {
    const others = this.list.filter((x) => x.id !== m.id);
    settings.set("masters", [...others, m]);
  }

  /** Pairs with a computer showing a pairing code (through the relay) and connects to it. */
  async pair(relay: string, code: string, myName: string): Promise<string> {
    try {
      const p = await invoke<{ relay: string; masterId: string; masterName: string; pairing: string }>("remote_pair", {
        relay: relay.trim(),
        code: code.trim(),
        name: myName.trim() || "A device",
      });
      const m: MasterSettings = {
        id: crypto.randomUUID(),
        name: p.masterName || "A computer",
        target: "relay",
        program: "",
        identity: "",
        autoConnect: true,
        relay: { url: p.relay, masterId: p.masterId, pairing: p.pairing },
      };
      this.save(m);
      await this.connect(m);
      return "";
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

  /** Adds a folder on the server as a project and opens it. */
  async addFolder(id: string, path: string): Promise<string> {
    try {
      const p = await callMaster<Project>(id, "projects_add", { path: path.trim() });
      own(p.id, id);
      projects.addRemote(id, [p]);
      projects.select(p.id);
      return "";
    } catch (e) {
      return errorText(e);
    }
  }
}

export const masters = new MastersStore();
