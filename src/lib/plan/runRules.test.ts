import { describe, expect, it } from "vitest";
import { DEFAULT_RULES, parsePatterns, rulesFacts, rulesLine, rulesOf } from "./runRules";

describe("run rules", () => {
  it("fills in what an older project record lacks", () => {
    expect(rulesOf(undefined)).toEqual(DEFAULT_RULES);
    expect(rulesOf({ run_rules: { set: true, allow: ["make docs"] } })).toEqual({
      set: true,
      branch: false,
      allow: ["make docs"],
      never: [],
    });
  });

  it("reads one pattern per line, list marks and repeats left out", () => {
    expect(parsePatterns("  make test \n\n- php artisan *\nmake test\n")).toEqual(["make test", "php artisan *"]);
  });

  it("tells the check before the run what is already decided", () => {
    const facts = rulesFacts({ set: true, branch: false, allow: ["make docs"], never: ["npm run e2e"] });
    expect(facts).toContain("Nothing goes into git");
    expect(facts).toContain("Also allowed: make docs");
    expect(facts).toContain("Never: npm run e2e");
    expect(rulesFacts({ ...DEFAULT_RULES, branch: true })).toContain("own git branch");
    expect(facts).toContain("its step is set aside");
    expect(rulesFacts(DEFAULT_RULES, true)).toContain("the agent judges safe may run");
  });

  it("says the answered card in one line, the git part only for git projects", () => {
    expect(rulesLine(DEFAULT_RULES, false)).toBe("tests and builds may run · never publishes or touches main");
    expect(rulesLine({ ...DEFAULT_RULES, allow: ["a"] }, true)).toBe(
      "tests and builds may run · never publishes or touches main · Qanuvi's own copies · 1 rule of your own",
    );
    expect(rulesLine(DEFAULT_RULES, false, true)).toBe(
      "Auto: the agent judges the rest · never publishes or touches main",
    );
  });
});
