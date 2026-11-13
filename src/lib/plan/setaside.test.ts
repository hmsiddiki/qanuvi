import { describe, expect, it } from "vitest";
import {
  closingRerunNote,
  dependencePrompt,
  isClosingStep,
  mentionedFiles,
  parseDependence,
  planDependence,
  setAsideBlock,
  waitingLine,
} from "./setaside";

describe("set aside (24u part 1)", () => {
  it("knows a section's closing step", () => {
    expect(isClosingStep("95z")).toBe(true);
    expect(isClosingStep("95b")).toBe(false);
    expect(isClosingStep(null)).toBe(false);
  });

  it("finds file names, leaving plan notes out", () => {
    expect(mentionedFiles("Change includes\\Checkout.php and src/a.ts; see MAP.md")).toEqual([
      "includes/checkout.php",
      "src/a.ts",
    ]);
  });

  it("reads dependence from the plan: Needs, same number, shared files", () => {
    const held = { number: "95b", title: "Refunds", details: ["edit includes/refund.php"] };
    expect(planDependence(held, { number: "72a", title: "x", needs: ["95b"] })).toMatch(/Needs/);
    expect(planDependence(held, { number: "95c", title: "x" })).toMatch(/same step number/);
    expect(planDependence(held, { number: "72a", title: "Log refund.php errors" })).toMatch(/refund\.php/);
    expect(planDependence(held, { number: "72a", title: "Docs only" })).toBeNull();
    expect(planDependence({ ...held, details: [], changed: ["src/x.ts"] }, { number: "1", title: "fix x.ts" })).toMatch(
      /x\.ts/,
    );
    // Different numbers, no shared files: the plan does not say.
    expect(planDependence({ number: "95a", title: "a" }, { number: "96a", title: "b" })).toBeNull();
  });

  it("asks the decider once and reads only known steps from the answer", () => {
    const p = dependencePrompt(
      [{ number: "95b", title: "Refunds", why: "a question" }],
      [{ number: "72a", title: "Mail" }],
    );
    expect(p).toContain("95b. Refunds (a question)");
    expect(p).toContain("72a. Mail");
    const m = parseDependence(
      'ok {"needs":[{"step":"72a","on":"95b","why":"mails the refund"},{"step":"99","on":"95b"},{"step":"72a","on":"1"}]}',
      ["72a"],
      ["95b"],
    );
    expect([...m]).toEqual([["72a", { on: "95b", why: "mails the refund" }]]);
    expect(parseDependence("no", ["72a"], ["95b"]).size).toBe(0);
  });

  it("words the wait and the closing step's note", () => {
    expect(waitingLine(1)).toMatch(/^Waiting for you \(1 answer\)/);
    expect(waitingLine(3)).toMatch(/3 answers/);
    expect(setAsideBlock([])).toBe("");
    expect(setAsideBlock([{ label: "95b", why: "waits for you" }])).toContain("95b (waits for you)");
  });

  it("tells a closing step that runs again which steps came in late", () => {
    expect(closingRerunNote(["1a"])).toContain("step 1a was set aside; it is done now");
    expect(closingRerunNote(["1a", "1b"])).toContain("steps 1a, 1b were set aside; they are done now");
  });
});
