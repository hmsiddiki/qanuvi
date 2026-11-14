/**
 * Window layout: which panes are shown and how big they are, plus the user's named layouts.
 * Plain functions only, so the rules can be tested without a window.
 */

export interface LayoutState {
  sidebarVisible: boolean;
  /** Sidebar shown as icons only. */
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  agentVisible: boolean;
  agentWidth: number;
  workVisible: boolean;
  terminalVisible: boolean;
  terminalHeight: number;
  /** Terminal fills the work pane (the editor is hidden). */
  terminalMaximized: boolean;
  /** Which side of the editor the conversation is on (15h: right in the VS Code-like layout). */
  agentSide: "left" | "right";
  /** What the sidebar shows when the layout is chosen. */
  sidebarMode: SidebarMode;
}

export type SidebarMode = "workspace" | "explorer" | "search" | "git" | "debug";

export const LIMITS = {
  sidebar: { min: 180, max: 380 },
  sidebarCollapsedWidth: 52,
  agent: { min: 300, max: 760 },
  terminal: { min: 90, max: 1600 },
} as const;

export const DEFAULT_LAYOUT: LayoutState = {
  sidebarVisible: true,
  sidebarCollapsed: false,
  sidebarWidth: 236,
  agentVisible: true,
  agentWidth: 400,
  workVisible: true,
  terminalVisible: true,
  terminalHeight: 220,
  terminalMaximized: false,
  agentSide: "left",
  sidebarMode: "workspace",
};

/** Built-in layouts. The user can save more; these can be changed but always come back if removed. */
export const PRESETS: Readonly<Record<string, LayoutState>> = {
  // Qanuvi's own: the assistant first, the workspace in the sidebar.
  Qanuvi: { ...DEFAULT_LAYOUT },
  // More room for code: the files in the sidebar, a narrower conversation.
  Coding: { ...DEFAULT_LAYOUT, sidebarMode: "explorer", agentWidth: 340 },
  // As VS Code users know it: files on the left, the editor in the middle, the chat on the right.
  "VS Code-like": { ...DEFAULT_LAYOUT, sidebarMode: "explorer", agentSide: "right", agentWidth: 380 },
  "Side screen": {
    ...DEFAULT_LAYOUT,
    sidebarCollapsed: true,
    workVisible: false,
    terminalVisible: false,
  },
  Review: {
    ...DEFAULT_LAYOUT,
    sidebarVisible: false,
    agentWidth: 460,
    terminalVisible: false,
  },
};

export const DEFAULT_LAYOUT_NAME = "Qanuvi";

/** What each built-in layout is for (the Layout tab in Settings). */
export const PRESET_NOTES: Readonly<Record<string, string>> = {
  Qanuvi: "The assistant first: the conversation in the middle, the workspace (projects, sessions) in the sidebar.",
  Coding: "More room for code: the project's files in the sidebar and a narrower conversation.",
  "VS Code-like": "Files on the left, the editor in the middle, the conversation on the right, the terminal below.",
  "Side screen": "Only the conversation, for a narrow window beside other work.",
  Review: "The conversation and the work area side by side, no sidebar or terminal: for reading changes.",
};
export const MAX_NAMED_LAYOUTS = 20;
export const MAX_LAYOUT_NAME_LENGTH = 40;

export function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.round(value)));
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function num(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

/** Turns anything (for example a saved value from an older version) into a valid layout. */
export function normalizeLayout(raw: unknown): LayoutState {
  const r = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const d = DEFAULT_LAYOUT;
  const state: LayoutState = {
    sidebarVisible: bool(r.sidebarVisible, d.sidebarVisible),
    sidebarCollapsed: bool(r.sidebarCollapsed, d.sidebarCollapsed),
    sidebarWidth: clamp(num(r.sidebarWidth, d.sidebarWidth), LIMITS.sidebar.min, LIMITS.sidebar.max),
    agentVisible: bool(r.agentVisible, d.agentVisible),
    agentWidth: clamp(num(r.agentWidth, d.agentWidth), LIMITS.agent.min, LIMITS.agent.max),
    workVisible: bool(r.workVisible, d.workVisible),
    terminalVisible: bool(r.terminalVisible, d.terminalVisible),
    terminalHeight: clamp(num(r.terminalHeight, d.terminalHeight), LIMITS.terminal.min, LIMITS.terminal.max),
    terminalMaximized: bool(r.terminalMaximized, d.terminalMaximized),
    agentSide: r.agentSide === "right" ? "right" : "left",
    sidebarMode:
      r.sidebarMode === "explorer" || r.sidebarMode === "search" || r.sidebarMode === "git" || r.sidebarMode === "debug"
        ? r.sidebarMode
        : "workspace",
  };
  // The window is never empty: at least the agent pane or the work pane is shown.
  if (!state.agentVisible && !state.workVisible) state.agentVisible = true;
  // A maximised terminal only makes sense while the terminal is shown.
  if (!state.terminalVisible) state.terminalMaximized = false;
  return state;
}

/** A layout name as typed by the user, cleaned; empty when it cannot be used. */
export function cleanLayoutName(name: string): string {
  return name.replace(/\s+/g, " ").trim().slice(0, MAX_LAYOUT_NAME_LENGTH);
}

export interface SavedLayouts {
  current: LayoutState;
  named: Record<string, LayoutState>;
  active: string;
}

/** Reads what was saved; anything missing or broken falls back to the defaults and the presets. */
export function parseSavedLayouts(json: string | null): SavedLayouts {
  let data: Record<string, unknown> = {};
  try {
    const parsed = json ? JSON.parse(json) : null;
    if (parsed && typeof parsed === "object") data = parsed as Record<string, unknown>;
  } catch {
    data = {};
  }

  const named: Record<string, LayoutState> = {};
  for (const [name, layout] of Object.entries(PRESETS)) named[name] = { ...layout };
  if (data.named && typeof data.named === "object") {
    for (const [rawName, layout] of Object.entries(data.named as Record<string, unknown>)) {
      const name = cleanLayoutName(rawName);
      if (!name || Object.keys(named).length >= MAX_NAMED_LAYOUTS) continue;
      named[name] = normalizeLayout(layout);
    }
  }

  const active = typeof data.active === "string" && data.active in named ? data.active : DEFAULT_LAYOUT_NAME;
  const current = data.current ? normalizeLayout(data.current) : { ...named[active] };
  return { current, named, active };
}

/** Pane width the sidebar takes on screen (0 when hidden). */
export function sidebarScreenWidth(state: LayoutState): number {
  if (!state.sidebarVisible) return 0;
  return state.sidebarCollapsed ? LIMITS.sidebarCollapsedWidth : state.sidebarWidth;
}
