import { describe, expect, it } from "vitest";
import {
  clockTime,
  timelineRows,
  currentModel,
  looksLikeCall,
  formatCost,
  formatTokens,
  itemsToMarkdown,
  reduceAll,
  resultImage,
  shortToolName,
  toolSummary,
} from "./events";

const lines = [
  JSON.stringify({ type: "qanuvi_user", text: "Create hello.txt" }),
  JSON.stringify({ type: "system", subtype: "init", model: "claude-haiku-5-5", session_id: "abc" }),
  JSON.stringify({
    type: "assistant",
    message: { content: [{ type: "thinking", thinking: "…", signature: "x" }] },
  }),
  JSON.stringify({
    type: "assistant",
    message: {
      content: [
        { type: "tool_use", id: "tool1", name: "Write", input: { file_path: "C:/p/hello.txt", content: "hi\n" } },
      ],
    },
  }),
  JSON.stringify({
    type: "control_request",
    request_id: "req1",
    request: {
      subtype: "can_use_tool",
      tool_name: "Write",
      display_name: "Write",
      description: "hello.txt",
      input: { file_path: "C:/p/hello.txt", content: "hi\n" },
    },
  }),
  JSON.stringify({ type: "qanuvi_permission", request_id: "req1", allow: false }),
  JSON.stringify({
    type: "user",
    message: { content: [{ type: "tool_result", tool_use_id: "tool1", is_error: true, content: "Refused." }] },
  }),
  JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "I could not create it." }] } }),
  JSON.stringify({ type: "result", subtype: "success", duration_ms: 7200, total_cost_usd: 0.0044, is_error: false }),
  "not json",
];

describe("reduceAll", () => {
  const items = reduceAll(lines);

  it("shows the conversation in order, thinking included", () => {
    expect(items.map((i) => i.kind)).toEqual(["user", "notice", "thinking", "tool", "approval", "text", "result"]);
  });

  it("follows a tool from running to its result", () => {
    const tool = items.find((i) => i.kind === "tool");
    expect(tool).toMatchObject({ name: "Write", file: "C:/p/hello.txt", state: "failed", result: "Refused." });
  });

  it("records the answer to an approval", () => {
    const approval = items.find((i) => i.kind === "approval");
    expect(approval).toMatchObject({ tool: "Write", summary: "hello.txt", state: "refused" });
    expect(approval && "detail" in approval && approval.detail).toContain("hi");
  });

  it("marks an approval answered by an earlier 'for this session'", () => {
    const auto = reduceAll([
      lines.find((l) => l.includes("can_use_tool"))!,
      JSON.stringify({ type: "qanuvi_permission", request_id: "req1", allow: true, auto: true }),
    ]).find((i) => i.kind === "approval");
    expect(auto).toMatchObject({ state: "allowed", auto: true });
  });

  it("ends with time and cost", () => {
    expect(items.at(-1)).toMatchObject({ kind: "result", ok: true, seconds: 7.2, costUsd: 0.0044 });
  });
});

describe("toolSummary", () => {
  it("prefers a description, then the command or file", () => {
    expect(toolSummary("Bash", { command: "npm test", description: "Run tests" })).toBe("Run tests");
    expect(toolSummary("Bash", { command: "npm test\nsecond line" })).toBe("npm test");
    expect(toolSummary("Grep", { pattern: "price" })).toBe("price");
    expect(toolSummary("Other", {})).toBe("Other");
  });
});

describe("formatting", () => {
  it("writes tokens and cost compactly", () => {
    expect(formatTokens(950)).toBe("950");
    expect(formatTokens(18_401)).toBe("18k");
    expect(formatTokens(2_500)).toBe("2.5k");
    expect(formatTokens(1_200_000)).toBe("1.2M");
    expect(formatCost(0)).toBe("$0");
    expect(formatCost(0.0044)).toBe("$0.0044");
    expect(formatCost(1.234)).toBe("$1.23");
  });
});

describe("itemsToMarkdown", () => {
  it("writes the conversation with steps and answers", () => {
    const md = itemsToMarkdown("Eggs", reduceAll(lines));
    expect(md.startsWith("# Eggs\n")).toBe(true);
    expect(md).toContain("**You:** Create hello.txt");
    expect(md).toContain("- Write: C:/p/hello.txt (failed)");
    expect(md).toContain("(refused)");
    expect(md).toContain("_Finished in 7.2 s_");
  });
});

describe("browser tools in the pane", () => {
  it("drops the MCP prefix from tool names", () => {
    expect(shortToolName("mcp__qanuvi-browser__browser_open")).toBe("browser_open");
    expect(shortToolName("mcp__qanuvi__find_symbol")).toBe("find_symbol");
    expect(shortToolName("Read")).toBe("Read");
  });

  it("keeps a screenshot from a tool's answer", () => {
    const use = JSON.stringify({
      type: "assistant",
      message: {
        content: [{ type: "tool_use", id: "s1", name: "mcp__qanuvi-browser__browser_screenshot", input: {} }],
      },
    });
    const result = JSON.stringify({
      type: "user",
      message: {
        content: [
          {
            type: "tool_result",
            tool_use_id: "s1",
            content: [
              { type: "image", source: { type: "base64", media_type: "image/jpeg", data: "/9j/AA" } },
              { type: "text", text: "Screenshot of http://localhost/" },
            ],
          },
        ],
      },
    });
    const [tool] = reduceAll([use, result]);
    expect(tool).toMatchObject({
      kind: "tool",
      name: "browser_screenshot",
      result: "Screenshot of http://localhost/",
      image: "data:image/jpeg;base64,/9j/AA",
    });
    expect(resultImage([{ type: "image", source: { media_type: "text/html", data: "x" } }])).toBe("");
  });
});

describe("live text and the model at work", () => {
  const delta = (text: string) =>
    JSON.stringify({
      type: "stream_event",
      event: { type: "content_block_delta", delta: { type: "text_delta", text } },
    });

  it("shows text as it is written, then the whole message in its place", () => {
    const live = reduceAll([delta("Hel"), delta("lo")]);
    expect(live).toMatchObject([{ kind: "text", text: "Hello", live: true }]);
    const whole = reduceAll([
      delta("Hel"),
      delta("lo"),
      JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "Hello." }] } }),
    ]);
    expect(whole.filter((i) => i.kind === "text")).toMatchObject([{ text: "Hello." }]);
    expect(whole.some((i) => i.kind === "text" && i.live)).toBe(false);
    expect(looksLikeCall('{"name": "Read"')).toBe(true);
    expect(looksLikeCall("```json\n{")).toBe(true);
    expect(looksLikeCall("The file says")).toBe(false);
  });

  it("names the model Qanuvi core picked, then the one that took over", () => {
    const route = (model: string) => JSON.stringify({ type: "system", subtype: "qanuvi_route", text: "x", model });
    const items = reduceAll([
      JSON.stringify({ type: "system", subtype: "init", model: "Qanuvi core (auto)" }),
      route("openrouter/a:free"),
    ]);
    expect(currentModel(items)).toBe("openrouter/a:free");
    expect(currentModel(reduceAll([...[route("openrouter/a:free")], route("ollama/b")]))).toBe("ollama/b");
    expect(currentModel(reduceAll([JSON.stringify({ type: "system", subtype: "init", model: "sonnet" })]))).toBe(
      "sonnet",
    );
  });
});

describe("messages sent while the agent works", () => {
  it("keeps the mark, and only on those messages", () => {
    const items = reduceAll([
      JSON.stringify({ type: "qanuvi_user", text: "Fix the header" }),
      JSON.stringify({ type: "qanuvi_user", text: "Also the footer", during: true }),
    ]);
    expect(items.map((i) => i.kind === "user" && i.during)).toEqual([undefined, true]);
  });
});

describe("images in the user's messages", () => {
  it("shows stored and in-hand images, and leaves out anything else", () => {
    const items = reduceAll([
      JSON.stringify({
        type: "qanuvi_user",
        text: "Look",
        attachments: ["a.png", "notes.txt"],
        images: [
          { name: "a.png", file: "0123abcd.png" },
          { name: "b.png", src: "data:image/png;base64,QUJD" },
          { name: "bad", src: "javascript:alert(1)" },
          { name: "none" },
        ],
      }),
    ]);
    expect(items[0]).toMatchObject({
      kind: "user",
      attachments: ["a.png", "notes.txt"],
      images: [
        { name: "a.png", file: "0123abcd.png" },
        { name: "b.png", src: "data:image/png;base64,QUJD" },
      ],
    });
  });
});

describe("step cards, thinking and to-do lists", () => {
  const call = (id: string, name: string, input: object) =>
    JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", id, name, input }] } });
  const answer = (id: string, content: unknown, is_error = false) =>
    JSON.stringify({
      type: "user",
      message: { content: [{ type: "tool_result", tool_use_id: id, content, is_error }] },
    });

  it("keeps a shell step's command as IN and its whole result as OUT", () => {
    const items = reduceAll([
      call("a", "Bash", { command: "npm test\n--run", description: "Run the tests" }),
      answer("a", "line 1\nline 2\nline 3\n"),
    ]);
    expect(items[0]).toMatchObject({
      kind: "tool",
      summary: "Run the tests",
      input: "npm test\n--run",
      output: "line 1\nline 2\nline 3",
      result: "line 1",
      state: "done",
    });
  });

  it("names a read's file and range, and a browser step's address or element", () => {
    const items = reduceAll([
      call("r", "Read", { file_path: "C:/p/a.ts", offset: 10, limit: 20 }),
      call("b", "mcp__qanuvi-browser__browser_click", { selector: "#buy", number: 3 }),
    ]);
    expect(items[0]).toMatchObject({ input: "C:/p/a.ts (from line 10, 20 lines)" });
    expect(items[1]).toMatchObject({ name: "browser_click", input: "selector: #buy\nnumber: 3" });
  });

  it("cuts a very long result", () => {
    const items = reduceAll([call("a", "Bash", { command: "x" }), answer("a", "y".repeat(9000))]);
    const out = (items[0] as { output: string }).output;
    expect(out.length).toBeLessThan(8100);
    expect(out).toContain("1000 more characters");
  });

  it("shows thinking as its own item", () => {
    const items = reduceAll([
      JSON.stringify({ type: "assistant", message: { content: [{ type: "thinking", thinking: "First the tests." }] } }),
    ]);
    expect(items).toEqual([{ kind: "thinking", id: "th0.0", text: "First the tests." }]);
  });

  it("keeps one to-do card and updates it in place", () => {
    const items = reduceAll([
      call("t1", "TodoWrite", {
        todos: [
          { content: "Read", status: "in_progress", activeForm: "Reading" },
          { content: "Fix", status: "pending" },
        ],
      }),
      call("a", "Bash", { command: "ls" }),
      call("t2", "TodoWrite", {
        todos: [
          { content: "Read", status: "completed" },
          { content: "Fix", status: "in_progress", activeForm: "Fixing" },
        ],
      }),
    ]);
    expect(items.map((i) => i.kind)).toEqual(["todos", "tool"]);
    expect(items[0]).toMatchObject({
      todos: [
        { text: "Read", status: "completed" },
        { text: "Fixing", status: "in_progress" },
      ],
    });
  });
});

describe("the timeline", () => {
  const tool = (id: string, name: string, file: string | null = null, state: "done" | "failed" | "running" = "done") =>
    ({ kind: "tool", id, name, summary: file ?? name, file, state, result: "", input: "", output: "" }) as const;
  const text = (id: string) => ({ kind: "text", id, text: "ok" }) as const;

  it("groups three or more look-around steps in a row and says what they did", () => {
    const rows = timelineRows([
      tool("a", "Read", "a.ts"),
      tool("b", "Read", "b.ts"),
      tool("c", "Grep"),
      tool("d", "Read", "a.ts"),
      tool("e", "Edit", "a.ts"),
      tool("f", "Read", "c.ts"),
    ]);
    expect(rows.map((r) => r.kind)).toEqual(["group", "tool", "tool"]);
    expect(rows[0]).toMatchObject({ label: "Read 2 files · searched once", state: "done" });
  });

  it("keeps fewer than three as single steps and shows a failure on the group", () => {
    expect(timelineRows([tool("a", "Read", "a.ts"), tool("b", "Grep")]).map((r) => r.kind)).toEqual(["tool", "tool"]);
    const [g] = timelineRows([tool("a", "Glob"), tool("b", "Grep", null, "failed"), tool("c", "Grep")]);
    expect(g).toMatchObject({ kind: "group", label: "Searched 3 times", state: "failed" });
  });

  it("folds a long run of steps and keeps the newest in view", () => {
    const steps = Array.from({ length: 14 }, (_, i) => tool(`s${i}`, "Bash"));
    const rows = timelineRows([text("t0"), ...steps, text("t1")]);
    expect(rows.map((r) => r.kind)).toEqual(["text", "fold", "tool", "tool", "tool", "tool", "text"]);
    expect(rows[1]).toMatchObject({ count: 10 });
  });

  it("stamps new items with the time they arrived and keeps a step's start time", () => {
    const call = JSON.stringify({
      type: "assistant",
      message: { content: [{ type: "tool_use", id: "x", name: "Bash", input: { command: "ls" } }] },
    });
    const done = JSON.stringify({
      type: "user",
      message: { content: [{ type: "tool_result", tool_use_id: "x", content: "a" }] },
    });
    const items = reduceAll([call, done], [100, 160]);
    expect(items[0]).toMatchObject({ state: "done", at: 100 });
  });

  it("shows the clock time today and the date on other days", () => {
    const at = new Date(2026, 10, 12, 9, 5).getTime() / 1000;
    expect(clockTime(at, new Date(2026, 10, 12, 18).getTime())).not.toMatch(/Nov/);
    expect(clockTime(at, new Date(2026, 10, 14).getTime())).toMatch(/12/);
  });
});

describe("thinking as it is written", () => {
  it("grows one live thinking item, replaced by the whole thinking when it arrives", () => {
    const d = (thinking: string) =>
      JSON.stringify({ type: "stream_event", event: { delta: { type: "thinking_delta", thinking } } });
    let items = reduceAll([d("First "), d("the tests.")]);
    expect(items).toEqual([{ kind: "thinking", id: "livethink0", text: "First the tests.", live: true }]);
    items = reduceAll([
      d("First "),
      JSON.stringify({ type: "assistant", message: { content: [{ type: "thinking", thinking: "First the tests." }] } }),
    ]);
    expect(items).toEqual([{ kind: "thinking", id: "th1.0", text: "First the tests." }]);
  });
});
