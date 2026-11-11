import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { budgetState, chooseModel, jobType, learn, learnedModel, monthStart } from "./routing";

const models = CLAUDE_CODE.models; // fable, opus, sonnet, haiku
const H = "claude-haiku-5-5";
const S = "claude-sonnet-5-5";
const F = "claude-fable-5-1";

describe("jobType", () => {
  it("sorts steps by the kind of work", () => {
    expect(jobType("Update the README with install steps")).toBe("docs");
    expect(jobType("Add tests for the cart")).toBe("tests");
    expect(jobType("Restyle the layout of the settings modal")).toBe("ui");
    expect(jobType("Fix the crash when the list is empty")).toBe("bugfix");
    expect(jobType("Rename CartService to Cart and move it")).toBe("refactor");
    expect(jobType("Add a wishlist page")).toBe("feature");
  });

  it("understands plurals", () => {
    expect(jobType("Round discounted prices")).toBe("security");
    expect(jobType("Coupons")).toBe("security");
    expect(jobType("Update the docs and comments")).toBe("docs");
    expect(jobType("Restyle the buttons")).toBe("ui");
    expect(jobType("Fixes for two crashes")).toBe("bugfix");
  });

  it("lets money and security win over everything else", () => {
    expect(jobType("Fix the refund docs and add tests")).toBe("security");
    expect(jobType("Checkout: pay what you want")).toBe("security");
  });
});

const attempt = (model: string, outcome: "done" | "open" | "stopped", costUsd: number) => ({
  model,
  outcome,
  costUsd,
  sessionId: "",
  inputTokens: 0,
  outputTokens: 0,
  seconds: 0,
  note: "",
});

const runs = [
  {
    steps: [
      { line: 1, number: "1", title: "Update README", status: "done" as const, attempts: [attempt(H, "done", 0.01)] },
      { line: 2, number: "2", title: "Docs for setup", status: "done" as const, attempts: [attempt(H, "done", 0.012)] },
      {
        line: 3,
        number: "3",
        title: "Fix typo in docs",
        status: "done" as const,
        attempts: [attempt(H, "done", 0.008)],
      },
      {
        line: 4,
        number: "4",
        title: "Add a wishlist page",
        status: "done" as const,
        attempts: [attempt(H, "open", 0.01), attempt(S, "done", 0.06), attempt(H, "stopped", 0.01)],
      },
    ],
  },
];

describe("learn and learnedModel", () => {
  const stats = learn(runs, jobType);

  it("counts tries, successes and cost per job type and model, ignoring stopped tries", () => {
    expect(stats.get(`docs|${H}`)).toMatchObject({ tries: 3, successes: 3 });
    expect(stats.get(`docs|${H}`)?.costUsd).toBeCloseTo(0.03);
    expect(stats.get(`feature|${H}`)).toEqual({ tries: 1, successes: 0, costUsd: 0.01 });
  });

  it("picks the cheapest model that kept succeeding, once there are enough tries", () => {
    expect(learnedModel(stats, "docs", models)?.model).toBe(H);
    expect(learnedModel(stats, "feature", models)).toBeNull();
  });
});

describe("budgetState", () => {
  it("is ok without a limit, tight from 80% and over at 100%", () => {
    expect(budgetState({ monthlyUsd: 0, spentUsd: 999 })).toBe("ok");
    expect(budgetState({ monthlyUsd: 10, spentUsd: 7.9 })).toBe("ok");
    expect(budgetState({ monthlyUsd: 10, spentUsd: 8 })).toBe("tight");
    expect(budgetState({ monthlyUsd: 10, spentUsd: 10 })).toBe("over");
  });
});

describe("chooseModel", () => {
  const stats = learn(runs, jobType);
  const base = { rules: {}, stats, models, defaultModel: S, budget: { monthlyUsd: 0, spentUsd: 0 } };

  it("follows the order: the step, project rules, learned, default", () => {
    expect(chooseModel({ ...base, stepModel: F, type: "docs" })).toEqual({ model: F, reason: "set on the step" });
    expect(chooseModel({ ...base, stepModel: null, type: "docs", rules: { docs: S } }).reason).toBe(
      "project rule for docs and wording",
    );
    expect(chooseModel({ ...base, stepModel: null, type: "docs" })).toEqual({
      model: H,
      reason: "learned: 3/3 done on docs and wording",
    });
    expect(chooseModel({ ...base, stepModel: null, type: "feature" })).toEqual({ model: S, reason: "default model" });
  });

  it("moves one model cheaper when the monthly budget is tight, except for money and security", () => {
    const tight = { monthlyUsd: 10, spentUsd: 9 };
    expect(chooseModel({ ...base, budget: tight, stepModel: null, type: "feature" }).model).toBe(H);
    expect(chooseModel({ ...base, budget: tight, stepModel: null, type: "security" }).model).toBe(S);
    expect(chooseModel({ ...base, budget: tight, stepModel: F, type: "feature" }).model).toBe(F);
  });
});

describe("monthStart", () => {
  it("is the first moment of the month", () => {
    const start = new Date(monthStart(new Date(2026, 10, 10, 15, 30)) * 1000);
    expect([start.getFullYear(), start.getMonth(), start.getDate(), start.getHours()]).toEqual([2026, 10, 1, 0]);
  });
});
