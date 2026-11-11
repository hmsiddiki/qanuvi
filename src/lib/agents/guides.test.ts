import { describe, expect, it } from "vitest";
import {
  BUILT_IN_GUIDES,
  changes,
  jsonIn,
  parseGuides,
  recommend,
  sizeOf,
  suggestLocal,
  updatePrompt,
  type FoundModel,
} from "./guides";

const free = (name: string, context = 128_000, extra: Partial<FoundModel> = {}): FoundModel => ({
  id: `openrouter/${name}`,
  endpoint: "openrouter",
  name,
  local: false,
  free: true,
  tools: true,
  context,
  ...extra,
});

describe("setup guides", () => {
  it("ship valid guides for Ollama, OpenRouter and OmniRoute", () => {
    const parsed = parseGuides({ guides: BUILT_IN_GUIDES });
    expect(Array.isArray(parsed)).toBe(true);
    expect((parsed as unknown[]).length).toBe(3);
  });

  it("refuse an update that is unsafe or broken", () => {
    const g = structuredClone(BUILT_IN_GUIDES[0]);
    const bad = (change: (x: typeof g) => void) => {
      const c = structuredClone(g);
      change(c);
      return parseGuides({ guides: [c] });
    };
    expect(bad((x) => (x.provider = "evil"))).toMatch(/Unknown provider/);
    expect(bad((x) => (x.checkedOn = "yesterday"))).toMatch(/date/);
    expect(bad((x) => (x.steps[0].url = "javascript:alert(1)"))).toMatch(/not a web address/);
    expect(bad((x) => (x.steps[0].url = "http://example.com"))).toMatch(/not a web address/);
    expect(bad((x) => (x.steps[1] = { title: "Run", text: "x", action: "copy", command: "a\nrm -rf /" }))).toMatch(
      /one line/,
    );
    expect(bad((x) => (x.steps[0].action = "delete" as never))).toMatch(/unknown action/);
    expect(parseGuides(null)).toMatch(/No guides/);
  });

  it("read an agent's answer, even wrapped in text, and say what changed", () => {
    const next = structuredClone(BUILT_IN_GUIDES[0]);
    next.checkedOn = "2027-01-02";
    next.steps[4].text = "New text.";
    const answer = "Here you go:\n```json\n" + JSON.stringify({ guides: [next] }) + "\n```";
    const parsed = parseGuides(jsonIn(answer));
    expect(Array.isArray(parsed)).toBe(true);
    expect(changes(BUILT_IN_GUIDES[0], (parsed as typeof BUILT_IN_GUIDES)[0])).toBe("1 changed");
    expect(updatePrompt(BUILT_IN_GUIDES, "2027-01-02")).toContain("Set checkedOn to 2027-01-02");
  });
});

describe("recommending free models", () => {
  it("reads sizes from names", () => {
    expect(sizeOf("qwen3-coder-30b-a3b:free")).toBe(30);
    expect(sizeOf("liquid/lfm-3-1.7b:free")).toBe(1.7);
    expect(sizeOf("deepseek-chat")).toBe(0);
  });

  it("prefers coding and larger models, leaves tiny and toolless ones out, keeps the free router last", () => {
    const picks = recommend([
      free("openrouter/free"),
      free("liquid/lfm-3-1.7b:free"),
      free("google/gemma-4-31b-it:free", 262_144),
      free("cohere/north-mini-code:free", 256_000),
      free("big/thing-550b:free", 1_000_000, { tools: false }),
      free("paid/model", 128_000, { free: false }),
    ]);
    expect(picks.map((p) => p.model.name)).toEqual([
      "google/gemma-4-31b-it:free",
      "cohere/north-mini-code:free",
      "openrouter/free",
    ]);
    expect(picks[0].reasons).toContain("large (31B)");
  });

  it("puts OmniRoute's coding route first and learns from finished turns", () => {
    const picks = recommend([free("auto/coding"), free("a/x-coder-7b:free")]);
    expect(picks[0].model.name).toBe("auto/coding");
    const learned = recommend([free("a/m-30b:free"), free("b/m-30b:free")], {
      "openrouter/b/m-30b:free": { turns: 10, finished: 10 },
      "openrouter/a/m-30b:free": { turns: 10, finished: 2 },
    });
    expect(learned[0].model.name).toBe("b/m-30b:free");
  });
});

describe("local models for this computer", () => {
  it("fits a coder and a thinking model to the graphics card", () => {
    const s = suggestLocal({
      ramGb: 15.3,
      gpu: { name: "RTX 3050 Ti", vramGb: 4, kind: "nvidia" },
      os: "windows",
      arch: "x86_64",
    });
    expect(s.coder?.model.name).toBe("qwen2.5-coder:3b");
    expect(s.chat?.model.name).toBe("qwen3:4b");
    expect(s.chat?.fast).toBe(true);
  });

  it("uses bigger models with a big card, and slower memory-only ones without a card", () => {
    const big = suggestLocal({
      ramGb: 64,
      gpu: { name: "RTX 4090", vramGb: 24, kind: "nvidia" },
      os: "linux",
      arch: "x86_64",
    });
    expect(big.coder?.model.name).toBe("qwen3-coder:30b");
    expect(big.chat?.model.name).toBe("qwen3.8:27b");
    const none = suggestLocal({ ramGb: 16, gpu: null, os: "windows", arch: "x86_64" });
    expect(none.coder?.fast).toBe(false);
    expect(none.coder?.model.name).toBe("qwen2.5-coder:7b");
    expect(suggestLocal({ ramGb: 1, gpu: null, os: "linux", arch: "x86_64" }).coder).toBeNull();
  });
});

describe("free models through OmniRoute", () => {
  it("ranks frontier models first, though their names carry no size", () => {
    const omni = (name: string) => ({ ...free(name), id: `omniroute/${name}`, endpoint: "omniroute" });
    const picks = recommend([
      omni("antigravity/gemini-3.8-flash-high"),
      omni("antigravity/gpt-oss-120b-medium"),
      omni("antigravity/claude-opus-4-6-thinking"),
      omni("antigravity/claude-sonnet-4-6"),
      omni("antigravity/gemini-3.1-flash-lite"),
    ]);
    expect(picks.map((p) => p.model.name)).toEqual([
      "antigravity/claude-sonnet-4-6",
      "antigravity/claude-opus-4-6-thinking",
      "antigravity/gpt-oss-120b-medium",
    ]);
    expect(picks[0].reasons).toContain("a frontier model");
  });
});

describe("telling the truth about local models", () => {
  it("says a 4 GB card is for questions, a 24 GB card for coding, and too little memory for none", async () => {
    const { localVerdict } = await import("./guides");
    const laptop = localVerdict({
      ramGb: 15.3,
      gpu: { name: "RTX 3050 Ti", vramGb: 4, kind: "nvidia" },
      os: "windows",
      arch: "x86_64",
    });
    expect(laptop.level).toBe("questions");
    expect(laptop.text).toContain("not good enough for real coding");
    expect(
      localVerdict({ ramGb: 64, gpu: { name: "RTX 4090", vramGb: 24, kind: "nvidia" }, os: "linux", arch: "x86_64" })
        .level,
    ).toBe("coding");
    expect(localVerdict({ ramGb: 1, gpu: null, os: "linux", arch: "x86_64" }).level).toBe("none");
  });
});
