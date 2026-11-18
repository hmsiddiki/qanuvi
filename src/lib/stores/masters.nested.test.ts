/**
 * Migration M3c: the machines a master connects to itself show under it, keyed by their path ("vps/box"), at most
 * three levels deep, and behave as This PC's servers do: calls along the path, connecting through their master, a
 * lost one held on screen, one taken out of its master's list gone with why, the same computer twice shown once.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (e: unknown) => void = () => {};
  },
  invoke: vi.fn(),
}));
const handlers: Record<string, (e: { payload: { id: string } }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, fn: (e: { payload: { id: string } }) => void) => {
    handlers[name] = fn;
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
const { masters, depthOf, parentOf } = await import("./masters.svelte");
const { settings } = await import("./settings.svelte");
const { switcher } = await import("./switcher.svelte");
const { showMachine } = await import("$lib/remote");

type Args = {
  master: string;
  cmd: string;
  args: Record<string, unknown>;
  onEvent?: { onmessage: (e: unknown) => void };
};

const hello = (machineId: string, host: string, forwards = true) => ({
  name: "Qanuvi master",
  version: "0",
  os: "linux",
  arch: "x86_64",
  host,
  dataDir: "/d",
  machineId,
  forwards,
});
const server = (id: string, name: string) => ({
  id,
  name,
  target: `root@${id}`,
  program: "qanuvi-master",
  identity: "",
  autoConnect: false,
});

/** Each master's own list, how its servers stand, and its watch stream. */
let lists: Record<string, ReturnType<typeof server>[]>;
let states: Record<string, Record<string, unknown>>;
const watches: Record<string, (e: unknown) => void> = {};
let calls: Args[];

const flush = async () => {
  for (let i = 0; i < 10; i++) await new Promise((r) => setTimeout(r, 0));
};

beforeEach(async () => {
  for (const k of Object.keys(masters.status)) await masters.disconnect(k);
  switcher.held = null;
  masters.nested = {};
  lists = { vps: [server("box", "build-box")], "vps/box": [server("deep", "")] };
  states = {
    vps: { box: { state: "on", info: hello("m-box", "box") } },
    "vps/box": { deep: { state: "off" } },
  };
  calls = [];
  settings.set("masters", [server("vps", "VPS")]);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, a?: Args) => {
    if (cmd === "remote_connect") {
      const id = (a as unknown as { id: string }).id;
      return id === "direct" ? hello("m-box", "box", false) : hello("m-vps", "vps");
    }
    if (cmd !== "remote_call" || !a) return null;
    // M5c part 2: This PC's master connects This PC's servers; shown here as the app's own connect was.
    if (a.master === "this-computer" && /^remote_(connect|disconnect)$/.test(a.cmd)) return invoke(a.cmd, a.args);
    calls.push(a);
    if (a.cmd === "servers_list") return { imported: true, servers: lists[a.master] ?? [] };
    if (a.cmd === "remote_watch") {
      if (a.onEvent) watches[a.master] = (e) => a.onEvent!.onmessage(e);
      return states[a.master] ?? {};
    }
    if (a.cmd === "remote_connect") return hello(`m-${a.args.id}`, String(a.args.id));
    return /list|sessions|status|runs|active/.test(a.cmd) ? [] : null;
  }) as never);
  await masters.connect(masters.list[0]);
  await flush();
  machines.show.mockClear();
  machines.showOffline.mockClear();
});

describe("machines reached through a master", () => {
  it("said to be on before its hello is in: connecting, then on with its hello (finding n3)", async () => {
    await masters.disconnect("vps");
    states.vps = { box: { state: "on", info: null } };
    await masters.connect(masters.list[0]);
    await flush();
    expect(masters.statusOf("vps/box").state).toBe("connecting");
    watches.vps({ connected: "box", info: hello("m-box", "box") });
    await flush();
    expect(masters.statusOf("vps/box")).toMatchObject({ state: "on", info: { machineId: "m-box" } });
    expect(calls.some((c) => c.master === "vps/box" && c.cmd === "projects_list")).toBe(true);
  });

  it("show under it, keyed by their path, at most three levels deep", () => {
    expect(masters.tree.map((r) => [r.m.id, r.depth])).toEqual([
      ["vps", 1],
      ["vps/box", 2],
      ["vps/box/deep", 3],
    ]);
    expect(parentOf("vps/box/deep")).toBe("vps/box");
    expect(depthOf("vps/box/deep")).toBe(3);
    expect(masters.statusOf("vps/box").state).toBe("on");
    expect(masters.statusOf("vps/box/deep").state).toBe("off");
    // One its master did not name shows its computer name once connected; the way to it reads as a route.
    expect(masters.route("vps/box")).toBe("VPS › build-box");
    // The one on is read as This PC's servers are: its projects, through the path.
    expect(calls.some((c) => c.master === "vps/box" && c.cmd === "projects_list")).toBe(true);
  });

  it("connects through its master, and the deepest level follows no list of its own", async () => {
    await masters.connect(masters.find("vps/box/deep")!);
    expect(calls).toContainEqual(expect.objectContaining({ master: "vps/box", cmd: "remote_connect" }));
    expect(calls.find((c) => c.cmd === "remote_connect")!.args).toMatchObject({ id: "deep" });
    await flush();
    expect(masters.statusOf("vps/box/deep").state).toBe("on");
    expect(masters.find("vps/box/deep")!.name).toBe("deep");
    expect(calls.some((c) => c.master === "vps/box/deep" && c.cmd === "servers_list")).toBe(false);
  });

  it("an older master that passes nothing on shows nothing under it", async () => {
    states.vps = { box: { state: "on", info: hello("m-box", "box", false) } };
    await masters.disconnect("vps");
    await masters.connect(masters.list[0]);
    await flush();
    expect(masters.tree.map((r) => r.m.id)).toEqual(["vps", "vps/box"]);
  });

  it("lost while shown: held on screen and tried again through its master", async () => {
    showMachine("vps/box");
    switcher.shown = "vps/box";
    watches.vps({ closed: "box", error: "The connection ended." });
    expect(switcher.held).toMatchObject({ id: "vps/box", mode: "lost" });
    // The machines reached through it go with it; its last known list stays, so they show off under it.
    expect(masters.statusOf("vps/box/deep").state).toBe("off");
    expect(masters.tree.map((r) => r.m.id)).toEqual(["vps", "vps/box", "vps/box/deep"]);
    expect(await masters.retryNow()).toBe(true);
    expect(calls.filter((c) => c.master === "vps" && c.cmd === "remote_connect")).toHaveLength(1);
    expect(machines.show).toHaveBeenCalledWith("vps/box");
    switcher.held = null;
  });

  it("taken out of its master's list: gone from the switcher, and its screen says why", async () => {
    showMachine("vps/box");
    switcher.shown = "vps/box";
    lists.vps = [];
    watches.vps({ closed: "box", removed: true });
    await flush();
    expect(masters.tree.map((r) => r.m.id)).toEqual(["vps"]);
    expect(machines.showOffline).toHaveBeenCalledWith("vps/box", "It was taken out of the server list of VPS.");
    showMachine(null);
    switcher.shown = null;
  });

  it("its master's connection ends: the machines under it go too, and stay listed under it (finding n4)", async () => {
    handlers["remote-closed"]({ payload: { id: "vps" } });
    expect(masters.statusOf("vps/box")).toMatchObject({ state: "error", error: "The connection to VPS ended." });
    expect(masters.tree.map((r) => r.m.id)).toEqual(["vps", "vps/box", "vps/box/deep"]);
  });

  it("its master's connection ends while one under it is shown: held, and back through it (finding n4)", async () => {
    showMachine("vps/box");
    switcher.shown = "vps/box";
    handlers["remote-closed"]({ payload: { id: "vps" } });
    expect(switcher.held).toMatchObject({ id: "vps/box", mode: "lost" });
    expect(machines.show).not.toHaveBeenCalledWith(null);
    expect(await masters.retryNow()).toBe(true);
    expect(masters.statusOf("vps").state).toBe("on");
    expect(machines.show).toHaveBeenCalledWith("vps/box");
    switcher.held = null;
  });

  it("its master disconnected on purpose: the one under it shown goes, nothing is tried again", async () => {
    showMachine("vps/box");
    switcher.shown = "vps/box";
    await masters.disconnect("vps");
    expect(switcher.held).toBeNull();
    expect(machines.show).toHaveBeenCalledWith(null);
    expect(masters.statusOf("vps/box").state).toBe("off");
    switcher.shown = null;
  });

  it("the same computer reached another way is shown once", async () => {
    watches["vps/box"]({ connected: "deep", info: hello("m-vps", "vps") });
    await flush();
    expect(masters.statusOf("vps/box/deep")).toMatchObject({ state: "error", twin: "vps" });
  });

  it("reached directly after it was reached through a master: the direct way is kept (finding n1)", async () => {
    settings.set("masters", [server("vps", "VPS"), server("direct", "box direct")]);
    await masters.connect(masters.list[1]);
    await flush();
    expect(masters.statusOf("direct").state).toBe("on");
    expect(masters.statusOf("vps/box")).toMatchObject({ state: "error", twin: "direct" });
    // What was reached through it goes with it.
    expect(masters.tree.map((r) => r.m.id)).toEqual(["vps", "vps/box", "direct"]);
  });

  it("both ways connecting at once: the direct one is kept, the nested row leads to it (finding n1)", async () => {
    await masters.disconnect("vps");
    settings.set("masters", [server("vps", "VPS"), server("direct", "box direct")]);
    await Promise.all([masters.connect(masters.list[0]), masters.connect(masters.list[1])]);
    await flush();
    expect(masters.statusOf("direct").state).toBe("on");
    expect(masters.statusOf("vps/box")).toMatchObject({ state: "error", twin: "direct" });
    expect(masters.onKeys.sort()).toEqual(["direct", "vps"]);
  });
});
