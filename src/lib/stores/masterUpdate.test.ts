import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const { viewOf } = await import("./masterUpdate.svelte");
type State = Parameters<typeof viewOf>[0] & object;

const base = (): State => ({
  current: "0.1.25",
  checking: false,
  checkedAt: 1,
  available: null,
  error: "",
  install: { stage: "", version: "", done: 0, total: 0, waitingFor: "", error: "", program: "" },
});

describe("a master's update on screen (M8c)", () => {
  it("is quiet before its first look and up to date after it", () => {
    expect(viewOf(null).state).toBe("idle");
    expect(viewOf({ ...base(), checkedAt: null }).state).toBe("idle");
    expect(viewOf(base()).state).toBe("none");
  });

  it("offers the button for a build for that machine, and says when there is none yet", () => {
    const found = { version: "0.1.26", notes: "n", url: "http://x/b" };
    const v = viewOf({ ...base(), available: found });
    expect(v).toMatchObject({ state: "available", version: "0.1.26", notes: "n" });
    const none = viewOf({ ...base(), available: { ...found, url: "" } });
    expect(none.state).toBe("none");
    expect(none.noBuild).toContain("no build for this machine yet");
  });

  it("follows the install's stages", () => {
    const at = (stage: State["install"]["stage"]) =>
      viewOf({ ...base(), install: { ...base().install, stage, version: "0.1.26" } });
    expect(at("downloading").state).toBe("downloading");
    expect(at("checking").state).toBe("verifying");
    expect(at("ready").state).toBe("ready");
    expect(at("waiting").state).toBe("waiting");
    expect(at("restarting").state).toBe("restarting");
    expect(at("ready").version).toBe("0.1.26");
  });

  it("offers the button again after a failed install, with the reason", () => {
    const v = viewOf({
      ...base(),
      available: { version: "0.1.26", notes: "", url: "http://x/b" },
      install: { ...base().install, stage: "failed", version: "0.1.26", error: "The signature is wrong." },
    });
    expect(v.state).toBe("available");
    expect(v.error).toBe("The update failed: The signature is wrong.");
  });

  it("says a failed look", () => {
    expect(viewOf({ ...base(), error: "no answer" })).toMatchObject({ state: "error", error: "no answer" });
  });
});

describe("This PC's update (M8d)", () => {
  it("is read from This PC's own master, and an answer without its install part still reads", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const { masterUpdate } = await import("./masterUpdate.svelte");
    const { THIS_PC } = await import("$lib/remote");
    const asked: unknown[] = [];
    vi.mocked(invoke).mockImplementation(async (cmd, args) => {
      asked.push([cmd, (args as { master: string }).master, (args as { cmd: string }).cmd]);
      // An older master: no install part.
      return { current: "0.1.25", checking: false, checkedAt: 1, available: null, error: "" };
    });
    masterUpdate.switched();
    await vi.waitFor(() => expect(masterUpdate.raw).not.toBeNull());
    expect(asked[0]).toEqual(["remote_call", THIS_PC, "update_status"]);
    expect(masterUpdate.thisPc).toBe(true);
    expect(masterUpdate.state).toBe("none");
    expect(masterUpdate.raw?.install.stage).toBe("");
  });
});
