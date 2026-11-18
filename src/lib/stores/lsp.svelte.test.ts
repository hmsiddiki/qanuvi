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
const started: {
  projectId: string;
  root: string;
  machine: string | null;
  stop: () => void;
  exit: (why: string) => void;
}[] = [];
vi.mock("$lib/lsp/client", () => ({
  LspClient: class {
    ready = Promise.resolve();
    openCount = 0;
    stop = vi.fn();
    on = vi.fn();
    applyEdit = async () => false;
    open = vi.fn();
    close = vi.fn();
    exit: (why: string) => void;
    constructor(
      readonly projectId: string,
      readonly server: string,
      readonly root: string,
      onExit: (why: string) => void,
      readonly machine: string | null = null,
    ) {
      this.exit = onExit;
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

const { lsp, RESTARTS, RESTART_SPAN_MS, RESTART_WAIT_MS } = await import("./lsp.svelte");

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
    expect(api.lsp.install).toHaveBeenCalledWith("php", null, expect.any(Function));
    expect(lsp.status("php", null)?.found).toBe("qanuvi");
  });

  it("shows the newest lines of an install while it runs, and clears them after", async () => {
    let seen: string[] = [];
    api.lsp.install.mockImplementationOnce((async (id: string, _m: string | null, onLine: (l: string) => void) => {
      for (const l of ["a", "b", "c", "d", "e"]) onLine(l);
      seen = [...lsp.installLog];
      return st(id, "qanuvi");
    }) as never);
    expect(await lsp.install("php", "vps")).toBe(true);
    expect(seen).toEqual(["b", "c", "d", "e"]);
    expect(lsp.installLog).toEqual([]);
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

  it("says why a file gets no server: the machine's tools or that language's server are off", () => {
    expect(lsp.offFor("cart.php")).toBeNull();
    expect(lsp.offFor("notes.md"), "no server for that language at all").toBeNull();
    lsp.setEnabled("php", false);
    expect(lsp.offFor("cart.php")).toBe("server");
    lsp.setToolsOff(true);
    expect(lsp.offFor("cart.php")).toBe("tools");
  });

  it("tells open editors when tools or a server are switched on again, never when off", () => {
    const at = lsp.switchedOn;
    lsp.setToolsOff(true);
    lsp.setEnabled("php", false);
    expect(lsp.switchedOn).toBe(at);
    lsp.setToolsOff(false);
    lsp.setEnabled("php", true);
    expect(lsp.switchedOn).toBe(at + 2);
  });
});

describe("a server that ended by itself (M5c)", () => {
  it("starts again for its open files, says so meanwhile, and stops trying after a few ends", async () => {
    vi.useFakeTimers();
    try {
      switcher.shown = "vps";
      const doc = { key: "t9", projectId: "there", path: "cart.php" };
      await lsp.attach(doc, () => "<?php");
      lsp.sync(["t9"]);
      expect(lsp.state["there::php"]).toBe("ready");
      for (let i = 1; i <= RESTARTS; i++) {
        started[started.length - 1].exit("it crashed");
        expect(lsp.running("there")[0].state, "the Problems list says Checking… meanwhile").toBe("restarting");
        await vi.advanceTimersByTimeAsync(RESTART_WAIT_MS);
        expect(started.length).toBe(i + 1);
        expect(lsp.state["there::php"]).toBe("ready");
      }
      // One end too many in a short time: it stays stopped, with why.
      started[started.length - 1].exit("it crashed");
      expect(lsp.state["there::php"]).toBe("stopped");
      expect(lsp.lastError).toContain("it crashed");
      await vi.advanceTimersByTimeAsync(RESTART_WAIT_MS);
      expect(started.length).toBe(RESTARTS + 1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not start again when its file closed meanwhile", async () => {
    vi.useFakeTimers();
    try {
      switcher.shown = "vps";
      // The ends of the test before are long past.
      vi.setSystemTime(Date.now() + 2 * RESTART_SPAN_MS);
      const doc = { key: "t10", projectId: "there", path: "b.php" };
      await lsp.attach(doc, () => "<?php");
      lsp.sync(["t10"]);
      started[0].exit("it crashed");
      lsp.sync([]);
      await vi.advanceTimersByTimeAsync(RESTART_WAIT_MS);
      expect(started.length).toBe(1);
      expect(lsp.running("there")).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });
});
