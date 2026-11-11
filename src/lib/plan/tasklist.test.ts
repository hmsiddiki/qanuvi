import { describe, expect, it } from "vitest";
import {
  findTaskLists,
  nextStatus,
  parsePlan,
  setStepModel,
  setStepStatus,
  setStepTitle,
  setStepSkipped,
} from "./tasklist";

const SAMPLE = [
  "# Work map",
  "",
  "## Stage 1",
  "",
  "- [x] **1. Project setup and an empty window**",
  "  - Done on Windows.",
  "    More detail.",
  "",
  "- [-] **2. Layout: sidebar and panes** (built 2026-11-10, waiting for the owner's",
  "  review)",
  "  - Three panes.",
  "  - Model: claude-opus-5-5",
  "",
  "- [ ] **1b. Updates** (moved later)",
  "",
  "## Stage 2",
  "",
  "- [!] Plain step without bold",
  "- [ ] **15b. Git like VS Code** (added by the owner)",
  "  - Decorations.",
  "",
  "Some paragraph.",
].join("\n");

describe("parsePlan", () => {
  const plan = parsePlan(SAMPLE);

  it("finds sections and steps with their status, number, title and note", () => {
    expect(plan.steps.map((s) => [s.status, s.number, s.title])).toEqual([
      ["x", "1", "Project setup and an empty window"],
      ["-", "2", "Layout: sidebar and panes"],
      [" ", "1b", "Updates"],
      ["!", null, "Plain step without bold"],
      [" ", "15b", "Git like VS Code"],
    ]);
    expect(plan.sections.map((s) => [s.title, s.steps.length])).toEqual([
      ["Work map", 0],
      ["Stage 1", 3],
      ["Stage 2", 2],
    ]);
  });

  it("joins a note that continues on the next line", () => {
    expect(plan.steps[1].note).toBe("(built 2026-11-10, waiting for the owner's review)");
  });

  it("keeps details and reads the model line", () => {
    expect(plan.steps[0].details).toEqual(["- Done on Windows.", "  More detail."]);
    expect(plan.steps[1].details).toEqual(["- Three panes."]);
    expect(plan.steps[1].model).toBe("claude-opus-5-5");
    expect(plan.steps[0].model).toBeNull();
  });

  it("ends a step at an unindented paragraph", () => {
    expect(plan.steps[4].details).toEqual(["- Decorations."]);
  });

  it("parses a Windows file with CRLF line endings", () => {
    const crlf = parsePlan(SAMPLE.replace(/\n/g, "\r\n"));
    expect(crlf.eol).toBe("\r\n");
    expect(crlf.steps).toHaveLength(5);
  });
});

describe("editing", () => {
  it("changes only the status character of one line", () => {
    const out = setStepStatus(SAMPLE, 13, "x");
    expect(out.split("\n")[13]).toBe("- [x] **1b. Updates** (moved later)");
    const changed = out.split("\n").filter((l, i) => l !== SAMPLE.split("\n")[i]);
    expect(changed).toHaveLength(1);
  });

  it("keeps CRLF line endings", () => {
    const crlf = SAMPLE.replace(/\n/g, "\r\n");
    const out = setStepStatus(crlf, 4, " ");
    expect(out).toBe(crlf.replace("- [x] **1. Project", "- [ ] **1. Project"));
  });

  it("refuses to edit when the file no longer matches what was shown", () => {
    expect(() => setStepStatus(SAMPLE, 5, "x")).toThrow(/changed/);
    expect(() => setStepStatus(SAMPLE, 4, "x", "Another title")).toThrow(/changed/);
  });

  it("renames a step and keeps its number and note", () => {
    const out = setStepTitle(SAMPLE, 13, "Updates and **channels**");
    expect(out.split("\n")[13]).toBe("- [ ] **1b. Updates and channels** (moved later)");
    expect(setStepTitle(SAMPLE, 17, "Plain renamed").split("\n")[17]).toBe("- [!] Plain renamed");
    expect(() => setStepTitle(SAMPLE, 13, "  ")).toThrow();
  });

  it("adds, changes and removes the model line", () => {
    const added = setStepModel(SAMPLE, 13, "claude-haiku-5-5");
    expect(added.split("\n").slice(13, 15)).toEqual([
      "- [ ] **1b. Updates** (moved later)",
      "  - Model: claude-haiku-5-5",
    ]);
    expect(parsePlan(added).steps[2].model).toBe("claude-haiku-5-5");

    // A step with a continued note gets the model line after the continuation.
    const second = setStepModel(SAMPLE, 4, "claude-sonnet-5-5");
    expect(second.split("\n")[5]).toBe("  - Model: claude-sonnet-5-5");

    const changed = setStepModel(SAMPLE, 8, "claude-fable-5-2");
    expect(changed.split("\n")[11]).toBe("  - Model: claude-fable-5-2");
    const removed = setStepModel(SAMPLE, 8, null);
    expect(removed.split("\n")).toHaveLength(SAMPLE.split("\n").length - 1);
    expect(parsePlan(removed).steps[1].model).toBeNull();
    expect(() => setStepModel(SAMPLE, 8, "two words")).toThrow();
  });

  it("cycles through the statuses", () => {
    expect(nextStatus(" ")).toBe("-");
    expect(nextStatus("-")).toBe("x");
    expect(nextStatus("x")).toBe("!");
    expect(nextStatus("!")).toBe(" ");
  });
});

describe("findTaskLists", () => {
  it("finds likely task lists, top-level first", () => {
    expect(
      findTaskLists([
        "docs/ROADMAP.md",
        "README.md",
        "DEVELOPMENT-TASKLIST.md",
        "node_modules/x/TODO.md",
        "notes/todo-list.md",
        "QLOPY-CORE-UPGRADE-PLAN.md",
        "content/plugins/shop/PLAN-D.md",
        "uploads/updates/backups/2026/content/plugins/shop/DEVELOPMENT-TASKLIST.md",
        "vendor/lib/TODO.md",
      ]),
    ).toEqual([
      "DEVELOPMENT-TASKLIST.md",
      "QLOPY-CORE-UPGRADE-PLAN.md",
      "docs/ROADMAP.md",
      "notes/todo-list.md",
      "content/plugins/shop/PLAN-D.md",
    ]);
  });
});

describe("needs and skipped lines (24f)", () => {
  const text = [
    "## Steps",
    "- [ ] **1. First**",
    "- [ ] **2. Second**",
    "  - do it",
    "  - Needs: 1, docs/OTHER.md#4",
    "- [ ] **3. Third**",
    "",
  ].join("\n");

  it("reads what a step needs", () => {
    const steps = parsePlan(text).steps;
    expect(steps[1].needs).toEqual(["1", "docs/OTHER.md#4"]);
    expect(steps[0].needs).toEqual([]);
  });

  it("writes, replaces and removes the skipped line, leaving the rest", () => {
    const once = setStepSkipped(text, 2, "needs step 1, which is not done", "Second");
    expect(once).toContain(
      "  - Needs: 1, docs/OTHER.md#4\n  - Skipped: needs step 1, which is not done\n- [ ] **3. Third**",
    );
    const twice = setStepSkipped(once, 2, "blocked", "Second");
    expect(twice.match(/Skipped:/g)).toHaveLength(1);
    expect(setStepSkipped(twice, 2, null, "Second")).toBe(text);
  });
});

import { clearStaleSkips } from "./tasklist";

describe("clearStaleSkips", () => {
  it("removes skip lines whose needed steps are done, and keeps the rest", () => {
    const text = [
      "- [x] **1. One**",
      "- [ ] **2. Two**",
      "  - Needs: 1",
      "  - Skipped: needs step 1, which is not done",
      "- [ ] **3. Three**",
      "  - Needs: 2",
      "  - Skipped: needs step 2, which is not done",
      "",
    ].join("\n");
    const out = clearStaleSkips(text);
    expect(out).not.toContain("needs step 1");
    expect(out).toContain("needs step 2");
    expect(clearStaleSkips(out)).toBe(out);
  });
});
