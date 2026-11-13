import { describe, expect, it } from "vitest";
import { asksSecret, DECIDE_TEXT, holdForm, holdKey, holdReply } from "./holdask";

describe("held steps as Inbox questions", () => {
  it("keys one question per plan step", () => {
    expect(holdKey("p1", "docs/PLAN.md", "1c")).toBe("hold:p1:docs/PLAN.md:1c");
  });

  it("offers go, decide and skip for a hold, with a text box", () => {
    const f = holdForm("hold", "it pushes to the live server");
    expect(f.choices.map((c) => c.id)).toEqual(["go", "decide", "skip"]);
    expect(f.choices[0].label).toBe("Go ahead");
    expect(f).toHaveProperty("text");
    expect(f).not.toHaveProperty("secret");
  });

  it("offers try again and skip for a failed step", () => {
    const f = holdForm("failed", "the tests kept failing");
    expect(f.choices.map((c) => c.label)).toEqual(["Try again", "Skip this step"]);
  });

  it("never asks for a login in the Inbox", () => {
    expect(asksSecret("it needs the FTP password")).toBe(true);
    expect(asksSecret("needs an API key for the mail service")).toBe(true);
    expect(asksSecret("sign in to the admin page")).toBe(true);
    expect(asksSecret("it changes the date format")).toBe(false);
    const f = holdForm("hold", "it needs the database login");
    expect(f).toMatchObject({ secret: true });
    expect(f).not.toHaveProperty("text");
    expect(f.choices.map((c) => c.id)).toEqual(["go", "skip"]);
  });

  it("turns an answer into what the step runs with", () => {
    expect(holdReply({ choice: "go", text: " yes " })).toEqual({ choice: "go", text: "yes" });
    expect(holdReply({ choice: "skip" })).toEqual({ choice: "skip", text: "" });
    expect(holdReply({ choice: "decide" })).toEqual({ choice: "go", text: DECIDE_TEXT });
    expect(holdReply({ choice: "decide", text: "keep it small" }).text).toBe(`${DECIDE_TEXT}\nkeep it small`);
  });
});
