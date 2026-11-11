import { describe, expect, it } from "vitest";
import { matchShortcut, shortcutLabel } from "./shortcuts";

const press = (key: string, mods: Partial<{ ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key,
  ctrlKey: !!mods.ctrl,
  metaKey: !!mods.meta,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe("matchShortcut", () => {
  it("uses Ctrl on Windows and Linux", () => {
    expect(matchShortcut(press("b", { ctrl: true }), false)).toBe("toggleSidebar");
    expect(matchShortcut(press("b", { meta: true }), false)).toBeNull();
  });

  it("uses Cmd on macOS", () => {
    expect(matchShortcut(press("b", { meta: true }), true)).toBe("toggleSidebar");
    expect(matchShortcut(press("b", { ctrl: true }), true)).toBeNull();
  });

  it("tells shift and alt variants apart and ignores letter case", () => {
    expect(matchShortcut(press("j", { ctrl: true }), false)).toBe("toggleTerminal");
    expect(matchShortcut(press("J", { ctrl: true, shift: true }), false)).toBe("toggleTerminalMaximized");
    expect(matchShortcut(press("a", { ctrl: true, alt: true }), false)).toBe("toggleAgent");
    expect(matchShortcut(press("a", { ctrl: true }), false)).toBeNull();
  });
});

describe("shortcutLabel", () => {
  it("writes shortcuts for each system", () => {
    expect(shortcutLabel("toggleTerminalMaximized", false)).toBe("Ctrl+Shift+J");
    expect(shortcutLabel("toggleAgent", true)).toBe("Cmd+Option+A");
  });
});
