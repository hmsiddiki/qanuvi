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
  /** h4: direct = its answers go over a direct link on the home network now (missing from older masters). */
  sessions: { session: number; device: string; since: number; direct?: boolean }[];
  code: [string, number] | null;
  error: string;
  /** 24rl R3: the road the link took: "own port" (the relay's own TLS port) or "443"; missing from older masters. */
  road?: string;
  /** 24rl F3: today's use as a capped relay told it (missing: no cap, not told yet, or an older master). */
  usage?: RelayUsage | null;
}

/** 24rl F3: relayed bytes today, the relay's daily cap (0 = none), and when slowing ends (unix seconds, 0 = not). */
export interface RelayUsage {
  used: number;
  cap: number;
  slowedUntil: number;
}

/** Bytes as people read them: 120 MB, 2 GB, 512 KB. */
export function sizeText(bytes: number): string {
  const units: [number, string][] = [
    [1 << 30, "GB"],
    [1 << 20, "MB"],
    [1 << 10, "KB"],
  ];
  for (const [n, unit] of units) {
    if (bytes >= n) {
      const v = bytes / n;
      return `${v >= 10 || Number.isInteger(v) ? Math.round(v) : v.toFixed(1)} ${unit}`;
    }
  }
  return `${bytes} B`;
}

/** 24rl F4: the line about today's share of the relay; "" when there is nothing to say (no cap). */
export function usageText(u: RelayUsage | null | undefined, now = Date.now() / 1000): { text: string; over: boolean } {
  if (!u || !u.cap) return { text: "", over: false };
  if (u.slowedUntil > now) {
    const at = new Date(u.slowedUntil * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return {
      text: `Slowed until ${at}: today's share of this relay (${sizeText(u.cap)}) is used. For full speed now, pick a free relay or your own under Relay · Change.`,
      over: true,
    };
  }
  return { text: `Today through the relay: ${sizeText(u.used)} of ${sizeText(u.cap)}`, over: false };
}

/** True when the relay's address names its own port (wss://host:47443, ws://127.0.0.1:47391), or is plain ws://. */
function ownPortAddress(relay: string | undefined): boolean {
  const r = (relay ?? "").trim().toLowerCase();
  if (r.startsWith("ws://")) return true;
  const host = r.replace(/^wss:\/\//, "").split(/[/#?]/)[0];
  const port = host.match(/:(\d+)$/)?.[1];
  return !!port && port !== "443";
}

/** What the panel says about a working link (the road only when the master tells it). fb2: the saved road is
 * "443" only when the saved address is on 443; a relay whose address names another port says no port. */
export function connectedText(st: Pick<RelayStatus, "road"> & { relay?: string }): string {
  if (st.road === "own port") return "Connected to the relay (its own port)";
  if (st.road === "443" && !ownPortAddress(st.relay)) return "Connected to the relay (through port 443)";
  return "Connected to the relay";
}

/** Relay kit K5/K6: the kit for running your own relay, on the update channel (moves to qanuvi.com with it). */
export const RELAY_KIT_URL = "https://qlopy.top/qanuvi-updates/dev/qanuvi-relay-kit-linux-x86_64.tar.gz";

/** 24rl A1: Qanuvi's relay, used when no relay is given (the master has the same default). */
export const DEFAULT_RELAY = "wss://relay.qanuvi.com";

/** An empty relay, or Qanuvi's own address typed out, is the default relay. */
export function isDefaultRelay(relay: string | undefined): boolean {
  const r = (relay ?? "").trim().replace(/\/+$/, "").toLowerCase();
  return r === "" || r === DEFAULT_RELAY;
}

/** 24rl F1/F2: a relay from the signed free relay list, as the shown machine tested it just now. */
export interface FreeRelay {
  url: string;
  name: string;
  runBy: string;
  region: string;
  note: string;
  /** Milliseconds to connect, or null when it could not (then `error` says why). */
  ms: number | null;
  error: string;
}

/** The address with its trailing slash gone and in lower case, to compare relays. */
function same(a: string, b: string): boolean {
  const n = (r: string) => r.trim().replace(/\/+$/, "").toLowerCase();
  return n(a) === n(b);
}

/** 24rl F2: the free relay list, fetched and tested by the shown machine when a relay choice opens. */
class FreeRelays {
  list = $state<FreeRelay[]>([]);
  error = $state("");
  loading = $state(false);

  async load(): Promise<void> {
    if (this.loading) return;
    this.loading = true;
    try {
      const a = await callMaster<{ relays: FreeRelay[]; error: string }>(target(), "free_relays", {});
      this.list = a.relays ?? [];
      this.error = a.error ?? "";
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.loading = false;
    }
  }

  /** The free relay at this address, when it is one on the list. */
  find(relay: string | undefined): FreeRelay | undefined {
    return relay ? this.list.find((r) => same(r.url, relay)) : undefined;
  }
}

export const freeRelays = new FreeRelays();

/** 24rl F2: what the user is told before a free relay is used (said once, answered with a yes). */
export function freeRelayAsk(r: Pick<FreeRelay, "name" | "runBy">): string {
  const who = r.runBy ? `${r.name} (run by ${r.runBy})` : r.name;
  return `${who} will carry this connection. It cannot read anything (it is sealed end to end), but it sees when this computer connects, from which internet address, and how much passes. Devices paired on another relay pair again here. Use it?`;
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

/** h4: who uses this machine now and how each reaches it ("Zorin Laptop · direct on the home network"). */
export function usersText(sessions: { device: string; direct?: boolean }[]): string {
  const road = new Map<string, boolean>();
  for (const s of sessions) road.set(s.device, (road.get(s.device) ?? false) || !!s.direct);
  return [...road]
    .map(([d, direct]) => `${d} · ${direct ? "direct on the home network" : "through the relay"}`)
    .join(", ");
}
