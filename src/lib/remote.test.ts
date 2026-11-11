import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));

const { disown, forgetMaster, own, remember, routeOf } = await import("./remote");

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

describe("a project on this computer", () => {
  it("stays here when a server lists the same id", () => {
    own("same-id", "vps");
    expect(routeOf("git_status", { projectId: "same-id" })).toBe("vps");
    disown("same-id");
    expect(routeOf("git_status", { projectId: "same-id" })).toBeNull();
  });
});
