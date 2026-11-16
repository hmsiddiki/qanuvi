import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LspStatus } from "$lib/api";

// Migration M1b: a shown master's language servers run on that master. Statuses are kept per machine, a master's
// project starts its server there, a machine leaving the screen ends its servers, and each machine has a switch that
// keeps its coding tools off.
const st = (id: string, found: string): LspStatus =>
  ({ id, label: id, serves: id, found, install: "Install", missing: "" }) as LspStatus;
const api = {
  lsp: {
    statuses: vi.fn(async (master: string | null) => [st("php", master ? "qanuvi" : "")]),
    install: vi.fn(async (id: string) => st(id, "qanuvi")),
    stopWindow: vi.fn(async () => {}),
  },
};
vi.mock("$lib/api", () => ({ api, errorText: (e: unknown) => String(e) }));
const started: { projectId: string; root: string; machine: string | null; stop: () => void }[] = [];
vi.mock("$lib/lsp/client", () => ({
  LspClient: class {
    ready = Promise.resolve();
    openCount = 0;
    stop = vi.fn();
    on = vi.fn();
    applyEdit = async () => false;
    constructor(
      readonly projectId: string,
      readonly server: string,
      readonly root: string,
      _onExit: unknown,
      readonly machine: string | null = null,
    ) {
      started.push(this);
    }
  },
}));
const projects = {
  list: [
    { id: "here", path: "E:/shop" },
    { id: "there", path: "/root/shop", master: "vps" },
  ],
};
vi.mock("./projects.svelte", () => ({ projects }));
const settings = {
  value: { languages: { off: [] as string[], never: [] as string[], idleMinutes: 10, toolsOff: false } },
  set: vi.fn((k: string, v: typeof settings.value.languages) => void (settings.value.languages = v)),
};
vi.mock("./settings.svelte", () => ({ settings }));
const switcher = { shown: null as string | null };
vi.mock("./switcher.svelte", () => ({ switcher }));

const { lsp } = await import("./lsp.svelte");

beforeEach(() => {
  vi.clearAllMocks();
  started.length = 0;
  settings.value.languages = { off: [], never: [], idleMinutes: 10, toolsOff: false };
  lsp.stopMachine(null);
  lsp.stopMachine("vps");
  lsp.offer = null;
});

describe("language servers per machine", () => {
  it("starts a shown master's server on that master, with the folder there", async () => {
    switcher.shown = "vps";
    const c = await lsp.clientFor({ key: "t1", projectId: "there", path: "cart.php" });
    expect(c?.uri).toBe("file:///root/shop/cart.php");
    expect(api.lsp.statuses).toHaveBeenCalledWith("vps");
    expect(started[0]).toMatchObject({ projectId: "there", root: "/root/shop", machine: "vps" });
    expect(lsp.statuses[0].found, "the Languages page shows the shown machine's").toBe("qanuvi");
  });

  it("offers the install on the machine whose file it is", async () => {
    switcher.shown = null;
    expect(await lsp.clientFor({ key: "t2", projectId: "here", path: "a.php" })).toBeNull();
    expect(lsp.offer).toMatchObject({ server: "php", machine: null });
    await lsp.install("php");
    expect(api.lsp.install).toHaveBeenCalledWith("php", null);
    expect(lsp.status("php", null)?.found).toBe("qanuvi");
  });

  it("ends a machine's servers when it leaves the screen, and only its own", async () => {
    switcher.shown = "vps";
    await lsp.clientFor({ key: "t1", projectId: "there", path: "cart.php" });
    await lsp.install("php", null);
    await lsp.clientFor({ key: "t2", projectId: "here", path: "a.php" });
    expect(started.map((c) => c.machine)).toEqual(["vps", null]);
    lsp.stopMachine("vps");
    expect(started[0].stop).toHaveBeenCalled();
    expect(started[1].stop).not.toHaveBeenCalled();
  });

  it("keeps every coding tool off on a machine whose switch says so", async () => {
    switcher.shown = "vps";
    await lsp.clientFor({ key: "t1", projectId: "there", path: "cart.php" });
    lsp.setToolsOff(true);
    expect(started[0].stop, "running servers end").toHaveBeenCalled();
    expect(await lsp.clientFor({ key: "t1", projectId: "there", path: "cart.php" })).toBeNull();
    expect(started.length).toBe(1);
  });
});
