/**
 * The comparison check for 24z step A (R1): the same inputs through the TypeScript plan logic and its Rust port.
 * This test keeps src-tauri/src/plan/compare.json (inputs and TypeScript outputs) up to date with the TypeScript; the
 * Rust test plan::compare runs the same inputs and must give the same outputs. After changing a case or the
 * TypeScript, write the file again with: npx vitest run -u src/lib/plan/compare.test.ts
 */
import { describe, expect, it } from "vitest";
import * as decider from "$lib/core/decider";
import * as limits from "./limits";
import * as holdask from "./holdask";
import * as setaside from "./setaside";
import * as costs from "./costs";
import * as picks from "./picks";
import { taskListPrompt } from "./prompts";
import * as tasklist from "./tasklist";
import * as routing from "./routing";
import * as runner from "./runner";
import * as finishing from "./finishing";

const FILE = "../../../src-tauri/src/plan/compare.json";

const steps: decider.StepBrief[] = [
  { number: "1", title: "Write a README", details: [], type: "docs" },
  {
    number: "2",
    title: "Add a checkout page",
    details: ["Stripe", "Webhooks", "Refunds", "Tests", "Fifth"],
    type: "security",
  },
  { number: "3", title: "Add a filter to the list", details: ["Ünïcode détail"], type: "feature" },
  { number: "4", title: "Tidy", details: [], type: "refactor" },
];
const profile = (model: string, tries: number, successes: number, tokens: number, cost: number, heavy: boolean) => ({
  model,
  tries,
  successes,
  tokensPerSuccess: tokens,
  costPerSuccess: cost,
  heavy,
});
/** An edit's text, or the error it gives (Rust returns the same message as an error). */
const tried = (f: () => string) => {
  try {
    return f();
  } catch (e) {
    return { error: (e as Error).message };
  }
};
const PLAN = [
  "# Work map",
  "",
  "## Stage 1 ##",
  "- [x] **1. Project setup** (a note that",
  "  goes on)",
  "  - Done on Windows.",
  "    More detail.",
  "",
  "- [-] **2. Layout** (built)",
  "  - Three panes.",
  "  - Model: claude-opus-5-5",
  "  - model: second-ignored",
  "  - Needs: 1, docs/OTHER.md#4 x",
  "  - Skipped: needs step 1",
  "- [ ] **1b. Updates**",
  "1. not a step",
  "## Stage 2",
  "- [X] Plain step without bold",
  " - [!] 12-2. Plain numbered",
  "  * star detail",
  "Some paragraph.",
  "",
].join("\r\n");
/** Maps and sets as JSON objects and arrays (JSON.stringify drops a Map's entries). */
const statsOf = (o: Record<string, routing.ModelStats>) => new Map(Object.entries(o));
const att = (model: string, outcome: string, costUsd: number, seconds = 2, tokens = 100) => ({
  model,
  sessionId: "s",
  outcome,
  costUsd,
  inputTokens: tokens,
  outputTokens: tokens / 10,
  seconds,
  note: "",
});
const sr = (number: string | null, status: string, attempts: unknown[] = [], more: Record<string, unknown> = {}) => ({
  line: 1,
  number,
  title: `Step ${number ?? "x"}`,
  attempts,
  status,
  ...more,
});
const H5 = "claude-haiku-5-5";
const S5 = "claude-sonnet-5-5";
const O5 = "claude-opus-5-5";
const F5 = "claude-fable-5-2";
const priced = [
  { id: F5, label: "Fable 5.2", price: [8, 40] },
  { id: O5, label: "Opus 5.5", price: [4, 20] },
  { id: S5, label: "Sonnet 5.5", price: [2, 10] },
  { id: H5, label: "Haiku 5.5", price: [0.1, 0.5] },
];
const STATS = {
  [`docs|${H5}`]: { tries: 5, successes: 5, costUsd: 0.25, tokens: 50_000 },
  [`docs|${S5}`]: { tries: 34, successes: 33, costUsd: 4.125, tokens: 7_000_001 },
  [`docs|${O5}`]: { tries: 2, successes: 2, costUsd: 1, tokens: 900_000 },
  [`security|${O5}`]: { tries: 4, successes: 3, costUsd: 2, tokens: 300_000 },
  [`security|${F5}`]: { tries: 4, successes: 4, costUsd: 4, tokens: 250_000 },
  [`feature|${H5}`]: { tries: 3, successes: 1, costUsd: 0.5, tokens: 0 },
  [`feature|qanuvi:auto`]: { tries: 3, successes: 3, costUsd: 0, tokens: 1 },
};
const RUNS = [
  {
    steps: [
      sr("1", "done", [att(H5, "done", 0.25), att(H5, "stopped", 1)], { title: "Update README" }),
      sr("2", "done", [att(H5, "open", 0.5), att(S5, "done", 0.75)], { jobType: "security", title: "x" }),
      sr("3", "failed", [att(F5, "error", 2, 1, 333)], { title: "Fix the crash" }),
    ],
  },
  { steps: [sr("4", "done", [att("qanuvi:auto", "done", 0)], { title: "Add a page" })] },
];
const RUN_STEPS = [
  sr("1", "done", [att(H5, "done", 0.25, 30), att(S5, "done", 0.5, 90)], { jobType: "docs" }),
  sr("2", "done", [att("qanuvi:auto", "open", 0, 10), att("qanuvi:auto", "done", 0, 20)], { jobType: "docs" }),
  sr("3", "done", [att("qanuvi:auto", "done", 0, 40)], { jobType: "ui" }),
  sr("4", "skipped", [], { reason: "needs step 2, which is not done" }),
  sr(null, "blocked", [], { reason: "" }),
  sr("6", "held", [], { held: { kind: "hold", why: "it pushes to the live server" } }),
  sr("7", "set-aside", [], { waitsFor: { step: "6", why: "uses its file" } }),
  sr("8", "set-aside", []),
  sr("9", "failed", [att(O5, "open", 1.5, 60)], { reason: "tests kept failing" }),
];
const FINISH = {
  level: "full",
  status: "done",
  model: O5,
  startedAt: Date.UTC(2026, 10, 14, 19, 5, 30),
  endedAt: null,
  costUsd: 1.125,
  seconds: 90.5,
  checks: { command: "npm test", ok: false, summary: "2 failed" },
  findings: ["a.ts: debug log left", "b.ts: name differs"],
  stillOpen: ["set the SMTP login"],
  note: "ran twice",
};
const BRIEFS = [
  { number: "3", title: "Add the cart", done: ["- Done: " + "é".repeat(450), "two", "three", "four", "five"] },
  { number: null, title: "No number", done: [] },
];
const useLine = (name: string, input: Record<string, unknown> = {}) =>
  JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", id: "x", name, input }] } });

/** Function name → what it runs; the Rust side has the same names in plan/compare.rs. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const fns: Record<string, (...a: any[]) => unknown> = {
  parseTeam: decider.parseTeam,
  deciderSeat: decider.deciderSeat,
  freeWorks: (team, f, p) => decider.freeWorks(decider.parseTeam(team), f, p),
  assignPrompt: decider.assignPrompt,
  parseAssignment: decider.parseAssignment,
  pickPrompt: decider.pickPrompt,
  parsePicks: decider.parsePicks,
  reviewPrompt: decider.reviewPrompt,
  parseReview: decider.parseReview,
  chatModelPrompt: decider.chatModelPrompt,
  parseChatModel: decider.parseChatModel,
  stepDiff: decider.stepDiff,
  overruleNote: decider.overruleNote,
  withoutOverruleNote: decider.withoutOverruleNote,
  overruleIn: decider.overruleIn,
  strongerModel: decider.strongerModel,
  isLimit: limits.isLimit,
  limitResetAt: (text: string, now: number) => limits.limitResetAt(text, new Date(now))?.getTime() ?? null,
  holdKey: holdask.holdKey,
  asksSecret: holdask.asksSecret,
  holdForm: holdask.holdForm,
  holdReply: holdask.holdReply,
  isClosingStep: setaside.isClosingStep,
  mentionedFiles: setaside.mentionedFiles,
  planDependence: setaside.planDependence,
  dependencePrompt: setaside.dependencePrompt,
  parseDependence: (t, o, h) => [...setaside.parseDependence(t, o, h)],
  waitingLine: setaside.waitingLine,
  closingRerunNote: setaside.closingRerunNote,
  setAsideBlock: setaside.setAsideBlock,
  toolKinds: costs.toolKinds,
  finishedCost: costs.finishedCost,
  planHistory: costs.planHistory,
  overLimit: costs.overLimit,
  pickFor: picks.pickFor,
  taskListPrompt: (file: string) => taskListPrompt(file),
  parsePlan: tasklist.parsePlan,
  setStepStatus: (t, l, st, e) => tried(() => tasklist.setStepStatus(t, l, st, e ?? undefined)),
  setStepTitle: (t, l, ti, e) => tried(() => tasklist.setStepTitle(t, l, ti, e ?? undefined)),
  setStepModel: (t, l, m, e) => tried(() => tasklist.setStepModel(t, l, m, e ?? undefined)),
  setStepSkipped: (t, l, r, e) => tried(() => tasklist.setStepSkipped(t, l, r, e ?? undefined)),
  nextStatus: tasklist.nextStatus,
  findTaskLists: tasklist.findTaskLists,
  clearStaleSkips: tasklist.clearStaleSkips,
  jobType: routing.jobType,
  learn: (runs) => Object.fromEntries(routing.learn(runs, routing.jobType)),
  reliability: routing.reliability,
  learnedModel: (stats, type, models) => routing.learnedModel(statsOf(stats), type, models),
  modelProfiles: (stats, type, models) => routing.modelProfiles(statsOf(stats), type, models),
  budgetState: routing.budgetState,
  chooseModel: (o) => routing.chooseModel({ ...o, stats: statsOf(o.stats) }),
  freeFirstFor: runner.freeFirstFor,
  freeWantedButMissing: runner.freeWantedButMissing,
  costRatio: runner.costRatio,
  stepsToRun: (text, from, only, ranges) =>
    runner.stepsToRun(tasklist.parsePlan(text).steps, from, only, runner.parseRanges(ranges).ranges).map((s) => s.line),
  parseRanges: runner.parseRanges,
  inRanges: (n, text) => runner.inRanges(n, runner.parseRanges(text).ranges),
  unmetNeed: (needs, done, others) =>
    runner.unmetNeed(
      needs,
      new Set(done),
      new Map(Object.entries(others as Record<string, string[]>).map(([k, v]) => [k, new Set(v)])),
    ),
  endStatus: (steps) => runner.endStatus({ steps }),
  runSummary: (steps) => runner.runSummary({ steps }),
  startModel: (model, d) => runner.startModel({ model }, d),
  nextModel: runner.nextModel,
  stepOutcome: runner.stepOutcome,
  stepPrompt: (file, text, i, done) => runner.stepPrompt(file, tasklist.parsePlan(text).steps[i], done),
  runTotals: (steps, deciderCalls, finish) => {
    const t = runner.runTotals({ steps, deciderCalls, finish: finish ?? undefined });
    return { ...t, byModel: Object.fromEntries(t.byModel) };
  },
  nextModelFreeFirst: runner.nextModelFreeFirst,
  freeSavings: (steps, per) => runner.freeSavings({ steps }, per),
  paidPerStep: (stats) => runner.paidPerStep(statsOf(stats)),
  finishEstimate: (steps, level) => finishing.finishEstimate({ steps }, level),
  offersFinish: (status, steps, finish) => finishing.offersFinish({ status, steps, finish: finish ?? undefined }),
  finishReviewPrompt: finishing.finishReviewPrompt,
  parseFinishReview: finishing.parseFinishReview,
  finishFullPrompt: finishing.finishFullPrompt,
  stillOpenIn: finishing.stillOpenIn,
  doneNotes: finishing.doneNotes,
  isPlanFile: finishing.isPlanFile,
  finishEntry: finishing.finishEntry,
};

const models = [
  { id: "claude-opus-5-5", label: "Opus 5.5" },
  { id: "claude-haiku-5-5", label: "Haiku 5.5" },
];
const ids = models.map((m) => m.id);
const assignAnswer =
  'Sure:\n```json\n{"steps":[{"number":1,"worker":"free","why":"plain docs"},{"number":"2","worker":"free"},{"number":"4","worker":"paid","why":"' +
  "y".repeat(250) +
  '"}]}\n```';
const held = { number: "95b", title: "Refunds", details: ["edit includes/refund.php"] };
const saved = {
  at: 1,
  providerId: "claude-code",
  freeFirst: false,
  picks: {
    "95a": { title: "Erasure", worker: "paid", model: "claude-opus-5-5", why: "money" },
    "6": { title: "Write docs", worker: "free", why: "x" },
    "7": { title: "Write docs", worker: "free", why: "y" },
    "8": { title: "Add tests", worker: "paid", why: "z" },
  },
};

const cases: [string, unknown[]][] = [
  ["parseTeam", [{ mode: "direct", priority: "solve" }]],
  ["parseTeam", [{ mode: "nonsense", priority: 3 }]],
  ["parseTeam", [null]],
  ["deciderSeat", [{ agent: "claude-code", model: "opus" }, [{ id: "or/a", local: false }]]],
  [
    "deciderSeat",
    [
      null,
      [
        { id: "ollama/q", local: true },
        { id: "omniroute/s", local: false },
      ],
    ],
  ],
  ["deciderSeat", [null, []]],
  ["freeWorks", [{ mode: "priority" }, true, true]],
  ["freeWorks", [{ mode: "free-decides" }, true, true]],
  ["freeWorks", [{ mode: "direct" }, true, false]],
  ["freeWorks", [{}, false, false]],
  ["assignPrompt", [steps, "save"]],
  ["assignPrompt", [steps, "solve"]],
  ["parseAssignment", [assignAnswer, steps, "balanced"]],
  ["parseAssignment", ["no idea", steps, "save"]],
  ["parseAssignment", ["{broken json}", steps, "solve"]],
  [
    "pickPrompt",
    [
      steps,
      {
        priority: "balanced",
        freeFirst: true,
        models,
        profiles: {
          docs: [],
          security: [
            profile("claude-fable-5-2", 5, 2, 336_500, 4.5, true),
            profile("claude-opus-5-5", 34, 33, 219_757.5, 0.125, false),
          ],
        },
      },
    ],
  ],
  ["pickPrompt", [steps, { priority: "save", freeFirst: false, models, profiles: {} }]],
  [
    "parsePicks",
    [
      '{"steps":[{"number":"1","worker":"free","model":"claude-haiku-5-5"},{"number":"3","model":"gpt-9","worker":"free"}]}',
      steps,
      { priority: "save", freeFirst: true, modelIds: ids },
    ],
  ],
  [
    "parsePicks",
    [
      '{"steps":[{"number":"1","worker":"free","model":"claude-haiku-5-5"}]}',
      steps,
      { priority: "save", freeFirst: false, modelIds: ids },
    ],
  ],
  ["reviewPrompt", [{ title: "Sort", details: ["a", "b", "c", "d", "e", "f", "g"] }, "x".repeat(12_005), ""]],
  ["reviewPrompt", [{ title: "Sort", details: [] }, "", "3 passed"]],
  ["parseReview", ['{"ok": true, "why": "fine"}']],
  ["parseReview", ['```json\n{"ok": "yes"}\n```']],
  ["parseReview", ['{"ok": false}']],
  ["chatModelPrompt", ["rename a variable", models]],
  ["parseChatModel", ['{"model":" claude-haiku-5-5 ","why":"small"}', models]],
  ["parseChatModel", ['{"model":"gpt-9"}', models]],
  [
    "stepDiff",
    [
      "notes.txt",
      "Shopping list\n- milk\n- apples\n- bread\n- butter\n",
      "Shopping list\n- apples\n- bread\n- butter\n- milk\n",
    ],
  ],
  ["stepDiff", ["a", "x\ny", "x\ny"]],
  ["stepDiff", ["a", "", "new\nfile"]],
  ["overruleNote", ["Haiku 5.5", "a short question"]],
  ["overruleNote", ["Haiku 5.5", ""]],
  ["withoutOverruleNote", ["Is it sorted?" + decider.overruleNote("Haiku 5.5", "short")]],
  ["overruleIn", ["NEEDS_STRONGER_MODEL: the refactor\nspans many files"]],
  ["overruleIn", ["  NEEDS_STRONGER_MODEL"]],
  ["overruleIn", ["The file holds prices. NEEDS_STRONGER_MODEL is not needed."]],
  ["strongerModel", [[{ id: "fable" }, { id: "opus" }], "opus"]],
  ["strongerModel", [[{ id: "fable" }, { id: "opus" }], "unknown"]],
  ["strongerModel", [[], "x"]],
  ["isLimit", ["You've hit your session limit · resets 5:20am (Asia/Dhaka)"]],
  ["isLimit", ["Rate limit was reached"]],
  ["isLimit", ["Tests failed."]],
  ["limitResetAt", ["You've hit your session limit · resets 5:20am (Asia/Dhaka)", Date.UTC(2026, 10, 11, 21)]],
  ["limitResetAt", ["hit your weekly limit · resets 11pm (UTC)", Date.UTC(2026, 10, 12, 22, 30)]],
  ["limitResetAt", ["hit your limit · resets at 12am (America/New_York)", Date.UTC(2026, 10, 12, 4, 59)]],
  ["limitResetAt", ["You've hit your usage limit", Date.UTC(2026, 10, 11, 21)]],
  ["limitResetAt", ["no limit here", Date.UTC(2026, 10, 11, 21)]],
  ["holdKey", ["p1", "docs/PLAN.md", "1c"]],
  ["asksSecret", ["needs an API key for the mail service"]],
  ["asksSecret", ["it changes the date format"]],
  ["holdForm", ["hold", "it pushes to the live server"]],
  ["holdForm", ["failed", "the tests kept failing"]],
  ["holdForm", ["hold", "it needs the database login"]],
  ["holdReply", [{ choice: "go", text: " yes " }]],
  ["holdReply", [{ choice: "skip" }]],
  ["holdReply", [{ choice: "decide", text: "keep it small" }]],
  ["isClosingStep", ["95Z"]],
  ["isClosingStep", ["95b"]],
  ["isClosingStep", [null]],
  ["mentionedFiles", ["Change includes\\Checkout.php and src/a.ts, src/a.ts; see MAP.md and app.tsx, x.yml"]],
  ["planDependence", [held, { number: "72a", title: "x", needs: [" 95b "] }]],
  ["planDependence", [held, { number: "95c", title: "x" }]],
  ["planDependence", [held, { number: "72a", title: "Log refund.php errors" }]],
  ["planDependence", [held, { number: "72a", title: "Change lib/includes/refund.php" }]],
  [
    "planDependence",
    [
      { ...held, details: [], changed: ["src\\X.ts"] },
      { number: "1", title: "fix x.ts" },
    ],
  ],
  [
    "planDependence",
    [
      { number: "95a", title: "a" },
      { number: "96a", title: "b" },
    ],
  ],
  [
    "dependencePrompt",
    [
      [{ number: "95b", title: "Refunds", why: "a question" }],
      [
        { number: "72a", title: "Mail", details: ["one", "two", "three", "four"] },
        { number: null, title: "No number" },
      ],
    ],
  ],
  [
    "parseDependence",
    [
      'ok {"needs":[{"step":"72a","on":"95b","why":"mails the refund"},{"step":"99","on":"95b"},{"step":"72a","on":"1"}]}',
      ["72a"],
      ["95b"],
    ],
  ],
  ["parseDependence", ["no", ["72a"], ["95b"]]],
  ["waitingLine", [1]],
  ["waitingLine", [3]],
  ["closingRerunNote", [["1a"]]],
  ["closingRerunNote", [["1a", "1b"]]],
  ["setAsideBlock", [[]]],
  [
    "setAsideBlock",
    [
      [
        { label: "95b", why: "waits for you" },
        { label: "96", why: "failed" },
      ],
    ],
  ],
  [
    "toolKinds",
    [
      [
        useLine("Read"),
        useLine("mcp__qanuvi__find_symbol"),
        useLine("Bash", { description: "Read CheckoutPage first half" }),
        useLine("Bash", { command: "cd src && grep -n x a.ts" }),
        useLine("PowerShell", { description: null, command: "npm test" }),
        useLine("Edit"),
        useLine("TodoWrite"),
        "not json",
      ],
    ],
  ],
  [
    "finishedCost",
    [{ steps: [{ status: "done", attempts: [{ costUsd: 2 }, { costUsd: 1.25 }] }, { status: "blocked" }] }],
  ],
  [
    "planHistory",
    [
      [
        { id: "a", file: "P.md", data: JSON.stringify({ steps: [{ status: "done", attempts: [{ costUsd: 5 }] }] }) },
        { id: "b", file: "P.md", data: "not json" },
        { id: "now", file: "P.md", data: JSON.stringify({ steps: [] }) },
      ],
      "P.md",
      "now",
    ],
  ],
  ["planHistory", [[], "P.md", "now"]],
  ["overLimit", [3.125, 10, { usd: 3 }]],
  ["overLimit", [1, 45.5, { usd: 3, minutes: 30 }]],
  ["overLimit", [1, 10, { usd: 0, minutes: 30 }]],
  ["pickFor", [saved, { number: "95a", title: "Erasure" }, "claude-code"]],
  ["pickFor", [saved, { number: "95a", title: "Erasure" }, "codex"]],
  ["pickFor", [saved, { number: "9", title: "Write docs" }, "claude-code"]],
  ["pickFor", [saved, { number: "9", title: "Add tests" }, "claude-code"]],
  ["pickFor", [null, { number: "1", title: "x" }, "claude-code"]],
  ["taskListPrompt", ["PLAN.md"]],
  ["parsePlan", [PLAN]],
  ["parsePlan", [""]],
  ["parsePlan", ["\uFEFF- [ ] **1. Bom**\n- [ ] ** 2.  Spaced  title **   after  "]],
  ["setStepStatus", [PLAN, 8, "x", null]],
  ["setStepStatus", [PLAN, 18, " ", "Plain numbered"]],
  ["setStepStatus", [PLAN, 9, "x", null]],
  ["setStepStatus", [PLAN, 3, "x", "Wrong"]],
  ["setStepTitle", [PLAN, 3, "  New\t**bold**  title ", null]],
  ["setStepTitle", [PLAN, 14, "Renamed", "Updates"]],
  ["setStepTitle", [PLAN, 18, "Renamed plain", null]],
  ["setStepTitle", [PLAN, 17, "Renamed", null]],
  ["setStepTitle", [PLAN, 3, " ** ", null]],
  ["setStepModel", [PLAN, 3, "claude-haiku-5-5", null]],
  ["setStepModel", [PLAN, 8, " claude-fable-5-2 ", null]],
  ["setStepModel", [PLAN, 8, null, null]],
  ["setStepModel", [PLAN, 14, "", null]],
  ["setStepModel", [PLAN, 14, "two words", null]],
  ["setStepSkipped", [PLAN, 8, "  waits\nfor   you ", "Layout"]],
  ["setStepSkipped", [PLAN, 8, null, "Layout"]],
  ["setStepSkipped", [PLAN, 14, "blocked", null]],
  ["setStepSkipped", [PLAN, 3, " ", null]],
  ["nextStatus", ["!"]],
  ["nextStatus", ["-"]],
  [
    "findTaskLists",
    [
      [
        "docs/ROADMAP.md",
        "README.md",
        "DEVELOPMENT-TASKLIST.md",
        "node_modules/x/TODO.md",
        "notes/todo-list.md",
        "QLOPY-CORE-UPGRADE-PLAN.md",
        "content/plugins/shop/PLAN-D.md",
        "b/plan.md",
        "B/Plan.md",
        "a_plan.md",
        "a-plan.md",
        "a1plan.md",
        "PLAN-E.md",
        "plan-e.md",
        "old/PLAN.archive.md",
        "x/.history/plan.md",
        "Backup/plan.md",
      ],
    ],
  ],
  [
    "clearStaleSkips",
    [
      [
        "- [x] **1. One**",
        "- [ ] **2. Two**",
        "  - Needs: 1",
        "  - Skipped: needs step 1",
        "- [ ] **3. Three**",
        "  - Needs: 2, 9",
        "  - Skipped: needs step 2",
        "- [ ] **4. Four**",
        "  - Needs: 1, P.md#2",
        "  - Skipped: other plan",
        "",
      ].join("\n"),
      ["9"],
    ],
  ],
  ["jobType", ["Fix the refund docs and add tests"]],
  ["jobType", ["Restyle the buttons"]],
  ["jobType", ["Update CHANGELOG.md"]],
  ["jobType", ["Add authentication_flow"]],
  ["jobType", ["prefix_tests"]],
  ["jobType", ["Rename CartService and move it"]],
  ["jobType", ["Fixes for two crashes"]],
  ["jobType", ["Add a wishlist page"]],
  ["jobType", ["Ünïcode pay"]],
  ["learn", [RUNS]],
  ["reliability", [{ tries: 34, successes: 33 }]],
  ["reliability", [{ tries: 5, successes: 5 }]],
  ["reliability", [{ tries: 0, successes: 0 }]],
  ["learnedModel", [STATS, "docs", priced]],
  ["learnedModel", [STATS, "security", priced]],
  ["learnedModel", [STATS, "feature", priced]],
  ["modelProfiles", [STATS, "docs", priced]],
  ["modelProfiles", [STATS, "security", priced]],
  ["modelProfiles", [STATS, "feature", priced]],
  ["budgetState", [{ monthlyUsd: 10, spentUsd: 8 }]],
  ["budgetState", [{ monthlyUsd: 10, spentUsd: 10 }]],
  ["budgetState", [{ monthlyUsd: 0, spentUsd: 99 }]],
  [
    "chooseModel",
    [
      {
        stepModel: " x ",
        type: "docs",
        rules: {},
        stats: STATS,
        models: priced,
        defaultModel: S5,
        budget: { monthlyUsd: 0, spentUsd: 0 },
      },
    ],
  ],
  [
    "chooseModel",
    [
      {
        stepModel: null,
        type: "docs",
        rules: { docs: "" },
        stats: STATS,
        models: priced,
        defaultModel: S5,
        budget: { monthlyUsd: 10, spentUsd: 9 },
      },
    ],
  ],
  [
    "chooseModel",
    [
      {
        stepModel: "  ",
        type: "ui",
        rules: { ui: O5 },
        stats: STATS,
        models: priced,
        defaultModel: S5,
        budget: { monthlyUsd: 10, spentUsd: 9 },
      },
    ],
  ],
  [
    "chooseModel",
    [
      {
        stepModel: null,
        type: "security",
        rules: {},
        stats: STATS,
        models: priced,
        defaultModel: S5,
        budget: { monthlyUsd: 10, spentUsd: 9 },
        picked: { model: O5, why: "" },
      },
    ],
  ],
  [
    "chooseModel",
    [
      {
        stepModel: null,
        type: "feature",
        rules: {},
        stats: STATS,
        models: priced,
        defaultModel: H5,
        budget: { monthlyUsd: 10, spentUsd: 9 },
        picked: { model: "gpt-9", why: "no" },
      },
    ],
  ],
  ["freeFirstFor", [undefined, true, true]],
  ["freeFirstFor", ["free-only", false, true]],
  ["freeWantedButMissing", [undefined, "priority", false, false]],
  ["freeWantedButMissing", ["free-only", "direct", false, false]],
  ["costRatio", [priced, [O5, H5], S5]],
  ["costRatio", [priced, [H5], "unknown"]],
  ["costRatio", [[{ id: "a", label: "A", price: [0, 0] }], ["a"], "a"]],
  ["stepsToRun", [PLAN, 0, false, ""]],
  ["stepsToRun", [PLAN, 9, false, ""]],
  ["stepsToRun", [PLAN, 14, true, ""]],
  ["stepsToRun", [PLAN, 0, false, "1-2; 12"]],
  ["stepsToRun", [PLAN, 14, true, "1-2"]],
  ["parseRanges", [" 2 - 6 ,, 10 to 15; 20, 9–7, x, 3-, 007"]],
  ["parseRanges", [undefined]],
  ["inRanges", ["12b", "10-12"]],
  ["inRanges", [null, "1-99"]],
  ["inRanges", ["b2", "1-99"]],
  ["unmetNeed", [["1", "docs/B.md#4"], ["1"], { "docs/B.md": ["3"] }]],
  ["unmetNeed", [["#5", "2"], ["#5"], {}]],
  ["unmetNeed", [["a#b#7"], [], { "a#b": ["7"] }]],
  ["endStatus", [RUN_STEPS]],
  ["endStatus", [RUN_STEPS.slice(0, 4)]],
  ["runSummary", [RUN_STEPS]],
  ["runSummary", [[]]],
  ["startModel", [" m ", "d"]],
  ["startModel", [null, "d"]],
  ["nextModel", [priced, [att(H5, "open", 0), att(H5, "done", 0), att(H5, "error", 0)], H5, 2]],
  ["nextModel", [priced, [att(H5, "blocked", 0)], S5, 0]],
  ["nextModel", [priced, [att(O5, "open", 0)], H5, 0]],
  ["stepOutcome", [PLAN, { number: "2", title: "Layout" }]],
  ["stepOutcome", [PLAN, { number: null, title: "Plain step without bold" }]],
  ["stepOutcome", [PLAN, { number: "1", title: "Project setup" }]],
  ["stepOutcome", [PLAN, { number: "9", title: "Nope" }]],
  [
    "stepPrompt",
    ["PLAN.md", PLAN, 1, Array.from({ length: 12 }, (_, i) => ({ number: i % 3 ? String(i) : null, title: `T${i}` }))],
  ],
  ["stepPrompt", ["PLAN.md", PLAN, 2, []]],
  ["runTotals", [RUN_STEPS, [{ model: S5, costUsd: 0.125 }], FINISH]],
  ["runTotals", [[], undefined, null]],
  [
    "nextModelFreeFirst",
    [priced, [att("qanuvi:auto", "open", 0), att("qanuvi:auto", "error", 0)], "qanuvi:auto", S5, 2],
  ],
  ["nextModelFreeFirst", [priced, [att("qanuvi:auto", "done", 0)], "qanuvi:auto", S5, 1]],
  ["nextModelFreeFirst", [priced, [att("qanuvi:auto", "open", 0), att(F5, "open", 0)], "qanuvi:auto", S5, 1]],
  ["freeSavings", [RUN_STEPS, { docs: 0.5 }]],
  ["freeSavings", [RUN_STEPS.slice(1, 3), {}]],
  ["freeSavings", [[], {}]],
  ["paidPerStep", [STATS]],
  ["finishEstimate", [RUN_STEPS, "light"]],
  ["finishEstimate", [RUN_STEPS, "full"]],
  ["finishEstimate", [[], "full"]],
  ["offersFinish", ["done", RUN_STEPS, null]],
  ["offersFinish", ["done", RUN_STEPS, FINISH]],
  ["offersFinish", ["needs-you", RUN_STEPS, null]],
  ["finishReviewPrompt", ["P.md", BRIEFS, "d".repeat(40_001), FINISH.checks]],
  ["finishReviewPrompt", ["P.md", [], "", null]],
  ["finishReviewPrompt", ["P.md", BRIEFS, "diff", { command: "npm test", ok: true, summary: "" }]],
  ["parseFinishReview", ['{"ok": true, "findings": [" a ", "", null, 3, {"x": 1}], "stillOpen": "no"}']],
  ["parseFinishReview", ['{"ok": "yes"}']],
  ["parseFinishReview", ["nothing"]],
  [
    "finishFullPrompt",
    [
      "P.md",
      BRIEFS,
      { findings: ["a"], stillOpen: ["b", "c"] },
      FINISH.checks,
      Array.from({ length: 62 }, (_, i) => `f${i}.ts`),
    ],
  ],
  ["finishFullPrompt", ["P.md", [], { findings: [], stillOpen: [] }, null, []]],
  [
    "stillOpenIn",
    ["Done.\r\n\r\n### **Still open:** first thing\r\n- second\r\n  * third\r\n- None.\r\n3) fourth\r\nEnd."],
  ],
  ["stillOpenIn", ["Still open: none\nStill open\n- last heading wins"]],
  ["stillOpenIn", ["still open\n\n- after a blank"]],
  ["stillOpenIn", ["Nothing here"]],
  ["doneNotes", [["  - Done: x ", "- DONE: y", "-Done: z", "  - Model: m"]]],
  ["isPlanFile", ["sub\\docs\\PLAN.md", "docs\\PLAN.md", "logs"]],
  ["isPlanFile", ["docs/PLAN.ARCHIVE.md", "docs/PLAN.MD", "logs"]],
  ["isPlanFile", ["logs/x.md", "P.md", "logs"]],
  ["isPlanFile", ["logsx/x.md", "P.md", "logs"]],
  ["finishEntry", [FINISH]],
  ["finishEntry", [{ ...FINISH, checks: null, findings: [], stillOpen: [], note: "", costUsd: 0.125, seconds: 2.5 }]],
];

function outputs() {
  return cases.map(([fn, args]) => {
    const out = fns[fn](...args);
    return { fn, args, out: out === undefined ? null : JSON.parse(JSON.stringify(out)) };
  });
}

describe("the comparison check with the Rust port", () => {
  it("has the TypeScript's current outputs", async () => {
    const now = JSON.stringify(outputs(), null, 1) + "\n";
    await expect(now).toMatchFileSnapshot(FILE);
  });
});
