/**
 * Remote access for this computer (Milestone 20): its master service runs on the app's own data and keeps a link to a
 * relay, so paired devices reach this computer's real projects from anywhere. Shows who is connected and stops them.
 *
 * Per machine (24z3 D6): with a master shown, the page is that master's own remote access (its relay link runs in its
 * service, with its own pairings); with This PC shown, This PC's own master service.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { THIS_PC, callMaster, shownMachine } from "$lib/remote";
import { settings } from "./settings.svelte";

/** This PC's own master service: the same link the window's calls about This PC take (M5a). */
const ME = THIS_PC;

/** Whose remote access the page shows: the shown master, or This PC's own master service. */
function target(): string {
  return shownMachine() ?? ME;
}

export interface RelayStatus {
  relay: string;
  enabled: boolean;
  connected: boolean;
  name: string;
  devices: [string, string, number][];
  sessions: { session: number; device: string; since: number }[];
  code: [string, number] | null;
  error: string;
  /** 24rl R3: the road the link took: "own port" (the relay's own TLS port) or "443"; missing from older masters. */
  road?: string;
}

/** What the panel says about a working link (the road only when the master tells it). */
export function connectedText(st: Pick<RelayStatus, "road">): string {
  if (st.road === "own port") return "Connected to the relay (its own port)";
  if (st.road === "443") return "Connected to the relay (through port 443)";
  return "Connected to the relay";
}

/** 24rl A1: Qanuvi's relay, used when no relay is given (the master has the same default). */
export const DEFAULT_RELAY = "wss://relay.qanuvi.com";

/** An empty relay, or Qanuvi's own address typed out, is the default relay. */
export function isDefaultRelay(relay: string | undefined): boolean {
  const r = (relay ?? "").trim().replace(/\/+$/, "").toLowerCase();
  return r === "" || r === DEFAULT_RELAY;
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
  /** Bumped on each switch: an answer about the machine before is dropped. */
  #shown = 0;

  /** Connects to this computer's own master service (starting it when needed); a shown master is connected already. */
  async #attach(): Promise<void> {
    if (shownMachine()) return;
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

  /** The window shows another machine: its remote access, never the machine before's devices or code. */
  async switched(): Promise<void> {
    this.#shown++;
    clearInterval(this.#timer);
    this.status = null;
    this.activity = [];
    this.error = "";
    if (shownMachine()) this.#watch();
    else await this.init();
  }

  #watch() {
    clearInterval(this.#timer);
    const shown = this.#shown;
    const who = target();
    const tick = async () => {
      try {
        const st = await callMaster<RelayStatus>(who, "relay_status", {});
        if (shown === this.#shown) this.status = st;
      } catch {
        if (shown === this.#shown) this.status = null;
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
      // Nothing picked is Qanuvi's relay, sent written out so a master from before 24rl A1 takes it too.
      const r = relay.trim() || DEFAULT_RELAY;
      await callMaster(target(), "relay_configure", { relay: r, name: name.trim(), enabled });
      settings.set("remoteAccess", { enabled, relay: r, name: name.trim() });
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
      await callMaster<string>(target(), "relay_pair_start", {});
      this.status = await callMaster<RelayStatus>(target(), "relay_status", {});
    } catch (e) {
      this.error = errorText(e);
    }
  }

  async removeDevice(pairing: string): Promise<void> {
    await callMaster(target(), "relay_device_remove", { pairing }).catch((e) => (this.error = errorText(e)));
    this.status = await callMaster<RelayStatus>(target(), "relay_status", {}).catch(() => this.status);
  }

  /** Ends every remote connection now. */
  async stop(): Promise<void> {
    await callMaster(target(), "relay_stop", {}).catch((e) => (this.error = errorText(e)));
    this.status = await callMaster<RelayStatus>(target(), "relay_status", {}).catch(() => this.status);
  }

  async loadActivity(): Promise<void> {
    this.activity = await callMaster<Activity[]>(target(), "relay_activity", {}).catch(() => []);
  }
}

export const remoteAccess = new RemoteAccessStore();
