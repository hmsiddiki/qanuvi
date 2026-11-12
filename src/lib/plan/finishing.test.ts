import { describe, expect, it } from "vitest";
import {
  doneNotes,
  finishEntry,
  finishEstimate,
  finishFullPrompt,
  finishReviewPrompt,
  isPlanFile,
  offersFinish,
  parseFinishReview,
  stillOpenIn,
  type FinishPass,
} from "./finishing";
import { runTotals, type Attempt, type StepRun } from "./runner";

function attempt(costUsd: number, seconds: number, outcome: Attempt["outcome"] = "done"): Attempt {
  return { model: "m", sessionId: "s", outcome, costUsd, inputTokens: 0, outputTokens: 0, seconds, note: "" };
}
function step(status: StepRun["status"], attempts: Attempt[]): StepRun {
  return { line: 1, number: "1", title: "t", attempts, status };
}

describe("finishEstimate", () => {
  it("takes half a typical paid step for a light pass", () => {
    const run = { steps: [step("done", [attempt(1, 100)]), step("done", [attempt(3, 300)])] };
    expect(finishEstimate(run, "light")).toEqual({ costUsd: 1, seconds: 100 });
  });
  it("grows with the run for a full pass, up to four steps", () => {
    const run = { steps: [step("done", [attempt(2, 60)])] };
    expect(finishEstimate(run, "full").costUsd).toBeCloseTo(3.2);
    const long = { steps: Array.from({ length: 40 }, () => step("done", [attempt(1, 60)])) };
    expect(finishEstimate(long, "full").costUsd).toBe(4);
  });
  it("knows no cost when only free steps finished, and nothing without finished steps", () => {
    expect(finishEstimate({ steps: [step("done", [attempt(0, 50)])] }, "light")).toEqual({
      costUsd: null,
      seconds: 25,
    });
    expect(finishEstimate({ steps: [step("failed", [attempt(1, 1, "open")])] }, "light")).toEqual({
      costUsd: null,
      seconds: null,
    });
  });
});

describe("offersFinish", () => {
  const done = step("done", [attempt(1, 1)]);
  it("offers it once, for a run that got done with work in it", () => {
    expect(offersFinish({ status: "done", steps: [done] })).toBe(true);
    expect(offersFinish({ status: "stopped", steps: [done] })).toBe(false);
    expect(offersFinish({ status: "done", steps: [step("done", [])] })).toBe(false);
    expect(offersFinish({ status: "done", steps: [done], finish: { status: "done" } as FinishPass })).toBe(false);
  });
});

describe("prompts", () => {
  const steps = [{ number: "3", title: "Add the cart", done: ["- Done: cart added, tests pass"] }];
  it("hands the review the steps, their Done notes, the checks and the diff", () => {
    const p = finishReviewPrompt("PLAN.md", steps, "diff --git a/x", {
      command: "npm test",
      ok: false,
      summary: "1 failed",
    });
    expect(p).toContain("3. Add the cart");
    expect(p).toContain("- Done: cart added");
    expect(p).toContain("npm test): FAILED\n1 failed");
    expect(p).toContain("diff --git a/x");
    expect(p).toContain('"stillOpen"');
  });
  it("cuts a very long diff", () => {
    const p = finishReviewPrompt("PLAN.md", steps, "x".repeat(50_000), null);
    expect(p).toContain("(cut: the diff is longer)");
    expect(p).toContain("no test command");
  });
  it("keeps live tests to scratch copies and asks for a Still open list", () => {
    const p = finishFullPrompt("PLAN.md", steps, { findings: ["a.ts: debug log left"], stillOpen: [] }, null, ["a.ts"]);
    expect(p).toContain("never on the owner's real folders");
    expect(p).toContain("`Still open:`");
    expect(p).toContain("- a.ts: debug log left");
    expect(p).toContain("- a.ts");
  });
});

describe("parseFinishReview", () => {
  it("reads the answer, fenced or not", () => {
    expect(
      parseFinishReview('```json\n{"ok": false, "findings": ["a: b"], "stillOpen": ["set the key"]}\n```'),
    ).toEqual({
      ok: false,
      findings: ["a: b"],
      stillOpen: ["set the key"],
    });
  });
  it("never passes quietly on an answer it cannot read", () => {
    const r = parseFinishReview("Looks good to me");
    expect(r.ok).toBe(false);
    expect(r.findings[0]).toContain("could not be read");
  });
});

describe("stillOpenIn", () => {
  it("reads the list under the heading", () => {
    expect(stillOpenIn("Fixed two things.\n\nStill open:\n- set the SMTP login\n- decide the price\n")).toEqual([
      "set the SMTP login",
      "decide the price",
    ]);
    expect(stillOpenIn("## Still open\n1. one\n2) two")).toEqual(["one", "two"]);
    expect(stillOpenIn("**Still open:**\n* x")).toEqual(["x"]);
  });
  it("reads none as an empty list, and a missing heading as null", () => {
    expect(stillOpenIn("Still open:\n- none")).toEqual([]);
    expect(stillOpenIn("Still open: none")).toEqual([]);
    expect(stillOpenIn("All done.")).toBeNull();
  });
  it("stops at the first text after the list", () => {
    expect(stillOpenIn("Still open:\n- a\n\nThat is all.\n- not this")).toEqual(["a"]);
  });
});

describe("helpers", () => {
  it("finds the Done notes", () => {
    expect(doneNotes(["  - Model: x", "  - Done: did it", "- done: lower"])).toEqual([
      "- Done: did it",
      "- done: lower",
    ]);
  });
  it("leaves the plan, its archive and its logs out of the run's changes", () => {
    expect(isPlanFile("docs/PLAN.md", "docs/PLAN.md", ".qanuvi/plans/docs_PLAN")).toBe(true);
    expect(isPlanFile("docs/PLAN.archive.md", "docs/PLAN.md", ".qanuvi/plans/docs_PLAN")).toBe(true);
    expect(isPlanFile(".qanuvi/plans/docs_PLAN/3.md", "docs/PLAN.md", ".qanuvi/plans/docs_PLAN")).toBe(true);
    expect(isPlanFile("src/cart.ts", "docs/PLAN.md", ".qanuvi/plans/docs_PLAN")).toBe(false);
  });
  it("writes a log entry", () => {
    const f: FinishPass = {
      level: "light",
      status: "done",
      model: "m",
      startedAt: 0,
      endedAt: 1000,
      costUsd: 0.4,
      seconds: 61,
      checks: { command: "npm test", ok: true, summary: "" },
      findings: ["x"],
      stillOpen: [],
    };
    const e = finishEntry(f);
    expect(e).toContain("## Finishing pass (light)");
    expect(e).toContain("$0.40");
    expect(e).toContain("Checks (npm test): passed");
    expect(e).toContain("  - x");
    expect(e).toContain("Still open: nothing");
  });
  it("counts the pass in the run's cost", () => {
    const f = { costUsd: 0.5, seconds: 10, model: "m" } as FinishPass;
    const t = runTotals({ steps: [step("done", [attempt(1, 5)])], finish: f });
    expect(t.costUsd).toBeCloseTo(1.5);
    expect(t.byModel.get("finish:m")?.costUsd).toBe(0.5);
  });
});
