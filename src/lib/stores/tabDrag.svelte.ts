/**
 * Dragging a file tab (15i part 4): to another place in its strip, to the other editor group, or to the edge of the
 * editor to split it. Pointer events, not HTML drag and drop, so the window's own file-drop handling never gets in
 * the way. Drop places are marked in the page with data-drop-group ("0", "1", or "new") and tabs with data-key.
 */
import { editor } from "./editor.svelte";

export interface DropTarget {
  /** A group index, or "new" for a new group beside the only one. */
  group: number | "new";
  /** The tab to drop before (null: at the end). */
  before: string | null;
}

class TabDrag {
  key = $state<string | null>(null);
  from = 0;
  label = $state("");
  x = $state(0);
  y = $state(0);
  target = $state<DropTarget | null>(null);

  start(key: string, from: number, label: string): void {
    this.key = key;
    this.from = from;
    this.label = label;
  }

  moveTo(x: number, y: number): void {
    this.x = x;
    this.y = y;
    this.target = targetAt(x, y);
  }

  /** Ends the drag, moving the tab when it was let go over a drop place. */
  drop(x: number, y: number): void {
    const key = this.key;
    const t = targetAt(x, y);
    this.key = null;
    this.target = null;
    if (!key || !t) return;
    const to = t.group === "new" ? editor.groups.length : t.group;
    if (to === this.from && (t.before === null || t.before === key)) return;
    editor.moveTab(key, this.from, to, t.before === key ? null : t.before);
    editor.focusTick++;
  }

  cancel(): void {
    this.key = null;
    this.target = null;
  }
}

function targetAt(x: number, y: number): DropTarget | null {
  const el = document.elementFromPoint(x, y);
  const zone = el?.closest<HTMLElement>("[data-drop-group]");
  if (!zone) return null;
  const raw = zone.dataset.dropGroup;
  const group = raw === "new" ? "new" : Number(raw);
  if (group !== "new" && !Number.isInteger(group)) return null;
  const tab = el?.closest<HTMLElement>("[data-key]");
  return { group, before: tab && zone.contains(tab) ? (tab.dataset.key ?? null) : null };
}

export const tabDrag = new TabDrag();
