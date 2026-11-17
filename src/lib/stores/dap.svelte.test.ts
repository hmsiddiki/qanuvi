import { describe, expect, it, vi } from "vitest";

// Migration M1d: a start error belongs to the machine that was shown; it goes when that machine leaves the screen,
// so another machine's sidebar never shows it.
const statuses = vi.fn();
vi.mock("$lib/api", () => ({ api: { dap: { stopWindow: vi.fn(async () => {}), statuses } }, errorText: String }));
vi.mock("$lib/dap/client", () => ({ DapClient: class {} }));
vi.mock("./editor.svelte", () => ({ editor: { active: null } }));
vi.mock("./projects.svelte", () => ({ projects: { list: [] } }));
vi.mock("./settings.svelte", () => ({ settings: { value: { languages: { xdebugFile: "" } } } }));
const switcher = { shown: "vps" as string | null };
vi.mock("./switcher.svelte", () => ({ switcher }));

const { dap } = await import("./dap.svelte");

describe("start error per machine", () => {
  it("goes when its machine leaves the screen, and only then", async () => {
    await dap.start("p", { name: "x", type: "cobol", request: "launch" });
    expect(dap.lastError).toMatch(/no debugger for "cobol"/);
    dap.stopMachine(null);
    expect(dap.lastError).not.toBe("");
    dap.stopMachine("vps");
    expect(dap.lastError).toBe("");
  });
});

// r1: a failed reload keeps the last list but says so, per machine, until a reload works.
describe("statuses reload", () => {
  it("keeps the old list on a failure, says why, and clears once a reload works", async () => {
    const old = [{ id: "js-debug", missing: "needs Node.js 20" }];
    statuses.mockResolvedValueOnce(old);
    await dap.loadStatuses("vps");
    expect(dap.statusErrorOn("vps")).toBe("");
    statuses.mockRejectedValueOnce("the machine did not answer");
    await dap.loadStatuses("vps");
    expect(dap.statusesOn("vps")).toEqual(old);
    expect(dap.statusError).toBe("the machine did not answer");
    expect(dap.statusErrorOn(null)).toBe("");
    statuses.mockResolvedValueOnce([{ id: "js-debug", missing: "" }]);
    await dap.loadStatuses("vps");
    expect(dap.statusError).toBe("");
    expect(dap.statusesOn("vps")[0]).toMatchObject({ missing: "" });
  });
});
