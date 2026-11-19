import { describe, expect, it } from "vitest";
import { EarlyInput, opensLink, osc52Copy, pasteText, urlsIn } from "./terminalText";

const b64 = (s: string) => btoa(String.fromCharCode(...new TextEncoder().encode(s)));

describe("terminal links", () => {
  it("open only on a left Ctrl or Cmd click", () => {
    expect(opensLink({ button: 0, ctrlKey: true, metaKey: false })).toBe(true);
    expect(opensLink({ button: 0, ctrlKey: false, metaKey: true })).toBe(true);
    // A plain click (the end of a selection) and a right-click do nothing.
    expect(opensLink({ button: 0, ctrlKey: false, metaKey: false })).toBe(false);
    expect(opensLink({ button: 2, ctrlKey: true, metaKey: false })).toBe(false);
  });

  it("finds addresses in a line, without the sentence's punctuation", () => {
    expect(urlsIn("see https://example.com/a?b=1. Then go")).toEqual([
      { start: 4, end: 29, url: "https://example.com/a?b=1" },
    ]);
    expect(urlsIn("(at http://x.test/p)").map((u) => u.url)).toEqual(["http://x.test/p"]);
    expect(urlsIn("https://en.wiki.test/A_(b)").map((u) => u.url)).toEqual(["https://en.wiki.test/A_(b)"]);
    expect(urlsIn("no link here, https:// alone")).toEqual([]);
  });
});

describe("copy requests from the shell (OSC 52)", () => {
  it("decodes the text to copy, also outside ASCII", () => {
    expect(osc52Copy(`c;${b64("https://claude.test/x?y=1")}`)).toBe("https://claude.test/x?y=1");
    expect(osc52Copy(`;${b64("ä → ok")}`)).toBe("ä → ok");
  });

  it("never answers a request to read the clipboard, and drops broken ones", () => {
    expect(osc52Copy("c;?")).toBeNull();
    expect(osc52Copy("c;")).toBeNull();
    expect(osc52Copy("no-separator")).toBeNull();
    expect(osc52Copy("c;%%%")).toBeNull();
  });
});

describe("pasted text", () => {
  it("drops the line break after one line, keeps several lines as they are", () => {
    expect(pasteText("echo hi\r\n")).toBe("echo hi");
    expect(pasteText("echo hi\n\n")).toBe("echo hi");
    expect(pasteText("echo hi")).toBe("echo hi");
    expect(pasteText("a\nb\n")).toBe("a\nb\n");
  });
});

describe("input before the terminal is known (v3)", () => {
  it("is held in order and given once, then nothing is held", () => {
    const early = new EarlyInput();
    // The shell's cursor question answered before the spawn's answer, then a key.
    expect(early.hold(null, "\x1b[1;1R")).toBe(true);
    expect(early.hold(null, "l")).toBe(true);
    expect(early.take()).toBe("\x1b[1;1Rl");
    expect(early.take()).toBe("");
    // Known: straight on.
    expect(early.hold("t-1", "s")).toBe(false);
    expect(early.take()).toBe("");
  });
});
