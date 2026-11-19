import { describe, expect, it } from "vitest";
import { firstMatch } from "./settingsFind";

describe("firstMatch (settings search)", () => {
  const texts = ["Agents", "Chat handover", "Each step re-reads the whole chat.", "Limit (thousand tokens, 0 = off)"];

  it("finds the whole phrase, case and spaces aside", () => {
    expect(firstMatch(texts, "  chat   HANDOVER ")).toEqual({ index: 1, start: 0, end: 13 });
  });

  it("falls back to the text holding the most words", () => {
    expect(firstMatch(texts, "tokens limit")).toEqual({ index: 3, start: 16, end: 22 });
  });

  it("answers null for nothing or an empty query", () => {
    expect(firstMatch(texts, "zebra")).toBeNull();
    expect(firstMatch(texts, "   ")).toBeNull();
  });
});
