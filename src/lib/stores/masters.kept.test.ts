/** Migration M3a: This PC keeps its server list in its database; the window's old list is moved in once. */
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
const { thisPcAsPlainCommands } = await import("$lib/testing/thisPc");
thisPcAsPlainCommands(invoke);
const { masters } = await import("./masters.svelte");
const { settings } = await import("./settings.svelte");

const server = (id: string, target: string, name = "") => ({
  id,
  name,
  target,
  program: "qanuvi-master",
  identity: "",
  autoConnect: false,
});

/** This PC's database, as the commands see it. */
let db: { imported: boolean; servers: ReturnType<typeof server>[] };
let fail = "";
const calls: string[] = [];

beforeEach(() => {
  settings.set("masters", []);
  db = { imported: false, servers: [] };
  fail = "";
  calls.length = 0;
  masters.keepError = "";
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
    calls.push(cmd);
    if (fail && cmd === fail) throw new Error("disk full");
    if (cmd === "servers_list") return structuredClone(db);
    if (cmd === "servers_import") {
      if (!db.imported) db = { imported: true, servers: structuredClone(args!.servers) as typeof db.servers };
      return structuredClone(db.servers);
    }
    if (cmd === "servers_save") {
      const s = args!.server as (typeof db.servers)[0];
      const i = db.servers.findIndex((x) => x.id === s.id);
      if (i >= 0) db.servers[i] = s;
      else db.servers.push(s);
      return null;
    }
    if (cmd === "servers_remove") {
      db.servers = db.servers.filter((x) => x.id !== args!.id);
      return true;
    }
    return null;
  }) as never);
});

describe("This PC's kept server list", () => {
  it("moves the window's old list in on the first start, and only then", async () => {
    settings.set("masters", [server("a", "root@vps", "VPS"), server("b", "local")]);
    await masters.load();
    expect(calls).toEqual(["servers_list", "servers_import"]);
    expect(db.servers.map((s) => s.id)).toEqual(["a", "b"]);
    expect(masters.list.map((m) => m.name)).toEqual(["VPS", ""]);

    // A later start: the kept list is used, an older window copy is not moved in again.
    settings.set("masters", [server("old", "x@y")]);
    calls.length = 0;
    await masters.load();
    expect(calls).toEqual(["servers_list"]);
    expect(masters.list.map((m) => m.id)).toEqual(["a", "b"]);
  });

  it("an empty window list is moved in too, so nothing comes back from it later", async () => {
    await masters.load();
    expect(db).toEqual({ imported: true, servers: [] });
  });

  it("keeps renames and removals on This PC", async () => {
    db = { imported: true, servers: [server("a", "root@vps"), server("b", "local")] };
    await masters.load();
    expect(masters.rename("a", "VPS")).toBe("");
    await masters.remove("b");
    await masters.kept();
    expect(db.servers).toEqual([server("a", "root@vps", "VPS")]);
    expect(masters.keepError).toBe("");
  });

  it("keeps the window's copy and says so when This PC cannot read its list", async () => {
    settings.set("masters", [server("a", "root@vps")]);
    fail = "servers_list";
    await masters.load();
    expect(masters.list.map((m) => m.id)).toEqual(["a"]);
    expect(masters.keepError).toContain("could not be read: disk full");
  });

  it("says when a change could not be kept", async () => {
    db = { imported: true, servers: [server("a", "root@vps")] };
    await masters.load();
    fail = "servers_save";
    masters.setColor("a", "#112233");
    await masters.kept();
    expect(masters.keepError).toContain("could not keep the change");
    fail = "";
    masters.setColor("a", "#445566");
    await masters.kept();
    expect(masters.keepError).toBe("");
    expect(db.servers[0]).toMatchObject({ color: "#445566" });
  });
});
