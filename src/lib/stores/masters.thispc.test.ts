/**
 * M5c part 2: This PC's master holds This PC's server connections. The window asks it to connect them, follows its
 * watch stream for their comings and goings, and loses them when This PC's own link ends; only the test master next to
 * the app ("local") and remote access ("self") stay the app's own links.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (e: unknown) => void = () => {};
  },
  invoke: vi.fn(),
}));
const handlers: Record<string, ((e: { payload: { id: string } }) => void)[]> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, fn: (e: { payload: { id: string } }) => void) => {
    (handlers[name] ??= []).push(fn);
    return () => {};
  }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ listen: async () => {} }) }));
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} });
vi.stubGlobal("document", { activeElement: null });

const machines = {
  show: vi.fn(async (_m: string | null) => {}),
  showOffline: vi.fn(async (_m: string, _e: string) => {}),
};
vi.mock("./machines", () => machines);

const { invoke } = await import("@tauri-apps/api/core");
const { masters, appHeld, THIS_PC_STOPPED } = await import("./masters.svelte");
const { settings } = await import("./settings.svelte");
const { switcher } = await import("./switcher.svelte");
const { onLinkEnd, showMachine } = await import("$lib/remote");

type Call = {
  master: string;
  cmd: string;
  args: Record<string, unknown>;
  onEvent?: { onmessage: (e: unknown) => void };
};

const hello = (machineId: string, host: string) => ({
  name: "Qanuvi master",
  version: "0",
  os: "linux",
  arch: "x86_64",
  host,
  dataDir: "/d",
  machineId,
  forwards: true,
});
const vps = { id: "vps", name: "VPS", target: "root@vps", program: "qanuvi-master", identity: "", autoConnect: false };
const local = { id: "loc", name: "Test", target: "local", program: "", identity: "", autoConnect: false };

/** How This PC's master says its servers stand, and its watch stream. */
let thisPcStates: Record<string, unknown>;
let thisPcWatch: ((e: unknown) => void) | null;
let thisPcCalls: Call[];

const flush = async () => {
  for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));
};

beforeEach(async () => {
  for (const k of Object.keys(masters.status)) await masters.disconnect(k);
  switcher.held = null;
  showMachine(null);
  switcher.shown = null;
  thisPcStates = {};
  thisPcWatch = null;
  thisPcCalls = [];
  settings.set("masters", [vps, local]);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, a?: Call) => {
    if (cmd === "remote_connect") return hello("m-local", "pc");
    if (cmd !== "remote_call" || !a) return null;
    if (a.master === "this-computer") {
      thisPcCalls.push(a);
      if (a.cmd === "remote_connect") return hello(`m-${a.args.id}`, String(a.args.id));
      if (a.cmd === "remote_watch") {
        if (a.onEvent) thisPcWatch = (e) => a.onEvent!.onmessage(e);
        return thisPcStates;
      }
      if (a.cmd === "servers_list") return { imported: true, servers: settings.value.masters };
    }
    return /list|sessions|status|runs|active/.test(a.cmd) ? [] : null;
  }) as never);
  machines.show.mockClear();
  machines.showOffline.mockClear();
});

const appConnects = () => vi.mocked(invoke).mock.calls.filter(([c]) => c === "remote_connect");

describe("This PC's servers are held by This PC's master", () => {
  it("knows which machines the window links to itself", () => {
    expect(appHeld(local)).toBe(true);
    expect(appHeld({ target: "self" })).toBe(true);
    expect(appHeld(vps)).toBe(false);
    expect(appHeld({ target: "local", relay: { url: "u", masterId: "m", pairing: "p" } })).toBe(false);
  });

  it("connects a server through This PC's master, and the test master next to the app itself", async () => {
    await masters.connect(vps);
    expect(masters.statusOf("vps")).toMatchObject({ state: "on", info: { machineId: "m-vps" } });
    expect(thisPcCalls.some((c) => c.cmd === "remote_connect" && c.args.id === "vps")).toBe(true);
    expect(appConnects()).toHaveLength(0);
    await masters.connect(local);
    expect(masters.statusOf("loc").state).toBe("on");
    expect(appConnects()).toHaveLength(1);
    expect(thisPcCalls.some((c) => c.cmd === "remote_connect" && c.args.id === "loc")).toBe(false);
    // Disconnected on purpose: This PC's master lets its connection go.
    await masters.disconnect("vps");
    expect(thisPcCalls.some((c) => c.cmd === "remote_disconnect" && c.args.id === "vps")).toBe(true);
    expect(masters.statusOf("vps").state).toBe("off");
  });

  it("joins a connection This PC's master already holds when it starts following", async () => {
    thisPcStates = { vps: { state: "on", info: hello("m-vps", "vps") }, loc: { state: "off" } };
    await masters.followThisPc();
    await flush();
    expect(masters.statusOf("vps").state).toBe("on");
    // The test master is the window's own: what This PC's master says of it is not used.
    expect(masters.statusOf("loc").state).toBe("off");
  });

  it("follows its watch: a connection ended by itself is said, and a terminal on it is told", async () => {
    await masters.followThisPc();
    await masters.connect(vps);
    let ended = 0;
    await onLinkEnd("vps", () => ended++);
    thisPcWatch!({ closed: "vps", error: "The connection ended." });
    await flush();
    expect(masters.statusOf("vps")).toMatchObject({ state: "error", error: "The connection ended." });
    expect(ended).toBe(1);
    // Connected again by This PC's master (its minute retry): it comes back on here.
    thisPcWatch!({ connected: "vps", info: hello("m-vps", "vps") });
    await flush();
    expect(masters.statusOf("vps").state).toBe("on");
  });

  it("a link ended by itself keeps the terminal tabs; Disconnect takes them and holds the server off (z1, z2)", async () => {
    const { terminals } = await import("./terminals.svelte");
    const tab = (key: string) => ({ key, projectId: "p", machine: "vps", label: "Server terminal" }) as never;
    await masters.followThisPc();
    await masters.connect(vps);
    terminals.tabs = [tab("t1")];
    thisPcWatch!({ closed: "vps", error: "The connection ended." });
    await flush();
    // z1: the tab stays and says its shell ended (j1), instead of going without a word.
    expect(terminals.tabs.map((t) => t.key)).toEqual(["t1"]);
    thisPcWatch!({ connected: "vps", info: hello("m-vps", "vps") });
    await flush();
    // z2: Disconnect on purpose asks This PC's master to hold it off its minute retry, and the tabs go.
    await masters.disconnect("vps");
    expect(thisPcCalls.some((c) => c.cmd === "remote_disconnect" && c.args.id === "vps" && c.args.held)).toBe(true);
    expect(terminals.tabs).toEqual([]);
  });

  it("a shown server whose connection ended stays shown under cover and is tried again", async () => {
    await masters.followThisPc();
    await masters.connect(vps);
    showMachine("vps");
    switcher.shown = "vps";
    thisPcWatch!({ closed: "vps", error: "The connection ended." });
    await flush();
    expect(switcher.held).toMatchObject({ id: "vps", mode: "lost" });
    switcher.held = null;
  });

  it("This PC's own master stopped: the cover over its shown server says so (finding w1)", async () => {
    await masters.followThisPc();
    await masters.connect(vps);
    showMachine("vps");
    switcher.shown = "vps";
    vi.useFakeTimers({ toFake: ["setTimeout"] });
    try {
      for (const fn of handlers["remote-closed"] ?? []) fn({ payload: { id: "this-computer" } });
      expect(switcher.held).toMatchObject({ id: "vps", mode: "lost", why: THIS_PC_STOPPED });
    } finally {
      vi.useRealTimers();
    }
    switcher.held = null;
  });

  it("taken out from another window: it goes here too", async () => {
    await masters.followThisPc();
    await masters.connect(vps);
    thisPcWatch!({ closed: "vps", removed: true });
    await flush();
    expect(masters.list.map((m) => m.id)).toEqual(["loc"]);
    expect(masters.status.vps).toBeUndefined();
  });

  it("shows a connection's road: direct at home, back to the relay, and news that came before it was on (sl1 S3)", async () => {
    await masters.followThisPc();
    // The direct link is up a moment after hello, sometimes before this window marks the machine on.
    thisPcWatch!({ road: "vps", direct: true });
    await masters.connect(vps);
    expect(masters.statusOf("vps")).toMatchObject({ state: "on", direct: true });
    thisPcWatch!({ road: "vps", direct: false });
    await flush();
    expect(masters.statusOf("vps").direct).toBe(false);
    thisPcWatch!({ road: "vps", direct: true });
    await flush();
    expect(masters.statusOf("vps").direct).toBe(true);
    // Disconnected: the road is forgotten, so a new connection starts as relayed until told.
    await masters.disconnect("vps");
    await masters.connect(vps);
    expect(masters.statusOf("vps").direct).toBeUndefined();
  });

  it("This PC's own link ended: the servers its master held go with why, the window's own stay", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout"] });
    try {
      await masters.connect(vps);
      await masters.connect(local);
      let ended = 0;
      await onLinkEnd("vps", () => ended++);
      for (const fn of handlers["remote-closed"] ?? []) fn({ payload: { id: "this-computer" } });
      expect(masters.statusOf("vps")).toMatchObject({ state: "error", error: THIS_PC_STOPPED });
      expect(masters.statusOf("loc").state).toBe("on");
      expect(ended).toBe(1);
      // The follow is asked for again a moment later.
      const watches = thisPcCalls.filter((c) => c.cmd === "remote_watch").length;
      await vi.advanceTimersByTimeAsync(1100);
      expect(thisPcCalls.filter((c) => c.cmd === "remote_watch").length).toBe(watches + 1);
    } finally {
      vi.useRealTimers();
    }
  });
});
