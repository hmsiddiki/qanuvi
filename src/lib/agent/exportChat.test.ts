import { describe, expect, it } from "vitest";
import type { Item } from "./events";
import { exportFileName, exportMarkdown } from "./exportChat";

const tool = (over: Partial<Extract<Item, { kind: "tool" }>>): Item => ({
  kind: "tool",
  id: "t",
  name: "Read",
  summary: "src/notes.js",
  file: null,
  state: "done",
  result: "",
  input: "",
  output: "",
  ...over,
});

describe("exportMarkdown (24n)", () => {
  it("writes messages, steps as lines, edits as diffs and commands as code; leaves thinking out", () => {
    const items: Item[] = [
      { kind: "user", id: "u", text: "Add a comment" },
      { kind: "thinking", id: "th", text: "secret musing" },
      tool({ id: "1" }),
      tool({ id: "2", name: "Edit", summary: "src/notes.js", edits: [{ old: "a\n", new: "// hi\na\n" }] }),
      tool({ id: "3", name: "Bash", summary: "check", input: "node --check src/notes.js" }),
      { kind: "text", id: "x", text: "Done." },
      { kind: "result", id: "r", ok: true, seconds: 38.84, costUsd: 0.0075, text: "" },
    ];
    const md = exportMarkdown(items, "Add a comment", "2026-11-20 05:40", "Claude Code · Sonnet 5.5");
    expect(md).toBe(
      [
        "# Add a comment",
        "",
        "Exported from Qanuvi, 2026-11-20 05:40. Agent: Claude Code · Sonnet 5.5.",
        "",
        "## You",
        "",
        "Add a comment",
        "",
        "## Agent",
        "",
        "- **Read** src/notes.js",
        "- **Edit** src/notes.js",
        "",
        "```diff",
        "- a",
        "+ // hi",
        "+ a",
        "```",
        "",
        "- **Bash** check",
        "",
        "```sh",
        "node --check src/notes.js",
        "```",
        "",
        "Done.",
        "",
        "_Finished · 38.8 s · $0.0075_",
        "",
      ].join("\n"),
    );
    expect(md).not.toContain("secret musing");
  });

  it("keeps a failed step's output short and fences text holding backticks safely", () => {
    const out = Array.from({ length: 25 }, (_, i) => `line ${i + 1}`).join("\n");
    const md = exportMarkdown([tool({ name: "Bash", state: "failed", input: "echo ```", output: out })], "", "d", "a");
    expect(md).toContain("- **Bash** src/notes.js (failed)");
    expect(md).toContain("````sh\necho ```\n````");
    expect(md).toContain("line 20\n… (5 more lines)");
    expect(md).not.toContain("line 21");
    expect(md.startsWith("# Conversation")).toBe(true);
  });

  it("names the file from the title and date", () => {
    expect(exportFileName("Step 1: Add a `countNotes()` helper!", "2026-11-20")).toBe(
      "step-1-add-a-countnotes-helper-2026-11-20.md",
    );
    expect(exportFileName("???", "2026-11-20")).toBe("conversation-2026-11-20.md");
  });
});
