/** 24z3 machine names: what the first answer fills in, the same computer added twice, renaming. */
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (m: unknown) => void;
  },
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ listen: async () => {} }) }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke } = await import("@tauri-apps/api/core");
const { displayName, masters } = await import("./masters.svelte");
const { settings } = await import("./settings.svelte");
const { COLORS } = await import("$lib/machineNames");

/** What each server answers in hello, by its SSH address. */
const hellos: Record<string, { host: string; machineId?: string }> = {};
const unpaired: string[] = [];

const server = (id: string, target: string, name = "") => ({
  id,
  name,
  target,
  program: "qanuvi-master",
  identity: "",
  autoConnect: false,
});

beforeEach(() => {
  settings.set("masters", []);
  unpaired.length = 0;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "remote_connect") {
      const h = hellos[(args as { target: string }).target];
      return { name: "Qanuvi master", version: "0", os: "linux", arch: "x86_64", dataDir: "/d", ...h };
    }
    if (cmd === "remote_unpair") unpaired.push((args as { pairing: string }).pairing);
    if (cmd === "remote_pair") return { relay: "wss://r", masterId: "m1", masterName: "", pairing: "p1" };
    if (cmd === "remote_call") {
      const c = (args as { cmd: string }).cmd;
      return /list|sessions|status|runs|active/.test(c) ? [] : null;
    }
    return null;
  }) as never);
});

describe("machine names on the Servers screen", () => {
  it("names a server without a name after its computer, and gives it an id and a colour", async () => {
    hellos["root@vps"] = { host: "basevm378", machineId: "id-vps" };
    expect(await masters.add(server("a", "root@vps"))).toBe("");
    const m = masters.list[0];
    expect(m.name).toBe("basevm378");
    expect(m.machineId).toBe("id-vps");
    expect(m.color).toBe(COLORS[0]);
    expect(masters.statusOf("a").state).toBe("on");
  });

  it("keeps a typed name exactly and refuses a taken one or This PC", async () => {
    hellos["root@vps"] = { host: "basevm378", machineId: "id-vps" };
    expect(await masters.add(server("a", "root@vps", "my vps"))).toBe("");
    expect(masters.list[0].name).toBe("my vps");
    expect(await masters.add(server("b", "other", "My VPS"))).toBe('Another machine is already named "my vps".');
    expect(await masters.add(server("c", "other", "This PC"))).toContain("what Qanuvi calls this computer");
    expect(masters.list.map((x) => x.id)).toEqual(["a"]);
  });

  it("does not add the same computer twice", async () => {
    hellos["root@vps"] = { host: "basevm378", machineId: "same" };
    hellos["vps-alias"] = { host: "basevm378", machineId: "same" };
    await masters.add(server("a", "root@vps"));
    expect(await masters.add(server("b", "vps-alias"))).toBe(
      "This is the same computer as basevm378. It was not added again.",
    );
    expect(masters.list.map((x) => x.id)).toEqual(["a"]);
    expect(
      vi.mocked(invoke).mock.calls.some(([c, a]) => c === "remote_disconnect" && (a as { id: string }).id === "b"),
    ).toBe(true);
  });

  it("refuses the same computer through the relay too, and forgets that pairing", async () => {
    hellos["root@vps"] = { host: "home", machineId: "same" };
    hellos["relay"] = { host: "home", machineId: "same" };
    await masters.add(server("a", "root@vps"));
    expect(await masters.pair("wss://r", "K7QF-29MX-T4HD", "laptop")).toContain("same computer as home");
    expect(masters.list).toHaveLength(1);
    expect(unpaired).toEqual(["p1"]);
  });

  it("checks this device's name before pairing", async () => {
    expect(await masters.pair("wss://r", "K7QF", "x".repeat(41))).toContain("This device's name: the name has 41");
  });

  it("renames on the card with the same rules and keeps the card's place", async () => {
    hellos["one"] = { host: "h1", machineId: "1" };
    hellos["two"] = { host: "h2", machineId: "2" };
    await masters.add(server("a", "one"));
    await masters.add(server("b", "two"));
    expect(masters.rename("a", "h2")).toBe('Another machine is already named "h2".');
    expect(masters.rename("a", "  Build box ")).toBe("");
    expect(masters.list.map(displayName)).toEqual(["Build box", "h2"]);
    masters.setColor("b", COLORS[3]);
    expect(masters.list[1].color).toBe(COLORS[3]);
  });

  it("numbers a computer name another machine already has", async () => {
    hellos["one"] = { host: "basevm378", machineId: "1" };
    hellos["two"] = { host: "basevm378", machineId: "2" };
    await masters.add(server("a", "one"));
    await masters.add(server("b", "two"));
    expect(masters.list.map((x) => x.name)).toEqual(["basevm378", "basevm378 (2)"]);
    expect(masters.list[1].color).toBe(COLORS[1]);
  });
});
