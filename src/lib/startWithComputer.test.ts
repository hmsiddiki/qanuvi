import { describe, expect, it } from "vitest";
import { offerStartWithComputer, startSettingLabel } from "./startWithComputer";

describe("start with the computer (M5b)", () => {
  it("names the setting by the system, never as a sign-in", () => {
    expect(startSettingLabel("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/140")).toBe(
      "Start Qanuvi when Windows starts",
    );
    expect(startSettingLabel("Mozilla/5.0 (X11; Linux x86_64)")).toBe("Start Qanuvi when this computer starts");
    expect(startSettingLabel("Mozilla/5.0 (Macintosh)")).not.toMatch(/sign in/i);
  });

  it("offers it only for a run a shutdown cut off, while it is off and not turned away", () => {
    expect(offerStartWithComputer("shutdown", false, false)).toBe(true);
    expect(offerStartWithComputer("shutdown", true, false)).toBe(false);
    expect(offerStartWithComputer("shutdown", null, false)).toBe(false);
    expect(offerStartWithComputer("shutdown", false, true)).toBe(false);
    expect(offerStartWithComputer("crash", false, false)).toBe(false);
    expect(offerStartWithComputer(undefined, false, false)).toBe(false);
  });
});
