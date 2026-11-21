import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const { callMaster } = await import("$lib/remote");
const { shownError, linkState } = await import("./linkState.svelte");
const { switcher } = await import("./switcher.svelte");

const GONE = "The connection to the master has ended.";

describe("link error lines (ce1)", () => {
  it("other errors always show", () => {
    expect(shownError("The file could not be saved.")).toBe("The file could not be saved.");
    expect(shownError("")).toBe("");
    expect(shownError(null)).toBe("");
  });

  it("a link error shows while the link is gone, and goes once a call gets through", async () => {
    invoke.mockRejectedValueOnce(GONE);
    await expect(callMaster("m1", "projects_list", {})).rejects.toBe(GONE);
    expect(linkState.failing).toBe(true);
    expect(shownError(GONE)).toBe(GONE);
    invoke.mockResolvedValueOnce([]);
    await callMaster("m1", "projects_list", {});
    expect(linkState.failing).toBe(false);
    expect(shownError(GONE)).toBe("");
  });

  it("while the reconnect notice is up, the line is not repeated", async () => {
    invoke.mockRejectedValueOnce(GONE);
    await expect(callMaster("m1", "projects_list", {})).rejects.toBe(GONE);
    switcher.held = { id: "m1", mode: "lost", error: "", why: GONE, tries: 0, nextAt: 0, since: 0 };
    expect(shownError(GONE)).toBe("");
    switcher.held = null;
    expect(shownError(GONE)).toBe(GONE);
  });
});
