import type { EditorState } from "@codemirror/state";
import { ask } from "$lib/stores/confirm.svelte";
import { api, errorText } from "$lib/api";
import { languageFor } from "$lib/editor/languages";
import { settings } from "./settings.svelte";
import { lsp } from "./lsp.svelte";
import {
  addTo,
  cleanRatio,
  join,
  move,
  removeFrom,
  shownGroups,
  tidy,
  type Group,
  type SplitDirection,
} from "$lib/editor/groups";

export interface EditorTab {
  key: string;
  projectId: string;
  path: string;
  language: string | null;
  /** Fingerprint of the file as last read or saved, to notice changes made elsewhere. */
  hash: string;
  savedText: string;
  dirty: boolean;
  binary: boolean;
  tooLarge: boolean;
  /** A save that was refused because the file changed on disk. */
  conflict: boolean;
  error: string;
  /** Line to show once the editor has the file (from search results or agent steps). */
  revealLine: number | null;
  /** Column to put the cursor at with `revealLine` (1-based; default the line's start). */
  revealColumn?: number | null;
  /** Where the cursor is, for the project's saved workspace. */
  cursor?: { line: number; column: number };
  /** A save that failed for another reason (for example invalid JSON in a config file); editing continues. */
  saveError: string;
  /** Set for an agent's own configuration file instead of a project file. */
  config?: { provider: string; id: string; projectId: string | null };
  /** Tab text when the file name alone would be unclear. */
  label?: string;
  /** Shown whatever project is current (agent settings that apply everywhere). */
  global?: boolean;
  /** Raised when the file is read again from disk, so every editor showing it starts from the new text. */
  version?: number;
}

async function readTab(tab: EditorTab) {
  return tab.config
    ? api.agentConfig.read(tab.config.provider, tab.config.id, tab.config.projectId)
    : api.files.read(tab.projectId, tab.path);
}

async function writeTab(tab: EditorTab, text: string, expected: string | null) {
  return tab.config
    ? api.agentConfig.write(tab.config.provider, tab.config.id, tab.config.projectId, text, expected)
    : api.files.write(tab.projectId, tab.path, text, expected);
}

export const tabKey = (projectId: string, path: string) => `${projectId}::${path}`;

const SPLIT_KEY = "qanuvi.editorSplit";

function loadSplit(): { direction: SplitDirection; ratio: number } {
  try {
    const raw = JSON.parse(localStorage.getItem(SPLIT_KEY) ?? "{}");
    return { direction: raw.direction === "down" ? "down" : "right", ratio: cleanRatio(raw.ratio) };
  } catch {
    return { direction: "right", ratio: 0.5 };
  }
}

/**
 * Open files. Editor states (text, cursor, undo history) are kept outside the reactive state for speed.
 * `tabs` holds each open file once; the editor groups (15i part 4) say which group shows which, in what order.
 */
class EditorStore {
  tabs = $state<EditorTab[]>([]);
  /** One group, or two side by side / one above the other. */
  groups = $state<Group[]>([{ keys: [], active: null }]);
  /** The group worked in last: opening a file goes there. */
  focused = $state(0);
  /** Raised to put the keyboard in the focused group's editor (Ctrl+1, Ctrl+2, Open to the side). */
  focusTick = $state(0);
  split = $state(loadSplit());
  /** Last active tab per group and project, to come back to it when switching projects. */
  #lastActive = new Map<string, string>();
  states = new Map<string, EditorState>();
  /** The project shown now (set by projectChanged), so tabs of other projects are skipped when picking a neighbour. */
  #project: string | null = null;

  #shown = (key: string): boolean => {
    if (!settings.value.tabsFollowProject) return true;
    const t = this.tabs.find((x) => x.key === key);
    return !!t && (!!t.global || t.projectId === this.#project);
  };

  /** Every open file of a project (each once, whatever group shows it). */
  visible(projectId: string | null): EditorTab[] {
    if (!settings.value.tabsFollowProject) return this.tabs;
    return this.tabs.filter((t) => t.global || t.projectId === projectId);
  }

  /** The tabs one group shows for a project, in its order. */
  inGroup(projectId: string | null, group: number): EditorTab[] {
    const g = this.groups[group];
    if (!g) return [];
    const byKey = new Map(this.tabs.map((t) => [t.key, t]));
    const list = g.keys.map((k) => byKey.get(k)).filter((t): t is EditorTab => !!t);
    if (!settings.value.tabsFollowProject) return list;
    return list.filter((t) => t.global || t.projectId === projectId);
  }

  /** The groups with something to show for this project. */
  shownGroups(projectId: string | null): number[] {
    const follow = settings.value.tabsFollowProject;
    const tabs = new Map(this.tabs.map((t) => [t.key, t]));
    return shownGroups(this.groups, (k) => {
      const t = tabs.get(k);
      return !!t && (!follow || !!t.global || t.projectId === projectId);
    });
  }

  get activeKey(): string | null {
    return this.groups[this.focused]?.active ?? null;
  }

  get active(): EditorTab | null {
    const key = this.activeKey;
    return this.tabs.find((t) => t.key === key) ?? null;
  }

  /** The active tab of one group. */
  activeIn(group: number): EditorTab | null {
    const key = this.groups[group]?.active ?? null;
    return this.tabs.find((t) => t.key === key) ?? null;
  }

  /** Shows a tab: in the given group, else in the focused group if it holds it, else in the group that does. */
  activate(key: string | null, group?: number): void {
    let g = group ?? this.focused;
    if (key && !this.groups[g]?.keys.includes(key)) {
      const holder = this.groups.findIndex((x) => x.keys.includes(key));
      if (holder >= 0 && group === undefined) g = holder;
      else this.groups = addTo(this.groups, g, key);
    }
    if (!this.groups[g]) return;
    this.groups[g].active = key;
    this.focused = g;
    const tab = this.active;
    if (tab) this.#lastActive.set(`${g}:${tab.projectId}`, tab.key);
  }

  /** The group typed or clicked in; with `moveKeyboard`, its editor takes the keyboard. */
  focus(group: number, moveKeyboard = false): void {
    if (!this.groups[group]) return;
    if (this.focused !== group) this.focused = group;
    if (moveKeyboard) this.focusTick++;
  }

  /** Called when the current project changes. */
  projectChanged(projectId: string | null): void {
    this.#project = projectId;
    if (!settings.value.tabsFollowProject) return;
    this.groups.forEach((g, i) => {
      const visible = this.inGroup(projectId, i);
      const last = projectId ? this.#lastActive.get(`${i}:${projectId}`) : undefined;
      g.active = visible.find((t) => t.key === last)?.key ?? visible[visible.length - 1]?.key ?? null;
    });
    const shown = this.shownGroups(projectId);
    if (!shown.includes(this.focused)) this.focused = shown[0];
  }

  /** Opens a file in the focused group (or the one given); a file open in the other group opens here too. */
  async open(
    projectId: string,
    path: string,
    line: number | null = null,
    column: number | null = null,
    group?: number,
  ): Promise<void> {
    const key = tabKey(projectId, path);
    const existing = this.tabs.find((t) => t.key === key);
    if (existing) {
      existing.revealLine = line;
      existing.revealColumn = column;
      this.activate(key, group ?? this.#target());
      return;
    }
    const tab: EditorTab = {
      key,
      projectId,
      path,
      language: languageFor(path),
      hash: "",
      savedText: "",
      dirty: false,
      binary: false,
      tooLarge: false,
      conflict: false,
      error: "",
      revealLine: line,
      revealColumn: column,
      saveError: "",
    };
    await this.#load(tab, key, group ?? this.#target());
  }

  /** Where a newly opened file goes: the focused group, if it is one shown for this project. */
  #target(): number {
    return this.shownGroups(this.#project).includes(this.focused) ? this.focused : 0;
  }

  /** Opens one of an agent's configuration files (settings, instructions, MCP servers) in the editor. */
  async openConfig(provider: string, id: string, label: string, projectId: string | null): Promise<void> {
    const key = `config::${provider}::${id}::${projectId ?? ""}`;
    if (this.tabs.some((t) => t.key === key)) return this.activate(key, this.#target());
    const tab: EditorTab = {
      key,
      projectId: projectId ?? "",
      path: "",
      language: null,
      hash: "",
      savedText: "",
      dirty: false,
      binary: false,
      tooLarge: false,
      conflict: false,
      error: "",
      revealLine: null,
      saveError: "",
      config: { provider, id, projectId },
      label,
      global: projectId === null,
    };
    await this.#load(tab, key, this.#target());
  }

  async #load(tab: EditorTab, key: string, group: number): Promise<void> {
    try {
      const file = await readTab(tab);
      if (tab.config) {
        tab.path = file.path;
        tab.language = languageFor(file.path);
      }
      tab.hash = file.hash;
      tab.savedText = file.text;
      tab.binary = file.binary;
      tab.tooLarge = file.too_large;
    } catch (e) {
      tab.error = errorText(e);
    }
    if (this.tabs.some((t) => t.key === key)) return this.activate(key, group);
    this.tabs = [...this.tabs, tab];
    this.activate(key, group);
  }

  /** Saves text. With `force`, a file changed on disk is overwritten. Returns true when saved. */
  async save(key: string, text: string, force = false): Promise<boolean> {
    const tab = this.tabs.find((t) => t.key === key);
    if (!tab || tab.binary || tab.tooLarge || tab.error) return false;
    try {
      tab.hash = await writeTab(tab, text, force ? null : tab.hash);
      tab.savedText = text;
      if (!tab.config) lsp.saved(tab, text);
      tab.dirty = false;
      tab.conflict = false;
      tab.saveError = "";
      return true;
    } catch (e) {
      const message = errorText(e);
      tab.conflict = message.includes("changed on disk");
      tab.saveError = tab.conflict ? "" : message;
      return false;
    }
  }

  /** Reads the file again from disk, dropping unsaved changes. */
  async reload(key: string): Promise<void> {
    const tab = this.tabs.find((t) => t.key === key);
    if (!tab) return;
    try {
      const file = await readTab(tab);
      tab.hash = file.hash;
      tab.savedText = file.text;
      tab.dirty = false;
      tab.conflict = false;
      tab.error = "";
      tab.saveError = "";
      this.states.delete(key);
      // Every editor showing this file starts again from the new text.
      tab.version = (tab.version ?? 0) + 1;
    } catch (e) {
      tab.error = errorText(e);
    }
  }

  /**
   * Closes a tab. With a group, only that group's tab closes while the other group still shows the file; without
   * one, the file closes everywhere.
   */
  async close(key: string, group?: number): Promise<void> {
    const tab = this.tabs.find((t) => t.key === key);
    if (!tab) return;
    if (group !== undefined && this.groups.some((g, i) => i !== group && g.keys.includes(key))) {
      this.#setGroups(removeFrom(this.groups, group, key, this.#shown));
      return;
    }
    if (tab.dirty) {
      const name = tab.path.split("/").pop();
      const ok = await ask(`${name} has unsaved changes. Close it and lose them?`, {
        title: "Unsaved changes",
        kind: "warning",
        okLabel: "Close without saving",
        cancelLabel: "Keep open",
      });
      if (!ok) return;
    }
    let groups = this.groups;
    for (let i = 0; i < groups.length; i++) groups = removeFrom(groups, i, key, this.#shown);
    this.#setGroups(groups);
    this.tabs = this.tabs.filter((t) => t.key !== key);
    this.states.delete(key);
  }

  /** Drops the tabs of a project that was removed from the list. */
  forgetProject(projectId: string): void {
    const gone = new Set(this.tabs.filter((t) => t.projectId === projectId).map((t) => t.key));
    for (const k of gone) this.states.delete(k);
    this.tabs = this.tabs.filter((t) => !gone.has(t.key));
    this.#setGroups(
      this.groups.map((g) => ({
        keys: g.keys.filter((k) => !gone.has(k)),
        active: g.active && gone.has(g.active) ? null : g.active,
      })),
    );
  }

  /** Sets the groups, dropping an emptied one. */
  #setGroups(groups: Group[]): void {
    const t = tidy(groups, this.focused);
    this.groups = t.groups;
    this.focused = t.focused;
  }

  /** Opens a tab (the active one by default) in the other group, making that group if needed. */
  openToSide(key: string | null = this.activeKey, direction?: SplitDirection): void {
    if (!key) return;
    if (direction) this.setSplit({ direction });
    const to = this.groups.length === 1 ? 1 : this.focused === 0 ? 1 : 0;
    this.groups = addTo(this.groups, to, key);
    this.activate(key, to);
    this.focusTick++;
  }

  /** Moves a tab within its group or to the other one (dragging a tab); `to` = number of groups makes a new one. */
  moveTab(key: string, from: number, to: number, before: string | null = null): void {
    const target = Math.min(to, 1);
    this.#setGroups(move(this.groups, key, from, to, before, this.#shown));
    // The groups may have shifted (the source group emptied): find where the tab went.
    const holders = this.groups.map((g, i) => (g.keys.includes(key) ? i : -1)).filter((i) => i >= 0);
    const g = holders.includes(target) ? target : (holders[holders.length - 1] ?? 0);
    this.activate(key, g);
  }

  /** One group again: the second group's tabs join the first. */
  joinGroups(): void {
    this.groups = join(this.groups);
    this.focused = 0;
  }

  setSplit(patch: Partial<{ direction: SplitDirection; ratio: number }>): void {
    const next = { ...this.split, ...patch };
    this.split = { direction: next.direction === "down" ? "down" : "right", ratio: cleanRatio(next.ratio) };
    localStorage.setItem(SPLIT_KEY, JSON.stringify(this.split));
  }

  /**
   * Gives back a project's saved groups after its files were opened in the first: `second` are the files the second
   * group shows, `onlySecond` those the first does not.
   */
  restoreGroups(second: string[], onlySecond: string[], active: string | null, focused: number): void {
    const first = this.groups[0]?.active ?? null;
    let groups = this.groups;
    for (const k of second) if (this.tabs.some((t) => t.key === k)) groups = addTo(groups, 1, k);
    for (const k of onlySecond) groups = removeFrom(groups, 0, k, this.#shown);
    if (groups[1] && active && groups[1].keys.includes(active)) groups[1].active = active;
    if (groups[0] && first && groups[0].keys.includes(first)) groups[0].active = first;
    this.#setGroups(groups);
    if (this.groups[focused]) this.focused = focused;
  }

  get anyDirty(): boolean {
    return this.tabs.some((t) => t.dirty);
  }
}

export const editor = new EditorStore();
