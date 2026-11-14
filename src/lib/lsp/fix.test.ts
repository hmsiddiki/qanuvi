import { describe, expect, it } from "vitest";
import { actionLabels, fixText, guessIndent, orderActions, touches } from "./fix";

describe("actionLabels", () => {
  const range = (line: number) => ({ start: { line, character: 0 }, end: { line: line + 1, character: 0 } });
  it("adds the line only to titles that repeat", () => {
    const labels = actionLabels([
      { title: "Remove all unused imports" },
      { title: "remove the whole `use` item", diagnostics: [{ range: range(0) }] },
      {
        title: "remove the whole `use` item",
        edit: { changes: { "file:///a.rs": [{ range: range(1), newText: "" }] } },
      },
      { title: "Off", disabled: { reason: "not here" } },
    ]);
    expect(labels).toEqual([
      "Remove all unused imports",
      "remove the whole `use` item (line 1)",
      "remove the whole `use` item (line 2)",
      "Off (not here)",
    ]);
  });
});

const r = (l1: number, c1: number, l2: number, c2: number) => ({
  start: { line: l1, character: c1 },
  end: { line: l2, character: c2 },
});

describe("guessIndent", () => {
  it("reads tabs, 4 and 2 spaces", () => {
    expect(guessIndent("a\n\tb\n\t\tc\n")).toEqual({ tabSize: 4, insertSpaces: false });
    expect(guessIndent("a\n    b\n        c\n    d")).toEqual({ tabSize: 4, insertSpaces: true });
    expect(guessIndent("a\n  b\n    c\n  d\n      e")).toEqual({ tabSize: 2, insertSpaces: true });
  });

  it("ignores blank lines and block comment stars", () => {
    expect(guessIndent("/**\n * x\n */\nfn\n    y\n   \n")).toEqual({ tabSize: 4, insertSpaces: true });
  });

  it("falls back to 4 spaces", () => {
    expect(guessIndent("")).toEqual({ tabSize: 4, insertSpaces: true });
  });
});

describe("orderActions", () => {
  it("puts the preferred quick fix first and disabled ones last", () => {
    const list = orderActions([
      { title: "Organize imports", kind: "source.organizeImports" },
      { title: "Off", kind: "quickfix", disabled: { reason: "no" } },
      { title: "Extract", kind: "refactor.extract" },
      { title: "Import Foo", kind: "quickfix" },
      { title: "Add use", kind: "quickfix", isPreferred: true },
      { title: "" },
    ]);
    expect(list.map((a) => a.title)).toEqual(["Add use", "Import Foo", "Extract", "Organize imports", "Off"]);
  });
});

describe("touches", () => {
  it("meets ranges on the same and other lines", () => {
    expect(touches(r(2, 4, 2, 9), { line: 2, character: 0 }, { line: 2, character: 0 })).toBe(false);
    expect(touches(r(2, 4, 2, 9), { line: 2, character: 4 }, { line: 2, character: 4 })).toBe(true);
    expect(touches(r(2, 4, 2, 9), { line: 2, character: 9 }, { line: 2, character: 9 })).toBe(true);
    expect(touches(r(2, 4, 4, 1), { line: 3, character: 0 }, { line: 3, character: 0 })).toBe(true);
    expect(touches(r(2, 4, 2, 9), { line: 0, character: 0 }, { line: 5, character: 0 })).toBe(true);
    expect(touches(r(2, 4, 2, 9), { line: 3, character: 0 }, { line: 5, character: 0 })).toBe(false);
  });
});

describe("fixText", () => {
  const p = {
    path: "src/a.php",
    range: r(3, 2, 3, 6),
    severity: "error",
    message: "Undefined method\n'x'.",
    source: "intelephense P1013",
  };

  it("names the place, the message and the lines around it", () => {
    const text = fixText([p], "a\nb\nc\nd\ne\nf\ng\nh\ni\nj", "php");
    expect(text).toContain("Fix this error reported in @src/a.php:4:");
    expect(text).toContain("- line 4: Undefined method 'x'. (intelephense P1013)");
    expect(text).toContain("```php\n1  a\n2  b\n3  c\n4  d\n5  e\n6  f\n7  g\n```");
  });

  it("lists several problems without code", () => {
    const text = fixText([p, { ...p, range: r(7, 0, 7, 1), message: "Two", source: "" }], "x");
    expect(text).toContain("Fix these 2 problems reported in @src/a.php:");
    expect(text).toContain("- line 8: Two\n");
    expect(text).not.toContain("```");
  });

  it("is empty for no problems", () => {
    expect(fixText([], "x")).toBe("");
  });
});
