import { describe, expect, it, vi } from "vitest";

// Migration M1d: a start error belongs to the machine that was shown; it goes when that machine leaves the screen,
// so another machine's sidebar never shows it.
vi.mock("$lib/api", () => ({ api: { dap: { stopWindow: vi.fn(async () => {}) } }, errorText: String }));
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
