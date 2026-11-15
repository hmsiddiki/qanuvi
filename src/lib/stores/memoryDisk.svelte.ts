/**
 * The memory disk (Milestone 18): where the encrypted copy of the memory is kept, and syncing it. Each machine writes
 * its own file there; syncing takes in what other machines changed (newest wins, deletions included).
 *
 * Per machine (24z3 D6): the shown machine keeps its own disk (in its settings), its own passphrase and its own last
 * sync, and syncs on its own every 10 minutes, shown or not. The window shows how the shown machine's disk is, starts a
 * sync when asked or soon after the memory changed, and never syncs another machine's memory with This PC's.
 */
import { routed as invoke } from "$lib/remote";
import { errorText } from "$lib/api";
import { settings, type MemoryDiskSettings } from "./settings.svelte";
import { memory } from "./memory.svelte";

export type Disk =
  { kind: "folder"; path: string } | { kind: "server"; target: string; identity: string | null; dir: string };

export interface SyncReport {
  devices: string[];
  merged: { added: number; changed: number; deleted: number };
  written: number;
  at: number;
}

export interface DiskVersion {
  name: string;
  at: number;
}

/** How a machine's memory disk is (memory_disk_status). */
export interface DiskStatus {
  busy: boolean;
  passphraseSaved: boolean;
  last: SyncReport | null;
  error: string;
}

/** The disk the settings describe, or null when there is none (memory kept on this machine only). */
export function diskFrom(
  s: MemoryDiskSettings,
  masters: { id: string; target: string; identity: string }[],
): Disk | null {
  if (s.mode === "folder" && s.folder.trim()) return { kind: "folder", path: s.folder.trim() };
  if (s.mode === "server") {
    const m = masters.find((x) => x.id === s.server);
    if (m && s.serverDir.trim())
      return { kind: "server", target: m.target, identity: m.identity || null, dir: s.serverDir.trim() };
  }
  return null;
}

/**
 * The settings with the chosen server's SSH address and key written in, so the machine finds its disk with no window
 * (the list of servers is the window's own).
 */
export function withServer(
  s: MemoryDiskSettings,
  masters: { id: string; target: string; identity: string }[],
): MemoryDiskSettings {
  const m = s.mode === "server" ? masters.find((x) => x.id === s.server) : undefined;
  return { ...s, target: m?.target ?? "", identity: m?.identity ?? "" };
}

/** How often the window asks the shown machine how its disk is (its own syncs happen without the window). */
const LOOK_MS = 60 * 1000;
const AFTER_CHANGE_MS = 15 * 1000;

class MemoryDiskStore {
  busy = $state(false);
  error = $state("");
  last = $state<SyncReport | null>(null);
  passphraseSaved = $state(false);
  #timer: ReturnType<typeof setInterval> | undefined;
  #soon: ReturnType<typeof setTimeout> | undefined;
  #seenChange = 0;
  /** Bumped on each switch: an answer from the machine before is dropped. */
  #shown = 0;

  get disk(): Disk | null {
    return diskFrom(settings.value.memoryDisk, settings.value.masters);
  }

  async init(): Promise<void> {
    // The window's old copy of This PC's last sync (before D6): the machine keeps it now.
    localStorage.removeItem("qanuvi.memoryDisk.last");
    this.#seenChange = memory.changed;
    clearInterval(this.#timer);
    this.#timer = setInterval(() => void this.look(), LOOK_MS);
    this.#fillServer();
    await this.look();
  }

  /**
   * This PC's disk on a server gets the server's SSH address and key written in (chosen before 24z3 D6, or the address
   * changed since on the Servers page), so This PC's core finds it with no window.
   */
  #fillServer(): void {
    if (settings.machine !== null) return;
    const s = settings.value.memoryDisk;
    if (s.mode !== "server") return;
    const next = withServer(s, settings.value.masters);
    if (next.target !== s.target || next.identity !== s.identity) settings.set("memoryDisk", next);
  }

  /** Asks the shown machine how its memory disk is: last sync, problem, passphrase kept, syncing now. */
  async look(): Promise<void> {
    const shown = this.#shown;
    try {
      const st = await invoke<DiskStatus>("memory_disk_status");
      if (shown !== this.#shown) return;
      this.passphraseSaved = st.passphraseSaved;
      this.last = st.last;
      if (!this.busy) this.error = st.error;
      this.busy = this.busy || st.busy;
      if (st.busy) setTimeout(() => void this.#settle(shown), 2000);
    } catch (e) {
      if (shown !== this.#shown) return;
      // A machine that does not answer yet: its page says so, nothing of This PC's is shown instead.
      this.passphraseSaved = false;
      this.last = null;
      this.error = settings.value.memoryDisk.mode === "off" ? "" : errorText(e);
    }
  }

  /** The machine was syncing on its own: looks again until it is done. */
  async #settle(shown: number): Promise<void> {
    if (shown !== this.#shown) return;
    this.busy = false;
    await this.look();
  }

  /** The window shows another machine: its disk, its passphrase, its last sync. */
  async switched(): Promise<void> {
    this.#shown++;
    clearTimeout(this.#soon);
    this.busy = false;
    this.error = "";
    this.last = null;
    this.passphraseSaved = false;
    this.#seenChange = memory.changed;
    this.#fillServer();
    await this.look();
  }

  /** Syncs now when a disk and a passphrase are set and syncing is automatic (the machine also does every 10 min). */
  auto(): void {
    if (
      settings.value.memoryDisk.auto &&
      settings.value.memoryDisk.mode !== "off" &&
      this.passphraseSaved &&
      !this.busy
    )
      void this.sync();
  }

  /** Called when the memory changed: syncs a little later (several edits make one sync). */
  changed(): void {
    if (memory.changed === this.#seenChange) return;
    this.#seenChange = memory.changed;
    clearTimeout(this.#soon);
    this.#soon = setTimeout(() => this.auto(), AFTER_CHANGE_MS);
  }

  async sync(): Promise<boolean> {
    if (settings.value.memoryDisk.mode === "off") {
      this.error = "Choose where the memory disk is first.";
      return false;
    }
    const shown = this.#shown;
    this.busy = true;
    this.error = "";
    try {
      // The machine reads its disk from its settings: the change just made reaches it first.
      await settings.flushed();
      const report = await invoke<SyncReport>("memory_disk_sync");
      if (shown !== this.#shown) return true;
      this.last = report;
      const m = report.merged;
      if (m.added || m.changed || m.deleted) {
        this.#seenChange = memory.changed + 1;
        memory.touch();
      }
      return true;
    } catch (e) {
      if (shown === this.#shown) this.error = errorText(e);
      return false;
    } finally {
      if (shown === this.#shown) this.busy = false;
    }
  }

  async setPassphrase(passphrase: string): Promise<string> {
    try {
      await invoke("memory_disk_passphrase_set", { passphrase });
      this.passphraseSaved = true;
      return "";
    } catch (e) {
      return errorText(e);
    }
  }

  async forgetPassphrase(): Promise<string> {
    try {
      await invoke("memory_disk_passphrase_forget");
      this.passphraseSaved = false;
      return "";
    } catch (e) {
      return errorText(e);
    }
  }

  async versions(): Promise<DiskVersion[]> {
    if (settings.value.memoryDisk.mode === "off") return [];
    await settings.flushed();
    return invoke<DiskVersion[]>("memory_disk_versions");
  }

  async restore(name: string): Promise<string> {
    if (settings.value.memoryDisk.mode === "off") return "Choose where the memory disk is first.";
    try {
      await settings.flushed();
      // The machine restores, then syncs at once so the other machines hear of it.
      await invoke<number>("memory_disk_restore", { name });
      memory.touch();
      await this.look();
      return "";
    } catch (e) {
      return errorText(e);
    }
  }
}

export const memoryDisk = new MemoryDiskStore();
