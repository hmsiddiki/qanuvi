import { describe, expect, it } from "vitest";
import {
  DEFAULT_LAYOUT,
  LIMITS,
  MAX_NAMED_LAYOUTS,
  PRESETS,
  cleanLayoutName,
  clamp,
  normalizeLayout,
  parseSavedLayouts,
  sidebarScreenWidth,
} from "./layout";

describe("clamp", () => {
  it("keeps values inside the limits and rounds them", () => {
    expect(clamp(50, 100, 200)).toBe(100);
    expect(clamp(250, 100, 200)).toBe(200);
    expect(clamp(150.6, 100, 200)).toBe(151);
    expect(clamp(Number.NaN, 100, 200)).toBe(100);
  });
});

describe("normalizeLayout", () => {
  it("fills in defaults for anything missing or of the wrong type", () => {
    expect(normalizeLayout(undefined)).toEqual(DEFAULT_LAYOUT);
    expect(normalizeLayout({ sidebarWidth: "wide", agentVisible: "yes" })).toEqual(DEFAULT_LAYOUT);
  });

  it("clamps sizes to the limits", () => {
    const state = normalizeLayout({ sidebarWidth: 10, agentWidth: 5000, terminalHeight: 1 });
    expect(state.sidebarWidth).toBe(LIMITS.sidebar.min);
    expect(state.agentWidth).toBe(LIMITS.agent.max);
    expect(state.terminalHeight).toBe(LIMITS.terminal.min);
  });

  it("never leaves the window empty", () => {
    const state = normalizeLayout({ agentVisible: false, workVisible: false });
    expect(state.agentVisible).toBe(true);
  });

  it("drops a maximised terminal when the terminal is hidden", () => {
    expect(normalizeLayout({ terminalVisible: false, terminalMaximized: true }).terminalMaximized).toBe(false);
  });
});

describe("cleanLayoutName", () => {
  it("trims, joins spaces and limits the length", () => {
    expect(cleanLayoutName("  My   layout ")).toBe("My layout");
    expect(cleanLayoutName("   ")).toBe("");
    expect(cleanLayoutName("x".repeat(100))).toHaveLength(40);
  });
});

describe("parseSavedLayouts", () => {
  it("starts with the presets and the Qanuvi layout when nothing is saved", () => {
    const saved = parseSavedLayouts(null);
    expect(Object.keys(saved.named)).toEqual(Object.keys(PRESETS));
    expect(saved.active).toBe("Qanuvi");
    expect(saved.current).toEqual(PRESETS.Qanuvi);
  });

  it("survives broken JSON", () => {
    expect(parseSavedLayouts("{not json").active).toBe("Qanuvi");
  });

  it("has the VS Code-like preset: files on the left, the conversation on the right", () => {
    expect(PRESETS["VS Code-like"]).toMatchObject({ agentSide: "right", sidebarMode: "explorer" });
    expect(normalizeLayout({ agentSide: "middle", sidebarMode: "nowhere" })).toMatchObject({
      agentSide: "left",
      sidebarMode: "workspace",
    });
  });

  it("keeps the user's layouts and current state, cleaned", () => {
    const saved = parseSavedLayouts(
      JSON.stringify({
        current: { sidebarWidth: 300, workVisible: false },
        named: { "  Night  ": { terminalVisible: false }, "": {} },
        active: "Night",
      }),
    );
    expect(saved.named.Night.terminalVisible).toBe(false);
    expect(saved.named[""]).toBeUndefined();
    expect(saved.active).toBe("Night");
    expect(saved.current.sidebarWidth).toBe(300);
    expect(saved.current.workVisible).toBe(false);
  });

  it("brings back a removed preset and falls back from an unknown active name", () => {
    const saved = parseSavedLayouts(JSON.stringify({ named: {}, active: "Gone" }));
    expect(saved.named.Review).toBeDefined();
    expect(saved.active).toBe("Qanuvi");
  });

  it("limits the number of named layouts", () => {
    const named: Record<string, object> = {};
    for (let i = 0; i < 50; i++) named[`L${i}`] = {};
    expect(Object.keys(parseSavedLayouts(JSON.stringify({ named })).named)).toHaveLength(MAX_NAMED_LAYOUTS);
  });
});

describe("sidebarScreenWidth", () => {
  it("is zero when hidden, the icon width when collapsed, the set width otherwise", () => {
    expect(sidebarScreenWidth({ ...DEFAULT_LAYOUT, sidebarVisible: false })).toBe(0);
    expect(sidebarScreenWidth({ ...DEFAULT_LAYOUT, sidebarCollapsed: true })).toBe(LIMITS.sidebarCollapsedWidth);
    expect(sidebarScreenWidth({ ...DEFAULT_LAYOUT, sidebarWidth: 250 })).toBe(250);
  });
});
