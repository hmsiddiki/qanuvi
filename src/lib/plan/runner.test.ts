import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { parsePlan } from "./tasklist";
import {
  FREE_MODEL,
  costRatio,
  freeFirstFor,
  freeWantedButMissing,
  ladder,
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
  endStatus,
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
    const f = fail("claude-fable-5-2");
    expect(nextModel(models, [f], "claude-fable-5-2", 1)).toBeNull();
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

describe("run settings: one model, Team per run", () => {
  const models = CLAUDE_CODE.models;
  it("compares one model on every step with the plan's own models", () => {
    // Sonnet ($10 out) on every step, against Opus ($20) and Haiku ($0.50): (10 * 2) / 20.5.
    expect(costRatio(models, ["claude-opus-5-5", "claude-haiku-5-5"], "claude-sonnet-5-5")).toBeCloseTo(0.976, 2);
    expect(costRatio(models, ["claude-sonnet-5-5"], "claude-fable-5-2")).toBe(4);
    expect(costRatio(models, ["unknown"], "claude-fable-5-2")).toBeNull();
    expect(costRatio(models, [], "claude-fable-5-2")).toBeNull();
  });

  it("stays on one model when moving up is off", () => {
    const fail = (model: string) => ({ model, outcome: "open" as const });
    expect(nextModel(ladder(models, false), [fail("claude-sonnet-5-5")], "claude-sonnet-5-5", 1)).toBeNull();
    expect(nextModel(ladder(models, true), [fail("claude-sonnet-5-5")], "claude-sonnet-5-5", 1)).toBe(
      "claude-opus-5-5",
    );
  });

  it("lets the run's own Team choice decide whether free models go first", () => {
    expect(freeFirstFor("paid", true, true)).toBe(false);
    expect(freeFirstFor("free", false, true)).toBe(true);
    expect(freeFirstFor("settings", true, true)).toBe(true);
    expect(freeFirstFor("settings", false, true)).toBe(false);
    expect(freeFirstFor("free", true, false)).toBe(false);
  });

  it("notes when the Team choice wanted free models but none are set up", () => {
    expect(freeWantedButMissing("settings", "paid-decides", false, false)).toBe(true);
    expect(freeWantedButMissing("free", "direct", false, false)).toBe(true);
    expect(freeWantedButMissing("settings", "direct", false, false)).toBe(false);
    expect(freeWantedButMissing("paid", "paid-decides", false, false)).toBe(false);
    expect(freeWantedButMissing("settings", "paid-decides", true, false)).toBe(false);
    expect(freeWantedButMissing("settings", "paid-decides", false, true)).toBe(false);
  });
});

describe("how a run ends", () => {
  const step = (status: StepRun["status"]): StepRun => ({ line: 0, number: "1a", title: "t", attempts: [], status });
  it("needs you while steps are held or set aside, done otherwise", () => {
    expect(endStatus({ steps: [step("done"), step("held")] })).toBe("needs-you");
    expect(endStatus({ steps: [step("done"), step("set-aside")] })).toBe("needs-you");
    expect(endStatus({ steps: [step("done"), step("skipped")] })).toBe("done");
  });
});

describe("reviewWithoutDiff (PLAN-F 97a: a review that cannot see the change never rejects it alone)", async () => {
  const { reviewWithoutDiff, replacementsDone } = await import("./runner");
  it("lets the review go ahead when there is a diff, or when nothing changed at all", () => {
    expect(reviewWithoutDiff({ parts: ["--- a\n+x"] }, ["a"], true)).toBeNull();
    expect(reviewWithoutDiff({ parts: [] }, [], null)).toBeNull();
  });
  it("lets the tests decide when files changed but no diff came back", () => {
    const files = ["src/A.php", "src/B.php", "src/C.php", "src/D.php"];
    expect(reviewWithoutDiff({ parts: [] }, files, true)).toEqual({
      ok: true,
      why: "not reviewed: the step changed 4 files (src/A.php, src/B.php, src/C.php and 1 more) but no diff could be shown; the tests passed, so it counts as done",
    });
    expect(reviewWithoutDiff({ parts: [] }, ["x"], false)?.ok).toBe(false);
    expect(reviewWithoutDiff({ parts: [] }, ["x"], null)?.why).toContain("a file (x)");
    expect(reviewWithoutDiff({ parts: [] }, ["x"], null)?.why).toContain("no tests ran");
  });
  it("says why when the changes could not be read", () => {
    const r = reviewWithoutDiff({ parts: [], error: "The snapshot is gone" }, [], undefined);
    expect(r).toEqual({
      ok: true,
      why: "not reviewed: the changes could not be read (The snapshot is gone); no tests ran, so it counts as done",
    });
  });
  it("knows when every replacement of a step is done", () => {
    const s = (status: string, replaces?: string) =>
      ({ line: 0, number: null, title: "t", attempts: [], status, replaces }) as never;
    expect(replacementsDone({ steps: [s("blocked"), s("done", "97a"), s("done", "97a")] }, "97a")).toBe(true);
    expect(replacementsDone({ steps: [s("done", "97a"), s("held", "97a")] }, "97a")).toBe(false);
    expect(replacementsDone({ steps: [s("done")] }, "97a")).toBe(false);
  });
});
