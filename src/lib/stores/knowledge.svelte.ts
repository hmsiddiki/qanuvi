/**
 * Qanuvi's remote knowledge (24c): checked once a day; a newer, correctly signed file is shown to the user with what
 * it changes, and used only after the user keeps it. What is kept survives restarts; "Use the built-in ones" goes back.
 */
import { api, errorText } from "$lib/api";
import { FREE_PROVIDERS, FREE_PROVIDERS_CHECKED_ON, changes, type FreeProvider } from "$lib/agents/guides";
import { parseKnowledge, type KnowledgePack } from "$lib/agents/knowledge";
import { guides } from "./guides.svelte";

const KEPT = "qanuvi.knowledge.v1";
const CHECKED = "qanuvi.knowledge.checked";
const SKIPPED = "qanuvi.knowledge.skipped";
const DAY = 24 * 60 * 60 * 1000;

interface Kept {
  version: number;
  published: string;
  freeProviders: FreeProvider[];
  freeProvidersCheckedOn: string;
}

class KnowledgeStore {
  kept = $state<Kept | null>(null);
  /** A newer file waiting for the user's yes, with what it changes. */
  offer = $state<{ pack: KnowledgePack; summary: string[] } | null>(null);
  checking = $state(false);
  error = $state("");

  constructor() {
    try {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem(KEPT) : null;
      this.kept = raw ? (JSON.parse(raw) as Kept) : null;
    } catch {
      this.kept = null;
    }
  }

  /** The free providers to offer: the kept remote list, else the one shipped with Qanuvi. */
  get freeProviders(): FreeProvider[] {
    return this.kept?.freeProviders.length ? this.kept.freeProviders : FREE_PROVIDERS;
  }

  get freeProvidersCheckedOn(): string {
    return this.kept?.freeProviders.length ? this.kept.freeProvidersCheckedOn : FREE_PROVIDERS_CHECKED_ON;
  }

  /** Checks for a newer file: `force` also when checked today, and offers a version the user put off. */
  async check(force = false): Promise<void> {
    const last = Number(localStorage.getItem(CHECKED) || 0);
    if (!force && Date.now() - last < DAY) return;
    this.checking = true;
    this.error = "";
    try {
      const pack = parseKnowledge(await api.knowledge.fetch());
      localStorage.setItem(CHECKED, String(Date.now()));
      if (typeof pack === "string") throw new Error(pack);
      const skipped = Number(localStorage.getItem(SKIPPED) || 0);
      if (pack.version <= (this.kept?.version ?? 0) || (!force && pack.version <= skipped)) {
        this.offer = null;
        return;
      }
      const summary: string[] = [];
      for (const g of pack.guides)
        summary.push(`Guide for ${g.provider}: ${changes(guides.of(g.provider), g) || "same steps"}`);
      if (pack.freeProviders.length) {
        const now = this.freeProviders.map((f) => f.id);
        const added = pack.freeProviders.filter((f) => !now.includes(f.id)).map((f) => f.name);
        const gone = this.freeProviders
          .filter((f) => !pack.freeProviders.some((p) => p.id === f.id))
          .map((f) => f.name);
        summary.push(
          `Free providers (checked ${pack.freeProvidersCheckedOn}): ${pack.freeProviders.length}${added.length ? `; new: ${added.join(", ")}` : ""}${gone.length ? `; left out: ${gone.join(", ")}` : ""}`,
        );
      }
      this.offer = { pack, summary };
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.checking = false;
    }
  }

  /** Uses the offered file: its guides and its list of free providers. */
  keep(): void {
    const o = this.offer;
    if (!o) return;
    if (o.pack.guides.length) guides.keepGuides(o.pack.guides);
    this.kept = {
      version: o.pack.version,
      published: o.pack.published,
      freeProviders: o.pack.freeProviders.length ? o.pack.freeProviders : (this.kept?.freeProviders ?? []),
      freeProvidersCheckedOn: o.pack.freeProviders.length
        ? o.pack.freeProvidersCheckedOn
        : (this.kept?.freeProvidersCheckedOn ?? ""),
    };
    localStorage.setItem(KEPT, JSON.stringify(this.kept));
    this.offer = null;
  }

  /** Not now: this version is not offered again by the daily check (a manual check still shows it). */
  later(): void {
    if (this.offer) localStorage.setItem(SKIPPED, String(this.offer.pack.version));
    this.offer = null;
  }

  /** Back to what Qanuvi shipped with. */
  reset(): void {
    this.kept = null;
    localStorage.removeItem(KEPT);
    guides.reset();
  }
}

export const knowledge = new KnowledgeStore();
