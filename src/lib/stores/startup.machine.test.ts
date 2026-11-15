import { describe, expect, it, vi } from "vitest";

// 24z3 D4: Start-up and Claude Code's version follow the shown machine; an answer from the one before is dropped.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const { showMachine } = await import("$lib/remote");
const { startup } = await import("./startup.svelte");
const { claudeUpdate } = await import("./claudeUpdate.svelte");

const status = (id: string) => ({ id, name: id, state: "running", detail: "", output: "" });

describe("start-up on the shown machine", () => {
  it("asks the shown master and drops an answer that comes after a switch", async () => {
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd, args) => {
      if (cmd === "remote_call" && (args as { cmd: string }).cmd === "startup_status")
        return new Promise((r) => (answer = r));
      return Promise.resolve([status("pc")]);
    });
    showMachine("vps");
    const late = startup.refresh();
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "startup_status" }),
    );
    showMachine(null);
    startup.switched();
    answer([status("vps")]);
    await late;
    expect(startup.statuses).toEqual([]);
    await startup.refresh();
    expect(startup.statuses.map((s) => s.id)).toEqual(["pc"]);
  });

  it("says when the shown machine cannot answer, and forgets it on a switch", async () => {
    invoke.mockRejectedValue("This master does not know the command startup_status.");
    showMachine("vps");
    await startup.refresh();
    expect(startup.problem).toMatch(/cannot be done on .* yet \(startup_status\)/);
    showMachine(null);
    startup.switched();
    expect(startup.problem).toBe("");
  });
});

describe("Claude Code's version on the shown machine", () => {
  it("shows the shown machine's versions only, and never this window's update on another machine", async () => {
    let finish: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd, args) => {
      const c = cmd === "remote_call" ? (args as { cmd: string }).cmd : cmd;
      if (c === "claude_update") return new Promise((r) => (finish = r));
      if (c === "claude_versions") return Promise.resolve({ installed: "2.1.0", latest: "2.1.0", newer: false });
      return Promise.resolve(null);
    });
    showMachine(null);
    claudeUpdate.switched();
    const running = claudeUpdate.update();
    expect(claudeUpdate.busy).toBe(true);
    showMachine("vps");
    claudeUpdate.switched();
    expect(claudeUpdate.busy).toBe(false);
    expect(claudeUpdate.note).toBe("");
    finish("done");
    await running;
    expect(claudeUpdate.note).toBe("");
    expect(claudeUpdate.busy).toBe(false);
  });
});
