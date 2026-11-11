import { describe, expect, it } from "vitest";
import { moveSection, parsePanel } from "./panel.svelte";

describe("the project panel's state", () => {
  it("starts with the tree open and the extra section hidden", () => {
    const p = parsePanel(null);
    expect(p.view).toBe("explorer");
    expect(p.sections.map((s) => s.id)).toEqual(["editors", "files", "outline", "timeline", "touched"]);
    expect(p.sections.find((s) => s.id === "files")?.open).toBe(true);
    expect(p.sections.find((s) => s.id === "touched")?.hidden).toBe(true);
  });

  it("keeps a stored order, drops unknown sections, adds new ones, never hides the tree", () => {
    const p = parsePanel({
      view: "git",
      sections: [
        { id: "timeline", open: true, hidden: false, height: 9999 },
        { id: "bogus", open: true },
        { id: "files", open: true, hidden: true },
      ],
    });
    expect(p.view).toBe("git");
    expect(p.sections.map((s) => s.id)).toEqual(["timeline", "files", "editors", "outline", "touched"]);
    expect(p.sections[0].height).toBe(800);
    expect(p.sections[1].hidden).toBe(false);
  });

  it("moves a section by dragging it onto another", () => {
    const list = parsePanel(null).sections;
    expect(moveSection(list, "timeline", "editors").map((s) => s.id)).toEqual([
      "timeline",
      "editors",
      "files",
      "outline",
      "touched",
    ]);
    expect(moveSection(list, "editors", "outline").map((s) => s.id)).toEqual([
      "files",
      "outline",
      "editors",
      "timeline",
      "touched",
    ]);
  });
});
