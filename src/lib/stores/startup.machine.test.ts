import { describe, expect, it, vi } from "vitest";

// 24z3 D4: Start-up and Claude Code's version follow the shown machine; an answer from the one before is dropped.
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

describe("the On box", () => {
  it("off stops it now and keeps it off; on starts it now", async () => {
    const { settings } = await import("./settings.svelte");
    const calls: string[] = [];
    invoke.mockImplementation((cmd, args) => {
      calls.push(cmd === "remote_call" ? (args as { cmd: string }).cmd : cmd);
      return Promise.resolve([]);
    });
    showMachine(null);
    settings.set("startup", [{ id: "hr", name: "Headroom", command: "headroom proxy", waitFor: "", enabled: true }]);
    calls.length = 0;
    await startup.setEnabled("hr", false);
    expect(calls[0]).toBe("startup_stop");
    expect(invoke).toHaveBeenCalledWith("startup_stop", { id: "hr" });
    expect(settings.value.startup).toEqual([expect.objectContaining({ id: "hr", enabled: false })]);
    // Nothing turned on is started by the off box.
    expect(invoke).toHaveBeenLastCalledWith("startup_start", { items: [] });

    calls.length = 0;
    await startup.setEnabled("hr", true);
    expect(calls).not.toContain("startup_stop");
    expect(invoke).toHaveBeenLastCalledWith("startup_start", {
      items: [expect.objectContaining({ id: "hr", enabled: true })],
    });
    expect(settings.value.startup).toHaveLength(1);
  });
});

describe("the Claude Code card on the shown machine", () => {
  const pc = { available: true, path: "C:\claude.exe", version: "2.1.318" };
  const none = { available: false, path: "", version: "" };

  it("a switch asks the new machine and drops the machine before's path", async () => {
    const { agent } = await import("./agent.svelte");
    const { show } = await import("./machines");
    invoke.mockImplementation((cmd, args) => {
      const c = cmd === "remote_call" ? (args as { cmd: string }).cmd : cmd;
      if (c === "agent_info") return Promise.resolve(cmd === "remote_call" ? none : pc);
      return Promise.resolve(c === "settings_get" ? null : []);
    });
    showMachine(null);
    await agent.loadInfo();
    expect(agent.info?.path).toBe(pc.path);
    const shown = show("vps");
    expect(agent.info).toBeNull();
    await shown;
    expect(agent.info).toEqual(none);
    expect(invoke).toHaveBeenCalledWith("remote_call", expect.objectContaining({ master: "vps", cmd: "agent_info" }));
  });

  it("drops an answer that comes after a switch", async () => {
    const { agent } = await import("./agent.svelte");
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd) =>
      cmd === "remote_call" ? new Promise((r) => (answer = r)) : Promise.resolve(pc),
    );
    showMachine("vps");
    const late = agent.loadInfo();
    showMachine(null);
    await agent.loadInfo();
    answer(none);
    await late;
    expect(agent.info).toEqual(pc);
  });
});
