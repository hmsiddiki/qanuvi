/** 24z3 D2: the status bar, the setup installs and the notifications follow the machine they belong to. */
import { beforeEach, describe, expect, it, vi } from "vitest";

let stream: { onmessage?: (m: unknown) => void } | null = null;

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
const { thisPcAsPlainCommands } = await import("$lib/testing/thisPc");
thisPcAsPlainCommands(invoke);
const { machineFor, nameMachines, ownerOf, showMachine, windowOnly } = await import("$lib/remote");
const { api } = await import("$lib/api");
const { usage } = await import("./usage.svelte");
const { inbox } = await import("./inbox.svelte");

type Answer = (cmd: string, args: Record<string, unknown>) => unknown;
let answers: Record<string, Answer> = {};
const calls: { where: string; cmd: string; args: Record<string, unknown> }[] = [];

const used = (memory: number) => ({ memory, cpu: 1, processes: 2, parts: [] });

beforeEach(() => {
  showMachine(null);
  nameMachines((id) => (id === "vps" ? "VPS" : id));
  calls.length = 0;
  stream = null;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "remote_call") {
      const a = args as { master: string; cmd: string; args: Record<string, unknown>; onEvent: unknown };
      calls.push({ where: a.master, cmd: a.cmd, args: a.args });
      if (a.cmd === "inbox_watch") stream = a.onEvent as typeof stream;
      return answers[a.master](a.cmd, a.args);
    }
    calls.push({ where: "here", cmd, args: args ?? {} });
    return answers[""](cmd, args ?? {});
  }) as never);
  answers = { "": () => null, vps: () => null };
});

describe("routing (24z3 D2)", () => {
  it("keeps This PC's notification settings with the window and owns an install's terminal on its machine", async () => {
    expect(windowOnly("notify_settings")).toBe(true);
    for (const cmd of ["machine_info", "machine_room", "setup_tools", "setup_install_plan", "qanuvi_usage"]) {
      showMachine("vps");
      expect(machineFor(cmd)).toBe("vps");
      showMachine(null);
      expect(machineFor(cmd)).toBe(null);
    }
    answers.vps = (cmd) => (cmd === "setup_install_start" ? "term-on-vps" : null);
    showMachine("vps");
    expect(await api.term.installStart("ollama", 80, 24, () => {})).toBe("term-on-vps");
    expect(ownerOf("term-on-vps")).toBe("vps");
    // Typing in it goes to the master, also once This PC is shown again.
    showMachine(null);
    await api.term.write("term-on-vps", "secret\r");
    expect(calls.at(-1)).toMatchObject({ where: "vps", cmd: "term_write" });
  });
});

describe("the status bar's usage (24z3 D2)", () => {
  it("is the shown machine's, cleared at a switch, never the one before", async () => {
    answers[""] = (cmd) => (cmd === "qanuvi_usage" ? used(100) : null);
    answers.vps = (cmd) => (cmd === "qanuvi_usage" ? used(900) : null);
    await usage.look();
    expect(usage.now?.memory).toBe(100);
    expect(usage.place).toBe("this computer");

    // This PC's answer arrives late, after the master is shown: dropped.
    let late: (u: unknown) => void = () => {};
    answers[""] = (cmd) => (cmd === "qanuvi_usage" ? new Promise((r) => (late = r)) : null);
    const slow = usage.look();
    showMachine("vps");
    usage.switched();
    expect(usage.now).toBe(null);
    await usage.look();
    late(used(100));
    await slow;
    expect(usage.now?.memory).toBe(900);
    expect(usage.place).toBe("VPS");

    // An older master that cannot tell: nothing, not This PC's numbers.
    answers.vps = () => Promise.reject("This master does not know the command qanuvi_usage.");
    await usage.look();
    expect(usage.now).toBe(null);
  });

  it("adds the screen in on This PC and only tells it for a server (u2)", async () => {
    const MB = 1024 ** 2;
    answers[""] = (cmd) =>
      cmd === "qanuvi_usage"
        ? {
            memory: 100 * MB,
            cpu: 2,
            processes: 4,
            own: 60 * MB,
            parts: [{ name: "qanuvi-master", count: 2, memory: 60 * MB }],
          }
        : cmd === "screen_usage"
          ? {
              memory: 630 * MB,
              cpu: 1,
              processes: 6,
              parts: [
                { name: "msedgewebview2", count: 5, memory: 600 * MB },
                { name: "qanuvi-master", count: 1, memory: 20 * MB },
              ],
            }
          : null;
    await usage.look();
    await usage.lookScreen();
    expect(usage.bar?.text).toBe("730 MB · 3%");
    expect(usage.bar?.title.split("\n")).toEqual([
      "Qanuvi in all on this computer (10 processes):",
      "master 60 MB · screen 630 MB · agents, shells, language servers 40 MB",
      "msedgewebview2 ×5: 600 MB",
      "qanuvi-master ×3: 80 MB",
    ]);
    // The screen is asked of the window, never routed to a master.
    expect(calls.filter((c) => c.cmd === "screen_usage").every((c) => c.where === "here")).toBe(true);

    // A server shown: its master's numbers, the screen told apart; an older master has no split.
    answers.vps = (cmd) => (cmd === "qanuvi_usage" ? used(900 * MB) : null);
    showMachine("vps");
    usage.switched();
    await usage.look();
    await usage.lookScreen();
    expect(usage.bar?.text).toBe("900 MB · 1%");
    expect(usage.bar?.title.split("\n")).toEqual([
      "Qanuvi in all on VPS (2 processes):",
      "master 900 MB",
      "On this computer, the screen: 630 MB",
    ]);
  });
});

describe("a master's notifications (24z3 D2)", () => {
  it("go by that master's own Notifications settings", async () => {
    const q = { id: "vps-q", at: 1, type: "answer", kind: "run", title: "q", text: "", read: false, status: "open" };
    answers[""] = (cmd) => (cmd === "inbox_list" ? [] : null);
    answers.vps = (cmd) =>
      cmd === "settings_get"
        ? { notifications: { desktop: true, what: "all", bundle: false } }
        : cmd === "inbox_watch" || cmd === "inbox_list"
          ? []
          : null;
    await inbox.followMaster("vps");
    stream?.onmessage?.({ change: "add", item: q, new: true });
    const told = calls.find((c) => c.cmd === "notify_item");
    expect(told?.where).toBe("here");
    expect(told?.args.settings).toMatchObject({ what: "all", bundle: false });
    expect(calls.some((c) => c.cmd === "notify_settings")).toBe(false);
    inbox.dropMaster("vps");
  });
});
