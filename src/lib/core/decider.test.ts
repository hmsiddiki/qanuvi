import { describe, expect, it } from "vitest";
import {
  assignPrompt,
  chatModelPrompt,
  deciderSeat,
  freeWorks,
  parseAssignment,
  parseChatModel,
  parseReview,
  parseTeam,
  reviewPrompt,
  stepDiff,
  overruleIn,
  overruleNote,
  strongerModel,
  type StepBrief,
} from "./decider";

const steps: StepBrief[] = [
  { number: "1", title: "Write a README", details: [], type: "docs" },
  { number: "2", title: "Add a checkout page", details: ["Stripe"], type: "security" },
  { number: "3", title: "Add a filter to the list", details: [], type: "feature" },
];

describe("the decider", () => {
  it("is the paid agent when there is one, else the free models online first, local last", () => {
    expect(deciderSeat({ agent: "claude-code", model: "opus" }, [{ id: "or/a", local: false }])).toEqual({
      kind: "paid",
      agent: "claude-code",
      model: "opus",
    });
    expect(
      deciderSeat(null, [
        { id: "ollama/q", local: true },
        { id: "omniroute/s", local: false },
        { id: "openrouter/n", local: false },
      ]),
    ).toEqual({ kind: "free", models: ["omniroute/s", "openrouter/n", "ollama/q"] });
    expect(deciderSeat(null, [])).toEqual({ kind: "none" });
  });

  it("lets free models work except in Direct and Free-decides with a paid agent", () => {
    const t = (mode: Parameters<typeof parseTeam>[0]) => parseTeam(mode);
    expect(freeWorks(t({ mode: "paid-decides" }), true, true)).toBe(true);
    expect(freeWorks(t({ mode: "direct" }), true, true)).toBe(false);
    expect(freeWorks(t({ mode: "free-decides" }), true, true)).toBe(false);
    expect(freeWorks(t({ mode: "direct" }), true, false)).toBe(true);
    expect(parseTeam({ mode: "nonsense", priority: "solve" })).toEqual({ mode: "paid-decides", priority: "solve" });
  });

  it("asks once for the whole plan and keeps money and security work paid whatever it says", () => {
    expect(assignPrompt(steps, "save")).toContain("Save the paid model's tokens");
    const answer =
      'Sure:\n```json\n{"steps":[{"number":"1","worker":"free","why":"plain docs"},{"number":"2","worker":"free","why":"easy"}]}\n```';
    const a = parseAssignment(answer, steps, "balanced");
    expect(a[0]).toMatchObject({ worker: "free", why: "plain docs" });
    expect(a[1]).toMatchObject({ worker: "paid", why: "money or security work" });
    // Left out: the rule decides (balanced: a feature goes paid).
    expect(a[2]).toMatchObject({ worker: "paid" });
    // An answer that cannot be read: rules by priority.
    expect(parseAssignment("no idea", steps, "save").map((x) => x.worker)).toEqual(["free", "paid", "free"]);
    expect(parseAssignment("no idea", steps, "solve").map((x) => x.worker)).toEqual(["free", "paid", "paid"]);
  });

  it("accepts a free model's change only on a clear yes", () => {
    expect(reviewPrompt({ title: "t", details: [] }, "x".repeat(20_000), "")).toContain("(cut)");
    expect(parseReview('{"ok": true, "why": "fine"}')).toEqual({ ok: true, why: "fine" });
    expect(parseReview('{"ok": false, "why": "breaks the build"}').ok).toBe(false);
    expect(parseReview("looks good to me").ok).toBe(false);
  });

  it("picks a paid model for a chat message only from the list", () => {
    const models = [
      { id: "claude-opus-5-5", label: "Opus" },
      { id: "claude-haiku-5-5", label: "Haiku" },
    ];
    expect(chatModelPrompt("rename a variable", models)).toContain("claude-haiku-5-5");
    expect(parseChatModel('{"model":"claude-haiku-5-5","why":"small"}', models)).toEqual({
      model: "claude-haiku-5-5",
      why: "small",
    });
    expect(parseChatModel('{"model":"gpt-9"}', models)).toBeNull();
  });
});

describe("stepDiff", () => {
  it("shows only what changed since the step started, not earlier steps' changes", () => {
    // Step 2 added butter (not committed); step 3 sorts. The review must see the sort only.
    const before = "Shopping list\n- milk\n- apples\n- bread\n- butter\n";
    const after = "Shopping list\n- apples\n- bread\n- butter\n- milk\n";
    const d = stepDiff("notes.txt", before, after)!;
    expect(d).toContain("-- milk");
    expect(d).toContain("+- milk");
    expect(d).not.toContain("+- butter");
  });

  it("is empty when nothing changed, and gives up on very long files", () => {
    expect(stepDiff("a", "x\ny", "x\ny")).toBe("");
    const big = "l\n".repeat(3000);
    expect(stepDiff("a", big, big + "x")).toBeNull();
  });
});

describe("the paid agent overrules a free pick", () => {
  it("tells the paid agent the pick and how to overrule it", () => {
    const n = overruleNote("Haiku 5.5", "a short question");
    expect(n).toContain("Haiku 5.5");
    expect(n).toContain("NEEDS_STRONGER_MODEL");
  });

  it("reads an overrule only when the answer starts with it", () => {
    expect(overruleIn("NEEDS_STRONGER_MODEL: the refactor spans many files")).toBe("the refactor spans many files");
    expect(overruleIn("  NEEDS_STRONGER_MODEL")).toBe("no reason given");
    expect(overruleIn("The file holds prices. NEEDS_STRONGER_MODEL is not needed.")).toBeNull();
  });

  it("goes one model up, strongest first", () => {
    const models = [{ id: "fable" }, { id: "opus" }, { id: "sonnet" }, { id: "haiku" }];
    expect(strongerModel(models, "haiku")).toBe("sonnet");
    expect(strongerModel(models, "fable")).toBe("fable");
  });
});
