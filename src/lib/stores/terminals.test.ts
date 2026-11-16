import { describe, expect, it, vi } from "vitest";

// 24z3 Part B2: only the shown machine's terminals have a screen; a hidden machine's are parked (the shell goes on
// there, told only while it can be reached) and their tabs go when the machine's connection ends.
const invoke = vi.fn(async (..._args: unknown[]): Promise<unknown> => undefined);
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("./settings.svelte", () => ({ settings: { value: { defaultShell: "pwsh" } } }));

const { TerminalsStore } = await import("./terminals.svelte");
const { switcher } = await import("./switcher.svelte");

describe("terminals per machine", () => {
  it("shows the shown machine's only and parks a hidden machine's", async () => {
    const t = new TerminalsStore();
    switcher.shown = null;
    t.open("pc-project");
    t.open("pc-project");
    switcher.shown = "vps";
    t.open("vps-project");
    expect(t.mounted.map((x) => x.projectId)).toEqual(["vps-project"]);

    const [first, second] = t.tabs;
    t.started(first.key, "term-a");
    t.started(second.key, "term-b");
    second.ended = true;
    await t.park(null, true);
    expect(t.tabs.filter((x) => x.machine === null).every((x) => x.parked)).toBe(true);
    expect(t.tabs.find((x) => x.machine === "vps")?.parked).toBeFalsy();
    expect(invoke, "only the running one is told").toHaveBeenCalledWith("term_detach", { id: "term-a" });
    expect(invoke).toHaveBeenCalledTimes(1);

    switcher.shown = null;
    expect(t.mounted.map((x) => x.key)).toEqual([first.key, second.key]);
    t.attached(first.key);
    expect(t.tabs[0].parked).toBe(false);
  });

  it("a machine that cannot be reached is not told; its tabs go when it is forgotten", async () => {
    const t = new TerminalsStore();
    invoke.mockClear();
    switcher.shown = "vps";
    t.open("vps-project");
    t.started(t.tabs[0].key, "term-v");
    await t.park("vps", false);
    expect(t.tabs[0].parked).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    t.forgetMachine("vps");
    expect(t.tabs).toEqual([]);
  });

  it("a screen closed with its tab kept parks the shell; closing a parked tab stops it", () => {
    const t = new TerminalsStore();
    invoke.mockClear();
    switcher.shown = null;
    t.open("pc-project");
    const key = t.tabs[0].key;
    t.started(key, "term-x");
    t.parkOne(key);
    expect(t.tabs[0].parked).toBe(true);
    expect(t.tabs[0].termId, "the next screen attaches to the same shell").toBe("term-x");
    expect(invoke).toHaveBeenCalledWith("term_detach", { id: "term-x" });
    t.close(key);
    expect(invoke).toHaveBeenCalledWith("term_kill", { id: "term-x" });
    expect(t.tabs).toEqual([]);
  });
});

// 24z3 step 3, f2: the shell picker lists the shown machine's shells, never This PC's while a server is shown.
describe("shells of the shown machine", () => {
  const shellsOf = (machine: string | null) =>
    machine
      ? [{ id: "bash", label: "bash", program: "/bin/bash" }]
      : [{ id: "pwsh", label: "PowerShell", program: "pwsh" }];
  const answer = async (cmd: unknown, args: unknown) => {
    const a = (args ?? {}) as { master?: string; cmd?: string };
    if (cmd === "term_shells") return shellsOf(null);
    if (cmd === "remote_call" && a.cmd === "term_shells") return shellsOf(a.master ?? null);
    return undefined;
  };

  it("are read from the machine shown", async () => {
    invoke.mockImplementation(answer);
    const t = new TerminalsStore();
    switcher.shown = "vps";
    await t.loadShells();
    expect(t.shells.map((s) => s.id)).toEqual(["bash"]);
    switcher.shown = null;
    await t.loadShells();
    expect(t.shells.map((s) => s.id)).toEqual(["pwsh"]);
    invoke.mockReset();
  });

  it("an answer that comes after another switch is dropped", async () => {
    invoke.mockImplementation(answer);
    const t = new TerminalsStore();
    switcher.shown = null;
    await t.loadShells();
    switcher.shown = "vps";
    const late = t.loadShells();
    switcher.shown = null;
    await late;
    expect(
      t.shells.map((s) => s.id),
      "This PC's list stays",
    ).toEqual(["pwsh"]);
    invoke.mockReset();
  });
});
