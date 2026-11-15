import { describe, expect, it } from "vitest";
import { DECIDE_TEXT, holdReply } from "./holdask";

describe("held steps as Inbox questions", () => {
  it("turns an answer into what the step runs with", () => {
    expect(holdReply({ choice: "go", text: " yes " })).toEqual({ choice: "go", text: "yes" });
    expect(holdReply({ choice: "skip" })).toEqual({ choice: "skip", text: "" });
    expect(holdReply({ choice: "decide" })).toEqual({ choice: "go", text: DECIDE_TEXT });
    expect(holdReply({ choice: "decide", text: "keep it small" }).text).toBe(`${DECIDE_TEXT}\nkeep it small`);
  });
});
