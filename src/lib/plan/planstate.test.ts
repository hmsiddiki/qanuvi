import { describe, expect, it } from "vitest";
import { emptyPlanState, holdWording, keepAnswer, keepHolds, pruneState } from "./planstate";

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
});
