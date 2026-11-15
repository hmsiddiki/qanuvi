import { describe, expect, it } from "vitest";
import { finishedCost, planHistory } from "./costs";

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
