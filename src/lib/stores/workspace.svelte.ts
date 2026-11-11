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
}

export interface Workspace {
  tabs: SavedTab[];
  active: string | null;
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
      if (path && !path.includes("..") && !w.tabs.some((x) => x.path === path))
        w.tabs.push({ path, line: pos((t as SavedTab).line), column: pos((t as SavedTab).column) });
    }
  w.active = str(raw.active);
  if (w.active && !w.tabs.some((t) => t.path === w.active)) w.active = null;
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
}

export const workspace = new WorkspaceStore();
