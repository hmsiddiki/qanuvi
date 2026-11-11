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
