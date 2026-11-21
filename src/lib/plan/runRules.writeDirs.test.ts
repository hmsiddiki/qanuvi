import { describe, expect, it } from "vitest";
import { rulesOf } from "./runRules";

describe("run rules keep allowed folders (wd1)", () => {
  it("keeps write_dirs the core saved, so saving the card does not drop them", () => {
    const saved = { run_rules: { set: true, branch: false, allow: ["npm test"], never: [], write_dirs: ["E:/tmp/qna-scripts"] } };
    const r = rulesOf(saved);
    expect(r.write_dirs).toEqual(["E:/tmp/qna-scripts"]);
    // What the Run panel saves on Start: the rules spread with the card's lists.
    const next = { ...r, set: true, allow: ["npm test"], never: [] };
    expect(next.write_dirs).toEqual(["E:/tmp/qna-scripts"]);
  });

  it("leaves older records exactly as before", () => {
    expect(rulesOf({ run_rules: { set: true, branch: false, allow: [], never: [] } })).toEqual({
      set: true,
      branch: false,
      allow: [],
      never: [],
    });
  });
});
