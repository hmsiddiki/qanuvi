import { describe, expect, it } from "vitest";
import { parseSettings } from "./settings.svelte";

describe("parseSettings: budget and rules", () => {
  it("keeps a sane budget and clean rules", () => {
    const s = parseSettings(
      JSON.stringify({
        monthlyBudgetUsd: -5,
        rules: { p1: { docs: "claude-haiku-5-5", ui: "two words", tests: "" }, p2: "nonsense" },
      }),
    );
    expect(s.monthlyBudgetUsd).toBe(0);
    expect(s.rules).toEqual({ p1: { docs: "claude-haiku-5-5" } });
    expect(parseSettings(JSON.stringify({ monthlyBudgetUsd: 25 })).monthlyBudgetUsd).toBe(25);
  });
});

describe("parseSettings", () => {
  it("gives every agent its defaults", () => {
    const s = parseSettings(null);
    expect(s.agent).toBe("claude-code");
    expect(s.agents["claude-code"]).toEqual({
      model: "claude-sonnet-5-5",
      mode: "default",
      effort: "medium",
      baseUrl: "",
      indexTools: true,
      browserTools: true,
    });
  });

  it("carries over settings saved before agents had their own section", () => {
    const s = parseSettings(
      JSON.stringify({
        defaultModel: "claude-haiku-5-5",
        defaultMode: "auto",
        agentBaseUrl: " http://127.0.0.1:8787 ",
      }),
    );
    expect(s.agents["claude-code"]).toMatchObject({
      model: "claude-haiku-5-5",
      mode: "auto",
      baseUrl: "http://127.0.0.1:8787",
    });
  });

  it("drops unknown modes and effort levels and model names with spaces", () => {
    const s = parseSettings(
      JSON.stringify({ agents: { "claude-code": { mode: "bypassPermissions", effort: "huge", model: "two words" } } }),
    );
    expect(s.agents["claude-code"]).toMatchObject({ mode: "default", effort: "medium", model: "claude-sonnet-5-5" });
  });
});
