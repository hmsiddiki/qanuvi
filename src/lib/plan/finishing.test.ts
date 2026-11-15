import { describe, expect, it } from "vitest";
import { finishEstimate, offersFinish, type FinishPass } from "./finishing";
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

describe("helpers", () => {
  it("counts the pass in the run's cost", () => {
    const f = { costUsd: 0.5, seconds: 10, model: "m" } as FinishPass;
    const t = runTotals({ steps: [step("done", [attempt(1, 5)])], finish: f });
    expect(t.costUsd).toBeCloseTo(1.5);
    expect(t.byModel.get("finish:m")?.costUsd).toBe(0.5);
  });
});
