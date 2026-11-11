import { describe, expect, it } from "vitest";
import { parseKnowledge } from "./knowledge";
import { BUILT_IN_GUIDES } from "./guides";

const good = {
  format: 1,
  version: 2,
  published: "2026-11-12",
  note: "Groq added",
  guides: [BUILT_IN_GUIDES[0]],
  freeProviders: [{ id: "groq", name: "Groq", how: "key", url: "https://console.groq.com/keys", note: "fast" }],
  freeProvidersCheckedOn: "2026-11-12",
};

describe("parseKnowledge", () => {
  it("takes a good file", () => {
    const k = parseKnowledge(JSON.stringify(good));
    expect(typeof k).toBe("object");
    if (typeof k === "object") {
      expect(k.version).toBe(2);
      expect(k.freeProviders[0].id).toBe("groq");
      expect(k.guides).toHaveLength(1);
    }
  });

  it("refuses what it cannot trust", () => {
    expect(parseKnowledge("not json")).toContain("not JSON");
    expect(parseKnowledge(JSON.stringify({ ...good, format: 2 }))).toContain("newer Qanuvi");
    expect(parseKnowledge(JSON.stringify({ ...good, version: 0 }))).toContain("version");
    const http = { ...good, freeProviders: [{ ...good.freeProviders[0], url: "http://x" }] };
    expect(parseKnowledge(JSON.stringify(http))).toContain("https");
    expect(parseKnowledge(JSON.stringify({ ...good, guides: undefined, freeProviders: undefined }))).toContain(
      "nothing",
    );
  });
});
