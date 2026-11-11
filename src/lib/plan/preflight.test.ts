import { describe, expect, it } from "vitest";
import { parsePreflight, preflightPrompt, waitsBefore } from "./preflight";

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
});
