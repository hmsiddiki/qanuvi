import { describe, expect, it } from "vitest";
import { decidedNote, parsePreflight, preflightPrompt, recommendedRisk, settleCheck, waitsBefore } from "./preflight";

describe("the check before a run", () => {
  it("asks with what is known and the open steps", () => {
    const p = preflightPrompt("PLAN-D.md", [{ number: "82e", title: "Short field set", details: ["- Needs: 82d"] }], {
      secrets: ["DB_USER"],
      testCommand: "php tests/run.php",
      previewUrl: "",
      notes: ["use the shop currency"],
    });
    expect(p).toContain("DB_USER");
    expect(p).toContain("82e. Short field set");
    expect(p).toContain("use the shop currency");
    expect(p).toContain("Change nothing");
  });

  it("reads the answer and drops what is not usable", () => {
    const r = parsePreflight(
      'Here: {"secrets":[{"name":"QCOMMERCE_TEST_PASSWORD","why":"db tests","steps":["82e"]},{"name":"bad name","why":"x"}],' +
        '"questions":[{"step":"83c","question":"Which currency format?"},{"step":"x","question":""}],' +
        '"risky":[{"step":"84a","why":"refunds reach the payment provider"}],"setup":[{"what":"MySQL running","why":"tests"}]}',
    );
    expect(r.secrets.map((s) => s.name)).toEqual(["QCOMMERCE_TEST_PASSWORD"]);
    expect(r.questions).toHaveLength(1);
    expect(r.risky[0].step).toBe("84a");
    expect(r.setup[0].what).toBe("MySQL running");
    expect(parsePreflight("no json here")).toEqual({ secrets: [], questions: [], risky: [], setup: [] });
  });

  it("knows the steps to wait at", () => {
    const wait = [{ step: "84a", why: "refunds" }];
    expect(waitsBefore("84a", wait)).toBe("refunds");
    expect(waitsBefore("84b", wait)).toBeNull();
    expect(waitsBefore(null, wait)).toBeNull();
  });

  it("sorts asks: unsorted risky steps wait, a question with no recommended answer needs the owner", () => {
    const r = parsePreflight(
      JSON.stringify({
        questions: [
          { step: "1a", question: "Separator?", sort: "decide", recommended: "dash", because: "URLs" },
          { step: "1b", question: "Which bank?", sort: "decide" },
          { step: "1c", question: "Format?", recommended: "ISO" },
        ],
        risky: [
          { step: "2a", why: "migrates", sort: "checks", because: "test database" },
          { step: "2b", why: "pushes" },
          { step: "2c", why: "odd", sort: "maybe" },
        ],
      }),
    );
    expect(r.questions.map((q) => q.sort)).toEqual(["decide", "you", "decide"]);
    expect(r.questions[0].recommended).toBe("dash");
    expect(r.risky.map((x) => x.sort)).toEqual(["checks", "you", "you"]);
    expect(r.risky.map((x) => recommendedRisk(x.sort))).toEqual(["checks", "wait", "wait"]);
    expect(recommendedRisk("decide")).toBe("go");
  });

  it("asks the check to sort and to judge risk by where the step runs", () => {
    const p = preflightPrompt("P.md", [], { secrets: [], testCommand: "", previewUrl: "", notes: [] });
    expect(p).toContain('"decide"');
    expect(p).toContain("not real money");
  });

  it("settles the answers: Qanuvi's choices, the owner's, extra checks and holds", () => {
    const check = parsePreflight(
      JSON.stringify({
        questions: [
          { step: "1a", question: "Separator?", sort: "decide", recommended: "dash", because: "URLs" },
          { step: "1b", question: "Bank?", sort: "you", recommended: "test bank" },
          { step: "1c", question: "Column name?", sort: "checks", recommended: "slug" },
          { step: "1d", question: "Theme?", sort: "decide", recommended: "dark" },
        ],
        risky: [
          { step: "2a", why: "migrates", sort: "checks", because: "test database" },
          { step: "2b", why: "pushes", sort: "you" },
          { step: "2c", why: "deletes cache", sort: "decide" },
          { step: "1a", why: "renames files", sort: "you" },
        ],
      }),
    );
    const r = settleCheck(
      check,
      { 0: "dash", 1: "", 2: "slug", 3: "light" },
      { 3: true },
      { 0: "checks", 1: "wait", 2: "go", 3: "wait" },
      { 3: true },
    );
    // 1a decided, but a risk the owner chose to wait at holds it anyway.
    expect(r.decided.map((d) => d.step)).toEqual(["1a", "1c", "2a", "2c"]);
    expect(r.decided.find((d) => d.step === "2a")?.answer).toBe("go ahead with extra checks");
    expect(r.notes).toEqual(["The owner's answer for step 1d: Theme? → light"]);
    expect(r.extraChecks.map((c) => c.step)).toEqual(["1c", "2a"]);
    expect(r.holds).toEqual([
      { step: "1b", why: "a question was left open (Bank?)", by: "default" },
      { step: "2b", why: "pushes", by: "default" },
      { step: "1a", why: "renames files", by: "you" },
    ]);
    expect(r.cleared).toEqual(["1c", "1d", "2a", "2c"]);
  });

  it("a recommended answer the owner accepts for a step that needs them is theirs, not Qanuvi's", () => {
    const check = parsePreflight(
      JSON.stringify({ questions: [{ step: "1b", question: "Bank?", sort: "you", recommended: "test bank" }] }),
    );
    const r = settleCheck(check, { 0: "test bank" }, {}, {}, {});
    expect(r.decided).toEqual([]);
    expect(r.notes).toHaveLength(1);
  });

  it("writes a decided choice as a plan note that keeps it changeable", () => {
    const n = decidedNote({ step: "1a", what: "Separator?", answer: "dash", because: "URLs" });
    expect(n).toContain("Decided for you for step 1a: Separator? → dash, because URLs");
    expect(n).toContain("not hard-coded");
  });
});
