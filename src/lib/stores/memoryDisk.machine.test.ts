import { beforeEach, describe, expect, it, vi } from "vitest";

// 24z3 D6: the memory disk and remote access follow the shown machine; each machine syncs on its own.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const { showMachine } = await import("$lib/remote");
const { memoryDisk } = await import("./memoryDisk.svelte");
const { remoteAccess } = await import("./remoteAccess.svelte");
const { settings, parseSettings } = await import("./settings.svelte");

const nameOf = (c: unknown, args: unknown) => (c === "remote_call" ? (args as { cmd: string }).cmd : c);
const masterOf = (c: unknown, args: unknown) => (c === "remote_call" ? (args as { master: string }).master : null);
const report = (at: number) => ({ devices: [], merged: { added: 0, changed: 0, deleted: 0 }, written: 1, at });
const status = (at: number, error = "") => ({ busy: false, passphraseSaved: true, last: report(at), error });

beforeEach(async () => {
  invoke.mockReset();
  invoke.mockResolvedValue(null);
  kept.clear();
  showMachine(null);
  settings.value = parseSettings(null);
  await memoryDisk.switched();
});

describe("the memory disk on the shown machine", () => {
  it("shows the shown machine's last sync and drops an answer that comes after a switch", async () => {
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd, args) =>
      nameOf(cmd, args) === "memory_disk_status"
        ? cmd === "remote_call"
          ? new Promise((r) => (answer = r))
          : Promise.resolve(status(100))
        : Promise.resolve(null),
    );
    showMachine("vps");
    const late = memoryDisk.switched();
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "memory_disk_status" }),
    );
    showMachine(null);
    await memoryDisk.switched();
    answer(status(999, "The folder is gone."));
    await late;
    expect(memoryDisk.last?.at).toBe(100);
    expect(memoryDisk.error).toBe("");
    expect(memoryDisk.passphraseSaved).toBe(true);
  });

  it("an older master that does not answer shows nothing of This PC's", async () => {
    invoke.mockImplementation((cmd, args) =>
      cmd === "remote_call"
        ? Promise.reject(`This master does not know the command ${nameOf(cmd, args)}.`)
        : Promise.resolve(status(100)),
    );
    await memoryDisk.switched();
    expect(memoryDisk.last?.at).toBe(100);
    showMachine("vps");
    settings.value = parseSettings(JSON.stringify({ memoryDisk: { mode: "folder", folder: "/srv/m" } }));
    await memoryDisk.switched();
    expect(memoryDisk.last).toBeNull();
    expect(memoryDisk.passphraseSaved).toBe(false);
    expect(memoryDisk.error).toContain("cannot be done on");
  });

  it("syncs on the shown machine with the disk in its settings, never one sent from the window", async () => {
    invoke.mockImplementation((cmd, args) =>
      Promise.resolve(nameOf(cmd, args) === "memory_disk_sync" ? report(5) : status(1)),
    );
    showMachine("vps");
    settings.value = parseSettings(JSON.stringify({ memoryDisk: { mode: "folder", folder: "/srv/m" } }));
    expect(await memoryDisk.sync()).toBe(true);
    const call = invoke.mock.calls.find(([c, a]) => nameOf(c, a) === "memory_disk_sync");
    expect(masterOf(call?.[0], call?.[1])).toBe("vps");
    expect((call?.[1] as { args: Record<string, unknown> }).args).toEqual({});
    expect(memoryDisk.last?.at).toBe(5);
    // Off: nothing is asked.
    invoke.mockClear();
    settings.value = parseSettings(null);
    expect(await memoryDisk.sync()).toBe(false);
    expect(invoke.mock.calls.some(([c, a]) => nameOf(c, a) === "memory_disk_sync")).toBe(false);
  });
});

describe("remote access on the shown machine", () => {
  it("asks the shown master's own relay link, and This PC's own service when This PC is shown", async () => {
    invoke.mockResolvedValue({
      relay: "",
      enabled: false,
      connected: false,
      name: "",
      devices: [],
      sessions: [],
      code: null,
      error: "",
    });
    showMachine("vps");
    await remoteAccess.switched();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "remote_call",
        expect.objectContaining({ master: "vps", cmd: "relay_status" }),
      ),
    );
    // The shown master is connected already: This PC's service is not started for it.
    expect(invoke.mock.calls.some(([c]) => c === "remote_connect")).toBe(false);
    await remoteAccess.pairStart();
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "relay_pair_start" }),
    );
    // Back on This PC with remote access on: its own service.
    invoke.mockClear();
    showMachine(null);
    settings.value = parseSettings(JSON.stringify({ remoteAccess: { enabled: true, relay: "wss://r", name: "pc" } }));
    await remoteAccess.switched();
    expect(invoke).toHaveBeenCalledWith(
      "remote_connect",
      expect.objectContaining({ id: "this-computer", target: "self" }),
    );
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "remote_call",
        expect.objectContaining({ master: "this-computer", cmd: "relay_status" }),
      ),
    );
    await remoteAccess.switched();
  });
});
