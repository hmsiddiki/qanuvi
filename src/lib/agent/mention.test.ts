import { describe, expect, it } from "vitest";
import { fileMention, inProject, insertMention, isImagePath, mentionAt, symbolMention } from "./mention";

describe("@ mentions in the message box", () => {
  it("finds the @query the caret is in, only at a word start", () => {
    expect(mentionAt("@", 1)).toEqual({ start: 0, query: "" });
    expect(mentionAt("look at @src/ap", 15)).toEqual({ start: 8, query: "src/ap" });
    expect(mentionAt("mail me@home", 12)).toBeNull();
    expect(mentionAt("@a.ts done", 10)).toBeNull();
  });

  it("replaces the query with the mention and puts the caret after it", () => {
    expect(insertMention("see @ap", 4, 7, "@src/app.ts")).toEqual({ text: "see @src/app.ts ", caret: 16 });
    expect(insertMention("see @ap and more", 4, 7, "@src/app.ts")).toEqual({
      text: "see @src/app.ts and more",
      caret: 16,
    });
  });

  it("writes file and symbol mentions", () => {
    expect(fileMention("src/a.ts")).toBe("@src/a.ts");
    expect(symbolMention("src/a.ts", "start", 12)).toBe("@src/a.ts (start, line 12)");
  });

  it("knows dropped files inside the project and images", () => {
    expect(inProject("C:\\p\\app\\", "c:\\P\\app\\src\\a.ts")).toBe("src/a.ts");
    expect(inProject("C:/p/app", "C:/p/apple/a.ts")).toBeNull();
    expect(isImagePath("C:/x/shot.PNG")).toBe(true);
    expect(isImagePath("C:/x/notes.md")).toBe(false);
  });
});
