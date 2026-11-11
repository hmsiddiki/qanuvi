import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));

const { diskFrom } = await import("./memoryDisk.svelte");
const { parseSettings } = await import("./settings.svelte");

const masters = [{ id: "m1", target: "root@203.0.113.5", identity: "~/.ssh/vps_key" }];

describe("memory disk settings", () => {
  it("are off by default and read back safely", () => {
    expect(parseSettings(null).memoryDisk).toEqual({
      mode: "off",
      folder: "",
      server: "",
      serverDir: "~/.qanuvi-memory",
      auto: true,
    });
    const s = parseSettings(JSON.stringify({ memoryDisk: { mode: "cloud", folder: 5, auto: false } }));
    expect(s.memoryDisk.mode).toBe("off");
    expect(s.memoryDisk.folder).toBe("");
    expect(s.memoryDisk.auto).toBe(false);
  });

  it("describe a folder or a server folder, or nothing", () => {
    const base = parseSettings(null).memoryDisk;
    expect(diskFrom(base, masters)).toBeNull();
    expect(diskFrom({ ...base, mode: "folder", folder: " D:/Drive/Qanuvi " }, masters)).toEqual({
      kind: "folder",
      path: "D:/Drive/Qanuvi",
    });
    expect(diskFrom({ ...base, mode: "folder" }, masters)).toBeNull();
    expect(diskFrom({ ...base, mode: "server", server: "m1" }, masters)).toEqual({
      kind: "server",
      target: "root@203.0.113.5",
      identity: "~/.ssh/vps_key",
      dir: "~/.qanuvi-memory",
    });
    expect(diskFrom({ ...base, mode: "server", server: "gone" }, masters)).toBeNull();
  });
});
