import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const store = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => store.set(k, v),
});

const { parseBrain, parseSettings, settings, DEFAULT_MODEL_PROVIDERS } = await import("./settings.svelte");
const { ownSetup, setupFor } = await import("./brain.svelte");
const { PROVIDERS, QANUVI_AGENT } = await import("$lib/agents/providers");
const { reduce } = await import("$lib/agent/events");

describe("Qanuvi agent settings", () => {
  it("starts with Ollama, OpenRouter and OmniRoute and no models", () => {
    const s = parseSettings(null);
    expect(s.brain.providers.map((p) => p.id)).toEqual(["ollama", "openrouter", "omniroute"]);
    expect(s.brain.models).toEqual([]);
  });

  it("keeps the approval minutes between 1 and 240, 10 by default", () => {
    expect(parseSettings(null).approveMinutes).toBe(10);
    expect(parseSettings(JSON.stringify({ approveMinutes: 15 })).approveMinutes).toBe(15);
    expect(parseSettings(JSON.stringify({ approveMinutes: 9999 })).approveMinutes).toBe(240);
    expect(parseSettings(JSON.stringify({ approveMinutes: "x" })).approveMinutes).toBe(10);
  });

  it("keeps only sound providers and models that belong to one", () => {
    const b = parseBrain({
      providers: [
        ...DEFAULT_MODEL_PROVIDERS,
        { id: "bad id", kind: "openai", url: "http://x" },
        { id: "llamacpp", kind: "weird", url: "http://127.0.0.1:8080/v1" },
        { id: "nourl", kind: "openai", url: "ftp://x" },
      ],
      models: [
        { id: "ollama/qwen2.5-coder:3b", label: "qwen2.5-coder:3b", local: true, free: true, context: 16384 },
        { id: "ollama/qwen2.5-coder:3b", label: "twice" },
        { id: "nowhere/model", label: "x" },
        { id: "has space/x" },
      ],
    });
    expect(b.providers.map((p) => p.id)).toEqual(["ollama", "openrouter", "omniroute", "llamacpp"]);
    expect(b.providers[3].kind).toBe("openai");
    expect(b.models.map((m) => m.id)).toEqual(["ollama/qwen2.5-coder:3b"]);
  });

  it("sends only the providers the chosen models need, without keys", () => {
    settings.set("brain", {
      local: true,
      omniAssist: true,
      providers: DEFAULT_MODEL_PROVIDERS,
      models: [
        { id: "openrouter/qwen/qwen3-coder:free", label: "qwen3-coder", local: false, free: true, context: 262144 },
        { id: "ollama/qwen2.5-coder:3b", label: "qwen2.5-coder:3b", local: true, free: true, context: 16384 },
      ],
    });
    const s = ownSetup();
    expect(s.endpoints.map((e) => e.id).sort()).toEqual(["ollama", "openrouter"]);
    expect(JSON.stringify(s)).not.toContain("key");
    expect(s.models[0].id).toBe("openrouter/qwen/qwen3-coder:free");
    expect(setupFor("claude-code")).toBeNull();
    // The agent list offers "Auto" and the chosen models.
    expect(QANUVI_AGENT.models.map((m) => m.id)).toEqual([
      "auto",
      "openrouter/qwen/qwen3-coder:free",
      "ollama/qwen2.5-coder:3b",
    ]);
    expect(PROVIDERS.map((p) => p.id)).toContain("qanuvi");
  });
});

describe("a session of Qanuvi's agent", () => {
  it("shows when the brain hands a turn to another model", () => {
    const items = reduce(
      [],
      JSON.stringify({ type: "system", subtype: "qanuvi_route", text: "a could not answer (429); b takes over." }),
      1,
    );
    expect(items).toEqual([
      { kind: "notice", id: "n1", text: "a could not answer (429); b takes over.", tone: "info" },
    ]);
  });

  it("shows what an edit replaces before it is approved", () => {
    const ask = (name: string, input: object) =>
      reduce(
        [],
        JSON.stringify({
          type: "control_request",
          request_id: "r1",
          request: { subtype: "can_use_tool", tool_name: name, input },
        }),
        1,
      )[0];
    const edit = ask("Edit", { file_path: "notes.txt", old_string: "new notes", new_string: "new notes\nReviewed" });
    expect(edit).toMatchObject({
      kind: "approval",
      detail: "notes.txt\n\nReplace:\nnew notes\n\nWith:\nnew notes\nReviewed",
    });
    const write = ask("Write", { file_path: "notes.txt", content: "Reviewed" });
    expect(write).toMatchObject({ detail: "notes.txt (the whole file becomes)\n\nReviewed" });
  });
});

describe("start commands for other local servers", () => {
  it("keeps a provider's start command on one line", () => {
    const b = parseBrain({
      providers: [
        {
          id: "llamacpp",
          name: "llama.cpp",
          kind: "openai",
          url: "http://127.0.0.1:8080/v1",
          start: "llama-server\n-m a.gguf",
        },
      ],
      models: [],
    });
    expect(b.providers[0].start).toBe("llama-server -m a.gguf");
    expect(
      parseBrain({ providers: [{ id: "x", kind: "openai", url: "http://127.0.0.1:1/v1" }] }).providers[0].start,
    ).toBeUndefined();
  });
});

describe("online models only", () => {
  it("leaves models on this computer out when the user turns them off", () => {
    const brain = {
      providers: DEFAULT_MODEL_PROVIDERS,
      models: [
        { id: "ollama/qwen2.5-coder:3b", label: "qwen", local: true, free: true, context: 0 },
        { id: "openrouter/a:free", label: "a", local: false, free: true, context: 0 },
      ],
      local: true,
      omniAssist: true,
    };
    settings.set("brain", brain);
    expect(ownSetup().models.map((m) => m.id)).toEqual(["ollama/qwen2.5-coder:3b", "openrouter/a:free"]);
    settings.set("brain", { ...brain, local: false });
    expect(ownSetup().models.map((m) => m.id)).toEqual(["openrouter/a:free"]);
    expect(ownSetup().endpoints.map((e) => e.id)).toEqual(["openrouter"]);
    expect(parseBrain({}).local).toBe(true);
    expect(parseBrain({ local: false }).local).toBe(false);
  });
});
