import { describe, expect, it } from "vitest";
import type { Item, Row } from "./events";
import {
  BRANCH_CHARS,
  branchPreamble,
  matchesIn,
  rewindFrom,
  rewindText,
  rowsToOpen,
  searchItems,
} from "./sessionTools";

type Tool = Extract<Item, { kind: "tool" }>;
function tool(id: string, over: Partial<Tool> = {}): Tool {
  return {
    kind: "tool",
    id,
    name: "Edit",
    summary: "a.ts",
    file: "C:/p/a.ts",
    state: "done",
    result: "",
    input: "",
    output: "",
    ...over,
  };
}
const user = (id: string, text: string): Item => ({ kind: "user", id, text });
const said = (id: string, text: string): Item => ({ kind: "text", id, text });

describe("branchPreamble", () => {
  it("tells the branch what was said before, and whether the files were reset", () => {
    const p = branchPreamble([user("1", "add a cart"), tool("2"), said("3", "Added it.")], true);
    expect(p).toContain("Owner: add a cart");
    expect(p).toContain("(Agent step: Edit a.ts)");
    expect(p).toContain("Agent: Added it.");
    expect(p).toContain("were undone");
    expect(p.endsWith("The owner's message, as changed:\n\n")).toBe(true);
    expect(branchPreamble([user("1", "x")], false)).toContain("were not reset");
  });
  it("is empty for the first message, and leaves text still being written out", () => {
    expect(branchPreamble([], false)).toBe("");
    expect(branchPreamble([{ kind: "text", id: "1", text: "half", live: true }], false)).toBe("");
  });
  it("keeps the newest part of a long conversation", () => {
    const long = Array.from({ length: 400 }, (_, i) => said(String(i), `answer ${i} ${"x".repeat(100)}`));
    const p = branchPreamble(long, false);
    expect(p).toContain("the oldest part left out");
    expect(p).toContain("answer 399");
    expect(p).not.toContain("answer 0 ");
    expect(p.length).toBeLessThan(BRANCH_CHARS + 2000);
  });
});

describe("rewindFrom", () => {
  it("collects the edits from a point on, per file, oldest first", () => {
    const items: Item[] = [
      tool("0", { edits: [{ old: "a", new: "b" }] }),
      user("1", "more"),
      tool("2", { edits: [{ old: "b", new: "c" }] }),
      tool("3", { file: "C:/p/b.ts", edits: [{ old: "x", new: "y" }] }),
      tool("4", { edits: [{ old: "c", new: "d" }] }),
    ];
    const r = rewindFrom(items, 1);
    expect(r.steps).toBe(3);
    expect(r.files).toEqual([
      {
        file: "C:/p/a.ts",
        edits: [
          { old: "b", new: "c" },
          { old: "c", new: "d" },
        ],
      },
      { file: "C:/p/b.ts", edits: [{ old: "x", new: "y" }] },
    ]);
    expect(rewindText("let v = d;", r.files[0].edits)).toBe("let v = b;");
  });
  it("names files written whole, and skips failed and look-only steps", () => {
    const items: Item[] = [
      user("1", "go"),
      tool("2", { edits: [{ old: "a", new: "b" }] }),
      tool("3", { name: "Write", whole: true, edits: [{ old: "", new: "all" }] }),
      tool("4", { file: "C:/p/c.ts", state: "failed", edits: [{ old: "q", new: "r" }] }),
      tool("5", { name: "Read", file: "C:/p/d.ts" }),
    ];
    const r = rewindFrom(items, 0);
    expect(r.files).toEqual([]);
    expect(r.cannot).toEqual([{ file: "C:/p/a.ts", why: expect.stringContaining("written whole") }]);
    expect(r.steps).toBe(2);
  });
  it("cannot undo a file that changed since", () => {
    expect(rewindText("something else", [{ old: "a", new: "b" }])).toBeNull();
  });
});

describe("search", () => {
  const items: Item[] = [
    user("1", "Fix the Login page"),
    tool("2", { name: "Bash", input: "npm test", output: "3 passed" }),
    said("3", "The login works now."),
  ];
  it("finds items by their words, case not mattering, tool input and output too", () => {
    expect(searchItems(items, "login")).toEqual(["1", "3"]);
    expect(searchItems(items, "PASSED")).toEqual(["2"]);
    expect(searchItems(items, "  ")).toEqual([]);
  });
  it("opens the groups and folds that hold a hit", () => {
    const step = tool("s1");
    const rows: Row[] = [
      user("1", "x"),
      {
        kind: "fold",
        id: "f1",
        count: 12,
        rows: [{ kind: "group", id: "g1", label: "Read 3 files", steps: [step], state: "done" }, said("t", "y")],
      },
      { kind: "group", id: "g2", label: "Read 3 files", steps: [tool("s2")], state: "done" },
    ];
    expect(rowsToOpen(rows, new Set(["s1"])).sort()).toEqual(["f1", "g1"]);
    expect(rowsToOpen(rows, new Set(["t"]))).toEqual(["f1"]);
    expect(rowsToOpen(rows, new Set(["1"]))).toEqual([]);
  });
  it("finds every place in a text", () => {
    expect(matchesIn("Aa aA", "aa")).toEqual([0, 3]);
    expect(matchesIn("abc", "")).toEqual([]);
  });
});
