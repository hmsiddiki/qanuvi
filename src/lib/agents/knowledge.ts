/**
 * Qanuvi's remote knowledge (24c): one signed JSON file the Qanuvi team publishes with newer setup guides and lists of
 * free providers. The signature is checked in Rust; this checks the content, so a bad file changes nothing.
 *
 *   { "format": 1, "version": 3, "published": "2026-11-12", "note": "what changed",
 *     "guides": [ ...Guide ], "freeProviders": [ ...FreeProvider ], "freeProvidersCheckedOn": "2026-11-12" }
 */
import { parseGuides, type FreeProvider, type Guide } from "./guides";

export interface KnowledgePack {
  format: 1;
  /** Goes up with every published file; Qanuvi offers only a version newer than the one it uses. */
  version: number;
  published: string;
  note: string;
  guides: Guide[];
  freeProviders: FreeProvider[];
  freeProvidersCheckedOn: string;
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Checks a knowledge file. Returns the pack, or what is wrong with it. */
export function parseKnowledge(text: string): KnowledgePack | string {
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(text);
  } catch {
    return "The knowledge file is not JSON.";
  }
  if (!raw || typeof raw !== "object") return "The knowledge file is not an object.";
  if (raw.format !== 1) return "This knowledge file needs a newer Qanuvi.";
  const version = Number(raw.version);
  if (!Number.isInteger(version) || version < 1) return "The knowledge file has no version.";
  const published = typeof raw.published === "string" ? raw.published : "";
  if (!DATE.test(published)) return "The knowledge file has no publishing date.";
  const note = typeof raw.note === "string" ? raw.note.slice(0, 500) : "";
  let guides: Guide[] = [];
  if (raw.guides !== undefined) {
    const g = parseGuides({ guides: raw.guides });
    if (typeof g === "string") return `Guides: ${g}`;
    guides = g;
  }
  const freeProviders: FreeProvider[] = [];
  if (raw.freeProviders !== undefined) {
    if (!Array.isArray(raw.freeProviders)) return "freeProviders is not a list.";
    for (const p of raw.freeProviders as Record<string, unknown>[]) {
      const s = (v: unknown, max: number) => (typeof v === "string" ? v.trim().slice(0, max) : "");
      const id = s(p?.id, 64);
      const url = s(p?.url, 300);
      const how = p?.how === "sign-in" ? "sign-in" : p?.how === "key" ? "key" : null;
      if (!/^[a-z0-9][a-z0-9._-]*$/i.test(id)) return `A free provider has a bad id: ${id || "(none)"}.`;
      if (!how) return `${id}: "how" must be "key" or "sign-in".`;
      if (!url.startsWith("https://")) return `${id}: the address must start with https://.`;
      if (freeProviders.some((f) => f.id === id)) return `${id} appears twice.`;
      const reuse = s(p?.reuse, 32);
      freeProviders.push({
        id,
        name: s(p?.name, 80) || id,
        how,
        url,
        note: s(p?.note, 400),
        ...(reuse ? { reuse } : {}),
      });
    }
  }
  const freeProvidersCheckedOn = typeof raw.freeProvidersCheckedOn === "string" ? raw.freeProvidersCheckedOn : "";
  if (freeProviders.length && !DATE.test(freeProvidersCheckedOn)) return "freeProvidersCheckedOn must be a date.";
  if (!guides.length && !freeProviders.length) return "The knowledge file brings nothing.";
  return { format: 1, version, published, note, guides, freeProviders, freeProvidersCheckedOn };
}
