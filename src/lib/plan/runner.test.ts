import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { parsePlan } from "./tasklist";
import {
  FREE_MODEL,
  freeSavings,
  nextModel,
  nextModelFreeFirst,
  paidPerStep,
  runTotals,
  startModel,
  stepOutcome,
  stepPrompt,
  stepsToRun,
  parseRanges,
  unmetNeed,
  runSummary,
  type StepRun,
} from "./runner";

const PLAN = [
  "## Steps",
  "",
  "- [x] **1. Already done**",
  "- [ ] **2. Add eggs** (to the list)",
  "  - Edit notes.txt.",
  "  - Model: claude-haiku-5-5",
  "- [!] **3. Waiting for the owner**",
  "- [-] **4. Sort the list**",
].join("\n");

const models = CLAUDE_CODE.models; // strongest first: fable, opus, sonnet, haiku

describe("stepsToRun", () => {
  const steps = parsePlan(PLAN).steps;
  it("takes open steps from the chosen one, skipping done and blocked steps", () => {
    expect(stepsToRun(steps, 0, false).map((s) => s.number)).toEqual(["2", "4"]);
    expect(stepsToRun(steps, steps[3].line, false).map((s) => s.number)).toEqual(["4"]);
    expect(stepsToRun(steps, steps[1].line, true).map((s) => s.number)).toEqual(["2"]);
  });
});

describe("startModel", () => {
  it("uses the step's own model, else the default", () => {
    const steps = parsePlan(PLAN).steps;
    expect(startModel(steps[1], "claude-sonnet-5-5")).toBe("claude-haiku-5-5");
    expect(startModel(steps[3], "claude-sonnet-5-5")).toBe("claude-sonnet-5-5");
  });
});

describe("nextModel", () => {
  const fail = (model: string) => ({ model, outcome: "open" as const });
  it("stays on the first model until it has failed the set number of times", () => {
    expect(nextModel(models, [], "claude-haiku-5-5", 2)).toBe("claude-haiku-5-5");
    expect(nextModel(models, [fail("claude-haiku-5-5")], "claude-haiku-5-5", 2)).toBe("claude-haiku-5-5");
  });

  it("moves one model up after that, and asks the user after the strongest fails", () => {
    const h = fail("claude-haiku-5-5");
    expect(nextModel(models, [h, h], "claude-haiku-5-5", 2)).toBe("claude-sonnet-5-5");
    const s = fail("claude-sonnet-5-5");
    expect(nextModel(models, [h, h, s, s], "claude-haiku-5-5", 2)).toBe("claude-opus-5-5");
    const f = fail("claude-fable-5-1");
    expect(nextModel(models, [f], "claude-fable-5-1", 1)).toBeNull();
  });

  it("asks the user when a model outside the list fails", () => {
    expect(nextModel(models, [fail("custom-model")], "custom-model", 1)).toBeNull();
  });
});

describe("stepOutcome", () => {
  const step = { number: "2", title: "Add eggs" };
  it("reads the step's status after an attempt, wherever the line moved", () => {
    expect(stepOutcome(PLAN, step)).toBe("open");
    const done = PLAN.replace("- [ ] **2. Add eggs**", "- [x] **2. Add eggs**").replace(
      "## Steps",
      "## Steps\n\nIntro line.",
    );
    expect(stepOutcome(done, step)).toBe("done");
    expect(stepOutcome(PLAN.replace("- [ ] **2.", "- [!] **2."), step)).toBe("blocked");
    expect(stepOutcome("# empty", step)).toBe("missing");
  });
});

describe("stepPrompt", () => {
  it("gives the step, its details, earlier steps and the rules, nothing else", () => {
    const step = parsePlan(PLAN).steps[1];
    const prompt = stepPrompt("TASKLIST.md", step, [{ number: "1", title: "Already done" }]);
    expect(prompt).toContain("**2. Add eggs** (to the list)");
    expect(prompt).toContain("- Edit notes.txt.");
    expect(prompt).not.toContain("Model:");
    expect(prompt).toContain("- 1. Already done");
    expect(prompt).toContain('change its line in TASKLIST.md to "- [x]"');
  });
});

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

  it("tries the free agent, then the paid agent from its usual model up", () => {
    const first = "claude-sonnet-5-5";
    expect(nextModelFreeFirst(models, [], FREE_MODEL, first, 1)).toBe(FREE_MODEL);
    expect(nextModelFreeFirst(models, [a(FREE_MODEL, "open")], FREE_MODEL, first, 2)).toBe(FREE_MODEL);
    expect(nextModelFreeFirst(models, [a(FREE_MODEL, "open")], FREE_MODEL, first, 1)).toBe(first);
    expect(nextModelFreeFirst(models, [a(FREE_MODEL, "error"), a(first, "open")], FREE_MODEL, first, 1)).toBe(
      "claude-opus-5-5",
    );
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

  it("runs only the steps in the ranges", () => {
    const plan = parsePlan(
      [
        "- [ ] **1. A**",
        "- [ ] **2. B**",
        "- [x] **3. C**",
        "- [ ] **4. D**",
        "- [ ] **10b. E**",
        "- [ ] **F**",
        "",
      ].join("\n"),
    );
    const picked = stepsToRun(plan.steps, 0, false, parseRanges("2-4, 10").ranges).map((s) => s.number);
    expect(picked).toEqual(["2", "4", "10b"]);
  });

  it("finds the first need that is not done", () => {
    const done = new Set(["1", "2"]);
    expect(unmetNeed(["1", "2"], done)).toBeNull();
    expect(unmetNeed(["1", "3"], done)).toBe("needs step 3, which is not done");
    expect(unmetNeed(["docs/B.md#4"], done)).toContain("could not be read");
    expect(unmetNeed(["docs/B.md#4"], done, new Map([["docs/B.md", new Set(["4"])]]))).toBeNull();
  });

  it("reports what was done and what was left, with why", () => {
    const s = (number: string, status: StepRun["status"], reason?: string) =>
      ({ line: 0, number, title: number, attempts: [], status, reason }) as StepRun;
    expect(
      runSummary({
        steps: [s("1", "done"), s("2", "blocked", "needs a key"), s("3", "skipped", "needs step 2, which is not done")],
      }),
    ).toBe("1 done · 1 skipped: step 3 (needs step 2, which is not done) · 1 blocked: step 2 (needs a key)");
  });
});
