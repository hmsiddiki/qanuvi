import { describe, expect, it } from "vitest";
import { connectedText } from "./remoteAccess.svelte";

describe("relay road (24rl R3)", () => {
  it("says which road the link took, and nothing extra for older masters", () => {
    expect(connectedText({ road: "own port" })).toBe("Connected to the relay (its own port)");
    expect(connectedText({ road: "443" })).toBe("Connected to the relay (through port 443)");
    expect(connectedText({})).toBe("Connected to the relay");
  });
});
