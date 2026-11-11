/**
 * The Inbox (24f, 15f): what Qanuvi has to tell the user when they come back — plan run reports, sign-ins that need
 * renewing, newer knowledge to look at. Kept on this computer (the last 100), newest first.
 */
import type { WorkView } from "./ui.svelte";

const KEY = "qanuvi.inbox.v1";
const MAX = 100;

export interface InboxItem {
  id: string;
  at: number;
  kind: "run" | "sign-in" | "knowledge";
  title: string;
  text: string;
  read: boolean;
  /** Where the item's button leads. */
  open?: { label: string; view: WorkView; projectId?: string };
  /** Items with the same key replace each other (the newest stays), so a lasting problem is listed once. */
  key?: string;
}

class InboxStore {
  items = $state<InboxItem[]>([]);

  constructor() {
    try {
      const raw = typeof localStorage !== "undefined" ? localStorage.getItem(KEY) : null;
      const list = raw ? (JSON.parse(raw) as InboxItem[]) : [];
      this.items = Array.isArray(list) ? list : [];
    } catch {
      this.items = [];
    }
  }

  get unread(): number {
    return this.items.filter((i) => !i.read).length;
  }

  #save(): void {
    localStorage.setItem(KEY, JSON.stringify(this.items.slice(0, MAX)));
  }

  /** Adds an item; one with the same key is replaced (and stays read when nothing in it changed). */
  add(item: Omit<InboxItem, "id" | "at" | "read">): void {
    const same = item.key ? this.items.find((i) => i.key === item.key) : undefined;
    const read = !!same && same.read && same.text === item.text && same.title === item.title;
    const rest = item.key ? this.items.filter((i) => i.key !== item.key) : this.items;
    this.items = [{ ...item, id: crypto.randomUUID(), at: Date.now(), read }, ...rest].slice(0, MAX);
    this.#save();
  }

  /** Removes the item with this key, if any (the problem it told about is gone). */
  resolve(key: string): void {
    if (!this.items.some((i) => i.key === key)) return;
    this.items = this.items.filter((i) => i.key !== key);
    this.#save();
  }

  markRead(id?: string): void {
    this.items = this.items.map((i) => (id === undefined || i.id === id ? { ...i, read: true } : i));
    this.#save();
  }

  remove(id: string): void {
    this.items = this.items.filter((i) => i.id !== id);
    this.#save();
  }

  clear(): void {
    this.items = [];
    this.#save();
  }
}

export const inbox = new InboxStore();
