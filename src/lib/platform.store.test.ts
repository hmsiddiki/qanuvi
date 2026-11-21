import { describe, expect, it, vi } from "vitest";

const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => kept.set(k, v),
});
const setDecorations = vi.fn(() => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setDecorations }) }));

const { platform } = await import("./platform.svelte");

const on = (os: string) => (platform.info = { name: "Qanuvi", version: "0", os, arch: "x86_64" });

describe("window buttons (LT)", () => {
  it("are Qanuvi's on Windows and on Linux, the system's on macOS", () => {
    on("windows");
    expect(platform.drawsWindowControls).toBe(true);
    on("linux");
    expect(platform.drawsWindowControls).toBe(true);
    on("macos");
    expect(platform.drawsWindowControls).toBe(false);
  });

  it("on Linux, the system title bar can be chosen, kept for the window, and put back", async () => {
    on("linux");
    await platform.setSystemTitleBar(true);
    expect(platform.drawsWindowControls).toBe(false);
    expect(setDecorations).toHaveBeenLastCalledWith(true);
    expect(localStorage.getItem("qanuvi.systemTitleBar")).toBe("true");
    await platform.setSystemTitleBar(false);
    expect(platform.drawsWindowControls).toBe(true);
    expect(setDecorations).toHaveBeenLastCalledWith(false);
  });
});
