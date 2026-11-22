/** Migration M3b: a shown master's Servers screen reads and changes that master's own list, never This PC's. */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (m: unknown) => void;
  },
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ listen: async () => {} }) }));
const local = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => local.get(k) ?? null,
  setItem: (k: string, v: string) => void local.set(k, v),
  removeItem: (k: string) => void local.delete(k),
  clear: () => local.clear(),
});

const { invoke } = await import("@tauri-apps/api/core");
const { machineServers: ms } = await import("./machineServers.svelte");

const server = (id: string, target: string, name = "") => ({
  id,
  name,
  target,
  program: "qanuvi-master",
  identity: "",
  autoConnect: true,
});

/** The shown master's database and what it was asked (through This PC's remote_call). */
let db: ReturnType<typeof server>[];
let sent: { master: string; cmd: string; args: Record<string, unknown> }[];
/** The stream the master's remote_watch was opened with. */
let watch: { onmessage?: (m: unknown) => void } | null;
let hello: Record<string, unknown>;

beforeEach(() => {
  db = [server("s1", "root@vps", "VPS")];
  sent = [];
  watch = null;
  hello = { host: "shop-box", os: "linux", arch: "x86_64", version: "0.1.25", machineId: "m-shop" };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, a?: Record<string, unknown>) => {
    if (cmd !== "remote_call") throw new Error(`${cmd} went to This PC`);
    const { master, cmd: c, args } = a as { master: string; cmd: string; args: Record<string, unknown> };
    sent.push({ master, cmd: c, args });
    if (c === "servers_list") return { imported: false, servers: structuredClone(db) };
    if (c === "remote_watch") {
      watch = a!.onEvent as typeof watch;
      return { s1: { state: "off" } };
    }
    if (c === "remote_states") return { s1: { state: "off" } };
    if (c === "servers_save") {
      const s = args.server as ReturnType<typeof server>;
      const i = db.findIndex((x) => x.id === s.id);
      if (i >= 0) db[i] = s;
      else db.push(s);
      return null;
    }
    if (c === "servers_remove") {
      db = db.filter((x) => x.id !== args.id);
      return true;
    }
    if (c === "remote_connect") return hello;
    return null;
  }) as never);
});

describe("a shown master's own server list", () => {
  it("reads it from that master and opens the watch once per connection", async () => {
    const conn = {};
    await ms.load("vps-a", conn);
    expect(ms.list.map((s) => s.name)).toEqual(["VPS"]);
    expect(sent.every((s) => s.master === "vps-a")).toBe(true);
    await ms.load("vps-a", conn);
    expect(sent.filter((s) => s.cmd === "remote_watch")).toHaveLength(1);
    expect(sent.filter((s) => s.cmd === "remote_states")).toHaveLength(1);
    // A new connection to it: a new watch.
    await ms.load("vps-a", {});
    expect(sent.filter((s) => s.cmd === "remote_watch")).toHaveLength(2);
  });

  it("adds a server there, fills in its name, id and colour from its hello", async () => {
    await ms.load("vps-a", {});
    expect(await ms.add(server("s2", "deploy@shop"))).toBe("");
    const s2 = db.find((x) => x.id === "s2") as Record<string, unknown>;
    expect(s2.name).toBe("shop-box");
    expect(s2.machineId).toBe("m-shop");
    expect(typeof s2.color).toBe("string");
    expect(ms.statusOf("s2").state).toBe("on");
    // The same computer again under another address is taken out again.
    expect(await ms.add(server("s3", "root@shop-ip"))).toContain("same computer as shop-box");
    expect(db.map((x) => x.id)).toEqual(["s1", "s2"]);
    // A name another server there has is refused before anything is sent.
    const before = sent.length;
    expect(await ms.add(server("s4", "x@y", "vps"))).toContain("already named");
    expect(sent.length).toBe(before);
  });

  it("follows the master's watch: a server that went away, and one removed elsewhere", async () => {
    await ms.load("vps-a", {});
    watch!.onmessage!({ connected: "s1", info: hello });
    expect(ms.statusOf("s1").state).toBe("on");
    watch!.onmessage!({ closed: "s1", error: "The connection ended." });
    expect(ms.statusOf("s1")).toEqual({ state: "error", error: "The connection ended." });
    watch!.onmessage!({ closed: "s1", removed: true });
    expect(ms.list).toEqual([]);
  });

  it("follows a connected server's road: direct at home, the relay again when that drops", async () => {
    await ms.load("vps-a", {});
    watch!.onmessage!({ road: "s1", direct: true });
    expect(ms.statusOf("s1").direct).toBeUndefined();
    watch!.onmessage!({ connected: "s1", info: hello });
    watch!.onmessage!({ road: "s1", direct: true });
    expect(ms.statusOf("s1")).toEqual({ state: "on", info: hello, direct: true });
    watch!.onmessage!({ road: "s1", direct: false });
    expect(ms.statusOf("s1").direct).toBe(false);
  });

  it("removes a paired server with its pairing on that master", async () => {
    db.push({ ...server("p1", "relay"), relay: { url: "wss://r", masterId: "m", pairing: "ab12" } } as never);
    await ms.load("vps-a", {});
    await ms.remove(ms.list.find((x) => x.id === "p1")!);
    expect(sent.slice(-2).map((s) => s.cmd)).toEqual(["remote_unpair", "servers_remove"]);
    expect(sent.at(-2)!.args).toEqual({ pairing: "ab12" });
    expect(ms.list.map((x) => x.id)).toEqual(["s1"]);
  });
});
