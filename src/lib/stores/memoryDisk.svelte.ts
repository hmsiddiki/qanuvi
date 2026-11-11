/**
 * The memory disk (Milestone 18): where the encrypted copy of the memory is kept, and syncing it. Each machine writes
 * its own file there; syncing takes in what other machines changed (newest wins, deletions included).
 */
import { invoke } from "@tauri-apps/api/core";
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

const LAST_KEY = "qanuvi.memoryDisk.last";
const PERIOD_MS = 10 * 60 * 1000;
const AFTER_CHANGE_MS = 15 * 1000;

class MemoryDiskStore {
  busy = $state(false);
  error = $state("");
  last = $state<SyncReport | null>(null);
  passphraseSaved = $state(false);
  #timer: ReturnType<typeof setInterval> | undefined;
  #soon: ReturnType<typeof setTimeout> | undefined;
  #seenChange = 0;

  get disk(): Disk | null {
    return diskFrom(settings.value.memoryDisk, settings.value.masters);
  }

  async init(): Promise<void> {
    try {
      const saved = localStorage.getItem(LAST_KEY);
      if (saved) this.last = JSON.parse(saved);
    } catch {
      this.last = null;
    }
    this.passphraseSaved = await invoke<boolean>("memory_disk_passphrase_saved").catch(() => false);
    this.#seenChange = memory.changed;
    clearInterval(this.#timer);
    this.#timer = setInterval(() => this.auto(), PERIOD_MS);
    this.auto();
  }

  /** Syncs when a disk and a passphrase are set and syncing is automatic. */
  auto(): void {
    if (settings.value.memoryDisk.auto && this.disk && this.passphraseSaved && !this.busy) this.sync();
  }

  /** Called when the memory changed: syncs a little later (several edits make one sync). */
  changed(): void {
    if (memory.changed === this.#seenChange) return;
    this.#seenChange = memory.changed;
    clearTimeout(this.#soon);
    this.#soon = setTimeout(() => this.auto(), AFTER_CHANGE_MS);
  }

  async sync(): Promise<boolean> {
    const disk = this.disk;
    if (!disk) {
      this.error = "Choose where the memory disk is first.";
      return false;
    }
    this.busy = true;
    this.error = "";
    try {
      const report = await invoke<SyncReport>("memory_disk_sync", { disk });
      this.last = report;
      localStorage.setItem(LAST_KEY, JSON.stringify(report));
      const m = report.merged;
      if (m.added || m.changed || m.deleted) {
        this.#seenChange = memory.changed + 1;
        memory.touch();
      }
      return true;
    } catch (e) {
      this.error = errorText(e);
      return false;
    } finally {
      this.busy = false;
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

  async forgetPassphrase(): Promise<void> {
    await invoke("memory_disk_passphrase_forget").catch(() => {});
    this.passphraseSaved = false;
  }

  async versions(): Promise<DiskVersion[]> {
    const disk = this.disk;
    if (!disk) return [];
    return invoke<DiskVersion[]>("memory_disk_versions", { disk });
  }

  async restore(name: string): Promise<string> {
    const disk = this.disk;
    if (!disk) return "Choose where the memory disk is first.";
    try {
      await invoke<number>("memory_disk_restore", { disk, name });
      memory.touch();
      // Tell the other machines at once.
      await this.sync();
      return "";
    } catch (e) {
      return errorText(e);
    }
  }
}

export const memoryDisk = new MemoryDiskStore();
