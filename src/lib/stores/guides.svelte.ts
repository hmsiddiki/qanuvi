/**
 * The setup guides for free and local models (Milestone 24): the ones shipped with Qanuvi, or a newer version a paid
 * agent brought up to date with the user's permission (kept here after the user accepts it).
 */
import { invoke, Channel } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import {
  BUILT_IN_GUIDES,
  changes,
  jsonIn,
  parseGuides,
  suggestLocal,
  updatePrompt,
  type Guide,
  type MachineInfo,
} from "$lib/agents/guides";
import type { Project } from "$lib/api";
import { planner } from "./planner.svelte";
import { settings, type ModelProvider } from "./settings.svelte";

const KEY = "qanuvi.guides.v1";

export interface Proposal {
  guides: Guide[];
  /** Provider -> what changed. */
  summary: Record<string, string>;
  costUsd: number;
}

class GuidesStore {
  kept = $state<Guide[] | null>(null);
  machine = $state<MachineInfo | null>(null);
  checking = $state<Record<string, string>>({});
  pulling = $state<Record<string, { status: string; done: number; total: number } | null>>({});
  pullError = $state<Record<string, string>>({});
  updating = $state(false);
  updateError = $state("");
  proposal = $state<Proposal | null>(null);

  constructor() {
    try {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
      const parsed = raw ? parseGuides(JSON.parse(raw)) : null;
      if (Array.isArray(parsed)) this.kept = parsed;
    } catch {
      this.kept = null;
    }
  }

  get all(): Guide[] {
    return BUILT_IN_GUIDES.map((b) => this.kept?.find((k) => k.provider === b.provider) ?? b);
  }

  of(provider: string): Guide | undefined {
    return this.all.find((g) => g.provider === provider);
  }

  async loadMachine(): Promise<void> {
    if (this.machine) return;
    this.machine = await invoke<MachineInfo>("machine_info").catch(() => null);
  }

  get local() {
    return this.machine ? suggestLocal(this.machine) : null;
  }

  async checkKey(p: ModelProvider): Promise<void> {
    this.checking = { ...this.checking, [p.id]: "Checking…" };
    try {
      const text = await invoke<string>("model_key_check", { endpoint: { id: p.id, kind: p.kind, url: p.url } });
      this.checking = { ...this.checking, [p.id]: text };
    } catch (e) {
      this.checking = { ...this.checking, [p.id]: errorText(e) };
    }
  }

  /** Downloads a model into Ollama with progress; true when it arrived. */
  async pull(p: ModelProvider, model: string): Promise<boolean> {
    this.pullError = { ...this.pullError, [model]: "" };
    this.pulling = { ...this.pulling, [model]: { status: "starting", done: 0, total: 0 } };
    const channel = new Channel<{ status: string; done: number; total: number }>();
    channel.onmessage = (e) => (this.pulling = { ...this.pulling, [model]: e });
    try {
      await invoke("ollama_pull", { endpoint: { id: p.id, kind: p.kind, url: p.url }, model, onEvent: channel });
      return true;
    } catch (e) {
      this.pullError = { ...this.pullError, [model]: errorText(e) };
      return false;
    } finally {
      this.pulling = { ...this.pulling, [model]: null };
    }
  }

  /**
   * Asks a paid agent (the user agreed first) to bring the guides up to date from the web. Its answer is checked and
   * shown as a proposal; nothing is kept until the user accepts it.
   */
  async update(project: Project, agentId: string): Promise<void> {
    this.updating = true;
    this.updateError = "";
    this.proposal = null;
    try {
      const today = new Date().toISOString().slice(0, 10);
      const model = settings.agent(agentId).model;
      const answer = await planner.ask(project, updatePrompt(this.all, today), model, "Update the setup guides", {
        agentId,
        lean: false,
      });
      const parsed = parseGuides(jsonIn(answer.text));
      if (typeof parsed === "string") throw new Error(`The update could not be used: ${parsed}`);
      const summary: Record<string, string> = {};
      for (const g of parsed) summary[g.provider] = changes(this.of(g.provider), g);
      this.proposal = { guides: parsed, summary, costUsd: answer.costUsd };
    } catch (e) {
      this.updateError = errorText(e);
    } finally {
      this.updating = false;
    }
  }

  accept(): void {
    if (!this.proposal) return;
    const next = this.all.map((g) => this.proposal!.guides.find((p) => p.provider === g.provider) ?? g);
    this.kept = next;
    localStorage.setItem(KEY, JSON.stringify(next));
    this.proposal = null;
  }

  /** Keeps the given guides in place of the current ones for the same providers (24c: Qanuvi's remote knowledge). */
  keepGuides(list: Guide[]): void {
    const next = this.all.map((g) => list.find((p) => p.provider === g.provider) ?? g);
    this.kept = next;
    localStorage.setItem(KEY, JSON.stringify(next));
  }

  discard(): void {
    this.proposal = null;
  }

  /** Back to the guides shipped with Qanuvi. */
  reset(): void {
    this.kept = null;
    localStorage.removeItem(KEY);
  }
}

export const guides = new GuidesStore();
