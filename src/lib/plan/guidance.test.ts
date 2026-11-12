import { describe, expect, it } from "vitest";
import { addedSteps, guidanceNote, isStopOrder, stepKey, withoutGuidance } from "./guidance";
import { parsePlan } from "./tasklist";

describe("messages while a plan step runs", () => {
  it("keeps only the user's words when shown", () => {
    const sent = "use the shop currency" + guidanceNote("82a", "PLAN-D.md");
    expect(sent).toContain("82a-1");
    expect(withoutGuidance(sent)).toBe("use the shop currency");
  });

  it("knows a plain stop order from other words", () => {
    for (const t of ["stop", "Stop!", "stop the run", "halt now", "cancel"]) expect(isStopOrder(t)).toBe(true);
    for (const t of ["stop using floats", "do not stop", "stop adding tests to the cart"])
      expect(isStopOrder(t)).toBe(false);
  });

  it("finds steps added while the run went, numbered like 82a-1", () => {
    const before = parsePlan("- [ ] **82a. One**\n- [ ] **82b. Two**\n").steps;
    const now = parsePlan("- [x] **82a. One**\n- [ ] **82a-1. Extra**\n- [ ] **82b. Two**\n").steps;
    const added = addedSteps(now, new Set(before.map(stepKey)));
    expect(added.map((s) => s.number)).toEqual(["82a-1"]);
  });
});

import { looksSecret, notesBlock, notesIn } from "./guidance";

describe("run notes and secrets", () => {
  it("passes notes on and reads the ones agents leave", () => {
    expect(notesBlock(["use the shop currency"])).toContain("- use the shop currency");
    expect(notesIn("Done.\n**Note for the next steps:** tests need php 8.2\nbye")).toEqual(["tests need php 8.2"]);
    expect(notesIn("Note for the next steps: password: hunter22")).toEqual([]);
  });

  it("notices passwords and keys", () => {
    expect(looksSecret("root qz7test9 if you need db user pass")).toBe(true);
    expect(looksSecret("the db password is abc12345")).toBe(true);
    expect(looksSecret("key sk-test-abcdefghijklmnopqrstu")).toBe(true);
    expect(looksSecret("use the shop currency")).toBe(false);
    expect(looksSecret("do not change the login page")).toBe(false);
  });
});

import { reopenedSteps } from "./guidance";

describe("going back to a step", () => {
  it("runs a passed step again once when the plan opens it again", () => {
    const run = [
      { number: "82a", title: "One", status: "done" },
      { number: "82b", title: "Two", status: "blocked" },
      { number: "82c", title: "Three", status: "running" },
    ];
    const now = parsePlan("- [x] **82a. One**\n- [ ] **82b. Two**\n- [ ] **82c. Three**\n").steps;
    expect(reopenedSteps(run, now, new Set()).map((s) => s.number)).toEqual(["82b"]);
    expect(reopenedSteps(run, now, new Set(["82b|Two"]))).toEqual([]);
    const still = parsePlan("- [!] **82b. Two**\n").steps;
    expect(reopenedSteps(run, still, new Set())).toEqual([]);
  });
});

import { canCarry, contextSize, needingCount } from "./guidance";

describe("continuing a session across steps", () => {
  it("reads the context size from the last model call", () => {
    const line = (i: number, c: number) =>
      JSON.stringify({ type: "assistant", message: { usage: { input_tokens: i, cache_read_input_tokens: c } } });
    expect(contextSize([line(10, 100), "x", line(5, 2000), '{"type":"result"}'])).toBe(2005);
    expect(contextSize([])).toBe(0);
  });

  it("continues only on the same model and section while small", () => {
    const c = { model: "opus", section: "S82", tokens: 40000, steps: 2 };
    expect(canCarry(c, "opus", "S82", 80000)).toBe(true);
    expect(canCarry(c, "sonnet", "S82", 80000)).toBe(false);
    expect(canCarry(c, "opus", "S83", 80000)).toBe(false);
    expect(canCarry({ ...c, tokens: 90000 }, "opus", "S82", 80000)).toBe(false);
    expect(canCarry({ ...c, steps: 6 }, "opus", "S82", 80000)).toBe(false);
    expect(canCarry(null, "opus", "S82", 80000)).toBe(false);
  });

  it("counts the steps a blocked one holds up, through chains", () => {
    const steps = [
      { number: "82c", needs: ["82a"] },
      { number: "82d", needs: ["82b"] },
      { number: "82e", needs: ["82d"] },
      { number: "83a", needs: ["82z"] },
    ];
    expect(needingCount("82b", steps, new Set(["82c", "82d", "82e", "83a"]))).toBe(2);
  });
});

import { privateBlock } from "./guidance";

describe("a plan's private details", () => {
  it("reach later steps with the rule to keep them out of everything written", () => {
    expect(privateBlock([])).toBe("");
    const b = privateBlock(["db login: root / example1"]);
    expect(b).toContain("- db login: root / example1");
    expect(b).toContain("never write them into files");
  });
});

import { attemptEntry, logPath, logsBlock, resultEntry } from "./guidance";

describe("step logs", () => {
  it("has one file per step under the plan's folder", () => {
    expect(logPath("content/plugins/qcommerce/PLAN-D.md", { number: "82b", title: "x" })).toBe(
      ".qanuvi/plan-logs/content_plugins_qcommerce_PLAN-D/82b.md",
    );
    expect(logPath("PLAN.md", { number: null, title: "Add greet test" })).toBe(
      ".qanuvi/plan-logs/PLAN/Add_greet_test.md",
    );
  });

  it("records attempts and results, and tells a retried step to read its log", () => {
    const e = attemptEntry({
      when: new Date("2026-11-12T10:00:00Z"),
      model: "claude-opus-5-5",
      outcome: "open",
      costUsd: 2.19,
      seconds: 349,
      testsPassed: false,
      failure: "Expected 1200, got 1250",
      answer: "The cart cache was not cleared.",
    });
    expect(e).toContain("claude-opus-5-5 · turn ended · $2.19");
    expect(e).toContain("Tests: failed.");
    expect(e).toContain("Expected 1200, got 1250");
    expect(resultEntry("blocked", "no database login")).toContain("**Result: blocked** — no database login");
    expect(logsBlock("PLAN.md", ".qanuvi/plan-logs/PLAN/2.md", true)).toContain("tried before");
    expect(logsBlock("PLAN.md", ".qanuvi/plan-logs/PLAN/2.md", false)).not.toContain("tried before");
  });
});

import { contextBlock, mapPath } from "./guidance";

describe("what a step gets to find its way", () => {
  it("passes the map, the last changes, the index hint and the rule to keep the map", () => {
    const b = contextBlock(
      "PLAN-D.md",
      "src/Cart/CartLine.php — a cart line and its chosen amount",
      [
        { number: "82d", files: ["src/Checkout/Separate.php"] },
        { number: "82e", files: [] },
      ],
      true,
    );
    expect(b).toContain("CartLine.php — a cart line");
    expect(b).toContain("- 82d: src/Checkout/Separate.php");
    expect(b).not.toContain("- 82e");
    expect(b).toContain("find_symbol");
    expect(b).toContain(`update the plan map ${mapPath("PLAN-D.md")}`);
    expect(contextBlock("P.md", "", [], false)).not.toContain("find_symbol");
  });
});

import { apiSection, isCode, knowledgeBlock, mergeApi } from "./guidance";

describe("the plan's shared knowledge", () => {
  it("gives every step the map and facts first, and asks to keep them small", () => {
    const longMap = Array.from({ length: 90 }, (_, i) => `f${i}.php — x`).join("\n");
    const b = knowledgeBlock("PLAN-D.md", longMap, "Tests: php tests/run.php --strict", [], true);
    expect(b.startsWith("This plan's knowledge")).toBe(true);
    expect(b).toContain("FACTS.md:\nTests: php tests/run.php --strict");
    expect(b).toContain("MAP.md has 90 lines: tidy it to under 80");
    expect(b).toContain("never passwords or keys");
  });

  it("keeps API.md one part per file, replaced when a file changes", () => {
    expect(isCode("src/Cart/CartLine.php")).toBe(true);
    expect(isCode("README.md")).toBe(false);
    expect(isCode("vendor/x/y.php")).toBe(false);
    const a = apiSection("src/A.php", [{ name: "resetCache", kind: "method", line: 88, container: "CartStorage" }]);
    expect(a).toBe("### src/A.php\n- method CartStorage::resetCache :88");
    let api = mergeApi(
      "",
      new Map([
        ["src/B.php", "### src/B.php\n- class B :3"],
        ["src/A.php", a],
      ]),
    );
    expect(api.indexOf("src/A.php")).toBeLessThan(api.indexOf("src/B.php"));
    api = mergeApi(api, new Map([["src/B.php", ""]]));
    expect(api).not.toContain("src/B.php");
    expect(api).toContain("CartStorage::resetCache :88");
  });
});
