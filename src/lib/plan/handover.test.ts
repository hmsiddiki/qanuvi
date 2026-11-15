import { describe, expect, it } from "vitest";
import { contextSize } from "./guidance";

describe("a long step hands over to a new session", () => {
  it("reads the context size from one streamed message", () => {
    const line = JSON.stringify({
      type: "assistant",
      message: { usage: { input_tokens: 10, cache_read_input_tokens: 180_000, cache_creation_input_tokens: 25_000 } },
    });
    expect(contextSize([line])).toBe(205_010);
  });
});
