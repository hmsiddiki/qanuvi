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
