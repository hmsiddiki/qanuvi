import { describe, expect, it } from "vitest";
import { isLimit, limitResetAt } from "./limits";

describe("usage limits", () => {
  it("knows a limit message", () => {
    expect(isLimit("You've hit your session limit · resets 5:20am (Asia/Dhaka)")).toBe(true);
    expect(isLimit("Tests failed.")).toBe(false);
  });

  it("finds the reset time in the zone given", () => {
    // 03:00 in Dhaka (UTC+6) is 21:00 UTC the day before.
    const now = new Date("2026-11-11T21:00:00Z");
    const at = limitResetAt("You've hit your session limit · resets 5:20am (Asia/Dhaka)", now)!;
    expect((at.getTime() - now.getTime()) / 60_000).toBe(141); // 2 h 20 min, plus a minute
    const pm = limitResetAt("hit your weekly limit · resets 11pm (UTC)", new Date("2026-11-12T22:30:00Z"))!;
    expect((pm.getTime() - new Date("2026-11-12T22:30:00Z").getTime()) / 60_000).toBe(31);
    expect(limitResetAt("no limit here")).toBeNull();
    const unknown = limitResetAt("You've hit your usage limit", now)!;
    expect((unknown.getTime() - now.getTime()) / 60_000).toBe(60);
  });
});
