import { describe, expect, it } from "vitest";
import { listToSteps } from "./liststeps";
import { parsePlan } from "./tasklist";

describe("f1: a plain list becomes steps", () => {
  it("turns a numbered list into numbered steps the parser reads (driven live)", () => {
    const text = [
      "# Plan: notes",
      "",
      "1. Add `removeNote(id)` to src/notes.js.",
      "2. Add a Reports module. Every optional module is off by default,",
      "   but Reports should be on by default.",
      "3) Update the README",
      "",
    ].join("\r\n");
    const r = listToSteps(text)!;
    expect(r.count).toBe(3);
    expect(r.text.split("\r\n")).toEqual([
      "# Plan: notes",
      "",
      "- [ ] 1. Add `removeNote(id)` to src/notes.js.",
      "- [ ] 2. Add a Reports module. Every optional module is off by default,",
      "   but Reports should be on by default.",
      "- [ ] 3. Update the README",
      "",
    ]);
    const steps = parsePlan(r.text).sections.flatMap((s) => s.steps);
    expect(steps.map((s) => s.number)).toEqual(["1", "2", "3"]);
    expect(steps.every((s) => s.status === " ")).toBe(true);
  });

  it("uses bullets when nothing is numbered, and keeps a lazy wrapped line with its item", () => {
    const r = listToSteps("- First thing\nsecond line of it\n- Second thing\n\nA closing note.\n")!;
    expect(r.count).toBe(2);
    expect(r.text).toBe("- [ ] First thing\n  second line of it\n- [ ] Second thing\n\nA closing note.\n");
  });

  it("leaves code blocks, nested items and a plan that already has steps alone", () => {
    const r = listToSteps("1. Run it\n   - detail\n```\n1. not a step\n```\n")!;
    expect(r.text).toBe("- [ ] 1. Run it\n   - detail\n```\n1. not a step\n```\n");
    expect(listToSteps("- [ ] 1. Done already\n2. Other\n")).toBeNull();
    expect(listToSteps("# Only words\n\nNo list here.\n")).toBeNull();
  });
});
