import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Draft } from "$lib/api";

// 24z3 Part B2: unsaved text of a hidden machine is kept on it, or held in the window when it cannot be reached;
// taking a project's drafts gives back both, the window's copy winning, and leaves nothing behind.
const onMachine = new Map<string, Draft>();
const refuse = { put: false };
const asked: (string | null)[] = [];
vi.mock("$lib/remote", () => ({
  callOn: vi.fn(async (machine: string | null, cmd: string, args: Record<string, unknown>) => {
    asked.push(machine);
    if (cmd === "drafts_put") {
      if (refuse.put) throw new Error("not connected");
      const d = args.draft as Draft;
      onMachine.set(`${d.projectId}/${d.path}`, d);
    } else if (cmd === "drafts_list") return [...onMachine.values()].filter((d) => d.projectId === args.projectId);
    else if (cmd === "drafts_drop") onMachine.delete(`${args.projectId}/${args.path}`);
    return undefined;
  }),
}));

const { DraftsStore } = await import("./drafts");
const draft = (path: string, text: string, projectId = "p1"): Draft => ({ projectId, path, text, hash: "h" });

beforeEach(() => {
  onMachine.clear();
  asked.length = 0;
  refuse.put = false;
});

describe("drafts of a hidden machine", () => {
  it("are kept on the machine and taken back once", async () => {
    const d = new DraftsStore();
    await d.keep("vps", [draft("a.js", "one"), draft("b.js", "two", "p2")], true);
    expect(onMachine.size).toBe(2);
    expect(new Set(asked), "every call names the machine").toEqual(new Set(["vps"]));
    expect((await d.take("vps", "p1")).map((x) => x.text)).toEqual(["one"]);
    expect(onMachine.size, "taken drafts leave the machine").toBe(1);
    expect(await d.take("vps", "p1")).toEqual([]);
  });

  it("are held in the window when the machine cannot be reached, and given back on return", async () => {
    const d = new DraftsStore();
    await d.keep("vps", [draft("a.js", "unreachable")], false);
    refuse.put = true;
    await d.keep("vps", [draft("b.js", "no answer")], true);
    expect(onMachine.size).toBe(0);
    expect(d.held("vps").map((x) => x.path)).toEqual(["a.js", "b.js"]);
    expect(d.held(null), "each machine its own").toEqual([]);
    expect((await d.take("vps", "p1")).map((x) => x.text)).toEqual(["unreachable", "no answer"]);
    expect(d.held("vps")).toEqual([]);
  });

  it("a machine shown again at once reads what was just kept", async () => {
    const d = new DraftsStore();
    void d.keep("vps", [draft("a.js", "just kept")], true);
    expect((await d.take("vps", "p1")).map((x) => x.text)).toEqual(["just kept"]);
  });

  it("the window's held copy is the newer one", async () => {
    const d = new DraftsStore();
    await d.keep(null, [draft("a.js", "older")], true);
    await d.keep(null, [draft("a.js", "newer")], false);
    const back = await d.take(null, "p1");
    expect(back.map((x) => x.text)).toEqual(["newer"]);
    expect(onMachine.size).toBe(0);
  });
});

describe("draft preview in the Inbox", () => {
  it("gives short text whole and cuts long text at a line end, saying how much is left", async () => {
    const { draftPreview } = await import("./drafts");
    expect(draftPreview("a\nb")).toBe("a\nb");
    const long = "line one\nline two\nline three";
    const shown = draftPreview(long, 12);
    expect(shown.startsWith("line one\n…")).toBe(true);
    expect(shown).toContain(`${long.length - 8} more characters`);
  });
});
