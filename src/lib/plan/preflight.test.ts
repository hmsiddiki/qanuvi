import { describe, expect, it } from "vitest";
import {
  answersPrompt,
  clipReason,
  decidedNote,
  maskedValue,
  parseAnswerReadings,
  parsePreflight,
  preflightPrompt,
  readAnswers,
  recommendedRisk,
  secretNeedsReading,
  settleCheck,
  typedAnswers,
  withReadHolds,
  unsettledWhy,
  type AnswerReading,
} from "./preflight";

describe("p2: the check's answers read by meaning", () => {
  const check = parsePreflight(
    JSON.stringify({
      secrets: [
        { name: "DB_HOST", what: "Where the test database is", example: "localhost:3306", steps: ["2a"] },
        { name: "DB_PASSWORD", what: "The test database password", steps: ["2a", "2b"] },
        { name: "API_KEY", what: "The test key", steps: [] },
        { name: "SHOP_KEY", what: "The shop key", steps: ["3"] },
      ],
      questions: [
        { step: "4a", question: "Which currency format?", sort: "decide", recommended: "1,234.00" },
        { step: "4b", question: "Which bank?", sort: "you" },
        { step: "4c", question: "Which theme?", sort: "decide", recommended: "dark" },
      ],
    }),
  );
  const reading = (id: string, kind: AnswerReading["kind"], more: Partial<AnswerReading> = {}): AnswerReading => ({
    id,
    kind,
    value: "",
    where: "",
    copyFrom: null,
    skip: false,
    reply: "",
    ...more,
  });

  it("never sends a one-piece secret to a model", () => {
    expect(secretNeedsReading("hunter2")).toBe(false);
    expect(secretNeedsReading("  mysql:host=127.0.0.1  ")).toBe(false);
    expect(secretNeedsReading("find the mysql address yourself")).toBe(true);
    const items = typedAnswers(
      check,
      { DB_HOST: "find it in the config", DB_PASSWORD: "hunter2" },
      { 0: "1,234.00", 1: "the test one", 2: "light" },
      { 1: true, 2: true },
    );
    expect(items.map((x) => x.id)).toEqual(["secret:DB_HOST", "question:1", "question:2"]);
    expect(items[0].ask).toBe("Where the test database is (DB_HOST)");
    expect(JSON.stringify(items)).not.toContain("hunter2");
  });

  it("asks to sort each answer and to try instructions read-only", () => {
    const p = answersPrompt(
      "PLAN.md",
      [{ id: "secret:DB_HOST", ask: "Where (DB_HOST)", example: "localhost", answer: 'use "the" same' }],
      { secrets: ["DB_USER"], others: [{ name: "qchmmart", path: "E:/www/qchmmart", secrets: ["DB_HOST"] }] },
    );
    expect(p).toContain('answer: "use \\"the\\" same"');
    expect(p).toContain("qchmmart (E:/www/qchmmart): DB_HOST");
    expect(p).toContain("Known secrets of this project (names only): DB_USER");
    expect(p).toContain("Change nothing");
    expect(answersPrompt("P.md", [], { secrets: [], others: [] })).toContain("- none");
  });

  it("reads the answer, keeping only known ids", () => {
    const r = parseAnswerReadings(
      "Sure: " +
        JSON.stringify({
          answers: [
            { id: "secret:DB_HOST", kind: "instruction", value: "localhost:3307", where: "config.php" },
            { id: "secret:DB_HOST", kind: "value", value: "twice" },
            { id: "question:1", kind: "unclear", reply: "A bank is where…", where: "x", skip: true },
            { id: "question:2", kind: "maybe" },
            { id: "other", kind: "value" },
            { id: "secret:SHOP_KEY", kind: "instruction", copyFrom: { project: "qchmmart", name: "SHOP_KEY" } },
            [1],
          ],
        }),
      ["secret:DB_HOST", "question:1", "question:2", "secret:SHOP_KEY"],
    );
    expect(r.map((x) => [x.id, x.kind])).toEqual([
      ["secret:DB_HOST", "instruction"],
      ["question:1", "unclear"],
      ["secret:SHOP_KEY", "instruction"],
    ]);
    expect(r[0].where).toBe("config.php");
    expect(r[1]).toMatchObject({ where: "", skip: false });
    expect(r[2].copyFrom).toEqual({ project: "qchmmart", name: "SHOP_KEY" });
    expect(parseAnswerReadings("no json", ["a"])).toEqual([]);
    expect(parseAnswerReadings("null {", ["a"])).toEqual([]);
  });

  it("saves values, offers what was found, and holds what is unclear", () => {
    const secrets = {
      DB_HOST: "find the mysql address yourself",
      DB_PASSWORD: "hunter2",
      API_KEY: "what is this?",
      SHOP_KEY: "the same as qchmmart",
    };
    const answers = { 0: "1,234.00", 1: "dont get it", 2: "you pick" };
    const touched = { 1: true, 2: true };
    const readings = [
      reading("secret:DB_HOST", "instruction", { value: "localhost:3307", where: "config.php" }),
      reading("secret:API_KEY", "unclear", { reply: "It is the key from the test dashboard, like sk_test_123." }),
      reading("secret:SHOP_KEY", "instruction", { copyFrom: { project: "qchmmart", name: "SHOP_KEY" } }),
      reading("question:1", "unclear", { reply: "Which bank the payments go to" }),
      reading("question:2", "instruction", { skip: true }),
    ];
    const r = readAnswers(check, secrets, answers, touched, readings, {}, "1");
    expect(r.save).toEqual([{ name: "DB_PASSWORD", value: "hunter2" }]);
    expect(r.offered.map((f) => f.id)).toEqual(["secret:DB_HOST", "secret:SHOP_KEY"]);
    expect(r.holds.map((h) => h.step)).toEqual(["2a", "1", "3", "4b"]);
    expect(r.holds[1].why).toBe(
      'it needs the test key (API_KEY). Before the run you wrote "what is this?"; It is the key from the test dashboard, like sk_test_123. Give the value, or say where Qanuvi can find it',
    );
    expect(r.answers[1]).toBe("");
    expect(r.answers[2]).toBe("dark");
    expect(r.answersTouched[2]).toBe(false);

    const yes = readAnswers(
      check,
      secrets,
      answers,
      touched,
      readings,
      { "secret:DB_HOST": true, "secret:SHOP_KEY": true },
      "1",
    );
    expect(yes.save).toEqual([
      { name: "DB_HOST", value: "localhost:3307" },
      { name: "DB_PASSWORD", value: "hunter2" },
    ]);
    expect(yes.copy).toEqual([{ name: "SHOP_KEY", project: "qchmmart", from: "SHOP_KEY" }]);
    expect(yes.offered).toEqual([]);
    expect(yes.holds.map((h) => h.step)).toEqual(["1", "4b"]);

    const no = readAnswers(check, secrets, answers, touched, readings, { "secret:DB_HOST": false }, "1");
    expect(no.offered.map((f) => f.id)).toEqual(["secret:SHOP_KEY"]);
    expect(no.holds.map((h) => h.step)).toContain("2a");
  });

  it("holds a sentence nobody could read, and keeps a typed choice as before", () => {
    const r = readAnswers(check, { DB_HOST: "the usual one" }, { 2: "light please" }, { 2: true }, [], {}, "1");
    expect(r.save).toEqual([]);
    expect(r.holds).toEqual([
      {
        step: "2a",
        why: 'it needs where the test database is (DB_HOST). Before the run you wrote "the usual one". Give the value, or say where Qanuvi can find it',
        by: "default",
      },
    ]);
    expect(r.answers[2]).toBe("light please");
    const v = readAnswers(
      check,
      { DB_HOST: "it is localhost 3306" },
      { 2: "light please" },
      { 2: true },
      [
        reading("secret:DB_HOST", "value", { value: "localhost:3306" }),
        reading("question:2", "value", { value: "light" }),
      ],
      {},
      "1",
    );
    expect(v.save).toEqual([{ name: "DB_HOST", value: "localhost:3306" }]);
    expect(v.answers[2]).toBe("light");
  });

  it("asks both when a login and a choice hold the same step (p3, driven live)", () => {
    const both = parsePreflight(
      JSON.stringify({
        secrets: [{ name: "NOTES_DB_PASSWORD", what: "The password of the MySQL user", steps: ["3"] }],
        questions: [{ step: "3", question: "Should step 3 connect to MySQL?", sort: "you" }],
      }),
    );
    const r = readAnswers(
      both,
      { NOTES_DB_PASSWORD: "look for it in the config file" },
      { 0: "what is mysql2?" },
      { 0: true },
      [
        reading("secret:NOTES_DB_PASSWORD", "instruction", { reply: "the config has no password." }),
        reading("question:0", "unclear", { reply: "mysql2 is a free add-on library." }),
      ],
      {},
      "1",
    );
    expect(r.holds).toEqual([
      {
        step: "3",
        why:
          'it needs the password of the MySQL user (NOTES_DB_PASSWORD). Before the run you wrote "look for it in the ' +
          'config file"; the config has no password. Give the value, or say where Qanuvi can find it; also it needs ' +
          'your answer to: Should step 3 connect to MySQL? Before the run you wrote "what is mysql2?"; mysql2 is a ' +
          "free add-on library. Answer it in your own words",
        by: "default",
      },
    ]);
  });

  it("keeps names such as MySQL or API as written mid-sentence", () => {
    expect(unsettledWhy("MySQL login (A)", "x", "")).toContain("it needs MySQL login");
    expect(unsettledWhy("API key (A)", "x", "")).toContain("it needs API key");
    expect(unsettledWhy("A_KEY", "x", "")).toContain("it needs A_KEY.");
    expect(unsettledWhy("Password (A)", "x", "")).toContain("it needs password (A)");
  });

  it("puts the read holds in place of the default ones", () => {
    const settled = settleCheck(check, { 0: "1,234.00", 1: "", 2: "dark" }, {}, {}, {});
    const r = withReadHolds({ ...settled, holds: [...settled.holds, { step: "4c", why: "renames", by: "you" }] }, [
      { step: "4b", why: "it needs a bank", by: "default" },
      { step: "4c", why: "it needs a theme", by: "default" },
      { step: "4a", why: "it needs a format", by: "default" },
    ]);
    expect(r.holds).toEqual([
      { step: "4c", why: "renames; also it needs a theme", by: "you" },
      { step: "4b", why: "it needs a bank", by: "default" },
      { step: "4a", why: "it needs a format", by: "default" },
    ]);
    expect(r.cleared).toEqual([]);
  });

  it("shows only the start of a found secret", () => {
    expect(maskedValue("abc")).toBe("•••");
    expect(maskedValue("localhost:3307")).toBe("lo••••••••");
  });
});

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
    expect(parsePreflight("no json here")).toEqual({ secrets: [], questions: [], risky: [], setup: [], order: [] });
    const o = parsePreflight(
      '{"order":[{"step":"1b","after":["1c","1b","1c","a b"],"why":"README lists the final exports"},{"step":"2","after":"1"},{"step":"3","after":[]}]}',
    );
    expect(o.order).toEqual([
      { step: "1b", after: ["1c"], why: "README lists the final exports" },
      { step: "2", after: ["1"], why: "" },
    ]);
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

  it("asks for secrets the test runner really reads, explained in plain words with an example", () => {
    const p = preflightPrompt("PLAN.md", [], { secrets: ["DB_USER"], testCommand: "", previewUrl: "", notes: [] });
    expect(p).toContain("look at how the project's test command or test runner reads its settings");
    expect(p).toContain("a variable the runner fills in by itself is not missing");
    expect(p).toContain("for someone who does not know the code");
    const c = parsePreflight(
      JSON.stringify({
        secrets: [
          {
            name: "QC_TEST_SERVER_DSN",
            what: "Where the test database server is",
            example: "mysql:host=127.0.0.1;port=3306",
            ifEmpty: "the database tests are skipped",
            why: "the runner makes its own throwaway database there",
            steps: ["2"],
          },
          { name: "OLD_STYLE", why: "an answer without the new parts still reads", steps: [] },
        ],
      }),
    );
    expect(c.secrets[0]).toMatchObject({
      what: "Where the test database server is",
      example: "mysql:host=127.0.0.1;port=3306",
      ifEmpty: "the database tests are skipped",
    });
    expect(c.secrets[1]).toMatchObject({ name: "OLD_STYLE", what: "", example: "", ifEmpty: "" });
  });

  it("ends a long reason at a word with an ellipsis", () => {
    const long = "word ".repeat(80).trim();
    const c = clipReason(long);
    expect(c.length).toBeLessThanOrEqual(300);
    expect(c.endsWith("word…")).toBe(true);
    expect(clipReason("short")).toBe("short");
    const noSpace = "x".repeat(400);
    expect(clipReason(noSpace)).toBe("x".repeat(299) + "…");
  });
});
