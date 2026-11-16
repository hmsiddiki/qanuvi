import { beforeEach, describe, expect, it, vi } from "vitest";

// 24z3 Part B: the switcher shows the machine picked, remembers it for the next start (main window only), and at start
// shows it again only when it is connected.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});
const switched = () => ({ switched: vi.fn(async () => {}) });
vi.mock("./brain.svelte", () => ({ brain: switched() }));
vi.mock("./projects.svelte", () => ({ projects: { list: [], load: vi.fn(async () => {}) } }));
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
vi.mock("./editor.svelte", () => ({ editor: { park: () => [], openConfig: vi.fn(), applyDraft: vi.fn() } }));
vi.mock("./terminals.svelte", () => ({
  terminals: { park: vi.fn(async () => {}), loadShells: vi.fn(async () => {}) },
}));
vi.mock("./workspace.svelte", () => ({ workspace: { flush: vi.fn(), restored: new Set() } }));
vi.mock("./drafts", () => ({ drafts: { keep: vi.fn(async () => {}), take: vi.fn(async () => []) } }));

const { shownMachine } = await import("$lib/remote");
const { pick, restoreShown, show } = await import("./machines");
const { switcher, SHOWN_KEY } = await import("./switcher.svelte");

beforeEach(async () => {
  kept.clear();
  await show(null);
});

describe("the machine switcher", () => {
  it("shows the machine picked and remembers it in the main window only", async () => {
    await pick("vps", true);
    expect(shownMachine()).toBe("vps");
    expect(switcher.shown).toBe("vps");
    expect(switcher.switching).toBeNull();
    expect(kept.get(SHOWN_KEY)).toBe("vps");
    await pick("lab", false);
    expect(switcher.shown).toBe("lab");
    expect(kept.get(SHOWN_KEY), "a project window does not change the main window's choice").toBe("vps");
    await pick(null, true);
    expect(kept.has(SHOWN_KEY)).toBe(false);
  });

  it("a connection that ends shows This PC but keeps the choice", async () => {
    await pick("vps", true);
    await show(null);
    expect(switcher.shown).toBeNull();
    expect(kept.get(SHOWN_KEY)).toBe("vps");
  });

  it("at start shows the remembered machine only when it is connected", async () => {
    kept.set(SHOWN_KEY, "vps");
    await restoreShown(() => false);
    expect(switcher.shown).toBeNull();
    await restoreShown((id) => id === "vps");
    expect(switcher.shown).toBe("vps");
    expect(shownMachine()).toBe("vps");
  });
});
