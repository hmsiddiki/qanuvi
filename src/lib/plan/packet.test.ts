import { describe, expect, it } from "vitest";
import {
  contextBlock,
  excerptAround,
  failureSummary,
  fixPrompt,
  locationBlock,
  mentions,
  packetPrompt,
} from "./packet";

describe("mentions", () => {
  it("finds files and code names in a step", () => {
    const m = mentions(
      "**3. Add a discount** to `src/price.js`: make `applyDiscount(price, percent)` use roundMoney and keep MAX_PERCENT. Validate: tests in test/price.test.js pass.",
    );
    expect(m.paths).toEqual(["src/price.js", "test/price.test.js"]);
    expect(m.names).toEqual(["applyDiscount", "roundMoney", "MAX_PERCENT"]);
  });

  it("ignores plain words and repeats", () => {
    const m = mentions("Sort the list. Model: claude-sonnet-5-5. Validate: the items are sorted. `Done`");
    expect(m.names).toEqual([]);
    expect(mentions("`CartService::addMany` and CartService again").names).toEqual(["addMany", "CartService"]);
  });
});

describe("excerptAround", () => {
  it("takes a few lines before and the body after", () => {
    const text = Array.from({ length: 50 }, (_, i) => `line ${i + 1}`).join("\n");
    expect(excerptAround(text, 10, 2, 3)).toEqual({
      start: 8,
      text: "line 8\nline 9\nline 10\nline 11\nline 12\nline 13",
    });
    expect(excerptAround(text, 1, 2, 1).start).toBe(1);
    expect(excerptAround(text, 50, 2, 30).text.endsWith("line 50")).toBe(true);
  });
});

describe("contextBlock", () => {
  it("numbers the lines, names the reason, and stops at the size limit", () => {
    const block = contextBlock(
      [
        { path: "src/a.js", start: 4, text: "x\ny", reason: "definition of total" },
        { path: "src/b.js", start: 1, text: "z".repeat(500), reason: "mentioned" },
      ],
      120,
    );
    expect(block).toContain("src/a.js (definition of total):\n   4  x\n   5  y");
    expect(block).not.toContain("src/b.js");
  });
});

describe("failureSummary", () => {
  it("keeps failure lines with context and the end, drops passing noise and colours", () => {
    const out = [
      "\x1b[32m✔ adds\x1b[0m",
      ...Array.from({ length: 30 }, (_, i) => `ok ${i}`),
      "✖ applies a discount",
      "  AssertionError: expected 90 to equal 81",
      "    at test/price.test.js:12:5",
      ...Array.from({ length: 30 }, (_, i) => `ok more ${i}`),
      "# tests 3",
      "# fail 1",
    ].join("\n");
    const s = failureSummary(out);
    expect(s).toContain("✖ applies a discount");
    expect(s).toContain("AssertionError: expected 90 to equal 81");
    expect(s).toContain("# fail 1");
    expect(s).not.toContain("ok 5");
    expect(s).not.toContain("\x1b[");
    expect(failureSummary("x".repeat(5000), 100).length).toBeLessThanOrEqual(102);
  });

  it("drops passing lines, zero counters and runner stack frames next to a failure", () => {
    const out = [
      "✔ rounds the total (0.1ms)",
      "✖ percent has a space before the sign (1.5ms)",
      "  AssertionError: Expected values to be strictly equal:",
      "  '12.5%' !== '12.5 %'",
      "      at TestContext.<anonymous> (file:///x/test/format.test.js:6:10)",
      "      at Test.runInAsyncScope (node:async_hooks:227:14)",
      "      at Test.run (node:internal/test_runner/test:1402:25)",
      "✔ formatPrice rounds (0.1ms)",
      "ℹ pass 12",
      "ℹ fail 1",
      "ℹ cancelled 0",
      "ℹ skipped 0",
    ].join("\n");
    const s = failureSummary(out);
    expect(s).toContain("✖ percent has a space before the sign");
    expect(s).toContain("'12.5%' !== '12.5 %'");
    expect(s).toContain("format.test.js:6:10");
    expect(s).toContain("ℹ fail 1");
    expect(s).not.toContain("✔");
    expect(s).not.toContain("node:internal");
    expect(s).not.toContain("cancelled");
  });
});

describe("prompts", () => {
  it("adds the code and the test rule only when there is something to add", () => {
    expect(packetPrompt("Do it.", "", "")).toBe("Do it.");
    const p = packetPrompt("Do it.", "src/a.js (x):\n   1  y", "npm test");
    expect(p).toContain("Where the code this step mentions is");
    expect(p).toContain("Qanuvi runs `npm test` itself");
    expect(fixPrompt("npm test", "boom", 1, 2)).toContain("fix round 1 of 2");
  });
});

describe("locationBlock", () => {
  it("says where things are in one line each, and which files are new", () => {
    const text = locationBlock(
      [{ name: "applyDiscount", kind: "function", path: "src/price.js", line: 6, container: null }],
      [{ path: "src/price.js", items: [{ name: "MAX_PERCENT", kind: "constant", line: 3, container: null }] }],
      ["src/cart.js"],
    );
    expect(text).toBe(
      "applyDiscount: function at src/price.js:6\nsrc/price.js: MAX_PERCENT (constant, line 3)\nsrc/cart.js: does not exist yet",
    );
    expect(locationBlock([], [], ["a.js", "b.js"], 30)).toBe("a.js: does not exist yet");
  });
});
