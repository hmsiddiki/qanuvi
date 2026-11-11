import { describe, expect, it } from "vitest";
import { ago } from "./git.svelte";

describe("ago", () => {
  it("says how long ago, in the biggest whole unit", () => {
    const now = 1_700_000_000_000;
    const s = now / 1000;
    expect(ago(s - 20, now)).toBe("just now");
    expect(ago(s - 120, now)).toBe("2 minutes ago");
    expect(ago(s - 3600, now)).toBe("1 hour ago");
    expect(ago(s - 3 * 86400, now)).toBe("3 days ago");
    expect(ago(s - 400 * 86400, now)).toBe("1 year ago");
  });
});
