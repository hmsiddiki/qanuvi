/**
 * Each project's workspace (Milestone 15c): open files with their cursor, the active file, the shown view, terminals,
 * the agent session and the preview address. Saved while working and given back the first time the project is opened
 * after a restart (switching projects within a run keeps everything live anyway).
 */
import { SvelteSet } from "svelte/reactivity";
import type { WorkView } from "./ui.svelte";

export interface SavedTab {
  path: string;
  line: number;
  column: number;
  /** The editor groups showing it (15i part 4), when not only the first: [1] or [0, 1]. */
  groups?: number[];
}

export interface Workspace {
  tabs: SavedTab[];
  active: string | null;
  /** The second editor group's active file, when the editor is split. */
  active2: string | null;
  /** The editor group worked in last (0 or 1). */
  focused: number;
  view: WorkView | null;
  /** Shell ids of the open terminals, in order. */
  terminals: string[];
  session: string | null;
  preview: string | null;
}

/** Views worth coming back to (Agents and Servers are app-wide pages, not part of a project). */
const VIEWS: WorkView[] = ["editor", "preview", "plan", "search", "diff"];
const MAX_TABS = 40;

export const isProjectView = (v: WorkView) => VIEWS.includes(v);

export const emptyWorkspace = (): Workspace => ({
  tabs: [],
  active: null,
  active2: null,
  focused: 0,
  view: null,
  terminals: [],
  session: null,
  preview: null,
});

const key = (projectId: string) => `qanuvi.workspace.${projectId}`;

/** Reads a saved workspace, keeping only what makes sense. */
export function parseWorkspace(json: string | null): Workspace {
  const w = emptyWorkspace();
  let raw: Record<string, unknown> = {};
  try {
    const v = json ? JSON.parse(json) : null;
    if (v && typeof v === "object") raw = v as Record<string, unknown>;
  } catch {
    return w;
  }
  const str = (v: unknown) => (typeof v === "string" && v.length > 0 && v.length < 1000 ? v : null);
  const pos = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 1 ? Math.floor(v) : 1);
  if (Array.isArray(raw.tabs))
    for (const t of raw.tabs.slice(0, MAX_TABS)) {
      const path = str((t as SavedTab)?.path);
      if (!path || path.includes("..") || w.tabs.some((x) => x.path === path)) continue;
      const saved: SavedTab = { path, line: pos((t as SavedTab).line), column: pos((t as SavedTab).column) };
      const groups = (t as SavedTab).groups;
      if (Array.isArray(groups)) {
        const g = [0, 1].filter((n) => groups.includes(n));
        if (g.includes(1)) saved.groups = g;
      }
      w.tabs.push(saved);
    }
  w.active = str(raw.active);
  if (w.active && !w.tabs.some((t) => t.path === w.active)) w.active = null;
  w.active2 = str(raw.active2);
  if (w.active2 && !w.tabs.some((t) => t.path === w.active2 && t.groups?.includes(1))) w.active2 = null;
  w.focused = raw.focused === 1 && w.tabs.some((t) => t.groups?.includes(1)) ? 1 : 0;
  w.view = VIEWS.includes(raw.view as WorkView) ? (raw.view as WorkView) : null;
  if (Array.isArray(raw.terminals)) w.terminals = raw.terminals.filter((s) => str(s)).slice(0, 8) as string[];
  w.session = str(raw.session);
  const preview = str(raw.preview);
  w.preview = preview && /^(https?|file):/i.test(preview) ? preview : null;
  return w;
}

class WorkspaceStore {
  /** Projects whose workspace has been given back in this run (or had none); only these are saved. */
  restored = new SvelteSet<string>();
  #timers = new Map<string, ReturnType<typeof setTimeout>>();
  #pending = new Map<string, Workspace>();

  read(projectId: string): Workspace {
    return parseWorkspace(localStorage.getItem(key(projectId)));
  }

  /** Saves a change soon (typing moves the cursor often). */
  saveSoon(projectId: string, patch: Partial<Workspace>): void {
    const next = { ...(this.#pending.get(projectId) ?? this.read(projectId)), ...patch };
    this.#pending.set(projectId, next);
    clearTimeout(this.#timers.get(projectId));
    this.#timers.set(
      projectId,
      setTimeout(() => this.#write(projectId), 500),
    );
  }

  #write(projectId: string): void {
    const w = this.#pending.get(projectId);
    if (!w) return;
    this.#pending.delete(projectId);
    this.#timers.delete(projectId);
    localStorage.setItem(key(projectId), JSON.stringify(w));
  }

  /** Writes everything waiting (before the window closes). */
  flush(): void {
    for (const id of [...this.#pending.keys()]) {
      clearTimeout(this.#timers.get(id));
      this.#write(id);
    }
  }

  forget(projectId: string): void {
    clearTimeout(this.#timers.get(projectId));
    this.#pending.delete(projectId);
    localStorage.removeItem(key(projectId));
  }

  /** The agent session each project was last on (kept from clean-up: it is the chat that opens next). */
  sessions(): string[] {
    const out: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (!k?.startsWith(key(""))) continue;
      const s = parseWorkspace(localStorage.getItem(k)).session;
      if (s) out.push(s);
    }
    return out;
  }
}

export const workspace = new WorkspaceStore();
