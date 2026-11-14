/**
 * Qanuvi's own right-click menus (15d): one menu at a time, filled by the place that was right-clicked, and a small
 * name prompt for the actions that need one (rename, new file). The browser's own menu never shows in a release build.
 */

export interface MenuItem {
  label: string;
  /** A key hint shown on the right, for example "Ctrl+C". */
  hint?: string;
  action: () => unknown;
  disabled?: boolean;
  /** Shown in red: it removes or discards something. */
  danger?: boolean;
}

type Maybe<T> = T | false | null | undefined | "";
/** A menu: groups of items, with a line between groups; a group or item that is "nothing" is left out. */
export type MenuGroups = Maybe<readonly Maybe<MenuItem>[]>[];

class ContextMenuStore {
  /** `fromKeys`: a key opened it (Ctrl+.), so its first item is marked and Enter runs it, as in VS Code. */
  open = $state<{ x: number; y: number; groups: MenuItem[][]; fromKeys: boolean } | null>(null);
  prompt = $state<{ title: string; value: string; ok: string; done: (value: string) => unknown } | null>(null);

  /** Shows a menu where the mouse is; the browser's menu and outer menus do not show. */
  show(event: MouseEvent, groups: MenuGroups): void {
    event.preventDefault();
    event.stopPropagation();
    this.showAt(event.clientX, event.clientY, groups, false);
  }

  /** Shows a menu at a point of the window (a key opened it, for example Ctrl+. at the cursor). */
  showAt(x: number, y: number, groups: MenuGroups, fromKeys = true): void {
    const clean = groups
      .filter((g): g is readonly Maybe<MenuItem>[] => !!g)
      .map((g) => g.filter((i): i is MenuItem => !!i))
      .filter((g) => g.length);
    if (!clean.length) return;
    this.open = { x, y, groups: clean, fromKeys };
  }

  close(): void {
    this.open = null;
  }

  /** Asks for a name (selecting the part before the extension, as file managers do). */
  ask(title: string, value: string, ok: string, done: (value: string) => unknown): void {
    this.prompt = { title, value, ok, done };
  }
}

export const contextMenu = new ContextMenuStore();

/** Copies text to the clipboard; false when the system refused. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

/** The clipboard's text, or "" when it cannot be read. */
export async function clipboardText(): Promise<string> {
  try {
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}
