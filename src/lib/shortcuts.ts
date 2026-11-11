/** Keyboard shortcuts for the window layout. "Mod" is Ctrl on Windows and Linux and Cmd on macOS. */
export type LayoutAction =
  "toggleSidebar" | "toggleAgent" | "toggleWork" | "toggleTerminal" | "toggleTerminalMaximized";

export interface Shortcut {
  action: LayoutAction;
  key: string;
  mod: boolean;
  shift?: boolean;
  alt?: boolean;
}

export const SHORTCUTS: readonly Shortcut[] = [
  { action: "toggleSidebar", key: "b", mod: true },
  { action: "toggleTerminal", key: "j", mod: true },
  { action: "toggleTerminalMaximized", key: "j", mod: true, shift: true },
  { action: "toggleAgent", key: "a", mod: true, alt: true },
  { action: "toggleWork", key: "w", mod: true, alt: true },
];

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** The layout action a key press asks for, or null. */
export function matchShortcut(event: KeyLike, isMac: boolean): LayoutAction | null {
  const mod = isMac ? event.metaKey : event.ctrlKey;
  const key = event.key.toLowerCase();
  for (const s of SHORTCUTS) {
    if (s.key === key && s.mod === mod && !!s.shift === event.shiftKey && !!s.alt === event.altKey) return s.action;
  }
  return null;
}

/** How a shortcut is written in tooltips, for example "Ctrl+Shift+J" or "Cmd+B". */
export function shortcutLabel(action: LayoutAction, isMac: boolean): string {
  const s = SHORTCUTS.find((item) => item.action === action);
  if (!s) return "";
  const parts = [isMac ? "Cmd" : "Ctrl"];
  if (s.alt) parts.push(isMac ? "Option" : "Alt");
  if (s.shift) parts.push("Shift");
  parts.push(s.key.toUpperCase());
  return parts.join("+");
}
