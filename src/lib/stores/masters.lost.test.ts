/**
 * 24z3 Part E: the shown master's connection ends. Its screen stays (read-only under a cover) while it is tried again
 * with growing waits; back, it is shown again; not back within LOST_FOR, it is wiped to "not connected".
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const handlers: Record<string, (e: { payload: { id: string } }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, fn: (e: { payload: { id: string } }) => void) => {
    handlers[name] = fn;
    return () => {};
  }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: () => ({ listen: async () => {} }) }));
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} });
vi.stubGlobal("document", { activeElement: null });

const machines = {
  show: vi.fn(async (_m: string | null) => {}),
  showOffline: vi.fn(async (_m: string, _e: string) => {}),
};
vi.mock("./machines", () => machines);

const { invoke } = await import("@tauri-apps/api/core");
const { masters, LOST_FOR, retryWait } = await import("./masters.svelte");
const { settings } = await import("./settings.svelte");
const { switcher } = await import("./switcher.svelte");
const { showMachine, shownMachine } = await import("$lib/remote");

let reachable = true;

beforeEach(async () => {
  vi.useFakeTimers();
  reachable = true;
  switcher.held = null;
  settings.set("masters", [
    { id: "vps", name: "VPS", target: "root@vps", program: "qanuvi-master", identity: "", autoConnect: false },
  ]);
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "remote_connect") {
      if (!reachable) throw "Connection refused.";
      return { name: "Qanuvi master", version: "0", os: "linux", arch: "x86_64", host: "vps", dataDir: "/d" };
    }
    if (cmd === "remote_call")
      return /list|sessions|status|runs|active/.test((args as { cmd: string }).cmd) ? [] : null;
    return null;
  }) as never);
  await masters.connect(masters.list[0]);
  showMachine("vps");
  switcher.shown = "vps";
  machines.show.mockClear();
  machines.showOffline.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

/** The connection ends by itself while the machine is shown. */
function drop() {
  reachable = false;
  handlers["remote-closed"]({ payload: { id: "vps" } });
}

describe("the shown machine's connection ends", () => {
  it("stays shown under the cover, its calls never on This PC", () => {
    drop();
    expect(switcher.held).toMatchObject({ id: "vps", mode: "lost", tries: 0 });
    expect(shownMachine()).toBe("vps");
    expect(machines.show).not.toHaveBeenCalledWith(null);
    expect(masters.statusOf("vps").state).toBe("error");
  });

  it("tries again with growing waits, and is shown again once it answers", async () => {
    expect([0, 1, 2, 3, 4, 9].map(retryWait)).toEqual([2000, 4000, 8000, 15000, 30000, 30000]);
    drop();
    await vi.advanceTimersByTimeAsync(2000);
    expect(switcher.held).toMatchObject({ mode: "lost", tries: 1, error: "Connection refused." });
    reachable = true;
    await vi.advanceTimersByTimeAsync(4000);
    expect(masters.statusOf("vps").state).toBe("on");
    expect(machines.show).toHaveBeenCalledWith("vps");
  });

  it("not back within LOST_FOR: wiped to not connected, and no more tries by itself", async () => {
    drop();
    await vi.advanceTimersByTimeAsync(LOST_FOR + 31_000);
    expect(machines.showOffline).toHaveBeenCalledWith("vps", "Connection refused.");
    const calls = vi.mocked(invoke).mock.calls.filter(([c]) => c === "remote_connect").length;
    switcher.held = { ...switcher.held!, mode: "offline" };
    await vi.advanceTimersByTimeAsync(120_000);
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "remote_connect").length).toBe(calls);
  });

  it("Reconnect on the not connected screen brings it back", async () => {
    reachable = false;
    switcher.held = { id: "vps", mode: "offline", error: "No answer.", tries: 3, nextAt: 0, since: 1 };
    expect(await masters.retryNow()).toBe(false);
    expect(switcher.held).toMatchObject({ mode: "offline", tries: 4 });
    reachable = true;
    expect(await masters.retryNow()).toBe(true);
    expect(machines.show).toHaveBeenCalledWith("vps");
  });

  it("another machine shown meanwhile: the tries stop", async () => {
    drop();
    switcher.held = null;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "remote_connect").length).toBe(1);
  });

  it("disconnected on purpose: the cover goes and This PC is shown", async () => {
    drop();
    await masters.disconnect("vps");
    expect(switcher.held).toBeNull();
    expect(machines.show).toHaveBeenCalledWith(null);
  });
});
