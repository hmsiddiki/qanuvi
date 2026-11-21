import { describe, expect, it, vi } from "vitest";
import masterRs from "../../src-tauri/src/master.rs?raw";
import masterInboxRs from "../../src-tauri/src/master_inbox.rs?raw";
import masterRunsRs from "../../src-tauri/src/master_runs.rs?raw";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const {
  NotOnMachine,
  disown,
  forgetMaster,
  linkEndedText,
  machineFor,
  nameMachines,
  own,
  remember,
  routeOf,
  routed,
  showMachine,
  shownMachine,
  windowOnly,
} = await import("./remote");

describe("routing", () => {
  it("sends calls about a server's project, session or terminal to that server", () => {
    own("p1", "vps");
    remember("vps", "agent_start", { id: "s1", project_id: "p1" });
    remember("vps", "term_spawn", "t1");
    expect(routeOf("git_status", { projectId: "p1" })).toBe("vps");
    expect(routeOf("agent_stop", { sessionId: "s1" })).toBe("vps");
    expect(routeOf("term_write", { id: "t1", data: "ls\n" })).toBe("vps");
    expect(routeOf("projects_remove", { id: "p1" })).toBe("vps");
  });

  it("keeps everything else on this PC", () => {
    expect(routeOf("git_status", { projectId: "local-project" })).toBeNull();
    expect(routeOf("projects_list")).toBeNull();
    // `id` means a project or terminal only for those commands.
    own("x9", "vps");
    expect(routeOf("runs_list_all", { id: "x9" })).toBeNull();
  });

  it("forgets a server's ids when it disconnects", () => {
    remember("vps", "agent_sessions", [{ id: "s2" }, { id: "s3" }]);
    expect(routeOf("agent_events", { sessionId: "s3" })).toBe("vps");
    forgetMaster("vps");
    expect(routeOf("agent_events", { sessionId: "s3" })).toBeNull();
    expect(routeOf("git_status", { projectId: "p1" })).toBeNull();
  });
});

describe("the machine on screen (24z3 A1)", () => {
  it("gets calls with no id; This PC when none is shown", () => {
    expect(machineFor("projects_list")).toBeNull();
    showMachine("vps");
    expect(machineFor("projects_list")).toBe("vps");
    expect(machineFor("projects_add", { path: "/srv/site" })).toBe("vps");
    expect(machineFor("backups_info")).toBe("vps");
    showMachine(null);
    expect(machineFor("projects_add", { path: "E:/site" })).toBeNull();
  });

  it("never gets calls about this window or install", () => {
    showMachine("vps");
    for (const cmd of ["app_restart", "account_wait", "remote_connect", "notify_test", "attachment_file"]) {
      expect(windowOnly(cmd)).toBe(true);
      expect(machineFor(cmd)).toBeNull();
    }
    showMachine(null);
  });

  it("leaves an id with the machine that made it", () => {
    own("p7", "box");
    showMachine("vps");
    expect(machineFor("git_status", { projectId: "p7" })).toBe("box");
    showMachine(null);
    expect(machineFor("git_status", { projectId: "p7" })).toBe("box");
    forgetMaster("box");
  });

  it("goes back to This PC when the shown machine disconnects", () => {
    showMachine("vps");
    forgetMaster("vps");
    expect(shownMachine()).toBeNull();
    expect(machineFor("projects_list")).toBeNull();
  });

  it("keeps a lost machine shown while it reconnects, its calls never on This PC (24z3 Part E)", () => {
    showMachine("vps");
    forgetMaster("vps", true);
    expect(shownMachine()).toBe("vps");
    expect(machineFor("projects_list")).toBe("vps");
    showMachine(null);
  });

  it("sends a shown machine's calls over its connection", async () => {
    invoke.mockReset().mockResolvedValue([]);
    showMachine("vps");
    await routed("projects_list");
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "projects_list" }),
    );
    invoke.mockClear();
    await routed("app_info");
    expect(invoke).toHaveBeenCalledWith("app_info", undefined);
    showMachine(null);
  });

  it("says plainly what a master cannot do yet, never doing it on This PC", async () => {
    nameMachines((id) => (id === "vps" ? "VPS" : id));
    invoke.mockReset().mockRejectedValue("This master does not know the command backups_info.");
    showMachine("vps");
    const e: unknown = await routed("backups_info").catch((x: unknown) => x);
    expect(e).toBeInstanceOf(NotOnMachine);
    expect((e as Error).message).toBe("This cannot be done on VPS yet (backups_info).");
    expect(invoke).toHaveBeenCalledTimes(1);
    invoke.mockReset().mockRejectedValue("The master did not answer in 30 seconds.");
    expect(await routed("projects_list").catch((x) => x)).toBe("The master did not answer in 30 seconds.");
    showMachine(null);
  });
});

describe("the master's reply for an unknown command", () => {
  it("is the text the router looks for, in every master dispatch", () => {
    for (const [file, text] of Object.entries({ masterRs, masterInboxRs, masterRunsRs })) {
      expect(text, file).toContain('_ => Err(format!("This master does not know the command {cmd}.")),');
    }
  });
});

describe("a project on this computer", () => {
  it("stays here when a server lists the same id", () => {
    own("same-id", "vps");
    expect(routeOf("git_status", { projectId: "same-id" })).toBe("vps");
    disown("same-id");
    expect(routeOf("git_status", { projectId: "same-id" })).toBeNull();
  });
});

describe("This PC is a machine like any other (migration M5a)", () => {
  it("sends This PC's work to its own master, and keeps the screen's own commands in the app", async () => {
    invoke.mockReset().mockResolvedValue([]);
    showMachine(null);
    await routed("projects_list");
    expect(invoke).toHaveBeenLastCalledWith(
      "remote_call",
      expect.objectContaining({ master: "this-computer", cmd: "projects_list", args: {}, streams: false }),
    );
    await routed("app_info");
    expect(invoke).toHaveBeenLastCalledWith("app_info", undefined);
    // Its ids stay This PC's: nothing is owned by the link.
    await routed("agent_start", { projectId: "here" });
    expect(routeOf("git_status", { projectId: "here" })).toBeNull();
  });

  it("says what This PC's master does not answer, in words about this computer", async () => {
    invoke.mockReset().mockRejectedValue("This master does not know the command nothing_here.");
    showMachine(null);
    const e: unknown = await routed("nothing_here").catch((x: unknown) => x);
    expect(e).toBeInstanceOf(NotOnMachine);
    expect((e as Error).message).toBe("This cannot be done on this computer yet (nothing_here).");
  });
});

describe("a terminal whose link ended (j1)", () => {
  it("says the shell ended with it, naming the machine", () => {
    nameMachines((id) => (id === "vps" ? "VPS" : id));
    expect(linkEndedText(null)).toBe(
      "Qanuvi's background part on this computer stopped, and this shell ended with it. Close this tab or open a new terminal.",
    );
    expect(linkEndedText("vps")).toMatch(/^The connection to VPS ended, and this shell ended with it./);
  });
});

describe("link errors (ce1)", () => {
  it("knows the texts that say a link was gone, and not other errors", async () => {
    const { isLinkError } = await import("./remote");
    expect(isLinkError("The connection to the master has ended.")).toBe(true);
    expect(isLinkError("Home PC: That computer is not online.")).toBe(true);
    expect(isLinkError("The file could not be saved.")).toBe(false);
  });

  it("tells who asks of a call that got through, and not of one that failed", async () => {
    const { callMaster, onCallOk } = await import("./remote");
    const seen: string[] = [];
    const stop = onCallOk((m) => seen.push(m));
    invoke.mockRejectedValueOnce("The connection to the master has ended.");
    await expect(callMaster("laptop-x", "projects_list", {})).rejects.toBe("The connection to the master has ended.");
    expect(seen).toEqual([]);
    invoke.mockResolvedValueOnce([]);
    await callMaster("laptop-x", "projects_list", {});
    expect(seen).toEqual(["laptop-x"]);
    stop();
    invoke.mockResolvedValueOnce([]);
    await callMaster("laptop-x", "projects_list", {});
    expect(seen).toEqual(["laptop-x"]);
  });
});
