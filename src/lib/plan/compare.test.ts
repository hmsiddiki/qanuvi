/**
 * The comparison check for 24z step A (R1): the same inputs through the TypeScript plan logic and its Rust port.
 * This test keeps src-tauri/src/plan/compare.json (inputs and TypeScript outputs) up to date with the TypeScript; the
 * Rust test plan::compare runs the same inputs and must give the same outputs. After changing a case or the
 * TypeScript, write the file again with: npx vitest run -u src/lib/plan/compare.test.ts
 *
 * The run logic itself now lives only in Rust: its TypeScript functions were deleted after R2e. Their recorded cases
 * stay in compare.json as they were (any entry whose function is not in `fns` below is kept untouched), so the Rust
 * port is still held to the outputs the TypeScript gave.
 */
import recordedText from "../../../src-tauri/src/plan/compare.json?raw";
import { describe, expect, it } from "vitest";
import * as decider from "$lib/core/decider";
import * as limits from "./limits";
import * as holdask from "./holdask";
import * as costs from "./costs";
import * as picks from "./picks";
import { taskListPrompt } from "./prompts";
import * as tasklist from "./tasklist";
import * as routing from "./routing";
import * as runner from "./runner";
import * as finishing from "./finishing";
import * as preflight from "./preflight";
import * as planstate from "./planstate";
import * as housekeeping from "./housekeeping";
import * as planner from "./planner";
import * as guidance from "./guidance";

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

/** Function name → what it runs; the Rust side has the same names in plan/compare.rs. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const fns: Record<string, (...a: any[]) => unknown> = {
  parseTeam: decider.parseTeam,
  deciderSeat: decider.deciderSeat,
  freeWorks: (team, f, p) => decider.freeWorks(decider.parseTeam(team), f, p),
  parseAssignment: decider.parseAssignment,
  pickPrompt: decider.pickPrompt,
  parsePicks: decider.parsePicks,
  reviewPrompt: decider.reviewPrompt,
  parseReview: decider.parseReview,
  chatModelPrompt: decider.chatModelPrompt,
  parseChatModel: decider.parseChatModel,
  overruleNote: decider.overruleNote,
  withoutOverruleNote: decider.withoutOverruleNote,
  overruleIn: decider.overruleIn,
  strongerModel: decider.strongerModel,
  isLimit: limits.isLimit,
  holdReply: holdask.holdReply,
  finishedCost: costs.finishedCost,
  planHistory: costs.planHistory,
  pickFor: picks.pickFor,
  taskListPrompt: (file: string) => taskListPrompt(file),
  parsePlan: tasklist.parsePlan,
  setStepStatus: (t, l, st, e) => tried(() => tasklist.setStepStatus(t, l, st, e ?? undefined)),
  setStepTitle: (t, l, ti, e) => tried(() => tasklist.setStepTitle(t, l, ti, e ?? undefined)),
  setStepModel: (t, l, m, e) => tried(() => tasklist.setStepModel(t, l, m, e ?? undefined)),
  nextStatus: tasklist.nextStatus,
  findTaskLists: tasklist.findTaskLists,
  jobType: routing.jobType,
  learn: (runs) => Object.fromEntries(routing.learn(runs, routing.jobType)),
  reliability: routing.reliability,
  learnedModel: (stats, type, models) => routing.learnedModel(statsOf(stats), type, models),
  modelProfiles: (stats, type, models) => routing.modelProfiles(statsOf(stats), type, models),
  budgetState: routing.budgetState,
  chooseModel: (o) => routing.chooseModel({ ...o, stats: statsOf(o.stats) }),
  costRatio: runner.costRatio,
  parseRanges: runner.parseRanges,
  inRanges: (n, text) => runner.inRanges(n, runner.parseRanges(text).ranges),
  runTotals: (steps, deciderCalls, finish) => {
    const t = runner.runTotals({ steps, deciderCalls, finish: finish ?? undefined });
    return { ...t, byModel: Object.fromEntries(t.byModel) };
  },
  freeSavings: (steps, per) => runner.freeSavings({ steps }, per),
  paidPerStep: (stats) => runner.paidPerStep(statsOf(stats)),
  finishEstimate: (steps, level, prices) => finishing.finishEstimate({ steps }, level, prices ?? undefined),
  offersFinish: (status, steps, finish) => finishing.offersFinish({ status, steps, finish: finish ?? undefined }),
  preflightPrompt: preflight.preflightPrompt,
  parsePreflight: preflight.parsePreflight,
  recommendedRisk: preflight.recommendedRisk,
  decidedNote: preflight.decidedNote,
  settleCheck: (answer, a, at, r, rt) => preflight.settleCheck(preflight.parsePreflight(answer), a, at, r, rt),
  secretNeedsReading: preflight.secretNeedsReading,
  typedAnswers: (answer, s, a, at) => preflight.typedAnswers(preflight.parsePreflight(answer), s, a, at),
  answersPrompt: preflight.answersPrompt,
  parseAnswerReadings: preflight.parseAnswerReadings,
  unsettledWhy: preflight.unsettledWhy,
  readAnswers: (answer, s, a, at, readings, used, first) =>
    preflight.readAnswers(preflight.parsePreflight(answer), s, a, at, readings, used, first),
  withReadHolds: (answer, a, at, r, rt, holds) =>
    preflight.withReadHolds(preflight.settleCheck(preflight.parsePreflight(answer), a, at, r, rt), holds),
  holdWording: planstate.holdWording,
  keepHolds: planstate.keepHolds,
  keepAnswer: planstate.keepAnswer,
  keepDecided: planstate.keepDecided,
  changeDecided: planstate.changeDecided,
  archivePath: housekeeping.archivePath,
  finishedSections: (t) => housekeeping.finishedSections(t).map((s) => [s.title, s.line, s.steps.length]),
  archiveFinished: housekeeping.archiveFinished,
  mergePlans: housekeeping.mergePlans,
  stepNumbers: housekeeping.stepNumbers,
  plannerPrompt: planner.plannerPrompt,
  extractSteps: planner.extractSteps,
  lastNumber: planner.lastNumber,
  renumber: planner.renumber,
  appendSection: planner.appendSection,
  takeModels: planner.takeModels,
  sectionTitleFrom: planner.sectionTitleFrom,
  guidanceNote: guidance.guidanceNote,
  withoutGuidance: guidance.withoutGuidance,
  stoppedChatNote: guidance.stoppedChatNote,
  isGoOnOrder: guidance.isGoOnOrder,
  isYes: guidance.isYes,
  askedToResume: guidance.askedToResume,
  isStopOrder: guidance.isStopOrder,
  stepKey: guidance.stepKey,
  looksSecret: guidance.looksSecret,
  contextSize: guidance.contextSize,
  privateBlock: guidance.privateBlock,
  logDir: guidance.logDir,
  mapPath: guidance.mapPath,
  factsPath: guidance.factsPath,
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

// R1 part 2b: preflight, packet, planstate, housekeeping, planner, guidance.
// p2: the check's answers read by meaning.
const RA_CHECK = JSON.stringify({
  secrets: [
    { name: "DB_HOST", what: "Where the test database is", example: "localhost:3306", steps: ["2a"] },
    { name: "DB_PASSWORD", what: "The test database password", steps: ["2a", "2b"] },
    { name: "API_KEY", why: "The test key", steps: [] },
    { name: "SHOP_KEY", steps: ["3"] },
  ],
  questions: [
    { step: "4a", question: "Which currency format?", sort: "decide", recommended: "1,234.00" },
    { step: "4b", question: "Which bank?", sort: "you" },
    { step: "4c", question: "Which theme?", sort: "decide", recommended: "dark" },
    { step: "", question: "Which colour?", sort: "you" },
  ],
});
const RA_SECRETS = {
  DB_HOST: " find the mysql address yourself ",
  DB_PASSWORD: " hunter2 ",
  API_KEY: "what is this?",
  SHOP_KEY: "the same as qchmmart",
};
const RA_ANSWERS = { 0: "use the shop one", 1: "dont get it", 2: "you pick", 3: "blue please" };
const RA_TOUCHED = { 0: true, 1: true, 2: true, 3: true };
const RA_READ = (id: string, kind: string, more: Record<string, unknown> = {}) => ({
  id,
  kind,
  value: "",
  where: "",
  copyFrom: null,
  skip: false,
  reply: "",
  ...more,
});
const RA_READINGS = [
  RA_READ("secret:DB_HOST", "instruction", { value: "localhost:3307", where: "config.php" }),
  RA_READ("secret:API_KEY", "unclear", { reply: "It is the key from the test dashboard." }),
  RA_READ("secret:SHOP_KEY", "instruction", { copyFrom: { project: "qchmmart", name: "SHOP_KEY" } }),
  RA_READ("question:0", "instruction", { value: "$1,234.00", where: "shop settings" }),
  RA_READ("question:1", "unclear", { reply: "Which bank the payments go to" }),
  RA_READ("question:2", "instruction", { skip: true }),
  RA_READ("question:3", "value", { value: "blue" }),
];

const PF_ANSWER =
  "Checked:\n" +
  JSON.stringify({
    secrets: [
      { name: " DB_PASS ", why: "db", steps: ["1", "", 3, " 2 "] },
      { name: "9bad", why: "x" },
      { name: "A".repeat(101) },
      "str",
      null,
      [1],
    ],
    questions: [
      { step: "1a", question: "Separator?", sort: "decide", recommended: "dash", because: "URLs" },
      { step: "1b", question: "Bank?", sort: "checks" },
      { step: "1c", question: "  ", recommended: "x" },
      { step: "1d", question: "Format?", recommended: "ISO", sort: "odd" },
      { step: 5, question: "Numbered step?", recommended: "y", sort: "checks" },
      { step: "1e", question: "Column?", sort: "checks", recommended: "slug" },
      { step: "1f", question: "Bank login?", sort: "you", recommended: "test bank" },
    ],
    risky: [
      { step: "2a", why: "migrates", sort: "checks", because: "test db" },
      { step: "", why: "no step" },
      { step: "2b", why: "w".repeat(320) },
      { step: "2c", why: "clears cache", sort: "decide" },
      { step: "1a", why: "renames files", sort: "you" },
    ],
    setup: [{ what: " MySQL ", why: "tests" }, { why: "no what" }],
  }) +
  "\nThat is all.";
const KNOWN = {
  secrets: ["DB_USER", "DB_PASS"],
  testCommand: "npm test",
  previewUrl: "http://localhost:5173",
  notes: [],
};
const PF_STEPS = [
  { number: "82e", title: "Short field set", details: Array.from({ length: 10 }, (_, i) => `- d${i}`) },
  { number: null, title: "Unnumbered", details: [] },
  { number: "", title: "Empty number", details: ["x"] },
];
const PS = {
  holds: [
    { step: "1a", why: "q", by: "default", at: 1 },
    { step: "2a", why: "p", by: "you", at: 1 },
  ],
  answers: [{ step: "3a", choice: "go", text: "dash", at: 2 }],
  closings: [
    { closing: "1z", waiting: ["1a", "1b", "9x"], late: ["1c"] },
    { closing: "5z", waiting: [], late: [] },
    { closing: "gone", waiting: ["1a"], late: [] },
  ],
  decided: [
    { step: "1a", what: "Separator?", answer: "dash", because: "", at: 1 },
    { step: "1b", what: "Format?", answer: "ISO", because: "b", changed: "RFC", at: 1 },
    { step: "1c", what: "Width?", answer: "x", because: "", changed: "", at: 1 },
  ],
  checks: [
    { step: "2a", why: "migrates" },
    { step: "4a", why: "x" },
  ],
  models: [
    { step: "1a", title: "Prices", model: "claude-sonnet-5-5" },
    { step: "gone", title: "Gone", model: "claude-haiku-5-5" },
  ],
};
const DEC = (step: string, what: string, answer: string) => ({ step, what, answer, because: "" });
const HK_TARGET = "# Plan\n\n## A\n\n- [ ] **1. One**\n- [ ] **2. Two**\n";
const HK_SOURCE = [
  "Intro text before any heading.",
  "- [ ] **3-1. Loose sub**",
  "  - Needs: 3, ./PLAN.md#2",
  "## Other",
  "",
  "- [x] **1. Old done**",
  "- [ ] **2. Next**",
  "  - needs: 1,  2b ,docs/X.md#3",
  "  - Model: claude-opus-5-5",
  "- [ ] **2b. Sub**",
  "  - Needs: 2, PLAN.md#1, LIB.md#3, #4",
  "- [ ] **9. Free number**",
  "- [ ] **007. Zeros**",
  "",
  "",
].join("\n");
const HK_PLAN = [
  "# Work map",
  "",
  "## Stage 1",
  "",
  "- [x] **1. Setup**",
  "  - Done.",
  "- [x] **2. Layout**",
  "",
  "",
  "### Sub of 1",
  "",
  "- [ ] **2a. Open sub**",
  "## Stage 2",
  "Words.",
  "- [x] **3. Editor**",
  "## Empty stage",
  "",
  "#Not a heading",
  "## Stage 3",
  "- [x] **5. Done too**",
  "",
].join("\n");
const PL_ANSWER = [
  "Plan:\r",
  "```Markdown  \r",
  "# Heading",
  " - [X] **1. First**\r",
  "    - detail one",
  "\t- tab detail",
  "",
  "- [!] **2. Second** (note)",
  " - not a detail",
  "- [ ] Plain third",
  "  - Model: claude-haiku-5-5",
  "```",
  "after",
].join("\n");
const LOGS = [
  JSON.stringify({ type: "assistant", message: { usage: { input_tokens: 7, cache_creation_input_tokens: 3 } } }),
  '{"type":"assistant","usage" broken',
  JSON.stringify({ type: "assistant", message: { usage: null }, usage: 1 }),
  JSON.stringify({ type: "result", usage: { input_tokens: 99 } }),
];

const cases: [string, unknown[]][] = [
  ["preflightPrompt", ["PLAN-D.md", PF_STEPS, KNOWN]],
  ["preflightPrompt", ["P.md", [], { secrets: [], testCommand: "", previewUrl: "", notes: ["one", "two"] }]],
  ["parsePreflight", [PF_ANSWER]],
  ["parsePreflight", ["no json here"]],
  ["parsePreflight", ["} {"]],
  [
    "parsePreflight",
    [
      '{"order":[{"step":"1b","after":["1c","1b","1c","a b","x,y",3],"why":"README lists the exports"},' +
        '{"step":"2","after":"1"},{"step":"3","after":[]},{"after":["1"]},[1]]}',
    ],
  ],
  ["parsePreflight", ["{ not json }"]],
  ["parsePreflight", ['{"secrets": "x", "questions": {"a": 1}, "risky": [], "setup": [{"what": "é".repeat(400)}]}']],
  ["recommendedRisk", ["decide"]],
  ["recommendedRisk", ["checks"]],
  ["recommendedRisk", ["you"]],
  ["decidedNote", [{ step: "1a", what: "Sep?", answer: "dash", because: "URLs" }]],
  ["decidedNote", [DEC("2", "W?", "x")]],
  [
    "settleCheck",
    [
      PF_ANSWER,
      { 0: "dash", 1: "  ", 2: "ISO", 3: " y ", 4: "slug", 5: "test bank" },
      { 2: true },
      { 0: "go", 3: "wait", 4: "checks" },
      { 3: true },
    ],
  ],
  ["settleCheck", [PF_ANSWER, {}, {}, {}, {}]],
  ["secretNeedsReading", ["hunter2"]],
  ["secretNeedsReading", ["  x\u00a0y "]],
  ["secretNeedsReading", ["a\u0085b"]],
  ["typedAnswers", [RA_CHECK, RA_SECRETS, RA_ANSWERS, RA_TOUCHED]],
  ["typedAnswers", [RA_CHECK, { DB_HOST: "x ".repeat(400) }, {}, {}]],
  [
    "answersPrompt",
    [
      "PLAN.md",
      [
        { id: "secret:DB_HOST", ask: "Where (DB_HOST)", example: "localhost", answer: 'use "the" same é\n' },
        { id: "question:1", ask: "Bank?", example: "", answer: "dont get it" },
      ],
      { secrets: ["DB_USER", "X"], others: [{ name: "qchmmart", path: "E:/www/q", secrets: [] }] },
    ],
  ],
  ["answersPrompt", ["P.md", [], { secrets: [], others: [] }]],
  [
    "parseAnswerReadings",
    [
      "Read: " +
        JSON.stringify({
          answers: [
            { id: " secret:DB_HOST ", kind: "instruction", value: " localhost:3307 ", where: "config.php", skip: 1 },
            { id: "secret:DB_HOST", kind: "value", value: "twice" },
            { id: "question:1", kind: "unclear", reply: "A bank is…", where: "x", skip: true },
            { id: "question:2", kind: "maybe" },
            { id: "secret:SHOP_KEY", kind: "instruction", copyFrom: { project: "q", name: "" } },
            { id: "secret:API_KEY", kind: "instruction", copyFrom: { project: "q", name: "K" }, skip: true },
            { id: "other", kind: "value" },
            [1],
            null,
          ],
        }),
      ["secret:DB_HOST", "question:1", "question:2", "secret:SHOP_KEY", "secret:API_KEY"],
    ],
  ],
  ["parseAnswerReadings", ['{"answers": {"a": 1}}', ["a"]]],
  ["parseAnswerReadings", ["} {", ["a"]]],
  ["unsettledWhy", ["X (A)", "what?", "It is the key. \n"]],
  ["unsettledWhy", ["X (A)", "é".repeat(200), ""]],
  ["readAnswers", [RA_CHECK, RA_SECRETS, RA_ANSWERS, RA_TOUCHED, RA_READINGS, {}, "1"]],
  [
    "readAnswers",
    [
      RA_CHECK,
      RA_SECRETS,
      RA_ANSWERS,
      RA_TOUCHED,
      RA_READINGS,
      { "secret:DB_HOST": true, "secret:SHOP_KEY": true, "question:0": true },
      "1",
    ],
  ],
  ["readAnswers", [RA_CHECK, RA_SECRETS, RA_ANSWERS, RA_TOUCHED, RA_READINGS, { "secret:DB_HOST": false }, ""]],
  ["readAnswers", [RA_CHECK, RA_SECRETS, RA_ANSWERS, RA_TOUCHED, [], {}, "1"]],
  [
    "withReadHolds",
    [
      PF_ANSWER,
      {},
      {},
      { 1: "wait" },
      { 1: true },
      [
        { step: "1b", why: "it needs a bank", by: "default" },
        { step: "1a", why: "first", by: "default" },
        { step: "1a", why: "second", by: "default" },
      ],
    ],
  ],
  ["holdWording", [{ why: "pushes", by: "you" }]],
  ["holdWording", [{ why: "a question", by: "default" }]],
  [
    "keepHolds",
    [
      PS,
      [
        { step: "3a", why: "answered already", by: "default" },
        { step: "1a", why: "new", by: "you" },
        { step: "7a", why: "x", by: "default" },
      ],
      5,
    ],
  ],
  ["keepAnswer", [PS, "1a", "skip", "", 6]],
  ["keepAnswer", [PS, "3a", "go", "again", 6]],
  [
    "keepDecided",
    [
      PS,
      [
        DEC("1b", "Format?", "ISO"),
        DEC("1a", "Separator?", "under"),
        DEC("4a", "New?", "yes"),
        DEC("1c", "Width?", "y"),
      ],
      [{ step: "4a", why: "new check" }],
      ["2a"],
      7,
    ],
  ],
  ["keepDecided", [PS, [], [], [], 8]],
  ["changeDecided", [PS, "1a", "Separator?", "dot"]],
  ["changeDecided", [PS, "1a", "nope", "x"]],
  ["archivePath", ["docs/PLAN.md"]],
  ["archivePath", ["docs/PLAN.MD"]],
  ["archivePath", ["PLAN"]],
  ["archivePath", ["a.md.txt"]],
  ["finishedSections", [HK_PLAN]],
  ["finishedSections", [PLAN]],
  ["archiveFinished", [HK_PLAN, null, "docs/PLAN.md"]],
  ["archiveFinished", [HK_PLAN.replace(/\n/g, "\r\n"), "# Archive\r\n\r\n## Old\r\n\r\n\r\n", "P.md"]],
  ["archiveFinished", [HK_PLAN, "", "P.md"]],
  ["archiveFinished", [HK_PLAN, "# Archive\r\n", "P.md"]],
  ["archiveFinished", [PLAN, null, "PLAN.md"]],
  ["archiveFinished", ["## A\n\n- [ ] **1. Open**\n", null, "P"]],
  ["mergePlans", [HK_TARGET, "./PLAN.md", HK_SOURCE, "OTHER.md", {}]],
  ["mergePlans", [HK_TARGET, "PLAN.md", HK_SOURCE, "OTHER.md", { onlyOpen: true, taken: ["10", "x", ""] }]],
  ["mergePlans", [HK_TARGET.replace(/\n/g, "\r\n"), "PLAN.md", HK_SOURCE, "O.md", {}]],
  ["mergePlans", ["", "PLAN.md", "- [ ] **7. Loose**", "notes/TODO.md", {}]],
  ["mergePlans", ["# P\n\n\n", "PLAN.md", PLAN, "P2.md", { onlyOpen: true }]],
  ["mergePlans", [HK_TARGET, "PLAN.md", "- [x] **1. Done**\n", "O.md", { onlyOpen: true }]],
  ["stepNumbers", [HK_PLAN]],
  ["stepNumbers", [null]],
  ["plannerPrompt", ["  Add VAT to the cart \n", "TASKLIST.md", models]],
  ["extractSteps", [PL_ANSWER]],
  ["extractSteps", ["- [ ] **1. No fence**\n  - d\nText\n- [x] **2. Done one**"]],
  ["extractSteps", ["I could not plan this."]],
  ["extractSteps", ["```tasklist\n```"]],
  ["lastNumber", [PLAN]],
  ["lastNumber", ["- [x] **3. A**\n- [ ] **12b. B**\n- [ ] C"]],
  ["lastNumber", ["# empty"]],
  ["renumber", ["- [ ] **1. A**\n  - d\n- [ ] **2b.  B  **tail\n- [ ] Plain\nnot a step\n- [x] **3. Done**", 13]],
  ["renumber", ["- [ ] **1. A**\n- [ ] **2. B**", "12"]],
  ["renumber", ["- [ ] ****\n- [ ] **x**", 1]],
  [
    "appendSection",
    ["# P\r\n\r\n- [x] **1. Old**\r\n\r\n", "  Long \t title " + "x".repeat(100), "\n- [ ] **2. New**\n  - d\n"],
  ],
  ["appendSection", ["", "", "- [ ] **1. X**"]],
  ["appendSection", ["# P", "VAT", ""]],
  [
    "takeModels",
    [
      "- [ ] **12. Add VAT**\r\n  - Model: claude-sonnet-5-5\r\n  - Validate: ok\n- [ ] Plain step\n  - MODEL: x\n- [ ] **13. None**\n",
    ],
  ],
  ["takeModels", [""]],
  ["sectionTitleFrom", ["Add coupons. Then test them."]],
  ["sectionTitleFrom", ["Add optional VAT to the cart total using addVat from src/tax.js, with tests."]],
  ["sectionTitleFrom", ["  "]],
  ["sectionTitleFrom", ["\n\n  Do it now!!! Please\nSecond line"]],
  ["sectionTitleFrom", ["Version 1.2 is out. Next"]],
  ["sectionTitleFrom", ["Make the checkout page handle coupons, gift cards, and store credit;also refunds"]],
  ["sectionTitleFrom", ["Ünïcödé wörds äre hére tö tést thé cütting öf löng séntences néar sixty"]],
  ["sectionTitleFrom", ["x".repeat(70)]],
  ["guidanceNote", ["82a", "PLAN-D.md"]],
  ["withoutGuidance", ["use the shop currency" + guidance.guidanceNote("82a", "PLAN-D.md")]],
  ["withoutGuidance", ["ask" + guidance.stoppedChatNote("1", "P.md") + guidance.guidanceNote("1", "P.md")]],
  ["withoutGuidance", ["plain words"]],
  ["stoppedChatNote", ["53z", "PLAN-E.md"]],
  ...[
    "continue",
    " Resume. ",
    "go on!",
    "retry the step",
    "Continue The Run Again",
    "run it again",
    "continue?",
    "carry on please",
  ].map((t): [string, unknown[]] => ["isGoOnOrder", [t]]),
  ...["yes", "Yes!", "ok.", "go ahead", "yes, resume it", "yes resume", "y", "yes but first", "no", " ok "].map(
    (t): [string, unknown[]] => ["isYes", [t]],
  ),
  ["askedToResume", ["Done.\nShould I resume the plan run from step 53z? (yes/no)"]],
  ["askedToResume", ["RESUME THE RUN now."]],
  ["askedToResume", ["Should I use option 1? (yes/no)"]],
  ...[
    "stop",
    "Stop!",
    "stop the run",
    "halt now now",
    "cancel everything here.",
    "stop using floats",
    "do not stop",
  ].map((t): [string, unknown[]] => ["isStopOrder", [t]]),
  ["stepKey", [{ number: null, title: "T" }]],
  ["stepKey", [{ number: "4", title: "T|x" }]],
  ...[
    "root qz7test9 if you need db user pass",
    "the db password is abc12345",
    "key sk-test-abcdefghijklmnopqrstu",
    "use the shop currency",
    "do not change the login page",
    "token: a1b2",
    "token is x_abc1.def",
    "the API-key value is k9!@#$",
    "password word9plain",
    "Passwd: ab12cd",
    "login éabc123",
    "secret a.b.c.1d",
  ].map((t): [string, unknown[]] => ["looksSecret", [t]]),
  ["contextSize", [LOGS]],
  ["contextSize", [[]]],
  ["contextSize", [[LOGS[1]]]],
  ["privateBlock", [["db user: test\r\n  password: x1", "plain"]]],
  ["privateBlock", [[]]],
  ["logDir", ["content\\plugins\\qcommerce\\PLAN-D.MD"]],
  ["logDir", ["docs/plän (2).md.txt"]],
  ["mapPath", ["docs/PLAN.md"]],
  ["factsPath", ["docs/PLAN.md"]],
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
  [
    "reviewPrompt",
    [
      { title: "Schema", details: ["a"] },
      "",
      "",
      { home: "content/plugins/qc/", done: "Done: all there, ü".repeat(120) },
    ],
  ],
  ["reviewPrompt", [{ title: "Schema", details: [] }, "diff --git a/x", "passed", { home: "p/", done: "Done: y" }]],
  ["reviewPrompt", [{ title: "Schema", details: [] }, "", "", { home: "", done: "Done: z" }]],
  ["parseReview", ['{"ok": true, "why": "fine"}']],
  ["parseReview", ['```json\n{"ok": "yes"}\n```']],
  ["parseReview", ['{"ok": false}']],
  ["chatModelPrompt", ["rename a variable", models]],
  ["parseChatModel", ['{"model":" claude-haiku-5-5 ","why":"small"}', models]],
  ["parseChatModel", ['{"model":"gpt-9"}', models]],
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
  ["holdReply", [{ choice: "go", text: " yes " }]],
  ["holdReply", [{ choice: "skip" }]],
  ["holdReply", [{ choice: "decide", text: "keep it small" }]],
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
  ["costRatio", [priced, [O5, H5], S5]],
  ["costRatio", [priced, [H5], "unknown"]],
  ["costRatio", [[{ id: "a", label: "A", price: [0, 0] }], ["a"], "a"]],
  ["parseRanges", [" 2 - 6 ,, 10 to 15; 20, 9–7, x, 3-, 007"]],
  ["parseRanges", [undefined]],
  ["inRanges", ["12b", "10-12"]],
  ["inRanges", [null, "1-99"]],
  ["inRanges", ["b2", "1-99"]],
  ["runTotals", [RUN_STEPS, [{ model: S5, costUsd: 0.125 }], FINISH]],
  ["runTotals", [[], undefined, null]],
  ["freeSavings", [RUN_STEPS, { docs: 0.5 }]],
  ["freeSavings", [RUN_STEPS.slice(1, 3), {}]],
  ["freeSavings", [[], {}]],
  ["paidPerStep", [STATS]],
  ["finishEstimate", [RUN_STEPS, "light"]],
  ["finishEstimate", [RUN_STEPS, "full"]],
  ["finishEstimate", [[], "full"]],
  ["finishEstimate", [RUN_STEPS, "light", { pass: 20, byModel: { [H5]: 0.5, [S5]: 10 } }]],
  ["finishEstimate", [RUN_STEPS, "full", { pass: 0, byModel: { [H5]: 0.5 } }]],
  ["offersFinish", ["done", RUN_STEPS, null]],
  ["offersFinish", ["done", RUN_STEPS, FINISH]],
  ["offersFinish", ["needs-you", RUN_STEPS, null]],
];

interface Recorded {
  fn: string;
  args: unknown[];
  out: unknown;
}

/** The recorded cases of functions that no longer exist in TypeScript, in their recorded order. */
function frozen(): Recorded[] {
  const recorded: Recorded[] = JSON.parse(recordedText);
  return recorded.filter((r) => !(r.fn in fns));
}

function outputs(): Recorded[] {
  const now = cases.map(([fn, args]) => {
    const out = fns[fn](...args);
    return { fn, args, out: out === undefined ? null : JSON.parse(JSON.stringify(out)) };
  });
  return [...now, ...frozen()];
}

describe("the comparison check with the Rust port", () => {
  it("has the TypeScript's current outputs", async () => {
    const now = JSON.stringify(outputs(), null, 1) + "\n";
    await expect(now).toMatchFileSnapshot(FILE);
  });
});
