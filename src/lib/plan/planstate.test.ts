import { describe, expect, it } from "vitest";
import {
  closingRan,
  closingsDue,
  emptyPlanState,
  holdWording,
  keepAnswer,
  keepHolds,
  markLate,
  pruneState,
} from "./planstate";

describe("kept per plan (24u part 2)", () => {
  it("says who set a hold", () => {
    expect(holdWording({ why: "pushes to the server", by: "you" })).toBe(
      "you asked to see it first (pushes to the server)",
    );
    expect(holdWording({ why: "a question was left open (dash?)", by: "default" })).toBe(
      "Qanuvi's default: a question was left open (dash?)",
    );
  });

  it("replaces holds of the same step and keeps the others", () => {
    let s = keepHolds(emptyPlanState(), [{ step: "1a", why: "old", by: "default" }], 1);
    s = keepHolds(
      s,
      [
        { step: "1a", why: "new", by: "you" },
        { step: "2a", why: "x", by: "default" },
      ],
      2,
    );
    expect(s.holds.map((h) => `${h.step}:${h.why}:${h.by}`)).toEqual(["1a:new:you", "2a:x:default"]);
  });

  it("an answer removes the step's hold, and a later check does not hold an answered step again", () => {
    let s = keepHolds(emptyPlanState(), [{ step: "1a", why: "q", by: "default" }], 1);
    s = keepAnswer(s, "1a", "go", "dash", 2);
    expect(s.holds).toEqual([]);
    expect(s.answers).toEqual([{ step: "1a", choice: "go", text: "dash", at: 2 }]);
    s = keepHolds(s, [{ step: "1a", why: "q", by: "default" }], 3);
    expect(s.holds).toEqual([]);
    s = keepAnswer(s, "1a", "skip", "", 4);
    expect(s.answers).toEqual([{ step: "1a", choice: "skip", text: "", at: 4 }]);
  });

  it("drops entries of steps no longer open", () => {
    let s = keepHolds(emptyPlanState(), [{ step: "1a", why: "q", by: "default" }], 1);
    s = keepAnswer(s, "2a", "go", "", 2);
    s = pruneState(s, new Set(["1a"]));
    expect(s.holds.length).toBe(1);
    expect(s.answers).toEqual([]);
  });

  it("runs a closing step again once for its section, when nothing it waits for is pending", () => {
    let s = closingRan(emptyPlanState(), "1z", ["1a", "1b"]);
    s = markLate(s, "1a");
    expect(s.closings).toEqual([{ closing: "1z", waiting: ["1b"], late: ["1a"] }]);
    // 1b still pending in this run: not yet; the run ending: now.
    expect(closingsDue(s, (x) => x === "1b", false)).toEqual([]);
    expect(closingsDue(s, (x) => x === "1b", true).map((c) => c.closing)).toEqual(["1z"]);
    s = markLate(s, "1b");
    expect(closingsDue(s, () => false, false)[0].late).toEqual(["1a", "1b"]);
    // It ran again with nothing left aside: the record goes.
    expect(closingRan(s, "1z", []).closings).toEqual([]);
  });

  it("keeps a closing record across runs; a waited-for step done meanwhile counts as late", () => {
    const s = closingRan(emptyPlanState(), "1z", ["1a", "1b"]);
    const all = new Set(["1a", "1b", "1z"]);
    const kept = pruneState(s, new Set(["1b"]), all);
    expect(kept.closings).toEqual([{ closing: "1z", waiting: ["1b"], late: ["1a"] }]);
    expect(pruneState(s, new Set(), new Set(["1a"])).closings).toEqual([]);
  });
});
