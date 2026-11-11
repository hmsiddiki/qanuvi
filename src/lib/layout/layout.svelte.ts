import {
  LIMITS,
  PRESETS,
  clamp,
  cleanLayoutName,
  normalizeLayout,
  parseSavedLayouts,
  MAX_NAMED_LAYOUTS,
  type LayoutState,
} from "./layout";

const STORAGE_KEY = "qanuvi.layout.v1";

/** The live window layout, saved to local storage so it survives a restart. */
class LayoutStore {
  state = $state<LayoutState>(parseSavedLayouts(null).current);
  named = $state<Record<string, LayoutState>>({});
  active = $state("Coding");

  #saveTimer: ReturnType<typeof setTimeout> | undefined;

  load(): void {
    const saved = parseSavedLayouts(localStorage.getItem(STORAGE_KEY));
    this.state = saved.current;
    this.named = saved.named;
    this.active = saved.active;
  }

  /** Saves soon after the last change, so dragging a splitter does not write on every pixel. */
  saveSoon(): void {
    clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => this.saveNow(), 250);
  }

  saveNow(): void {
    clearTimeout(this.#saveTimer);
    const data = { current: this.state, named: this.named, active: this.active };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
  }

  #set(patch: Partial<LayoutState>): void {
    this.state = normalizeLayout({ ...this.state, ...patch });
    this.saveSoon();
  }

  toggleSidebar(): void {
    this.#set({ sidebarVisible: !this.state.sidebarVisible });
  }

  toggleSidebarCollapsed(): void {
    this.#set({ sidebarVisible: true, sidebarCollapsed: !this.state.sidebarCollapsed });
  }

  toggleAgent(): void {
    // Hiding the agent pane while the work pane is hidden shows the work pane instead of an empty window.
    if (this.state.agentVisible && !this.state.workVisible) this.#set({ agentVisible: false, workVisible: true });
    else this.#set({ agentVisible: !this.state.agentVisible });
  }

  toggleWork(): void {
    if (this.state.workVisible && !this.state.agentVisible) this.#set({ workVisible: false, agentVisible: true });
    else this.#set({ workVisible: !this.state.workVisible });
  }

  toggleTerminal(): void {
    if (!this.state.workVisible) this.#set({ workVisible: true, terminalVisible: true });
    else this.#set({ terminalVisible: !this.state.terminalVisible });
  }

  toggleTerminalMaximized(): void {
    this.#set({ workVisible: true, terminalVisible: true, terminalMaximized: !this.state.terminalMaximized });
  }

  setSidebarWidth(width: number): void {
    this.#set({ sidebarWidth: clamp(width, LIMITS.sidebar.min, LIMITS.sidebar.max), sidebarCollapsed: false });
  }

  setAgentWidth(width: number): void {
    this.#set({ agentWidth: clamp(width, LIMITS.agent.min, LIMITS.agent.max) });
  }

  setTerminalHeight(height: number): void {
    this.#set({ terminalHeight: clamp(height, LIMITS.terminal.min, LIMITS.terminal.max) });
  }

  apply(name: string): void {
    const layout = this.named[name];
    if (!layout) return;
    this.state = { ...layout };
    this.active = name;
    this.saveNow();
  }

  /** Saves the current layout under a name. Returns the name used, or an empty string if it could not be saved. */
  saveAs(rawName: string): string {
    const name = cleanLayoutName(rawName);
    if (!name) return "";
    if (!(name in this.named) && Object.keys(this.named).length >= MAX_NAMED_LAYOUTS) return "";
    this.named = { ...this.named, [name]: { ...this.state } };
    this.active = name;
    this.saveNow();
    return name;
  }

  /** Removes a saved layout. A built-in layout goes back to its original settings instead of disappearing. */
  remove(name: string): void {
    const next = { ...this.named };
    if (name in PRESETS) next[name] = { ...PRESETS[name] };
    else delete next[name];
    this.named = next;
    if (!(this.active in next)) this.active = "Coding";
    this.saveNow();
  }

  isPreset(name: string): boolean {
    return name in PRESETS;
  }
}

export const layout = new LayoutStore();
