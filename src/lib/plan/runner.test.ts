import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { FREE_MODEL, costRatio, freeSavings, paidPerStep, runTotals, parseRanges } from "./runner";

describe("runTotals", () => {
  it("adds up cost and tokens per model", () => {
    const a = (model: string, costUsd: number) => ({
      model,
      sessionId: "s",
      outcome: "done" as const,
      costUsd,
      inputTokens: 100,
      outputTokens: 10,
      seconds: 2,
      note: "",
    });
    const totals = runTotals({
      steps: [
        { line: 1, number: "1", title: "A", status: "done", attempts: [a("h", 0.01), a("s", 0.05)] },
        { line: 2, number: "2", title: "B", status: "done", attempts: [a("h", 0.02)] },
      ],
    });
    expect(totals.costUsd).toBeCloseTo(0.08);
    expect(totals.inputTokens).toBe(300);
    expect(totals.byModel.get("h")).toMatchObject({ attempts: 2, costUsd: 0.03 });
  });
});

describe("free models first", () => {
  const a = (model: string, outcome: "done" | "open" | "error", costUsd = 0) => ({
    model,
    outcome,
    costUsd,
    sessionId: "s",
    inputTokens: 0,
    outputTokens: 0,
    seconds: 0,
    note: "",
  });

  it("counts what free models saved from what paid steps of the same kind cost before", () => {
    const stats = new Map([
      ["docs|claude-sonnet-5-5", { tries: 4, successes: 4, costUsd: 0.4 }],
      ["docs|qanuvi:auto", { tries: 3, successes: 3, costUsd: 0 }],
    ]);
    const per = paidPerStep(stats);
    expect(per.docs).toBeCloseTo(0.1);
    const run = {
      steps: [
        {
          line: 1,
          number: "1",
          title: "Docs",
          status: "done" as const,
          jobType: "docs" as const,
          attempts: [a(FREE_MODEL, "done")],
        },
        {
          line: 2,
          number: "2",
          title: "Feature",
          status: "done" as const,
          jobType: "feature" as const,
          attempts: [a(FREE_MODEL, "open"), a("claude-sonnet-5-5", "done", 0.3)],
        },
      ],
    };
    expect(freeSavings(run, per)).toEqual({ steps: 1, estimateUsd: 0.1 });
    // No history for the kind: this run's paid steps are the measure; none at all: unknown.
    run.steps[0].jobType = "ui" as never;
    expect(freeSavings(run, per).estimateUsd).toBeCloseTo(0.3);
    expect(freeSavings({ steps: [run.steps[0]] }, {}).estimateUsd).toBeNull();
  });
});

describe("runTotals with the decider", () => {
  it("counts the decider's calls in the cost, as their own line", () => {
    const totals = runTotals({
      steps: [],
      deciderCalls: [
        { model: "claude-sonnet-5-5", costUsd: 0.01 },
        { model: "claude-sonnet-5-5", costUsd: 0.02 },
      ],
    });
    expect(totals.costUsd).toBeCloseTo(0.03);
    expect(totals.byModel.get("decider:claude-sonnet-5-5")).toMatchObject({ attempts: 2 });
  });
});

describe("runs that finish by themselves (24f)", () => {
  it("reads ranges and says what it could not read", () => {
    expect(parseRanges("2-6, 10-15, 20")).toEqual({
      ranges: [
        [2, 6],
        [10, 15],
        [20, 20],
      ],
      error: "",
    });
    expect(parseRanges("6-2").ranges).toEqual([[2, 6]]);
    expect(parseRanges("2-6, x").error).toContain("x");
    expect(parseRanges("").ranges).toEqual([]);
  });
});

describe("run settings: one model, Team per run", () => {
  const models = CLAUDE_CODE.models;
  it("compares one model on every step with the plan's own models", () => {
    // Sonnet ($10 out) on every step, against Opus ($20) and Haiku ($0.50): (10 * 2) / 20.5.
    expect(costRatio(models, ["claude-opus-5-5", "claude-haiku-5-5"], "claude-sonnet-5-5")).toBeCloseTo(0.976, 2);
    expect(costRatio(models, ["claude-sonnet-5-5"], "claude-fable-5-2")).toBe(4);
    expect(costRatio(models, ["unknown"], "claude-fable-5-2")).toBeNull();
    expect(costRatio(models, [], "claude-fable-5-2")).toBeNull();
  });
});
