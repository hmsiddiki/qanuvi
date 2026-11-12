import { describe, expect, it } from "vitest";
import { diffCounts, editDiff, revertEdits } from "./diff";
import { editsOf } from "./events";

describe("edits as diffs in the chat", () => {
  it("shows removed and added lines with context and gaps", () => {
    const before = ["a", "b", "c", "d", "e", "f", "g"].join("\n");
    const after = ["a", "b", "c", "D", "e", "f", "g"].join("\n");
    expect(editDiff(before, after)).toEqual([
      { type: "gap", text: "" },
      { type: "ctx", text: "b" },
      { type: "ctx", text: "c" },
      { type: "del", text: "d" },
      { type: "add", text: "D" },
      { type: "ctx", text: "e" },
      { type: "ctx", text: "f" },
      { type: "gap", text: "" },
    ]);
  });

  it("shows a new file's text as added lines", () => {
    expect(editDiff("", "x\ny")).toEqual([
      { type: "add", text: "x" },
      { type: "add", text: "y" },
    ]);
  });

  it("counts added and removed lines over all edits", () => {
    expect(
      diffCounts([
        { old: "a\nb", new: "a\nc\nd" },
        { old: "", new: "z" },
      ]),
    ).toEqual({ added: 3, removed: 1 });
  });

  it("undoes edits in reverse order, only where it is safe", () => {
    const now = "one TWO three FOUR";
    expect(
      revertEdits(now, [
        { old: "two", new: "TWO" },
        { old: "four", new: "FOUR" },
      ]),
    ).toBe("one two three four");
    // Changed since, or the text is there twice: not undone.
    expect(revertEdits("one three", [{ old: "two", new: "TWO" }])).toBeNull();
    expect(revertEdits("TWO TWO", [{ old: "two", new: "TWO" }])).toBeNull();
    expect(revertEdits("TWO TWO", [{ old: "two", new: "TWO", all: true }])).toBe("two two");
    // A deletion (nothing left to find) cannot be undone from the chat.
    expect(revertEdits("x", [{ old: "y", new: "" }])).toBeNull();
  });
});

describe("file steps carry their edits", () => {
  it("reads Edit, MultiEdit and Write", () => {
    expect(editsOf("Edit", { file_path: "a", old_string: "x", new_string: "y" })).toEqual({
      edits: [{ old: "x", new: "y" }],
    });
    expect(
      editsOf("MultiEdit", { edits: [{ old_string: "a", new_string: "b", replace_all: true }, { new_string: "c" }] }),
    ).toEqual({
      edits: [
        { old: "a", new: "b", all: true },
        { old: "", new: "c" },
      ],
    });
    expect(editsOf("Write", { content: "hi" })).toEqual({ edits: [{ old: "", new: "hi" }], whole: true });
    expect(editsOf("Read", { file_path: "a" })).toBeNull();
  });
});
