import { describe, expect, it } from "vitest";
import { finishedCost, overLimit, planHistory, toolKinds } from "./costs";

const use = (name: string, input: Record<string, unknown> = {}) =>
  JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", id: "x", name, input }] } });

describe("measuring plan runs", () => {
  it("sorts tool calls into reading, changing and running", () => {
    const k = toolKinds([
      use("Read"),
      use("mcp__qanuvi__find_symbol"),
      use("Bash", { description: "Read CheckoutPage first half" }),
      use("Bash", { description: "Run the embedded checkout test with the database" }),
      use("Edit"),
      use("Write"),
      "not json",
    ]);
    expect(k).toEqual({ read: 3, change: 2, run: 1 });
  });

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

  it("knows when a step went over its limits", () => {
    expect(overLimit(3.5, 10, { usd: 3 })).toContain("over the limit of $3.00");
    expect(overLimit(1, 45, { usd: 3, minutes: 30 })).toContain("45 minutes");
    expect(overLimit(1, 10, { usd: 3, minutes: 30 })).toBeNull();
    expect(overLimit(99, 99, {})).toBeNull();
  });
});
