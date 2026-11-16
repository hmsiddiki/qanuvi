import { describe, expect, it, vi } from "vitest";

// 24z3 step 4: a machine window opens on the machine its address names, the machine windows kept for the next start
// are read back safely, and the working icon's hover text counts agents.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), emitTo: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: vi.fn(), WebviewWindow: class {} }));

const { machineFromUrl, parseMachineWindows, projectFromUrl } = await import("./windows.svelte");
const { workingText } = await import("./working.svelte");

describe("machine windows", () => {
  it("reads the machine a window was opened for", () => {
    expect(machineFromUrl("?machine=pc")).toBeNull();
    expect(machineFromUrl("?machine=m-123_ab")).toBe("m-123_ab");
    expect(machineFromUrl("")).toBeUndefined();
    expect(machineFromUrl("?project=p1")).toBeUndefined();
    expect(machineFromUrl("?machine=../x")).toBeUndefined();
    expect(machineFromUrl("?machine=" + "a".repeat(81))).toBeUndefined();
    // A machine window is no project window.
    expect(projectFromUrl("?machine=m1")).toBeNull();
  });

  it("keeps only sound machine windows for the next start, once each", () => {
    expect(parseMachineWindows('["", "m1", "m1", "../bad", 4, null]')).toEqual(["", "m1"]);
    expect(parseMachineWindows("not json")).toEqual([]);
    expect(parseMachineWindows('{"m1": true}')).toEqual([]);
    expect(parseMachineWindows(null)).toEqual([]);
    expect(parseMachineWindows(JSON.stringify(Array.from({ length: 20 }, (_, i) => `m${i}`)))).toHaveLength(8);
  });
});

describe("working icon", () => {
  it("says how many agents work, nothing when none", () => {
    expect(workingText(undefined)).toBe("");
    expect(workingText(0)).toBe("");
    expect(workingText(1)).toBe("1 agent working");
    expect(workingText(3)).toBe("3 agents working");
  });
});
