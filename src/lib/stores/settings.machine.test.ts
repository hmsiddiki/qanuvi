import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke } = await import("@tauri-apps/api/core");
const { NotOnMachine, showMachine } = await import("$lib/remote");
const { changedKeys, machinePart, settings } = await import("./settings.svelte");
const { show } = await import("./machines");

type Values = Record<string, unknown>;
/** Each machine's settings table: "" is This PC. */
let dbs: Record<string, Values> = {};
let sets: { machine: string; values: Values }[] = [];
/** Machines that are an older version without settings_get. */
let old = new Set<string>();

function answer(machine: string, cmd: string, args: Values): unknown {
  if (old.has(machine) && cmd.startsWith("settings_")) throw `This master does not know the command ${cmd}.`;
  if (cmd === "settings_get") return { ...(dbs[machine] ?? {}) };
  if (cmd === "settings_set") {
    const values = args.values as Values;
    sets.push({ machine, values });
    for (const [k, v] of Object.entries(values)) {
      if (v === null) delete dbs[machine][k];
      else dbs[machine][k] = v;
    }
    return null;
  }
  if (cmd === "projects_list") return [];
  return null;
}

beforeEach(async () => {
  dbs = { "": {}, vps: { approveMinutes: 90, appearance: { editorFontSize: 18, uiScale: 100, minimap: true } } };
  sets = [];
  old = new Set();
  kept.clear();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    const a = (args ?? {}) as Values;
    if (cmd === "remote_call") return answer(a.master as string, a.cmd as string, a.args as Values);
    return answer("", cmd, a);
  });
  showMachine(null);
  settings.load();
  await settings.loadMachine();
  sets = [];
});

describe("settings belong to the shown machine (24z3 A3)", () => {
  it("helpers leave the window's keys out", () => {
    expect(machinePart({ ai: true, masters: [], account: {} })).toEqual({ ai: true });
    expect(
      changedKeys({ ai: true, approveMinutes: 5, masters: [] }, { ai: false, approveMinutes: 5, masters: [1] }),
    ).toEqual({
      ai: false,
    });
    expect(changedKeys({ ai: true }, {})).toEqual({ ai: null });
  });

  it("This PC's first read moves the window's old copy into its database, once", async () => {
    dbs[""] = {};
    kept.set(
      "qanuvi.settings.v1",
      JSON.stringify({ approveMinutes: 42, masters: [{ id: "vps" }], account: { signedIn: true } }),
    );
    settings.load();
    await settings.loadMachine();
    expect(dbs[""]).toEqual({ approveMinutes: 42 });
    expect(settings.value.approveMinutes).toBe(42);
    sets = [];
    await settings.loadMachine();
    expect(sets).toEqual([]);
  });

  it("a change goes to This PC's database as that key only", async () => {
    settings.set("approveMinutes", 25);
    await settings.flushed();
    expect(sets).toEqual([{ machine: "", values: { approveMinutes: 25 } }]);
    expect(JSON.parse(kept.get("qanuvi.settings.v1")!).approveMinutes).toBe(25);
  });

  it("showing a master shows its settings, with the window's own list of masters kept", async () => {
    settings.set("masters", [
      { id: "vps", name: "VPS", target: "x", program: "", identity: "", autoConnect: false },
    ] as never);
    await settings.flushed();
    expect(sets).toEqual([]);
    await show("vps");
    expect(settings.machine).toBe("vps");
    expect(settings.value.approveMinutes).toBe(90);
    expect(settings.value.appearance.editorFontSize).toBe(18);
    expect(settings.value.masters.map((m) => m.id)).toEqual(["vps"]);
  });

  it("a change while a master is shown goes to the master, and This PC's copy stays", async () => {
    settings.set("approveMinutes", 25);
    await settings.flushed();
    await show("vps");
    sets = [];
    settings.set("approveMinutes", 60);
    await settings.flushed();
    expect(sets).toEqual([{ machine: "vps", values: { approveMinutes: 60 } }]);
    expect(dbs[""].approveMinutes).toBe(25);
    expect(JSON.parse(kept.get("qanuvi.settings.v1")!).approveMinutes).toBe(25);
    await show(null);
    expect(settings.value.approveMinutes).toBe(25);
  });

  it("an older master without settings says so and never shows This PC's as its own", async () => {
    settings.set("approveMinutes", 25);
    await settings.flushed();
    old.add("vps");
    await show("vps");
    expect(settings.machine).toBe("vps");
    expect(settings.error).toContain("cannot be done on");
    expect(settings.value.approveMinutes).not.toBe(25);
    await show(null);
    expect(settings.error).toBe("");
    expect(settings.value.approveMinutes).toBe(25);
  });

  it("a change made before the machine answered is kept and sent once it has", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    vi.mocked(invoke).mockImplementationOnce(async () => {
      await gate;
      return { ...dbs[""] };
    });
    showMachine(null);
    const loading = settings.loadMachine();
    settings.set("approveMinutes", 33);
    expect(sets).toEqual([]);
    release();
    await loading;
    await settings.flushed();
    expect(sets).toEqual([{ machine: "", values: { approveMinutes: 33 } }]);
    expect(settings.value.approveMinutes).toBe(33);
  });

  it("NotOnMachine is what the router gives for it", () => {
    expect(new NotOnMachine("vps", "settings_get").message).toContain("settings_get");
  });
});
