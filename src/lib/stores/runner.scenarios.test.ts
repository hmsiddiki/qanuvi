/**
 * The plan runner end to end: the real runner store, the real plan logic and changes.ts, against a fake core
 * (testing/fakeCore.ts) with a scripted agent. Each scenario is a run that once went wrong or must keep working;
 * a change to the runner that breaks one fails here before it reaches a real plan. Add a scenario for every runner
 * bug found in a real run.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FakeCore } from "./testing/fakeCore";

const h = vi.hoisted(() => {
  const answered = new Set<(item: unknown) => void>();
  const state = {
    core: null as unknown as import("./testing/fakeCore").FakeCore,
    inboxItems: [] as Record<string, unknown>[],
    answered,
    planner: [] as { title: string; prompt: string }[],
    plannerAnswer: (_title: string, _prompt: string): string => '{"ok": true, "why": "fine"}',
    project: null as unknown as Record<string, unknown> & { settings: Record<string, unknown> },
  };
  return state;
});

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (e: unknown) => void;
  },
  invoke: vi.fn(),
}));
vi.mock("$lib/remote", () => ({ routed: (cmd: string, args: Record<string, unknown>) => h.core.invoke(cmd, args) }));
vi.stubGlobal(
  "localStorage",
  (() => {
    const m = new Map<string, string>();
    return {
      getItem: (k: string) => m.get(k) ?? null,
      setItem: (k: string, v: string) => m.set(k, v),
      removeItem: (k: string) => m.delete(k),
      clear: () => m.clear(),
    };
  })(),
);
vi.mock("./agent.svelte", () => ({
  agent: {
    sessions: [],
    runProviderId: "claude-code",
    notice: () => {},
    onPlanChange: null,
    onPlanInterrupt: null,
    planSessionId: null,
    handler: () => () => {},
    watch: () => {},
    track: () => {},
    rename: () => {},
    model: "",
    mode: "auto",
    privateNote: null,
  },
}));
vi.mock("./settings.svelte", () => ({
  settings: {
    agent: () => ({ model: "", effort: "", baseUrl: "", browserTools: false, lean: true, indexTools: false }),
    value: { team: "", brain: { models: [], providers: [] }, monthlyBudgetUsd: 0, rules: {}, agent: "claude-code" },
  },
}));
vi.mock("./inbox.svelte", () => ({
  inbox: {
    add: (item: Record<string, unknown>) => {
      const i = h.inboxItems.findIndex((x) => item.key && x.key === item.key);
      if (i >= 0) h.inboxItems.splice(i, 1);
      h.inboxItems.push(item);
    },
    onAnswer: (fn: (item: unknown) => void) => {
      h.answered.add(fn);
      return () => h.answered.delete(fn);
    },
    get questions() {
      return [];
    },
    close: () => {},
    answerKey: () => {},
  },
}));
vi.mock("./projects.svelte", () => ({
  projects: {
    get list() {
      return [h.project];
    },
    update: async (_id: string, _name: string, s: Record<string, unknown>) => {
      h.project.settings = s;
      return "";
    },
  },
}));
vi.mock("./planner.svelte", () => ({
  planner: {
    ask: async (_project: unknown, prompt: string, _model: string, title: string) => {
      h.planner.push({ title, prompt });
      return { text: h.plannerAnswer(title, prompt), costUsd: 0.001 };
    },
  },
  defaultPlannerModel: (p: { models: { id: string }[] }) => (p.models[1] ?? p.models[0]).id,
}));
vi.mock("./brain.svelte", () => ({
  freeDecide: async () => {
    throw new Error("no free models in tests");
  },
  setupFor: () => null,
}));
vi.mock("./memory.svelte", () => ({ memoryContext: async () => null }));

const { runner } = await import("./runner.svelte");

const FILE = "content/plugins/shop/PLAN.md";

function project(settings: Record<string, unknown> = {}) {
  return {
    id: "p1",
    name: "shop",
    path: "E:/shop",
    kind: "php",
    tags: ["php"],
    settings: {
      run_command: "",
      test_command: "php tests/run.php",
      preview_url: "",
      changes_from: "",
      ...settings,
    } as import("$lib/api").ProjectSettings,
    added_at: 0,
    last_opened_at: 0,
  };
}

function options(extra: Record<string, unknown> = {}) {
  return {
    fromLine: 0,
    onlyOne: false,
    ranges: "",
    pause: "never",
    mode: "auto",
    budgetUsd: 0,
    attemptsPerModel: 1,
    lean: true,
    indexTools: false,
    packets: false,
    replan: true,
    providerId: "claude-code",
    model: "claude-opus-5-5",
    strongerOnFail: false,
    team: "settings",
    carryOn: false,
    fullChecks: "section",
    waitBefore: [],
    extraChecks: [],
    onLimit: "wait",
    stepCostLimit: 0,
    stepMinutes: 0,
    finishPass: "off",
    ...extra,
  } as never;
}

/** The step number an agent's prompt is about ("Step 97a-1: …" or the bold title in it). */
function stepOf(prompt: string, numbers: string[]): string {
  const hit = numbers
    .filter((n) => prompt.includes(`**${n}. `) || prompt.includes(`step ${n}`) || prompt.includes(`Step ${n}`))
    .sort((a, b) => b.length - a.length)[0];
  if (!hit) throw new Error(`no known step in prompt: ${prompt.slice(0, 300)}`);
  return hit;
}

async function run(p = project(), opts = options()) {
  h.project = p as never;
  await runner.start(p as never, FILE, opts);
  return runner.run!;
}

beforeEach(() => {
  // Each scenario starts clean: kept plan state, notes and run settings live in localStorage today.
  localStorage.clear();
  h.core = new FakeCore();
  h.inboxItems = [];
  h.planner = [];
  h.plannerAnswer = () => '{"ok": true, "why": "fine"}';
});

describe("plan runner scenarios", () => {
  it("runs a plain plan to the end", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **1. First**\n  - do it\n- [ ] **2. Second**\n  - then this\n");
    h.core.agent = ({ prompt, core }) => {
      const n = stepOf(prompt, ["1", "2"]);
      core.write(`src/Step${n}.php`, `<?php // ${n}\n`);
      core.mark(FILE, n);
    };
    const r = await run();
    expect(r.status).toBe("done");
    expect(r.steps.map((s) => [s.number, s.status])).toEqual([
      ["1", "done"],
      ["2", "done"],
    ]);
    expect(h.core.read(FILE)).toContain("- [x] **1. First**");
    expect(h.core.read(FILE)).toContain("- [x] **2. Second**");
    expect(h.inboxItems.filter((i) => i.type === "answer")).toEqual([]);
  });

  it("PLAN-F 97a: a review that sees no diff of real work does not turn the step down", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **97a. Storage design**\n  - schema 26\n");
    h.core.blindDiff = true; // what git did in the folder it ignores
    h.core.agent = ({ core }) => {
      core.write("content/plugins/shop/src/Schema.php", "<?php // v26\n");
      core.mark(FILE, "97a");
    };
    const r = await run(project(), options({ extraChecks: [{ step: "97a", why: "schema migration" }] }));
    expect(r.status).toBe("done");
    expect(r.steps[0].status).toBe("done");
    expect(h.planner.filter((c) => c.title.startsWith("Decider: review"))).toEqual([]);
    const review = r.decisions?.find((d) => d.kind === "review");
    expect(review?.decision).toBe("accepted");
    expect(review?.why).toContain("no diff could be shown");
    expect(h.core.read(FILE)).toContain("- [x] **97a.");
  });

  it("a review sees a step's change in an ignored plugin folder and judges it", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **3. Contact column**\n");
    h.core.agent = ({ core }) => {
      core.write("content/plugins/shop/src/Cart.php", "<?php $contact_hash = 1;\n");
      core.mark(FILE, "3");
    };
    const r = await run(project(), options({ extraChecks: [{ step: "3", why: "schema" }] }));
    expect(r.status).toBe("done");
    const asked = h.planner.find((c) => c.title.startsWith("Decider: review"));
    expect(asked?.prompt).toContain("+<?php $contact_hash = 1;");
    expect(asked?.prompt).not.toContain("PLAN.md (before"); // the plan file itself is left out
  });

  it("PLAN-F 97a: replanned steps get free numbers, the replaced step is not held, and it is ticked after them", async () => {
    h.core.write(
      FILE,
      [
        "# Plan",
        "",
        "- [ ] **97a. Storage design**",
        "  - all of it",
        "- [ ] **97b. Phone mode**",
        "  - Needs: 97a",
        "- [ ] **97c. Texts**",
        "",
      ].join("\n"),
    );
    // 97a never finishes on the one model; the planner splits it, writing its own labels before the titles.
    h.plannerAnswer = (title) =>
      title.startsWith("Planner: rewrite")
        ? "```tasklist\n- [ ] **97a1. Make it visible**\n  - first\n- [ ] **97a2. Schema 26**\n  - second\n```"
        : '{"ok": true, "why": "fine"}';
    h.core.agent = ({ prompt, core }) => {
      const n = stepOf(prompt, ["97a", "97a-1", "97a-2", "97b", "97c"]);
      if (n === "97a") return; // nothing done, step stays open
      core.write(`src/${n}.php`, `<?php // ${n}\n`);
      core.mark(FILE, n);
    };
    const r = await run();
    const plan = h.core.read(FILE);
    expect(plan).toContain("- [x] **97a-1. Make it visible**");
    expect(plan).toContain("- [x] **97a-2. Schema 26**");
    expect(plan).toContain("- [x] **97a. Storage design**");
    expect(plan).toContain("- [x] **97b. Phone mode**");
    expect(plan).toContain("- [x] **97c. Texts**");
    expect(plan).not.toMatch(/\*\*97b\. 97a/);
    expect(r.status).toBe("done");
    expect(r.steps.find((s) => s.number === "97a")?.reason).toContain("done through its replacement steps");
    // Nobody was asked: the replaced step was never held and nothing waited on it.
    expect(h.inboxItems.filter((i) => i.type === "answer")).toEqual([]);
    expect(r.decisions?.filter((d) => d.kind === "set-aside") ?? []).toEqual([]);
  });

  it("git chosen but the plan's folder is out of git's sight: one question, and the run goes on", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    h.core.gitBlind = ["content/plugins/shop"];
    h.core.agent = ({ prompt, core }) => {
      const n = stepOf(prompt, ["1", "2"]);
      core.write(`content/plugins/shop/src/S${n}.php`, `<?php // ${n}\n`);
      core.mark(FILE, n);
    };
    const p = project({ changes_from: "git" });
    // No reviews in this run: the question comes from the check before the first step.
    const r = await run(p, options());
    expect(r.status).toBe("done");
    const asks = h.inboxItems.filter((i) => (i.source as { kind?: string } | undefined)?.kind === "changes-from");
    expect(asks).toHaveLength(1);
    expect(String(asks[0].text)).toContain("content/plugins/shop");
    // The owner picks Qanuvi's snapshots: the project setting changes.
    for (const fn of h.answered) fn({ ...asks[0], answer: { choice: "qanuvi" } });
    expect(p.settings.changes_from).toBe("qanuvi");
  });

  it("a blocked step goes to the Inbox, the run waits, and an answer lets it go on", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **1. Needs a key**\n- [ ] **2. Other**\n");
    let tries = 0;
    h.core.agent = ({ prompt, core }) => {
      const n = stepOf(prompt, ["1", "2"]);
      if (n === "1" && tries++ === 0) {
        core.mark(FILE, "1", "!");
        core.write(
          FILE,
          `${core.read(FILE)}`.replace("**1. Needs a key**", "**1. Needs a key**\n  - Blocked: the API key is missing"),
        );
        return;
      }
      core.mark(FILE, n);
    };
    h.project = project() as never;
    const running = runner.start(h.project as never, FILE, options({ replan: false }));
    await vi.waitFor(() => expect(runner.waitingAnswers).toBeGreaterThan(0), { timeout: 3000 });
    const q = h.inboxItems.find((i) => i.type === "answer");
    expect(String(q?.title)).toContain("step 1. Needs a key is blocked");
    expect(runner.run?.steps.find((s) => s.number === "2")?.status).toBe("done");
    runner.answer(
      runner.run!.steps.find((s) => s.number === "1")!,
      "go",
      "the key is in the secrets now",
    );
    await running;
    expect(runner.run?.status).toBe("done");
    expect(h.core.read(FILE)).toContain("- [x] **1. Needs a key**");
    expect(h.core.prompts.at(-1)?.prompt).toContain("the key is in the secrets now");
  });

  it("failing tests go back to the same session with only the failures, then pass", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **5. Coupon rule**\n");
    let fixed = false;
    h.core.checks = () =>
      fixed
        ? { ok: true, output: "OK (12 tests)" }
        : { ok: false, output: "FAIL CouponTest::testRounding\nexpected 10, got 9" };
    h.core.agent = ({ turn, core }) => {
      if (turn === 1) {
        core.write("src/Coupon.php", "<?php // round down\n");
        core.mark(FILE, "5");
      } else {
        core.write("src/Coupon.php", "<?php // round half up\n");
        fixed = true;
      }
    };
    const r = await run(project(), options({ extraChecks: [{ step: "5", why: "money" }] }));
    expect(r.status).toBe("done");
    const fix = h.core.prompts[1]?.prompt ?? "";
    expect(fix).toContain("CouponTest::testRounding");
    expect(h.core.prompts).toHaveLength(2); // one session, one fix turn
    expect(r.steps[0].attempts.at(-1)?.testsPassed).toBe(true);
  });

  it("pause after each step stops after a whole step, and the next run goes on from the next one", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **1. One**\n- [ ] **2. Two**\n");
    h.core.agent = ({ prompt, core }) => core.mark(FILE, stepOf(prompt, ["1", "2"]));
    const first = await run(project(), options({ pause: "step" }));
    expect(first.status).not.toBe("running");
    expect(first.steps.filter((s) => s.status === "done").map((s) => s.number)).toEqual(["1"]);
    expect(h.core.read(FILE)).toContain("- [ ] **2. Two**");
    const second = await run(project(), options());
    expect(second.status).toBe("done");
    expect(h.core.read(FILE)).toContain("- [x] **2. Two**");
  });

  it("stopping during a step ends the run as stopped, and the step stays open", async () => {
    h.core.write(FILE, "# Plan\n\n- [ ] **1. Long**\n- [ ] **2. Later**\n");
    // The owner presses Stop twice during the first turn (the second press stops at once). Only once: the gentle
    // stop sends one more message, and that late turn must not press Stop in the next scenario's run.
    h.core.agent = ({ turn }) => {
      if (turn !== 1) return;
      runner.stop();
      runner.stop();
    };
    const r = await run();
    expect(r.status).toBe("stopped");
    expect(h.core.read(FILE)).toContain("- [ ] **1. Long**");
    expect(h.core.read(FILE)).toContain("- [ ] **2. Later**");
    expect(h.inboxItems.filter((i) => i.type === "answer")).toEqual([]);
  });

  it("a step whose Needs line names an unfinished step in another plan is skipped with the reason", async () => {
    // A Needs line names another plan by its path in the project.
    h.core.write("OTHER.md", "# Other\n\n- [ ] **4. Base**\n");
    h.core.write(FILE, "# Plan\n\n- [ ] **1. Uses the base**\n  - Needs: OTHER.md#4\n- [ ] **2. Free of it**\n");
    h.core.agent = ({ prompt, core }) => core.mark(FILE, stepOf(prompt, ["1", "2"]));
    const r = await run();
    const one = r.steps.find((s) => s.number === "1");
    expect(one?.status).toBe("skipped");
    expect(one?.reason).toBe("needs step 4 of OTHER.md, which is not done");
    // Skipped once, listed once (it came back as "sent back by the owner" and was listed twice before 2026-11-14).
    expect(r.steps.filter((s) => s.number === "1")).toHaveLength(1);
    expect(r.message).toBe("1 done · 1 skipped: step 1 (needs step 4 of OTHER.md, which is not done)");
    expect(r.steps.find((s) => s.number === "2")?.status).toBe("done");
  });
});
