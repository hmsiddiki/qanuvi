import { describe, expect, it } from "vitest";
import { keepVerb, workText } from "./thisPcWork";

describe("what This PC's master would stop, in words (M5a)", () => {
  it("names agent sessions and plan runs, or nothing", () => {
    expect(workText({ agents: 0, runs: 0 })).toBeNull();
    expect(workText({ agents: 1, runs: 0 })).toBe("1 agent session");
    expect(workText({ agents: 2, runs: 1 })).toBe("2 agent sessions and 1 plan run");
    expect(workText({ agents: 0, runs: 3 })).toBe("3 plan runs");
    expect(keepVerb({ agents: 1, runs: 0 })).toBe("keeps");
    expect(keepVerb({ agents: 2, runs: 1 })).toBe("keep");
  });
});
