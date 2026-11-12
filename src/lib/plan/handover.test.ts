import { describe, expect, it } from "vitest";
import { HANDOVER_MAX, HANDOVER_RESUME, contextSize, handoverDue, handoverRequest } from "./guidance";

describe("a long step hands over to a new session", () => {
  it("hands over only past the limit, never when it is off, and not without end", () => {
    expect(handoverDue(150_000, 200_000, 0)).toBe(false);
    expect(handoverDue(200_000, 200_000, 0)).toBe(true);
    expect(handoverDue(900_000, 0, 0)).toBe(false);
    expect(handoverDue(300_000, 200_000, HANDOVER_MAX - 1)).toBe(true);
    expect(handoverDue(300_000, 200_000, HANDOVER_MAX)).toBe(false);
  });

  it("reads the context size from one streamed message", () => {
    const line = JSON.stringify({
      type: "assistant",
      message: { usage: { input_tokens: 10, cache_read_input_tokens: 180_000, cache_creation_input_tokens: 25_000 } },
    });
    expect(contextSize([line])).toBe(205_010);
  });

  it("asks for a finished edit and a note a new session can work from", () => {
    const ask = handoverRequest("PLAN-D/logs/83b.md", 212_345);
    expect(ask).toContain("212k");
    expect(ask).toContain("PLAN-D/logs/83b.md");
    expect(ask).toMatch(/half-written/);
    expect(ask).toMatch(/Do not mark the step done unless it is/);
    // The new session checks the earlier work before building on it.
    expect(HANDOVER_RESUME).toMatch(/check that what it calls done really is/);
  });
});
