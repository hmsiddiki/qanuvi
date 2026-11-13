import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { chooseModel, modelProfiles, type ModelStats } from "./routing";
import { loadPicks, pickFor, savePicks, type SavedPicks } from "./picks";
import { parsePicks, pickPrompt, type StepBrief } from "$lib/core/decider";

const models = CLAUDE_CODE.models; // fable, opus, sonnet, haiku
const F = "claude-fable-5-2";
const O = "claude-opus-5-5";
const S = "claude-sonnet-5-5";

/** Shaped like a real record: Fable finishes less often and burns far more per finished step. */
function record(): Map<string, ModelStats> {
  return new Map([
    [`security|${F}`, { tries: 5, successes: 2, costUsd: 9, tokens: 673_000 }],
    [`security|${O}`, { tries: 34, successes: 33, costUsd: 137.6, tokens: 7_252_000 }],
    [`security|${S}`, { tries: 5, successes: 5, costUsd: 9.95, tokens: 991_000 }],
  ]);
}

describe("modelProfiles", () => {
  it("marks a model heavy when it burns 1.5× what another proven model needs for the same work", () => {
    const p = modelProfiles(record(), "security", models);
    const by = Object.fromEntries(p.map((x) => [x.model, x]));
    expect(by[F].heavy).toBe(true);
    expect(by[O].heavy).toBe(false);
    expect(by[S].heavy).toBe(false);
    expect(by[O].tokensPerSuccess).toBe(Math.round(7_252_000 / 33));
  });
  it("marks nothing heavy with nothing lighter to compare with", () => {
    const one = new Map([[`docs|${O}`, { tries: 4, successes: 4, costUsd: 8, tokens: 800_000 }]]);
    expect(modelProfiles(one, "docs", models).some((x) => x.heavy)).toBe(false);
  });
});

describe("the decider's picks", () => {
  const steps: StepBrief[] = [
    { number: "95a", title: "Erasure clears gift card recipients", details: [], type: "security" },
    { number: "95z", title: "Docs check", details: [], type: "docs" },
  ];
  it("puts quality first and shows heavy models in the record", () => {
    const prompt = pickPrompt(steps, {
      priority: "balanced",
      freeFirst: false,
      models: models.map((m) => ({ id: m.id, label: m.label })),
      profiles: { security: modelProfiles(record(), "security", models) },
    });
    expect(prompt).toContain("High-quality code comes first");
    expect(prompt).toMatch(/claude-fable-5-2 2\/5 done, \d+k tokens and \$[\d.]+ per finished step HEAVY/);
    expect(prompt).not.toContain('worker "free"');
  });
  it("keeps only models from the list; without free models first every step is paid", () => {
    const answer = JSON.stringify({
      steps: [
        { number: "95a", worker: "free", model: O, why: "money logic" },
        { number: "95z", worker: "free", model: "some-other-model", why: "docs" },
      ],
    });
    const list = parsePicks(answer, steps, { priority: "save", freeFirst: false, modelIds: models.map((m) => m.id) });
    expect(list).toEqual([
      { number: "95a", worker: "paid", why: "money or security work", model: O },
      { number: "95z", worker: "paid", why: "docs" },
    ]);
  });
});

describe("chooseModel with a pick", () => {
  const base = {
    type: "security" as const,
    rules: {},
    stats: record(),
    models,
    defaultModel: O,
    budget: { monthlyUsd: 0, spentUsd: 0 },
    picked: { model: O, why: "money logic" },
  };
  it("the owner's own model on the step wins over everything", () => {
    expect(chooseModel({ ...base, stepModel: F })).toEqual({ model: F, reason: "set on the step" });
  });
  it("a project rule wins over the pick; the pick wins over what was learned", () => {
    expect(chooseModel({ ...base, stepModel: null, rules: { security: S } }).model).toBe(S);
    expect(chooseModel({ ...base, stepModel: null })).toEqual({ model: O, reason: "decider: money logic" });
    // Learned, quality first: 33 of 34 is surer than 5 of 5, though Sonnet costs less per step.
    expect(chooseModel({ ...base, stepModel: null, picked: null })).toEqual({
      model: O,
      reason: "learned: most reliable on money and security (33/34 done)",
    });
  });
});

describe("saved picks", () => {
  it("are used only for the same agent and the same step title", () => {
    const mem = new Map<string, string>();
    const store = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
    const saved: SavedPicks = {
      at: 1,
      providerId: "claude-code",
      freeFirst: false,
      picks: { "95a": { title: "Erasure", worker: "paid", model: O, why: "money" } },
    };
    savePicks(store, "p", "PLAN-E.md", saved);
    const back = loadPicks(store, "p", "PLAN-E.md");
    expect(pickFor(back, { number: "95a", title: "Erasure" }, "claude-code")?.model).toBe(O);
    expect(pickFor(back, { number: "95a", title: "Erasure, changed" }, "claude-code")).toBeNull();
    expect(pickFor(back, { number: "95a", title: "Erasure" }, "codex")).toBeNull();
    expect(loadPicks(store, "p", "OTHER.md")).toBeNull();
  });

  it("follow a step without a number when its line moves, by its title, only while that title is unique", () => {
    const pick = (title: string) => ({ title, worker: "paid" as const, model: O, why: "x" });
    const saved: SavedPicks = {
      at: 1,
      providerId: "claude-code",
      freeFirst: false,
      picks: { "5": pick("Add tests"), "6": pick("Write docs"), "7": pick("Write docs") },
    };
    expect(pickFor(saved, { number: "9", title: "Add tests" }, "claude-code")?.model).toBe(O);
    expect(pickFor(saved, { number: "9", title: "Write docs" }, "claude-code")).toBeNull();
    expect(pickFor(saved, { number: "9", title: "Something else" }, "claude-code")).toBeNull();
  });
});
