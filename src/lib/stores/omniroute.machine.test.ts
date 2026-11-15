import { beforeEach, describe, expect, it, vi } from "vitest";

// 24z3 D5: OmniRoute follows the shown machine; each core watches its own, the window only shows the shown one's.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const { showMachine } = await import("$lib/remote");
const { omni, moveOldMuted } = await import("./omniroute.svelte");
const { settings, parseSettings } = await import("./settings.svelte");

const conn = (id: string, state = "needs sign-in") => ({
  id,
  provider: "kiro",
  name: "",
  auth: "oauth",
  state,
  problem: "",
});
const nameOf = (c: unknown, args: unknown) => (c === "remote_call" ? (args as { cmd: string }).cmd : c);

beforeEach(() => {
  invoke.mockReset();
  showMachine(null);
  omni.switched();
});

describe("OmniRoute on the shown machine", () => {
  it("asks the shown machine's core to look and drops an answer that comes after a switch", async () => {
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd, args) =>
      nameOf(cmd, args) === "omniroute_look"
        ? cmd === "remote_call"
          ? new Promise((r) => (answer = r))
          : Promise.resolve([conn("pc")])
        : Promise.resolve(null),
    );
    showMachine("vps");
    const late = omni.look();
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "omniroute_look" }),
    );
    showMachine(null);
    omni.switched();
    await omni.look();
    answer([conn("vps")]);
    await late;
    expect(omni.connections?.map((c) => c.id)).toEqual(["pc"]);
    expect(omni.problems.map((c) => c.id)).toEqual(["pc"]);
  });

  it("a switch leaves nothing of the machine before; an older master shows nothing", async () => {
    invoke.mockResolvedValue([conn("pc")]);
    await omni.look();
    omni.note = "kiro answers.";
    omni.catalog = [{ id: "x", name: "X", category: "", hasFree: true, website: "" }];
    showMachine("vps");
    omni.switched();
    expect(omni.connections).toBeNull();
    expect(omni.catalog).toEqual([]);
    expect(omni.note).toBe("");
    invoke.mockRejectedValue("This master does not know the command omniroute_look.");
    await omni.look();
    expect(omni.problems).toEqual([]);
  });

  it("muting is the machine's setting, and its watch looks again at once", async () => {
    invoke.mockImplementation((cmd, args) =>
      Promise.resolve(
        nameOf(cmd, args) === "omniroute_look"
          ? [conn("a"), conn("b")]
          : nameOf(cmd, args) === "settings_get"
            ? {}
            : null,
      ),
    );
    await settings.loadMachine();
    await omni.look();
    omni.setMuted(conn("a") as never, true);
    expect(settings.value.omniMuted).toEqual(["a"]);
    expect(omni.problems.map((c) => c.id)).toEqual(["b"]);
    // The look waits for the muted list to reach the machine: its watch must read the new list.
    const order = () => invoke.mock.calls.map(([c, a]) => nameOf(c, a)).filter((n) => n !== "settings_get");
    await settings.flushed();
    await new Promise((r) => setTimeout(r, 0));
    const calls = order();
    expect(calls).toContain("settings_set");
    expect(calls.lastIndexOf("omniroute_look")).toBeGreaterThan(calls.lastIndexOf("settings_set"));
    expect(calls.filter((n) => n === "omniroute_look").length).toBe(2);
    omni.setMuted(conn("a") as never, false);
    expect(settings.value.omniMuted).toEqual([]);
  });

  it("the window's old muted list moves to This PC's settings once", () => {
    invoke.mockResolvedValue(null);
    settings.set("omniMuted", []);
    kept.set("qanuvi.omni.muted", JSON.stringify(["k1", 5, "k2"]));
    showMachine("vps");
    moveOldMuted();
    expect(kept.has("qanuvi.omni.muted")).toBe(true);
    showMachine(null);
    moveOldMuted();
    expect(settings.value.omniMuted).toEqual(["k1", "k2"]);
    expect(kept.has("qanuvi.omni.muted")).toBe(false);
  });

  it("settings keep a clean muted list of their own", () => {
    const a = parseSettings(JSON.stringify({ omniMuted: ["x", "x", 3, ""] }));
    const b = parseSettings(null);
    expect(a.omniMuted).toEqual(["x"]);
    b.omniMuted.push("y");
    expect(parseSettings(null).omniMuted).toEqual([]);
  });
});
