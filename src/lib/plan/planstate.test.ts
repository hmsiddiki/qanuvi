import { describe, expect, it } from "vitest";
import {
  changeDecided,
  decidedOpen,
  foldDecided,
  emptyPlanState,
  holdWording,
  keepAnswer,
  keepDecided,
  keepHolds,
  keptModel,
  type KeptDecided,
  setKeptModel,
} from "./planstate";

describe("kept per plan (24u part 2)", () => {
  it("says who set a hold", () => {
    expect(holdWording({ why: "pushes to the server", by: "you" })).toBe(
      "you asked to see it first (pushes to the server)",
    );
    expect(holdWording({ why: "a question was left open (dash?)", by: "default" })).toBe(
      "a question was left open (dash?)",
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
});

describe("decided for you, kept per plan (24u part 3)", () => {
  const d = (step: string, answer: string) => ({ step, what: "Separator?", answer, because: "" });

  it("keeps a check's choices and extra checks; a later check replaces those of the same steps", () => {
    let s = keepDecided(emptyPlanState(), [d("1a", "dash"), d("1b", "dash")], [{ step: "2a", why: "migrates" }], [], 1);
    s = keepDecided(s, [d("1a", "underscore")], [], ["2a"], 2);
    expect(s.decided.map((x) => [x.step, x.answer])).toEqual([
      ["1b", "dash"],
      ["1a", "underscore"],
    ]);
    expect(s.checks).toEqual([]);
  });

  it("an owner's change stays when the check decides again", () => {
    let s = keepDecided(emptyPlanState(), [d("1a", "dash")], [], [], 1);
    s = changeDecided(s, "1a", "Separator?", "dot");
    s = keepDecided(s, [d("1a", "dash")], [], [], 2);
    expect(s.decided).toHaveLength(1);
    expect(s.decided[0].changed).toBe("dot");
  });
});

describe("models chosen for steps in Qanuvi (R2e i)", () => {
  it("keeps a step's model in the plan's data, by number or title", () => {
    let s = setKeptModel(emptyPlanState(), { number: "1", title: "Export" }, "claude-sonnet-5-5");
    s = setKeptModel(s, { number: null, title: "Docs" }, "claude-haiku-5-5");
    expect(keptModel(s, { number: "1", title: "Export" })).toBe("claude-sonnet-5-5");
    expect(keptModel(s, { number: null, title: "Docs" })).toBe("claude-haiku-5-5");
    s = setKeptModel(s, { number: "1", title: "Export" }, "claude-opus-5-5");
    expect(s.models.filter((m) => m.step === "1")).toHaveLength(1);
    expect(keptModel(setKeptModel(s, { number: "1", title: "Export" }, null), { number: "1", title: "Export" })).toBe(
      null,
    );
  });
});

describe('"Decided for you" open or folded', () => {
  const choice = (what: string, changed?: string): KeptDecided => ({
    step: "1",
    what,
    answer: "a",
    because: "",
    at: 0,
    ...(changed ? { changed } : {}),
  });
  const withChoices = (n: number) => ({
    ...emptyPlanState(),
    decided: Array.from({ length: n }, (_, i) => choice(`q${i}`)),
  });

  it("opens while a choice is unchanged, until the owner folds it", () => {
    expect(decidedOpen(emptyPlanState())).toBe(false);
    expect(decidedOpen(withChoices(2))).toBe(true);
    const folded = foldDecided(withChoices(2), false);
    expect(folded.decidedFolded).toBe(2);
    // A step change or handover reloads the same choices: it stays folded.
    expect(decidedOpen(folded)).toBe(false);
    // Opened again by the owner: the fold is forgotten.
    expect(foldDecided(folded, true).decidedFolded).toBeUndefined();
    expect(decidedOpen(foldDecided(folded, true))).toBe(true);
  });

  it("opens again for a new choice, never for all changed ones", () => {
    const folded = foldDecided(withChoices(2), false);
    expect(decidedOpen({ ...folded, decided: [...folded.decided, choice("new")] })).toBe(true);
    expect(decidedOpen({ ...emptyPlanState(), decided: [choice("x", "b")] })).toBe(false);
  });
});
