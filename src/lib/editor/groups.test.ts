import { describe, expect, it } from "vitest";
import { addTo, cleanRatio, join, move, removeFrom, shownGroups, tidy, type Group } from "./groups";

const g = (keys: string[], active: string | null = keys[keys.length - 1] ?? null): Group => ({ keys, active });

describe("editor groups", () => {
  it("adds a tab at the end or before another, once, and makes it active", () => {
    expect(addTo([g(["a", "b"])], 0, "c")).toEqual([g(["a", "b", "c"], "c")]);
    expect(addTo([g(["a", "b"])], 0, "c", "b")).toEqual([g(["a", "c", "b"], "c")]);
    expect(addTo([g(["a", "b"])], 0, "a", null)).toEqual([g(["b", "a"], "a")]);
  });

  it("makes the second group when adding to it", () => {
    expect(addTo([g(["a"])], 1, "a")).toEqual([g(["a"]), g(["a"], "a")]);
  });

  it("picks the neighbour when the active tab goes: the one in its place, else the one before", () => {
    expect(removeFrom([g(["a", "b", "c"], "b")], 0, "b")[0]).toEqual(g(["a", "c"], "c"));
    expect(removeFrom([g(["a", "b", "c"], "c")], 0, "c")[0]).toEqual(g(["a", "b"], "b"));
    expect(removeFrom([g(["a"], "a")], 0, "a")[0]).toEqual(g([], null));
    // A tab that was not active leaves the active one alone.
    expect(removeFrom([g(["a", "b"], "a")], 0, "b")[0]).toEqual(g(["a"], "a"));
  });

  it("skips tabs of other projects when picking the neighbour", () => {
    const shown = (k: string) => k !== "other";
    expect(removeFrom([g(["a", "b", "other"], "b")], 0, "b", shown)[0].active).toBe("a");
  });

  it("moves a tab to the other group, or to a new one", () => {
    expect(move([g(["a", "b"], "b"), g(["c"])], "b", 0, 1)).toEqual([g(["a"], "a"), g(["c", "b"], "b")]);
    expect(move([g(["a", "b"], "b")], "b", 0, 1)).toEqual([g(["a"], "a"), g(["b"], "b")]);
    // No third group: it goes to the last one.
    expect(move([g(["a"]), g(["b"])], "a", 0, 2)).toEqual([g([], null), g(["b", "a"], "a")]);
  });

  it("reorders within a group", () => {
    expect(move([g(["a", "b", "c"], "a")], "c", 0, 0, "a")).toEqual([g(["c", "a", "b"], "c")]);
  });

  it("drops an emptied group and moves the focus with it", () => {
    expect(tidy([g([]), g(["b"])], 1)).toEqual({ groups: [g(["b"])], focused: 0 });
    expect(tidy([g(["a"]), g([])], 1)).toEqual({ groups: [g(["a"])], focused: 0 });
    expect(tidy([g(["a"]), g(["b"])], 1)).toEqual({ groups: [g(["a"]), g(["b"])], focused: 1 });
    expect(tidy([g([])], 0)).toEqual({ groups: [g([], null)], focused: 0 });
  });

  it("joins the groups without doubles", () => {
    expect(join([g(["a", "b"], "a"), g(["b", "c"])])).toEqual([g(["a", "b", "c"], "a")]);
  });

  it("shows only the groups with tabs for the project", () => {
    const groups = [g(["p1"]), g(["p2"])];
    expect(shownGroups(groups, (k) => k.startsWith("p"))).toEqual([0, 1]);
    expect(shownGroups(groups, (k) => k === "p2")).toEqual([1]);
    expect(shownGroups(groups, () => false)).toEqual([0]);
  });

  it("keeps the split ratio sensible", () => {
    expect(cleanRatio(0.3)).toBe(0.3);
    expect(cleanRatio(0.01)).toBe(0.15);
    expect(cleanRatio(2)).toBe(0.85);
    expect(cleanRatio("x")).toBe(0.5);
  });
});
