/**
 * Qanuvi's own agent (Milestone 24): finding models at the user's providers, keeping their keys (in the credential
 * store, through the app), choosing which models the brain may use, and what it has learned about them.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { settings, type ModelProvider, type OwnModel } from "./settings.svelte";

export interface ModelInfo {
  id: string;
  endpoint: string;
  name: string;
  local: boolean;
  free: boolean;
  tools: boolean;
  context: number;
  size: number;
}

/** A local provider's program on this computer (from local_server_status). */
export interface LocalServer {
  program: string;
  installed: boolean;
  path: string;
  start: string;
}

export interface ModelRecord {
  turns: number;
  finished: number;
  brokenCalls: number;
  calls: number;
  restUntil: number;
  lastProblem: string;
}

/** What Qanuvi's agent gets for a session: the providers its models need (keys are added by the app) and the models. */
export function ownSetup() {
  const all = settings.value.brain;
  // Online only: models on this computer are left out, so Ollama is never asked, loaded or started.
  const b = all.local ? all : { ...all, models: all.models.filter((m) => !m.local && !m.id.startsWith("ollama/")) };
  return {
    endpoints: b.providers
      .filter((p) => b.models.some((m) => m.id.startsWith(`${p.id}/`)))
      .map(({ id, kind, url }) => ({ id, kind, url })),
    models: b.models.map(({ id, local, free, context }) => ({ id, local, free, context })),
  };
}

/** The setup to send with a session of this agent (only Qanuvi's own agent needs one). */
export function setupFor(providerId: string) {
  return providerId === "qanuvi" ? ownSetup() : null;
}

class BrainStore {
  found = $state<Record<string, ModelInfo[]>>({});
  errors = $state<Record<string, string>>({});
  searching = $state<Record<string, boolean>>({});
  keys = $state<Record<string, boolean>>({});
  records = $state<Record<string, ModelRecord>>({});
  /** What Find is doing besides looking (starting a server), per provider. */
  notes = $state<Record<string, string>>({});
  /** Providers whose steps should be shown: the program is not installed here, or it needs its key. */
  missing = $state<Record<string, boolean>>({});
  /** Models tried with one tiny message (by model id): whether they answered. Kept while Qanuvi runs. */
  probes = $state<Record<string, { state: "testing" | "ok" | "failed"; text: string }>>({});

  /** Tries a model with one tiny message: routers list models whose providers are not connected. */
  async probe(p: ModelProvider, m: ModelInfo): Promise<boolean> {
    this.probes = { ...this.probes, [m.id]: { state: "testing", text: "testing…" } };
    try {
      const secs = await invoke<number>("model_probe", {
        endpoint: { id: p.id, kind: p.kind, url: p.url },
        model: m.name,
      });
      this.probes = { ...this.probes, [m.id]: { state: "ok", text: `answered in ${secs.toFixed(1)} s` } };
      return true;
    } catch (e) {
      this.probes = {
        ...this.probes,
        [m.id]: { state: "failed", text: `did not answer: ${errorText(e).slice(0, 140)}` },
      };
      return false;
    }
  }

  async find(p: ModelProvider, all = false): Promise<void> {
    const endpoint = { id: p.id, kind: p.kind, url: p.url };
    const list = () => invoke<ModelInfo[]>("models_list", { endpoint, all });
    this.searching = { ...this.searching, [p.id]: true };
    this.errors = { ...this.errors, [p.id]: "" };
    this.notes = { ...this.notes, [p.id]: "" };
    this.missing = { ...this.missing, [p.id]: false };
    try {
      let models: ModelInfo[];
      try {
        models = await list();
      } catch (e) {
        // Nothing answers at a local address: look for the program on this computer, start it if it is installed,
        // otherwise show how to install it.
        const server = /could not be reached/.test(errorText(e))
          ? await invoke<LocalServer | null>("local_server_status", { endpoint, start: p.start ?? null }).catch(
              () => null,
            )
          : null;
        if (!server) throw e;
        const name = server.program === "ollama" ? "Ollama" : server.program === "omniroute" ? "OmniRoute" : p.name;
        if (!server.installed) {
          this.missing = { ...this.missing, [p.id]: true };
          throw new Error(`${name} is not installed on this computer. The steps below install it.`);
        }
        this.notes = {
          ...this.notes,
          [p.id]: `${name} is installed (${server.path}) but was not running. Starting it: ${server.start}`,
        };
        await invoke("local_server_start", { endpoint, start: p.start ?? null });
        this.notes = { ...this.notes, [p.id]: `Started ${name}.` };
        models = await list();
      }
      this.found = { ...this.found, [p.id]: models };
      if (!models.length)
        this.errors = {
          ...this.errors,
          [p.id]:
            p.kind === "ollama"
              ? "No models yet. Get one with: ollama pull qwen2.5-coder:7b"
              : p.id === "omniroute"
                ? "No models yet: OmniRoute has no providers connected. Open its dashboard (step by step below) and connect the free ones."
                : "No models found.",
        };
    } catch (e) {
      let text = errorText(e);
      // Running, but it wants a key: say so and show the steps (they say where the key is).
      if (/\b401\b|authentication required|invalid_api_key|unauthori[sz]ed/i.test(text)) {
        text = this.keys[p.id]
          ? `${p.name} refused the key kept for it. Copy a new one from ${p.name} and save it above.`
          : p.id === "omniroute"
            ? `${p.name} is running and needs its key. Press Connect Qanuvi (OmniRoute makes one for Qanuvi), or copy one as the steps below say.`
            : `${p.name} is running and needs its key. The steps below say where to copy it; paste it above and press Save key.`;
        this.missing = { ...this.missing, [p.id]: true };
      }
      this.errors = { ...this.errors, [p.id]: text };
    } finally {
      this.searching = { ...this.searching, [p.id]: false };
    }
  }

  /** OmniRoute on this computer: it makes a key named Qanuvi, kept in the credential store; then its models are found. */
  async connect(p: ModelProvider): Promise<void> {
    this.searching = { ...this.searching, [p.id]: true };
    this.errors = { ...this.errors, [p.id]: "" };
    this.notes = { ...this.notes, [p.id]: "Asking OmniRoute for a key named Qanuvi…" };
    try {
      await invoke("omniroute_connect", { endpoint: { id: p.id, kind: p.kind, url: p.url } });
      this.keys = { ...this.keys, [p.id]: true };
      this.notes = {
        ...this.notes,
        [p.id]: "Connected: OmniRoute made a key named Qanuvi, kept in your system's credential store.",
      };
      this.missing = { ...this.missing, [p.id]: false };
    } catch (e) {
      this.notes = { ...this.notes, [p.id]: "" };
      this.errors = { ...this.errors, [p.id]: errorText(e) };
      return;
    } finally {
      this.searching = { ...this.searching, [p.id]: false };
    }
    const note = this.notes[p.id];
    await this.find(p);
    if (!this.errors[p.id]) this.notes = { ...this.notes, [p.id]: note };
  }

  async loadKeys(): Promise<void> {
    for (const p of settings.value.brain.providers) {
      if (p.kind === "ollama") continue;
      const has = await invoke<boolean>("model_key_has", { endpoint: p.id }).catch(() => false);
      this.keys = { ...this.keys, [p.id]: has };
    }
  }

  async setKey(providerId: string, key: string): Promise<string> {
    try {
      await invoke("model_key_set", { endpoint: providerId, key });
      this.keys = { ...this.keys, [providerId]: !!key.trim() };
      return "";
    } catch (e) {
      return errorText(e);
    }
  }

  async loadRecords(): Promise<void> {
    const r = await invoke<{ models: Record<string, ModelRecord> }>("brain_records").catch(() => ({ models: {} }));
    this.records = r.models;
  }

  uses(id: string): boolean {
    return settings.value.brain.models.some((m) => m.id === id);
  }

  use(m: ModelInfo): void {
    if (this.uses(m.id)) return;
    const next: OwnModel = { id: m.id, label: m.name, local: m.local, free: m.free, context: m.context };
    const b = settings.value.brain;
    settings.set("brain", { ...b, models: [...b.models, next] });
  }

  drop(id: string): void {
    const b = settings.value.brain;
    settings.set("brain", { ...b, models: b.models.filter((m) => m.id !== id) });
  }

  /** Empties the brain's list (the models stay where they are; they can be added again). */
  clear(): void {
    settings.set("brain", { ...settings.value.brain, models: [] });
  }

  /** Moves a model up (-1) or down (+1) in the user's order. */
  move(id: string, by: -1 | 1): void {
    const b = settings.value.brain;
    const list = [...b.models];
    const i = list.findIndex((m) => m.id === id);
    const j = i + by;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    settings.set("brain", { ...b, models: list });
  }

  saveProvider(p: ModelProvider): void {
    const b = settings.value.brain;
    const providers = b.providers.some((x) => x.id === p.id)
      ? b.providers.map((x) => (x.id === p.id ? p : x))
      : [...b.providers, p];
    settings.set("brain", { ...b, providers });
  }
}

export const brain = new BrainStore();
