import { describe, expect, it } from "vitest";
import { privateBlock } from "./guidance";
import { guidanceNote, isStopOrder, withoutGuidance } from "./guidance";

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
});

import { looksSecret } from "./guidance";

describe("run notes and secrets", () => {
  it("notices passwords and keys", () => {
    expect(looksSecret("root qz7test9 if you need db user pass")).toBe(true);
    expect(looksSecret("the db password is abc12345")).toBe(true);
    expect(looksSecret("key sk-test-abcdefghijklmnopqrstu")).toBe(true);
    expect(looksSecret("use the shop currency")).toBe(false);
    expect(looksSecret("do not change the login page")).toBe(false);
  });
});

import { contextSize } from "./guidance";

describe("continuing a session across steps", () => {
  it("reads the context size from the last model call", () => {
    const line = (i: number, c: number) =>
      JSON.stringify({ type: "assistant", message: { usage: { input_tokens: i, cache_read_input_tokens: c } } });
    expect(contextSize([line(10, 100), "x", line(5, 2000), '{"type":"result"}'])).toBe(2005);
    expect(contextSize([])).toBe(0);
  });
});

describe("a plan's private details", () => {
  it("reach later steps with the rule to keep them out of everything written", () => {
    expect(privateBlock([])).toBe("");
    const b = privateBlock(["db login: root / example1"]);
    expect(b).toContain("- db login: root / example1");
    expect(b).toContain("never write them into files");
  });
});

import { askedToResume, isGoOnOrder, isYes, stoppedChatNote, RESUME_ASK } from "./guidance";

describe("chat in a step the run stopped at", () => {
  it("is a chat: the note is hidden from the shown message and asks before the run goes on", () => {
    const note = stoppedChatNote("53z", "PLAN-E.md");
    expect(withoutGuidance("what is your best recommendation?" + note)).toBe("what is your best recommendation?");
    expect(note).toContain(`${RESUME_ASK} 53z? (yes/no)`);
    expect(note).toContain("Answer what they ask first");
  });

  it("goes on only on a plain order, never on a question or a longer message", () => {
    for (const t of [
      "continue",
      "Resume.",
      "go on",
      "retry the step",
      "continue the run",
      "run it again",
      "resume the plan run",
    ])
      expect(isGoOnOrder(t)).toBe(true);
    for (const t of [
      "continue?",
      "should we continue the run",
      "what is your best recommendation",
      "continue with option 1 but",
    ])
      expect(isGoOnOrder(t)).toBe(false);
  });

  it("a yes counts only after the agent asked to resume", () => {
    for (const t of ["yes", "Yes!", "ok", "go ahead", "yes, resume"]) expect(isYes(t)).toBe(true);
    for (const t of ["yes but first explain", "no", "1"]) expect(isYes(t)).toBe(false);
    expect(askedToResume(`Done.\n${RESUME_ASK} 53z? (yes/no)`)).toBe(true);
    expect(askedToResume("Should I use option 1? (yes/no)")).toBe(false);
  });
});

describe("the plan's private details", () => {
  it("keeps each detail on one line, so the stored copy can mask all of it", () => {
    const b = privateBlock(["db user: test\npassword: x1"]);
    expect(b.split("\n").filter((l) => l.startsWith("- "))).toEqual(["- db user: test password: x1"]);
    expect(privateBlock([])).toBe("");
  });
});
