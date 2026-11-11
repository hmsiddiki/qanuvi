/**
 * The left sidebar's modes (15h): Workspace (projects, sessions, pages), Explorer, Search and Git; and the Explorer's
 * sections: their order, which are open, which are hidden, and their largest height. Kept on this computer.
 */

export type PanelView = "workspace" | "explorer" | "search" | "git";
export type SectionId = "editors" | "files" | "outline" | "timeline" | "touched";

export interface SectionState {
  id: SectionId;
  open: boolean;
  hidden: boolean;
  /** Largest height in pixels (a section is only as tall as what it holds); Files takes the room that is left. */
  height: number;
}

export const SECTION_LABELS: Record<SectionId, string> = {
  editors: "Open editors",
  files: "Files",
  outline: "Outline",
  timeline: "Timeline",
  touched: "Touched in this session",
};

const KEY = "qanuvi.panel.v2";
const DEFAULT: SectionState[] = [
  { id: "files", open: true, hidden: false, height: 0 },
  { id: "editors", open: false, hidden: false, height: 180 },
  { id: "outline", open: false, hidden: false, height: 240 },
  { id: "timeline", open: false, hidden: false, height: 240 },
  { id: "touched", open: false, hidden: true, height: 200 },
];

/** Reads a stored panel state, keeping only known sections and adding any that are new. */
export function parsePanel(raw: unknown): { view: PanelView; sections: SectionState[] } {
  const r = (raw && typeof raw === "object" ? raw : {}) as { view?: unknown; sections?: unknown };
  const view: PanelView = r.view === "explorer" || r.view === "search" || r.view === "git" ? r.view : "workspace";
  const known = new Map(DEFAULT.map((d) => [d.id, d]));
  const out: SectionState[] = [];
  for (const s of Array.isArray(r.sections) ? (r.sections as Partial<SectionState>[]) : []) {
    const d = s && typeof s.id === "string" ? known.get(s.id as SectionId) : undefined;
    if (!d || out.some((o) => o.id === d.id)) continue;
    out.push({
      id: d.id,
      open: typeof s.open === "boolean" ? s.open : d.open,
      hidden: typeof s.hidden === "boolean" ? s.hidden : d.hidden,
      height:
        typeof s.height === "number" && Number.isFinite(s.height) ? Math.min(800, Math.max(60, s.height)) : d.height,
    });
  }
  for (const d of DEFAULT) if (!out.some((o) => o.id === d.id)) out.push({ ...d });
  // Files is always there: the tree is the panel's main part.
  const files = out.find((s) => s.id === "files")!;
  files.hidden = false;
  return { view, sections: out };
}

/** Moves a section to where another one is (drag and drop). */
export function moveSection(list: SectionState[], from: SectionId, to: SectionId): SectionState[] {
  if (from === to) return list;
  const moving = list.find((s) => s.id === from);
  if (!moving) return list;
  const rest = list.filter((s) => s.id !== from);
  const at = rest.findIndex((s) => s.id === to);
  if (at < 0) return list;
  const fromIndex = list.findIndex((s) => s.id === from);
  const toIndex = list.findIndex((s) => s.id === to);
  rest.splice(fromIndex < toIndex ? at + 1 : at, 0, moving);
  return rest;
}

class PanelStore {
  view = $state<PanelView>("workspace");
  sections = $state<SectionState[]>(DEFAULT.map((d) => ({ ...d })));
  /** The Explorer's sections menu (show or hide sections), opened from the sidebar's switch row. */
  menuOpen = $state(false);
  /** Goes up to ask the Search view to take focus. */
  focusSearch = $state(0);

  constructor() {
    try {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
      const p = parsePanel(raw ? JSON.parse(raw) : null);
      this.view = p.view;
      this.sections = p.sections;
    } catch {
      // Defaults.
    }
  }

  #save(): void {
    localStorage.setItem(KEY, JSON.stringify({ view: this.view, sections: this.sections }));
  }

  show(view: PanelView): void {
    this.view = view;
    this.#save();
  }

  update(id: SectionId, change: Partial<Omit<SectionState, "id">>): void {
    this.sections = this.sections.map((s) => (s.id === id ? { ...s, ...change } : s));
    this.#save();
  }

  move(from: SectionId, to: SectionId): void {
    this.sections = moveSection(this.sections, from, to);
    this.#save();
  }
}

export const panel = new PanelStore();
