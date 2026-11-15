import { describe, expect, it } from "vitest";
import { DEFAULT_NOTIFICATIONS, parseNotifications, parseSettings } from "./settings.svelte";

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

describe("appearance (15h)", () => {
  it("keeps sizes within what stays readable", () => {
    expect(parseSettings(null).appearance).toEqual({ editorFontSize: 13, uiScale: 100, minimap: false });
    expect(parseSettings(JSON.stringify({ appearance: { editorFontSize: 99, uiScale: 10 } })).appearance).toEqual({
      editorFontSize: 24,
      uiScale: 80,
      minimap: false,
    });
  });

  it("keeps the minimap off unless switched on (15i part 4)", () => {
    expect(parseSettings(JSON.stringify({ appearance: { minimap: true } })).appearance.minimap).toBe(true);
    expect(parseSettings(JSON.stringify({ appearance: { minimap: "yes" } })).appearance.minimap).toBe(false);
  });
});

describe("notification settings (24u part 4, session B)", () => {
  it("defaults when missing and keeps good values", () => {
    expect(parseSettings(null).notifications).toEqual(DEFAULT_NOTIFICATIONS);
    const kept = { desktop: false, what: "all", bundle: false, quiet: true, quietFrom: 60, quietTo: 420 };
    expect(parseNotifications(kept)).toEqual(kept);
  });

  it("falls back on odd values", () => {
    const n = parseNotifications({ desktop: "yes", what: "loud", quiet: 1, quietFrom: 5000, quietTo: -3 });
    expect(n).toEqual({ ...DEFAULT_NOTIFICATIONS, quietFrom: 1439, quietTo: 0 });
  });
});

describe("parseSettings: start-up list", () => {
  const item = (id: string, enabled: boolean) => ({ id, name: id, command: "node x.js", waitFor: "", enabled });

  it("gives every parse its own list, never growing or shared", () => {
    const json = JSON.stringify({ startup: [item("a", true)] });
    const first = parseSettings(json);
    const second = parseSettings(json);
    expect(first.startup).toHaveLength(1);
    expect(second.startup).toHaveLength(1);
    expect(second.startup).not.toBe(first.startup);
    // Another machine's settings with no list never show the one parsed before.
    expect(parseSettings(JSON.stringify({})).startup).toEqual([]);
    expect(parseSettings(null).startup).toEqual([]);
  });

  it("keeps one entry per item when it is turned off", () => {
    const on = parseSettings(JSON.stringify({ startup: [item("a", true)] }));
    const off = parseSettings(JSON.stringify({ ...on, startup: on.startup.map((i) => ({ ...i, enabled: false })) }));
    expect(off.startup).toEqual([{ ...item("a", false) }]);
  });
});
