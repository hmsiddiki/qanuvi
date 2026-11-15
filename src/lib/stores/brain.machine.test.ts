import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke } = await import("@tauri-apps/api/core");
const { nameMachines, showMachine } = await import("$lib/remote");
const { brain, keyEnvName } = await import("./brain.svelte");
const { settings } = await import("./settings.svelte");

/** What each machine answers: "" is This PC, "vps" a master. */
type Answer = (cmd: string, args: Record<string, unknown>) => unknown;
let answers: Record<string, Answer> = {};

const openrouter = () => settings.value.brain.providers.find((p) => p.id === "openrouter")!;
const ollama = () => settings.value.brain.providers.find((p) => p.id === "ollama")!;

beforeEach(async () => {
  showMachine(null);
  nameMachines((id) => (id === "vps" ? "VPS" : id));
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd === "remote_call") {
      const a = args as { master: string; cmd: string; args: Record<string, unknown> };
      return answers[a.master](a.cmd, a.args);
    }
    return answers[""](cmd, (args ?? {}) as Record<string, unknown>);
  });
  answers = {
    "": (cmd, a) =>
      cmd === "model_key_store" ? "store" : cmd === "model_key_has" ? a.endpoint === "openrouter" : { models: {} },
  };
  await brain.switched();
});

describe("the Models page follows the shown machine (24z3 D1)", () => {
  it("reads This PC's keys from its credential store", () => {
    expect(brain.keyStore).toBe("store");
    expect(brain.keys).toEqual({ openrouter: true, omniroute: false });
    expect(brain.keyError).toBe("");
  });

  it("shows a server's keys as kept in its environment, and its own records", async () => {
    answers.vps = (cmd, a) => {
      if (cmd === "model_key_store") return "environment";
      if (cmd === "model_key_has") return a.endpoint === "omniroute";
      if (cmd === "brain_records") return { models: { "openrouter/x": { turns: 3 } } };
      throw new Error(`unexpected ${cmd}`);
    };
    vi.mocked(invoke).mockClear();
    showMachine("vps");
    await brain.switched();
    expect(brain.keyStore).toBe("environment");
    expect(brain.keys).toEqual({ openrouter: false, omniroute: true });
    expect(Object.keys(brain.records)).toEqual(["openrouter/x"]);
    // Every call went to the master, none to This PC.
    expect(vi.mocked(invoke).mock.calls.map(([cmd]) => cmd)).toEqual(Array(4).fill("remote_call"));
  });

  it("says plainly when an older master cannot list its keys, never 'no key'", async () => {
    answers.vps = (cmd) => {
      throw `This master does not know the command ${cmd}.`;
    };
    showMachine("vps");
    await brain.switched();
    expect(brain.keys).toEqual({});
    expect(brain.keyError).toBe("This cannot be done on VPS yet (model_key_store).");
    expect(brain.records).toEqual({});
  });

  it("drops what the machine before found, and its late answers", async () => {
    let release: (v: unknown) => void = () => {};
    answers[""] = (cmd) => {
      if (cmd === "models_list") return new Promise((r) => (release = r));
      return cmd === "model_key_store" ? "store" : cmd === "model_key_has" ? false : { models: {} };
    };
    const finding = brain.find(openrouter());
    answers.vps = (cmd) =>
      cmd === "model_key_store" ? "environment" : cmd === "model_key_has" ? false : { models: {} };
    showMachine("vps");
    const count = brain.shownCount;
    await brain.switched();
    expect(brain.shownCount).toBe(count + 1);
    release([{ id: "openrouter/old", endpoint: "openrouter", name: "old" }]);
    await finding;
    expect(brain.found).toEqual({});
    expect(brain.searching).toEqual({});
  });

  it("names the master when its local program is not installed", async () => {
    answers.vps = (cmd) => {
      if (cmd === "models_list") throw "Ollama could not be reached at http://127.0.0.1:11434.";
      if (cmd === "local_server_status") return { program: "ollama", installed: false, path: "", start: "" };
      return cmd === "model_key_store" ? "environment" : cmd === "model_key_has" ? false : { models: {} };
    };
    showMachine("vps");
    await brain.switched();
    await brain.find(ollama());
    expect(brain.errors.ollama).toBe("Ollama is not installed on VPS. The steps below install it.");
  });

  it("names the environment variable as the master's core does", () => {
    expect(keyEnvName("openrouter")).toBe("QANUVI_KEY_OPENROUTER");
    expect(keyEnvName("open-router")).toBe("QANUVI_KEY_OPEN_ROUTER");
  });
});
