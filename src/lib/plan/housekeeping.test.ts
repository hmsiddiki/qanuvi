import { describe, expect, it } from "vitest";
import { archiveFinished, archivePath, doneNumbers, isArchive, mergePlans, stepNumbers } from "./housekeeping";
import { findTaskLists, parsePlan } from "./tasklist";

const PLAN = [
  "# Work map",
  "",
  "## Stage 1",
  "",
  "- [x] **1. Setup**",
  "  - Done.",
  "- [x] **2. Layout**",
  "",
  "## Stage 2",
  "",
  "Some words about stage 2.",
  "",
  "- [x] **3. Editor**",
  "- [ ] **4. Search**",
  "  - Needs: 1, 3",
  "",
  "## Stage 3",
  "",
  "- [x] **5. Done too**",
  "",
].join("\n");

describe("archivePath", () => {
  it("puts the archive next to the plan", () => {
    expect(archivePath("docs/PLAN.md")).toBe("docs/PLAN.archive.md");
    expect(isArchive("docs/PLAN.archive.md")).toBe(true);
    expect(isArchive("docs/PLAN.md")).toBe(false);
  });

  it("is never offered as a task list", () => {
    expect(findTaskLists(["PLAN.md", "PLAN.archive.md"])).toEqual(["PLAN.md"]);
  });
});

describe("archiveFinished", () => {
  it("moves only sections whose steps are all done", () => {
    const r = archiveFinished(PLAN, null, "docs/PLAN.md")!;
    expect(r.sections).toEqual([
      { title: "Stage 1", steps: 2 },
      { title: "Stage 3", steps: 1 },
    ]);
    expect(r.plan).toBe(
      [
        "# Work map",
        "",
        "## Stage 2",
        "",
        "Some words about stage 2.",
        "",
        "- [x] **3. Editor**",
        "- [ ] **4. Search**",
        "  - Needs: 1, 3",
        "",
      ].join("\n"),
    );
    expect(r.archive.startsWith("# Archive of PLAN.md\n\nFinished sections moved here from docs/PLAN.md.")).toBe(true);
    expect(r.archive).toContain(
      "## Stage 1\n\n- [x] **1. Setup**\n  - Done.\n- [x] **2. Layout**\n\n## Stage 3\n\n- [x] **5. Done too**\n",
    );
    expect(r.archive.endsWith("\n")).toBe(true);
  });

  it("appends to an archive that is there", () => {
    const r = archiveFinished(PLAN, "# Archive of PLAN.md\n\n## Old\n\n- [x] **0. Old**\n", "PLAN.md")!;
    expect(r.archive.startsWith("# Archive of PLAN.md\n\n## Old\n\n- [x] **0. Old**\n\n## Stage 1")).toBe(true);
  });

  it("keeps Windows line endings", () => {
    const r = archiveFinished(PLAN.replace(/\n/g, "\r\n"), null, "PLAN.md")!;
    expect(r.plan).not.toMatch(/[^\r]\n/);
    expect(r.archive).not.toMatch(/[^\r]\n/);
  });

  it("returns null when nothing is finished", () => {
    expect(archiveFinished("## A\n\n- [ ] **1. Open**\n", null, "PLAN.md")).toBeNull();
  });

  it("archived steps still count as done", () => {
    const r = archiveFinished(PLAN, null, "PLAN.md")!;
    const done = doneNumbers(r.plan, r.archive);
    expect(["1", "2", "3", "5"].every((n) => done.has(n))).toBe(true);
    expect(parsePlan(r.plan).steps.map((s) => s.number)).toEqual(["3", "4"]);
  });
});

describe("mergePlans", () => {
  const TARGET = "# Plan\n\n## A\n\n- [ ] **1. One**\n- [ ] **2. Two**\n";
  const SOURCE = [
    "## Other",
    "",
    "- [x] **1. Old done**",
    "- [ ] **2. Next**",
    "  - Needs: 1",
    "  - Model: claude-opus-5-5",
    "- [ ] **2b. Sub**",
    "  - Needs: 2, PLAN.md#1, LIB.md#3",
    "- [ ] **9. Free number**",
    "",
  ].join("\n");

  it("adds the steps under their heading and renumbers taken numbers", () => {
    const r = mergePlans(TARGET, "PLAN.md", SOURCE, "OTHER.md")!;
    expect(r.added).toBe(4);
    expect(r.renumbered).toEqual([
      ["1", "10"],
      ["2", "11"],
      ["2b", "11b"],
    ]);
    const plan = parsePlan(r.text);
    expect(plan.steps.map((s) => s.number)).toEqual(["1", "2", "10", "11", "11b", "9"]);
    expect(plan.steps.find((s) => s.number === "11")!.needs).toEqual(["10"]);
    expect(plan.steps.find((s) => s.number === "11")!.model).toBe("claude-opus-5-5");
    expect(plan.steps.find((s) => s.number === "11b")!.needs).toEqual(["11", "1", "LIB.md#3"]);
    expect(r.text.startsWith(TARGET.trimEnd() + "\n\n## Other\n\n")).toBe(true);
    expect(r.text.endsWith("**9. Free number**\n")).toBe(true);
  });

  it("can leave done steps out; their needs then point to the source plan", () => {
    const r = mergePlans(TARGET, "PLAN.md", SOURCE, "OTHER.md", { onlyOpen: true })!;
    expect(r.added).toBe(3);
    expect(r.leftOut).toBe(1);
    const plan = parsePlan(r.text);
    expect(plan.steps.find((s) => s.number === "11")!.needs).toEqual(["OTHER.md#1"]);
  });

  it("gives steps before any heading a heading of their own", () => {
    const r = mergePlans(TARGET, "PLAN.md", "- [ ] **7. Loose**\n", "notes/TODO.md")!;
    expect(r.text).toContain("## From notes/TODO.md\n\n- [ ] **7. Loose**");
  });

  it("returns null when there is nothing to add", () => {
    expect(mergePlans(TARGET, "PLAN.md", "- [x] **1. Done**\n", "O.md", { onlyOpen: true })).toBeNull();
  });
});

describe("mergePlans with an archive", () => {
  it("does not reuse numbers that are in the archive", () => {
    const r = mergePlans("## A\n\n- [ ] **5. Five**\n", "PLAN.md", "## B\n\n- [ ] **2. Two**\n", "O.md", {
      taken: stepNumbers("## Old\n\n- [x] **2. Old two**\n- [x] **9. Old nine**\n"),
    })!;
    expect(r.renumbered).toEqual([["2", "10"]]);
  });
});
