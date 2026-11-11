import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { parsePlan } from "./tasklist";
import {
  appendSection,
  extractSteps,
  labelRange,
  lastNumber,
  plannerPrompt,
  renumber,
  replaceStep,
  replanPrompt,
  sectionTitleFrom,
} from "./planner";

const ANSWER = [
  "I read src/cart.js and src/tax.js. Here is the plan:",
  "",
  "```tasklist",
  "## Plan",
  "- [ ] **1. Add VAT to the cart total**",
  "  - Give `cartTotal(items, { vat })` an optional VAT using `addVat` from src/tax.js.",
  "  - Validate: new tests in test/cart.test.js pass.",
  "  - Model: claude-sonnet-5-5",
  "",
  "- [x] **2. Document it**",
  "  - Add a README line.",
  "  - Model: claude-haiku-5-5",
  "```",
  "",
  "Let me know if you want changes.",
].join("\n");

describe("extractSteps", () => {
  it("takes only the steps from the answer, all open", () => {
    const { block, steps } = extractSteps(ANSWER);
    expect(steps.map((s) => [s.number, s.title, s.model])).toEqual([
      ["1", "Add VAT to the cart total", "claude-sonnet-5-5"],
      ["2", "Document it", "claude-haiku-5-5"],
    ]);
    expect(block).not.toContain("## Plan");
    expect(block).not.toContain("- [x]");
  });

  it("finds no steps in an answer without any", () => {
    expect(extractSteps("I could not plan this.").steps).toEqual([]);
  });
});

describe("numbering", () => {
  const { block } = extractSteps(ANSWER);

  it("continues after the last step number in the file", () => {
    expect(lastNumber("- [x] **3. A**\n- [ ] **12b. B**\n- [ ] C")).toBe(12);
    expect(lastNumber("# empty")).toBe(0);
    expect(parsePlan(renumber(block, 13)).steps.map((s) => s.number)).toEqual(["13", "14"]);
  });

  it("numbers a replacement as 12a, 12b and names the range", () => {
    const sub = renumber(block, "12");
    expect(parsePlan(sub).steps.map((s) => s.number)).toEqual(["12a", "12b"]);
    expect(labelRange(sub)).toBe("12a–12b");
  });

  it("gives an unnumbered step a number", () => {
    expect(renumber("- [ ] Plain step", 5)).toBe("- [ ] **5. Plain step**");
  });
});

describe("appendSection", () => {
  it("adds a section at the end and keeps CRLF line endings", () => {
    const file = "# Plan\r\n\r\n- [x] **1. Old**\r\n";
    const out = appendSection(file, "VAT in the cart", "- [ ] **2. New**\n  - detail");
    expect(out).toBe(
      "# Plan\r\n\r\n- [x] **1. Old**\r\n\r\n## VAT in the cart\r\n\r\n- [ ] **2. New**\r\n  - detail\r\n",
    );
    expect(appendSection("", "", "- [ ] **1. X**")).toBe("## Planned work\n\n- [ ] **1. X**\n");
  });
});

describe("replaceStep", () => {
  it("blocks the failed step with a note and inserts the new steps after its details", () => {
    const file = ["- [ ] **12. Big step**", "  - Do everything.", "- [ ] **13. Next**"].join("\n");
    const sub = renumber("- [ ] **1. Part one**\n- [ ] **2. Part two**", "12");
    const out = replaceStep(file, { number: "12", title: "Big step" }, sub, labelRange(sub));
    expect(out.split("\n")).toEqual([
      "- [!] **12. Big step**",
      "  - Do everything.",
      "  - Replaced by steps 12a–12b, rewritten by the planner after it failed on every model.",
      "- [ ] **12a. Part one**",
      "- [ ] **12b. Part two**",
      "- [ ] **13. Next**",
    ]);
    expect(() => replaceStep(file, { number: "99", title: "Gone" }, sub, "")).toThrow();
  });
});

describe("prompts", () => {
  it("asks for a read-only plan in the task-list format, with the models to choose from", () => {
    const p = plannerPrompt("Add VAT to the cart", "TASKLIST.md", CLAUDE_CODE.models);
    expect(p).toContain("Add VAT to the cart");
    expect(p).toContain("```tasklist");
    expect(p).toContain("claude-haiku-5-5");
    expect(p).toContain("Do not change any file");
    const step = parsePlan("- [ ] **4. Sort**\n  - Sort it.").steps[0];
    const r = replanPrompt("TASKLIST.md", step, "Tests kept failing: x !== y", CLAUDE_CODE.models);
    expect(r).toContain("**4. Sort**");
    expect(r).toContain("Tests kept failing");
  });
});

describe("sectionTitleFrom", () => {
  it("uses the first sentence, cut at a word boundary", () => {
    expect(sectionTitleFrom("Add coupons. Then test them.")).toBe("Add coupons");
    expect(sectionTitleFrom("Add optional VAT to the cart total using addVat from src/tax.js, with tests.")).toBe(
      "Add optional VAT to the cart total using addVat from…",
    );
    expect(sectionTitleFrom("  ")).toBe("Planned work");
  });
});
