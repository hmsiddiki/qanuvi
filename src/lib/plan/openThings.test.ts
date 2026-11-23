import { describe, expect, it } from "vitest";
import type { InboxItem } from "$lib/stores/inbox.svelte";
import { openThings, stepKey } from "./openThings";

const review = (status: InboxItem["status"], file = "PLAN.md"): InboxItem => ({
  id: "r1",
  at: 1,
  type: "answer",
  kind: "run",
  title: "To look at",
  text: "",
  read: false,
  status,
  source: {
    kind: "review",
    projectId: "p1",
    file,
    runId: "run1",
    entries: [
      { number: "3", title: "Cart page", what: "the page looks right" },
      { number: "3", title: "Cart page", what: "Left for later by step 3, and no open step does it: totals" },
      { title: "Closing", what: "the wording" },
    ],
  },
});

describe("ticked steps with things open", () => {
  it("names a step's open things from the plan's open review list", () => {
    const open = openThings([review("open")], "p1", "PLAN.md");
    expect(open.get(stepKey({ number: "3", title: "Cart page" }))).toHaveLength(2);
    expect(open.get(stepKey({ title: "Closing" }))).toEqual(["the wording"]);
    expect(open.has(stepKey({ number: "4", title: "Other" }))).toBe(false);
  });

  it("has nothing once the list was answered, or for another plan", () => {
    expect(openThings([review("answered")], "p1", "PLAN.md").size).toBe(0);
    expect(openThings([review("open", "OTHER.md")], "p1", "PLAN.md").size).toBe(0);
    expect(openThings([review("open")], "p2", "PLAN.md").size).toBe(0);
  });
});
