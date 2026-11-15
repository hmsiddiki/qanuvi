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
const { forgetMaster, ownerOf, showMachine } = await import("$lib/remote");
const { currentKey, projects } = await import("./projects.svelte");

/** Shows a machine for the project list alone (machines.ts also loads its settings). */
const showOnly = (m: string | null) => {
  showMachine(m);
  return projects.load();
};

const project = (id: string) => ({ id, name: id, path: `/p/${id}`, kind: "folder" });

/** Projects on each machine: "" is This PC. */
let on: Record<string, ReturnType<typeof project>[]> = {};

beforeEach(() => {
  on = { "": [project("here-1"), project("here-2")], vps: [project("vps-1"), project("here-1")] };
  localStorage.clear();
  showMachine(null);
  projects.startWith = null;
  projects.persist = true;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === "remote_call") {
      const a = args as { master: string; cmd: string; args: Record<string, unknown> };
      if (a.cmd === "projects_list") return on[a.master];
      if (a.cmd === "projects_add") return project("vps-new");
      return null;
    }
    if (cmd === "projects_list") return on[""];
    if (cmd === "app_startup_path" || cmd === "startup_path") return null;
    return null;
  });
});

describe("the sidebar lists one machine (24z3 A2)", () => {
  it("lists only This PC's projects at start", async () => {
    await showOnly(null);
    expect(projects.machine).toBeNull();
    expect(projects.list.map((p) => p.id)).toEqual(["here-1", "here-2"]);
    expect(projects.list.every((p) => !p.master)).toBe(true);
  });

  it("showing a master lists only its projects, tagged, without This PC's", async () => {
    await showOnly(null);
    await showOnly("vps");
    expect(projects.machine).toBe("vps");
    // here-1 is This PC's own (a computer paired with itself), so it stays off the master's list.
    expect(projects.list.map((p) => p.id)).toEqual(["vps-1"]);
    expect(projects.list[0].master).toBe("vps");
    expect(ownerOf("vps-1")).toBe("vps");
    expect(projects.isLocal("here-1")).toBe(true);
  });

  it("each machine opens with its own last project", async () => {
    await showOnly(null);
    projects.select("here-2");
    await showOnly("vps");
    expect(projects.currentId).toBe("vps-1");
    projects.select("vps-1");
    expect(localStorage.getItem(currentKey(null))).toBe("here-2");
    expect(localStorage.getItem(currentKey("vps"))).toBe("vps-1");
    await showOnly(null);
    expect(projects.currentId).toBe("here-2");
  });

  it("a project of another machine cannot be selected", async () => {
    await showOnly(null);
    projects.select("vps-1");
    expect(projects.currentId).not.toBe("vps-1");
  });

  it("adding while a master is shown adds it there", async () => {
    await showOnly(null);
    await showOnly("vps");
    const p = await projects.add("/srv/new");
    expect(p?.master).toBe("vps");
    expect(projects.currentId).toBe("vps-new");
    expect(ownerOf("vps-new")).toBe("vps");
  });

  it("when the shown master's connection ends, This PC is shown again", async () => {
    await showOnly(null);
    await showOnly("vps");
    forgetMaster("vps");
    await projects.load();
    expect(projects.machine).toBeNull();
    expect(projects.list.map((p) => p.id)).toEqual(["here-1", "here-2"]);
  });

  it("an older list answering late is dropped", async () => {
    await showOnly(null);
    let release: (v: unknown) => void = () => {};
    vi.mocked(invoke).mockImplementationOnce(() => new Promise((r) => (release = r)));
    const slow = showOnly(null);
    await showOnly("vps");
    release(on[""]);
    await slow;
    expect(projects.machine).toBe("vps");
    expect(projects.list.map((p) => p.id)).toEqual(["vps-1"]);
  });
});
