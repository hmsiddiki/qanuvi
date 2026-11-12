import type { EditorState } from "@codemirror/state";
import { ask } from "$lib/stores/confirm.svelte";
import { api, errorText } from "$lib/api";
import { languageFor } from "$lib/editor/languages";
import { settings } from "./settings.svelte";
import { lsp } from "./lsp.svelte";

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

/** Open files. Editor states (text, cursor, undo history) are kept outside the reactive state for speed. */
class EditorStore {
  tabs = $state<EditorTab[]>([]);
  activeKey = $state<string | null>(null);
  /** Last active tab per project, to come back to it when switching projects. */
  #lastActive = new Map<string, string>();
  states = new Map<string, EditorState>();

  visible(projectId: string | null): EditorTab[] {
    if (!settings.value.tabsFollowProject) return this.tabs;
    return this.tabs.filter((t) => t.global || t.projectId === projectId);
  }

  get active(): EditorTab | null {
    return this.tabs.find((t) => t.key === this.activeKey) ?? null;
  }

  activate(key: string | null): void {
    this.activeKey = key;
    const tab = this.active;
    if (tab) this.#lastActive.set(tab.projectId, tab.key);
  }

  /** Called when the current project changes. */
  projectChanged(projectId: string | null): void {
    if (!settings.value.tabsFollowProject) return;
    const visible = this.visible(projectId);
    const last = projectId ? this.#lastActive.get(projectId) : undefined;
    this.activeKey = visible.find((t) => t.key === last)?.key ?? visible[visible.length - 1]?.key ?? null;
  }

  async open(projectId: string, path: string, line: number | null = null, column: number | null = null): Promise<void> {
    const key = tabKey(projectId, path);
    const existing = this.tabs.find((t) => t.key === key);
    if (existing) {
      existing.revealLine = line;
      existing.revealColumn = column;
      this.activate(key);
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
    await this.#load(tab, key);
  }

  /** Opens one of an agent's configuration files (settings, instructions, MCP servers) in the editor. */
  async openConfig(provider: string, id: string, label: string, projectId: string | null): Promise<void> {
    const key = `config::${provider}::${id}::${projectId ?? ""}`;
    if (this.tabs.some((t) => t.key === key)) return this.activate(key);
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
    await this.#load(tab, key);
  }

  async #load(tab: EditorTab, key: string): Promise<void> {
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
    if (this.tabs.some((t) => t.key === key)) return this.activate(key);
    this.tabs = [...this.tabs, tab];
    this.activate(key);
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
      // Re-trigger the editor for this tab.
      this.tabs = [...this.tabs];
    } catch (e) {
      tab.error = errorText(e);
    }
  }

  async close(key: string): Promise<void> {
    const tab = this.tabs.find((t) => t.key === key);
    if (!tab) return;
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
    const visible = this.visible(tab.projectId);
    const index = visible.findIndex((t) => t.key === key);
    this.tabs = this.tabs.filter((t) => t.key !== key);
    this.states.delete(key);
    if (this.activeKey === key) {
      const rest = this.visible(tab.projectId);
      this.activate(rest[Math.min(index, rest.length - 1)]?.key ?? null);
    }
  }

  /** Drops the tabs of a project that was removed from the list. */
  forgetProject(projectId: string): void {
    for (const t of this.tabs) if (t.projectId === projectId) this.states.delete(t.key);
    this.tabs = this.tabs.filter((t) => t.projectId !== projectId);
    if (this.active === null) this.activeKey = null;
  }

  get anyDirty(): boolean {
    return this.tabs.some((t) => t.dirty);
  }
}

export const editor = new EditorStore();
