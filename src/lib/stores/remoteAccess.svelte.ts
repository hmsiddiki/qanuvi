/**
 * Remote access for this computer (Milestone 20): its master service runs on the app's own data and keeps a link to a
 * relay, so paired devices reach this computer's real projects from anywhere. Shows who is connected and stops them.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { callMaster } from "$lib/remote";
import { settings } from "./settings.svelte";

const ME = "this-computer";

export interface RelayStatus {
  relay: string;
  enabled: boolean;
  connected: boolean;
  name: string;
  devices: [string, string, number][];
  sessions: { session: number; device: string; since: number }[];
  code: [string, number] | null;
  error: string;
}

export interface Activity {
  at: number;
  device: string;
  what: string;
}

class RemoteAccessStore {
  status = $state<RelayStatus | null>(null);
  activity = $state<Activity[]>([]);
  error = $state("");
  busy = $state(false);
  #timer: ReturnType<typeof setInterval> | undefined;

  /** Connects to this computer's own master service (starting it when needed). */
  async #attach(): Promise<void> {
    await invoke("remote_connect", { id: ME, target: "self", program: "", identity: null, relay: null });
  }

  /** At start-up: when remote access is on, make sure the service runs, and watch it. */
  async init(): Promise<void> {
    if (!settings.value.remoteAccess.enabled) return;
    try {
      await this.#attach();
      this.#watch();
    } catch (e) {
      this.error = errorText(e);
    }
  }

  #watch() {
    clearInterval(this.#timer);
    const tick = async () => {
      try {
        this.status = await callMaster<RelayStatus>(ME, "relay_status", {});
      } catch {
        this.status = null;
      }
    };
    tick();
    this.#timer = setInterval(tick, 4000);
  }

  async set(enabled: boolean, relay: string, name: string): Promise<void> {
    this.busy = true;
    this.error = "";
    try {
      await this.#attach();
      await callMaster(ME, "relay_configure", { relay: relay.trim(), name: name.trim(), enabled });
      settings.set("remoteAccess", { enabled, relay: relay.trim(), name: name.trim() });
      this.#watch();
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.busy = false;
    }
  }

  async pairStart(): Promise<void> {
    this.error = "";
    try {
      await callMaster<string>(ME, "relay_pair_start", {});
      this.status = await callMaster<RelayStatus>(ME, "relay_status", {});
    } catch (e) {
      this.error = errorText(e);
    }
  }

  async removeDevice(pairing: string): Promise<void> {
    await callMaster(ME, "relay_device_remove", { pairing }).catch((e) => (this.error = errorText(e)));
    this.status = await callMaster<RelayStatus>(ME, "relay_status", {}).catch(() => this.status);
  }

  /** Ends every remote connection now. */
  async stop(): Promise<void> {
    await callMaster(ME, "relay_stop", {}).catch((e) => (this.error = errorText(e)));
    this.status = await callMaster<RelayStatus>(ME, "relay_status", {}).catch(() => this.status);
  }

  async loadActivity(): Promise<void> {
    this.activity = await callMaster<Activity[]>(ME, "relay_activity", {}).catch(() => []);
  }
}

export const remoteAccess = new RemoteAccessStore();
