/**
 * Running the OmniRoute on the shown machine for the user (24c; per machine since 24z3 D5): its connections and their
 * health, adding free providers, renewing sign-ins and Qanuvi's fallback combo. Keys never come back to the window.
 * Each machine's core watches its own OmniRoute for the Inbox; the window only shows the shown machine's.
 */
import { routed as invoke, shownMachine } from "$lib/remote";
import { errorText } from "$lib/api";
import { settings, type ModelProvider } from "./settings.svelte";

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

/** A connection's name for people: OmniRoute leaves some unnamed, then its provider says what it is. */
export function connectionName(c: OmniConnection): string {
  return c.name.trim() || c.provider || c.id;
}

const endpoint = (p: ModelProvider) => ({ id: p.id, kind: p.kind, url: p.url });

/** Before 24z3 D5 the muted connections were the window's; now they are This PC's setting (moved once). */
const OLD_MUTED_KEY = "qanuvi.omni.muted";
export function moveOldMuted(): void {
  if (typeof localStorage === "undefined" || shownMachine() !== null) return;
  const raw = localStorage.getItem(OLD_MUTED_KEY);
  if (raw === null) return;
  try {
    const v = JSON.parse(raw);
    const old = Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
    if (old.length) settings.set("omniMuted", [...new Set([...settings.value.omniMuted, ...old])]);
  } catch {
    // Unreadable: nothing to keep.
  }
  localStorage.removeItem(OLD_MUTED_KEY);
}

class OmniStore {
  connections = $state<OmniConnection[] | null>(null);
  catalog = $state<OmniCatalogEntry[]>([]);
  busy = $state<Record<string, string>>({});
  error = $state("");
  note = $state("");

  /** Connections the user does not want warnings about, on the shown machine (kept in OmniRoute, only quiet here). */
  get muted(): string[] {
    return settings.value.omniMuted;
  }

  /** Connections that need the user: a sign-in to renew, or a key that stopped working (not the muted ones). */
  get problems(): OmniConnection[] {
    return (this.connections ?? []).filter(
      (c) => (c.state === "needs sign-in" || c.state === "error") && !this.muted.includes(c.id),
    );
  }

  /** No more warnings about this connection (or warnings again); it stays in OmniRoute either way. */
  setMuted(c: OmniConnection, on: boolean): void {
    settings.set("omniMuted", on ? [...new Set([...this.muted, c.id])] : this.muted.filter((id) => id !== c.id));
    // The machine's watch files or resolves its Inbox item now, not in half an hour: once the list is saved there,
    // or it would look with the list before.
    void settings.flushed().then(() => this.look());
  }

  /**
   * The shown machine's core looks at its OmniRoute now (it keeps the Inbox items itself) and answers the connections
   * for the status bar. Nothing to watch there (or an older master): nothing shown.
   */
  async look(): Promise<void> {
    const machine = shownMachine();
    const list = await invoke<OmniConnection[] | null>("omniroute_look").catch(() => null);
    if (machine === shownMachine() && list) this.connections = list;
  }

  /** Another machine is shown (machines.ts): nothing of the machine before stays on screen. */
  switched(): void {
    this.connections = null;
    this.catalog = [];
    this.busy = {};
    this.error = "";
    this.note = "";
  }

  /** Removes the connection from OmniRoute itself. */
  async remove(p: ModelProvider, c: OmniConnection): Promise<boolean> {
    const ok = await this.#do(c.id, "Removing…", () =>
      invoke("omniroute_remove", { endpoint: endpoint(p), id: c.id }).then(() => true),
    );
    if (ok) {
      this.note = `${connectionName(c)} is removed from OmniRoute.`;
      if (this.muted.includes(c.id))
        settings.set(
          "omniMuted",
          this.muted.filter((id) => id !== c.id),
        );
      await this.load(p);
    }
    return !!ok;
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
    const machine = shownMachine();
    const list = await this.#do("load", "Reading OmniRoute…", () =>
      invoke<OmniConnection[]>("omniroute_connections", { endpoint: endpoint(p) }),
    );
    if (list && machine === shownMachine()) this.connections = list;
  }

  async loadCatalog(p: ModelProvider): Promise<void> {
    if (this.catalog.length) return;
    const machine = shownMachine();
    const list = await this.#do("catalog", "Reading the catalog…", () =>
      invoke<OmniCatalogEntry[]>("omniroute_catalog", { endpoint: endpoint(p) }),
    );
    if (list && machine === shownMachine())
      this.catalog = list.sort((a, b) => Number(b.hasFree) - Number(a.hasFree) || a.name.localeCompare(b.name));
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
