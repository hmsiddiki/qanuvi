/** 24z3 machine names: the name rules and the colours. */
import { describe, expect, it } from "vitest";
import { COLORS, NAME_MAX, nameKey, nameProblem, pickColor, uniqueName } from "./machineNames";

describe("machine names", () => {
  it("takes a name as typed and refuses the ones that would mix machines up", () => {
    expect(nameProblem("vps-Frankfurt", ["Home"])).toBe("");
    expect(nameProblem("   ")).toBe("Give it a name.");
    expect(nameProblem("home", ["Home"])).toBe('Another machine is already named "Home".');
    expect(nameProblem(" HOME ", ["Home"])).toContain("already named");
    expect(nameProblem("This PC")).toContain("what Qanuvi calls this computer");
    expect(nameProblem("this computer")).toContain("what Qanuvi calls this computer");
    expect(nameProblem("x".repeat(NAME_MAX))).toBe("");
    expect(nameProblem("x".repeat(NAME_MAX + 1))).toContain(`at most ${NAME_MAX}`);
    expect(nameProblem("VP\u200BS")).toContain("cannot be seen");
    expect(nameProblem("VPS\u202E")).toContain("cannot be seen");
    // Letters from any language count once each.
    expect(nameProblem("Büro-Rechner 😀")).toBe("");
  });

  it("checks a device name only for what can be sent", () => {
    expect(nameProblem("This PC", [], true)).toBe("");
    expect(nameProblem("a\u0007b", [], true)).toContain("cannot be seen");
  });

  it("makes a unique name from what a machine says about itself", () => {
    expect(uniqueName("basevm378", [])).toBe("basevm378");
    expect(uniqueName("basevm378", ["BaseVM378"])).toBe("basevm378 (2)");
    expect(uniqueName("basevm378", ["basevm378", "basevm378 (2)"])).toBe("basevm378 (3)");
    expect(uniqueName("This PC", [])).toBe("This PC (2)");
    expect(uniqueName("  in\u200Bvisible  ", [])).toBe("invisible");
    expect(uniqueName("\u200B", [])).toBe("");
    const long = uniqueName("y".repeat(60), ["y".repeat(NAME_MAX)]);
    expect([...long].length).toBe(NAME_MAX);
    expect(long.endsWith(" (2)")).toBe(true);
    expect(nameKey(" Ａｂ ")).toBe("ab");
  });

  it("picks a colour no other machine has", () => {
    expect(pickColor([])).toBe(COLORS[0]);
    expect(pickColor([COLORS[0], undefined])).toBe(COLORS[1]);
    expect(COLORS).toContain(pickColor([...COLORS]));
  });
});
