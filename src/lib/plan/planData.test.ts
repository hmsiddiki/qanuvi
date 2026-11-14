import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  calls: [] as { cmd: string; args: Record<string, unknown> }[],
  rows: [] as Record<string, unknown>[],
  fail: false,
  listener: null as null | ((e: { payload: unknown }) => void),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: Record<string, unknown> = {}) => {
    h.calls.push({ cmd, args });
    if (h.fail) throw "core is gone";
    if (cmd === "plan_data_list") return h.rows;
    return null;
  }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, fn: (e: { payload: unknown }) => void) => {
    h.listener = fn;
    return () => {};
  }),
}));

const store = new Map<string, string>();
vi.stubGlobal("localStorage", {
  get length() {
    return store.size;
  },
  key: (i: number) => [...store.keys()][i] ?? null,
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => store.set(k, v),
  removeItem: (k: string) => store.delete(k),
});

async function fresh() {
  vi.resetModules();
  return (await import("./planData.svelte")).planData;
}

beforeEach(() => {
  h.calls = [];
  h.rows = [];
  h.fail = false;
  h.listener = null;
  store.clear();
});

describe("plan data", () => {
  it("reads the project, file and kind from a key; file paths may have dots", async () => {
    const { parsePlanDataKey } = await import("./planData.svelte");
    expect(parsePlanDataKey("qanuvi.plannotes.p1.content/plugins/qcommerce/PLAN-F.md")).toEqual({
      project_id: "p1",
      file: "content/plugins/qcommerce/PLAN-F.md",
      kind: "notes",
    });
    expect(parsePlanDataKey("qanuvi.run.p1.PLAN.md")?.kind).toBe("run");
    expect(parsePlanDataKey("qanuvi.plan.p1")).toBeNull();
    expect(parsePlanDataKey("qanuvi.picks.p1")).toBeNull();
  });

  it("brings the old localStorage values in once, removes them, and keeps other keys", async () => {
    store.set("qanuvi.planstate.p1.PLAN.md", '{"holds":[{"step":"3","why":"x","by":"you","at":1}]}');
    store.set("qanuvi.plannotes.p1.PLAN.md", '["use staging"]');
    store.set("qanuvi.plan.p1", "PLAN.md");
    const planData = await fresh();
    const started = planData.start();
    // Readable at once, before the core answers.
    expect(planData.getItem("qanuvi.plannotes.p1.PLAN.md")).toBe('["use staging"]');
    await started;
    const imported = h.calls.find((c) => c.cmd === "plan_data_import");
    expect((imported?.args.entries as unknown[]).length).toBe(2);
    expect([...store.keys()]).toEqual(["qanuvi.plan.p1"]);
  });

  it("keeps old values in localStorage when the import fails", async () => {
    store.set("qanuvi.plannotes.p1.PLAN.md", '["a"]');
    h.fail = true;
    const planData = await fresh();
    await planData.start();
    expect(store.has("qanuvi.plannotes.p1.PLAN.md")).toBe(true);
    expect(planData.getItem("qanuvi.plannotes.p1.PLAN.md")).toBe('["a"]');
  });

  it("writes through, and follows changes made elsewhere without an old echo undoing a newer write", async () => {
    h.rows = [{ project_id: "p1", file: "PLAN.md", kind: "run", value: { budget: 1 } }];
    const planData = await fresh();
    await planData.start();
    expect(planData.getItem("qanuvi.run.p1.PLAN.md")).toBe('{"budget":1}');
    planData.setItem("qanuvi.run.p1.PLAN.md", '{"budget":2}');
    planData.setItem("qanuvi.run.p1.PLAN.md", '{"budget":3}');
    const sets = h.calls.filter((c) => c.cmd === "plan_data_set");
    expect(sets.map((c) => c.args.value)).toEqual([{ budget: 2 }, { budget: 3 }]);
    expect(sets[0].args).toMatchObject({ projectId: "p1", file: "PLAN.md", kind: "run" });
    // The first echo comes back after the second write: the copy keeps the newer value.
    h.listener!({ payload: { project_id: "p1", file: "PLAN.md", kind: "run", value: { budget: 2 } } });
    expect(planData.getItem("qanuvi.run.p1.PLAN.md")).toBe('{"budget":3}');
    h.listener!({ payload: { project_id: "p1", file: "PLAN.md", kind: "run", value: { budget: 3 } } });
    // Another window (or the runner in the core) changes it: taken.
    const before = planData.changed;
    h.listener!({ payload: { project_id: "p1", file: "PLAN.md", kind: "notes", value: ["new"] } });
    expect(planData.getItem("qanuvi.plannotes.p1.PLAN.md")).toBe('["new"]');
    expect(planData.changed).toBe(before + 1);
    h.listener!({ payload: { project_id: "p1", file: "PLAN.md", kind: "notes", value: null } });
    expect(planData.getItem("qanuvi.plannotes.p1.PLAN.md")).toBeNull();
  });

  it("removing sends an empty value", async () => {
    const planData = await fresh();
    planData.setItem("qanuvi.picks.p1.PLAN.md", '{"at":1,"picks":{}}');
    planData.removeItem("qanuvi.picks.p1.PLAN.md");
    expect(planData.getItem("qanuvi.picks.p1.PLAN.md")).toBeNull();
    expect(h.calls.at(-1)?.args).toMatchObject({ kind: "picks", value: null });
  });
});
