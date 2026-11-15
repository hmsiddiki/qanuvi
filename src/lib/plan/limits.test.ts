import { describe, expect, it } from "vitest";
import { isLimit } from "./limits";

describe("usage limits", () => {
  it("knows a limit message", () => {
    expect(isLimit("You've hit your session limit · resets 5:20am (Asia/Dhaka)")).toBe(true);
    expect(isLimit("Tests failed.")).toBe(false);
  });
});
