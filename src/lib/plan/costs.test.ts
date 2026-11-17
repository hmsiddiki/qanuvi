import { describe, expect, it } from "vitest";
import { budgetBelowFirstStep, finishedCost, planHistory, usualStepCost } from "./costs";

describe("measuring plan runs", () => {
  it("compares a run with the plan's earlier runs", () => {
    const run = (id: string, file: string, costs: number[], done: number) =>
      ({
        id,
        file,
        data: JSON.stringify({
          steps: costs.map((c, i) => ({ status: i < done ? "done" : "blocked", attempts: [{ costUsd: c }] })),
        }),
      }) as const;
    expect(finishedCost({ steps: [{ status: "done", attempts: [{ costUsd: 2 }, { costUsd: 1 }] }] })).toEqual({
      steps: 1,
      cost: 3,
    });
    const h = planHistory(
      [run("a", "P.md", [5, 6, 17], 2), run("b", "P.md", [2], 1), run("c", "Q.md", [9], 1), run("now", "P.md", [1], 1)],
      "P.md",
      "now",
    );
    expect(h.runs).toBe(2);
    expect(h.steps).toBe(3);
    expect(h.cost).toBe(30);
    expect(h.perStep).toBe(10);
  });
});

describe("a run budget against its first step (b1)", () => {
  const stats = new Map([
    ["feature|opus", { successes: 2, costUsd: 2.4 }],
    ["fix|opus", { successes: 1, costUsd: 0.3 }],
    ["docs|sonnet", { successes: 0, costUsd: 0.5 }],
  ]);
  it("uses the same kind of job first, else every kind on the model", () => {
    expect(usualStepCost(stats, "opus", "feature")).toBeCloseTo(1.2);
    expect(usualStepCost(stats, "opus", "docs")).toBeCloseTo(0.9);
    expect(usualStepCost(stats, "sonnet", "docs")).toBeNull();
    expect(usualStepCost(stats, "haiku", "feature")).toBeNull();
  });
  it("warns only when a set budget is below the usual cost", () => {
    expect(budgetBelowFirstStep(1, 1.16)).toBe(true);
    expect(budgetBelowFirstStep(2, 1.16)).toBe(false);
    expect(budgetBelowFirstStep(0, 5)).toBe(false);
    expect(budgetBelowFirstStep(1, null)).toBe(false);
  });
});
