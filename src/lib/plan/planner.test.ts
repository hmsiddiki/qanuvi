import { describe, expect, it } from "vitest";
import { CLAUDE_CODE } from "$lib/agents/providers";
import { parsePlan } from "./tasklist";
import {
  appendSection,
  extractSteps,
  lastNumber,
  plannerPrompt,
  renumber,
  sectionTitleFrom,
  takeModels,
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

describe("prompts", () => {
  it("asks for a read-only plan in the task-list format, with the models to choose from", () => {
    const p = plannerPrompt("Add VAT to the cart", "TASKLIST.md", CLAUDE_CODE.models);
    expect(p).toContain("Add VAT to the cart");
    expect(p).toContain("```tasklist");
    expect(p).toContain("claude-haiku-5-5");
    expect(p).toContain("Do not change any file");
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

describe("finalText (PLAN-F 97a-2: a review reason that was only a proxy placeholder)", async () => {
  const { assistantText, finalText } = await import("./planner");
  it("takes the agent's last text when the result holds a placeholder", () => {
    const said = assistantText({
      type: "assistant",
      message: { content: [{ type: "text", text: '{"ok": false, "why": "the diff is empty"}' }, { type: "tool_use" }] },
    });
    expect(said).toBe('{"ok": false, "why": "the diff is empty"}');
    expect(finalText('{"ok":false,"why":"<<ccr:d244e686c105,string,349B>>"}', said)).toBe(said);
  });
  it("keeps a plain result, and a placeholder when nothing else was said", () => {
    expect(finalText('{"ok":true}', "other")).toBe('{"ok":true}');
    expect(finalText("<<ccr:a,string,1B>>", "")).toBe("<<ccr:a,string,1B>>");
    expect(assistantText({ type: "user", message: { content: [{ type: "text", text: "x" }] } })).toBe("");
  });
});

describe("takeModels (R2e i)", () => {
  it("takes the planner's Model lines out of the block and keeps them per step", () => {
    const block = [
      "- [ ] **12. Add VAT**",
      "  - Validate: tests pass.",
      "  - Model: claude-sonnet-5-5",
      "- [ ] **13. Docs**",
      "  - model: claude-haiku-5-5",
      "- [ ] **14. No model**",
      "  - Do it.",
    ].join("\n");
    const r = takeModels(block);
    expect(r.block).not.toMatch(/model:/i);
    expect(r.block).toContain("  - Validate: tests pass.\n- [ ] **13. Docs**\n- [ ] **14. No model**\n  - Do it.");
    expect(r.models).toEqual([
      { number: "12", title: "Add VAT", model: "claude-sonnet-5-5" },
      { number: "13", title: "Docs", model: "claude-haiku-5-5" },
    ]);
    expect(takeModels("- [ ] **1. A**\n  - x").models).toEqual([]);
  });
});
