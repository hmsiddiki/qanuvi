import { describe, expect, it } from "vitest";
import { parsePlan } from "./tasklist";
import { taskListPrompt } from "./prompts";

describe("taskListPrompt", () => {
  it("names the file and shows a step format that Qanuvi itself can read", () => {
    const prompt = taskListPrompt("PLAN.md");
    expect(prompt).toContain("PLAN.md");
    const example = prompt.slice(prompt.indexOf("## Section name"), prompt.indexOf("For each step"));
    const step = parsePlan(example).steps[0];
    expect(step).toMatchObject({ number: "1", title: "Step title", model: "claude-opus-5-5" });
    expect(step.details).toHaveLength(2);
  });
});
