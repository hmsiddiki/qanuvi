/**
 * Running the OmniRoute on this computer for the user (24c): its connections and their health, adding free providers,
 * renewing sign-ins and Qanuvi's fallback combo. Keys never come back to the window.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import type { ModelProvider } from "./settings.svelte";

export interface OmniConnection {
  id: string;
  provider: string;
  name: string;
  auth: string;
  state: "working" | "needs sign-in" | "resting" | "error";
  problem: string;
}

export interface OmniCatalogEntry {
  id: string;
  name: string;
  category: string;
  hasFree: boolean;
  website: string;
}

const endpoint = (p: ModelProvider) => ({ id: p.id, kind: p.kind, url: p.url });

class OmniStore {
  connections = $state<OmniConnection[] | null>(null);
  catalog = $state<OmniCatalogEntry[]>([]);
  busy = $state<Record<string, string>>({});
  error = $state("");
  note = $state("");

  /** Connections that need the user: a sign-in to renew, or a key that stopped working. */
  get problems(): OmniConnection[] {
    return (this.connections ?? []).filter((c) => c.state === "needs sign-in" || c.state === "error");
  }

  async #do<T>(key: string, label: string, f: () => Promise<T>): Promise<T | null> {
    this.busy = { ...this.busy, [key]: label };
    this.error = "";
    try {
      return await f();
    } catch (e) {
      this.error = errorText(e);
      return null;
    } finally {
      const { [key]: _, ...rest } = this.busy;
      void _;
      this.busy = rest;
    }
  }

  async load(p: ModelProvider): Promise<void> {
    const list = await this.#do("load", "Reading OmniRoute…", () =>
      invoke<OmniConnection[]>("omniroute_connections", { endpoint: endpoint(p) }),
    );
    if (list) this.connections = list;
  }

  async loadCatalog(p: ModelProvider): Promise<void> {
    if (this.catalog.length) return;
    const list = await this.#do("catalog", "Reading the catalog…", () =>
      invoke<OmniCatalogEntry[]>("omniroute_catalog", { endpoint: endpoint(p) }),
    );
    if (list) this.catalog = list.sort((a, b) => Number(b.hasFree) - Number(a.hasFree) || a.name.localeCompare(b.name));
  }

  /** Connects a provider with a pasted key, or with the key Qanuvi keeps for `saved`. */
  async addKey(p: ModelProvider, provider: string, key: string, saved?: string): Promise<boolean> {
    const ok = await this.#do(provider, "Connecting…", () =>
      invoke("omniroute_add_key", { endpoint: endpoint(p), provider, key: key || null, saved: saved ?? null }).then(
        () => true,
      ),
    );
    if (ok) {
      this.note = `${provider} is connected in OmniRoute.`;
      await this.load(p);
    }
    return !!ok;
  }

  async test(p: ModelProvider, c: OmniConnection): Promise<void> {
    const ok = await this.#do(c.id, "Testing…", () =>
      invoke("omniroute_test", { endpoint: endpoint(p), id: c.id }).then(() => true),
    );
    this.note = ok ? `${c.name || c.provider} answers.` : "";
    await this.load(p);
  }

  /** Renews a sign-in: Antigravity in the browser (OmniRoute catches the answer); others on OmniRoute's own page. */
  async signIn(p: ModelProvider, provider: string): Promise<"done" | "panel" | "failed"> {
    this.busy = { ...this.busy, [provider]: "Waiting for the sign-in in your browser…" };
    this.error = "";
    try {
      await invoke("omniroute_sign_in", { endpoint: endpoint(p), provider });
      this.note = `Signed in to ${provider}.`;
      await this.load(p);
      return "done";
    } catch (e) {
      const text = errorText(e);
      if (text === "panel") return "panel";
      this.error = text;
      return "failed";
    } finally {
      const { [provider]: _, ...rest } = this.busy;
      void _;
      this.busy = rest;
    }
  }

  async makeCombo(p: ModelProvider, name: string, models: string[]): Promise<boolean> {
    const ok = await this.#do("combo", "Making the combo…", () =>
      invoke("omniroute_make_combo", { endpoint: endpoint(p), name, models }).then(() => true),
    );
    if (ok) this.note = `Combo ${name}: ${models.join(" → ")}.`;
    return !!ok;
  }
}

export const omni = new OmniStore();
