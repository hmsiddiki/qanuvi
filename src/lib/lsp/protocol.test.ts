import { describe, expect, it } from "vitest";
import {
  applyEdits,
  editsByFile,
  fileUri,
  languageId,
  linesOf,
  markupText,
  relativePath,
  serverFor,
  severity,
  toOffset,
  toPosition,
  uriKey,
} from "./protocol";

describe("language server helpers", () => {
  it("picks the server for a file", () => {
    expect(serverFor("src/a.PHP")).toBe("php");
    expect(serverFor("x/App.svelte")).toBe("svelte");
    expect(serverFor("lib.rs")).toBe("rust");
    expect(serverFor("a.jsx")).toBe("typescript");
    expect(serverFor("README.md")).toBeNull();
    expect(serverFor("Makefile")).toBeNull();
    expect(languageId("a.tsx")).toBe("typescriptreact");
  });

  it("makes and reads file addresses", () => {
    expect(fileUri("E:\\Projects\\my app", "src/a b.php")).toBe("file:///E:/Projects/my%20app/src/a%20b.php");
    expect(fileUri("/home/me/x", "a.rs")).toBe("file:///home/me/x/a.rs");
    expect(fileUri("E:\\Projects\\x")).toBe("file:///E:/Projects/x");
    // Servers on Windows often send the drive in lower case with the colon encoded.
    expect(uriKey("file:///e%3A/Projects/x/a.php")).toBe(uriKey("file:///E:/Projects/x/a.php"));
    expect(relativePath("E:\\Projects\\x", "file:///e%3A/Projects/x/src/A.php")).toBe("src/A.php");
    expect(relativePath("E:\\Projects\\x", "file:///E:/Projects/xy/a.php")).toBeNull();
    expect(relativePath("/home/me/x", "file:///home/me/x/a%20b.rs")).toBe("a b.rs");
    expect(relativePath("/home/me/x", "file:///home/Me/x/a.rs")).toBeNull();
  });

  it("converts positions", () => {
    const doc = linesOf("ab\ncdé\n\nx");
    expect(toPosition(doc, 0)).toEqual({ line: 0, character: 0 });
    expect(toPosition(doc, 5)).toEqual({ line: 1, character: 2 });
    expect(toPosition(doc, 8)).toEqual({ line: 3, character: 0 });
    expect(toOffset(doc, { line: 1, character: 3 })).toBe(6);
    // Past the line's end or the last line: clamped.
    expect(toOffset(doc, { line: 0, character: 99 })).toBe(2);
    expect(toOffset(doc, { line: 9, character: 0 })).toBe(doc.length);
  });

  it("applies edits from a rename", () => {
    const r = (l1: number, c1: number, l2: number, c2: number) => ({
      start: { line: l1, character: c1 },
      end: { line: l2, character: c2 },
    });
    const text = "$a = 1;\necho $a;\n";
    const out = applyEdits(text, [
      { range: r(0, 0, 0, 2), newText: "$total" },
      { range: r(1, 5, 1, 7), newText: "$total" },
    ]);
    expect(out).toBe("$total = 1;\necho $total;\n");
    const both = editsByFile({
      changes: { "file:///a": [{ range: r(0, 0, 0, 1), newText: "x" }] },
      documentChanges: [{ textDocument: { uri: "file:///a" }, edits: [{ range: r(1, 0, 1, 1), newText: "y" }] }],
    });
    expect(both.get("file:///a")?.length).toBe(2);
  });

  it("reads hover text and severities", () => {
    expect(markupText({ kind: "markdown", value: "**x**" })).toBe("**x**");
    expect(markupText([{ language: "php", value: "function f()" }, "Does f."])).toBe(
      "```php\nfunction f()\n```\n\nDoes f.",
    );
    expect(markupText(null)).toBe("");
    expect([1, 2, 3, 4, undefined].map(severity)).toEqual(["error", "warning", "info", "hint", "error"]);
  });
});
