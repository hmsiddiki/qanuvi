import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft } from "$lib/api";

// 24z3 Part B2: a machine leaving the screen is wiped from the window. Its unsaved text is kept on it while calls
// still go there (held in the window when its connection already ended), its terminals are parked, and its projects'
// workspaces are given back from scratch on return.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
const switched = () => ({ switched: vi.fn(async () => {}) });
vi.mock("./brain.svelte", () => ({ brain: switched() }));
const projects = { list: [{ id: "p1" }, { id: "p2" }] as { id: string }[], load: vi.fn(async () => {}) };
vi.mock("./projects.svelte", () => ({ projects }));
vi.mock("./settings.svelte", () => ({ settings: { loadMachine: vi.fn(async () => {}) } }));
vi.mock("./guides.svelte", () => ({ guides: switched() }));
vi.mock("./usage.svelte", () => ({ usage: switched() }));
vi.mock("./startup.svelte", () => ({ startup: switched() }));
vi.mock("./claudeUpdate.svelte", () => ({ claudeUpdate: switched() }));
vi.mock("./agent.svelte", () => ({ agent: { info: null, loadInfo: vi.fn(async () => {}) } }));
vi.mock("./omniroute.svelte", () => ({ omni: { switched: vi.fn(), look: vi.fn(async () => {}) } }));
vi.mock("./memoryDisk.svelte", () => ({ memoryDisk: switched() }));
vi.mock("./remoteAccess.svelte", () => ({ remoteAccess: switched() }));
vi.mock("./memory.svelte", () => ({ memory: { touch: vi.fn() } }));
const unsaved: Draft[] = [{ projectId: "p1", path: "a.js", text: "changed", hash: "h1" }];
const editor = { park: vi.fn(() => unsaved), openConfig: vi.fn(async () => {}), applyDraft: vi.fn() };
vi.mock("./editor.svelte", () => ({ editor }));
const terminals = { park: vi.fn(async () => {}), loadShells: vi.fn(async () => {}) };
vi.mock("./terminals.svelte", () => ({ terminals }));
const workspace = { flush: vi.fn(), restored: new Set<string>() };
vi.mock("./workspace.svelte", () => ({ workspace }));
const drafts = { keep: vi.fn(async () => {}), take: vi.fn(async (): Promise<Draft[]> => []) };
vi.mock("./drafts", () => ({ drafts }));

const { showMachine, shownMachine } = await import("$lib/remote");
const { show, showOffline } = await import("./machines");
const { switcher } = await import("./switcher.svelte");

beforeEach(async () => {
  switcher.held = null;
  await show(null);
  vi.clearAllMocks();
  workspace.restored = new Set(["p1", "p2", "other"]);
});

describe("hiding a machine", () => {
  it("keeps its unsaved text on it and parks its terminals, while calls still go there", async () => {
    await show("vps");
    expect(workspace.flush).toHaveBeenCalled();
    expect([...workspace.restored], "its projects' workspaces come back from scratch").toEqual(["other"]);
    expect(editor.park).toHaveBeenCalledOnce();
    expect(drafts.keep).toHaveBeenCalledWith(null, unsaved, true);
    expect(terminals.park).toHaveBeenCalledWith(null, true);
  });

  it("holds the drafts in the window when its connection already ended", async () => {
    await show("vps");
    vi.clearAllMocks();
    // The connection ended: calls already go to This PC (remote.ts forgetMaster) when This PC is shown.
    showMachine(null);
    await show(null);
    expect(drafts.keep).toHaveBeenCalledWith("vps", unsaved, false);
    expect(terminals.park).toHaveBeenCalledWith("vps", false);
  });

  it("does nothing when the machine shown is picked again", async () => {
    await show(null);
    expect(editor.park).not.toHaveBeenCalled();
    expect(drafts.take).not.toHaveBeenCalled();
  });

  it("reopens the agent configuration files kept unsaved on the machine shown", async () => {
    drafts.take.mockResolvedValueOnce([
      {
        projectId: "",
        path: "config::claude-code::settings::",
        text: "{ }",
        hash: "h2",
        meta: JSON.stringify({ provider: "claude-code", id: "settings", projectId: null, label: "Settings" }),
      },
    ]);
    await show("vps");
    await vi.waitFor(() => expect(editor.applyDraft).toHaveBeenCalled());
    expect(drafts.take).toHaveBeenCalledWith("vps", "");
    expect(editor.openConfig).toHaveBeenCalledWith("claude-code", "settings", "Settings", null);
    expect(editor.applyDraft).toHaveBeenCalledWith("config::claude-code::settings::", "{ }", "h2");
  });
});

// 24z3 Part E: a master that cannot be reached is shown as "not connected", never as a stale copy.
describe("a machine that cannot be reached", () => {
  it("holds a lost machine's drafts in the window when another is shown, and the cover goes", async () => {
    await show("vps");
    vi.clearAllMocks();
    switcher.held = { id: "vps", mode: "lost", error: "", tries: 1, nextAt: 0, since: 1 };
    await show(null);
    expect(drafts.keep).toHaveBeenCalledWith("vps", unsaved, false);
    expect(terminals.park).toHaveBeenCalledWith("vps", false);
    expect(switcher.held).toBeNull();
  });

  it("picked while unreachable: the machine before is hidden, nothing of the new one is loaded", async () => {
    await showOffline("vps", "Connection refused.");
    expect(drafts.keep).toHaveBeenCalledWith(null, unsaved, true);
    expect(switcher.shown).toBe("vps");
    expect(shownMachine(), "its calls go to it and fail, never to This PC").toBe("vps");
    expect(switcher.held).toMatchObject({ id: "vps", mode: "offline", error: "Connection refused.", tries: 0 });
    expect(projects.list).toEqual([]);
    expect(projects.load).not.toHaveBeenCalled();
    projects.list = [{ id: "p1" }, { id: "p2" }];
  });

  it("lost for too long: its own screen is wiped, the drafts held in the window, the tries kept", async () => {
    await show("vps");
    vi.clearAllMocks();
    switcher.held = { id: "vps", mode: "lost", error: "No answer.", tries: 5, nextAt: 0, since: 42 };
    await showOffline("vps", "No answer.");
    expect(drafts.keep).toHaveBeenCalledWith("vps", unsaved, false);
    expect(switcher.held).toMatchObject({ mode: "offline", tries: 5, since: 42 });
    projects.list = [{ id: "p1" }, { id: "p2" }];
  });
});
