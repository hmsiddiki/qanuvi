/**
 * The Inbox (24f, 15f, 24u part 4): what Qanuvi has to tell the owner — plan run reports, questions a run waits on,
 * sign-ins that need renewing, newer knowledge. Kept in Qanuvi's database, so every window shows the same list and
 * the first answer anywhere closes a question everywhere. This store is the window's copy, kept fresh by the
 * "qanuvi-inbox" event.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Place } from "./ui.svelte";
import { keepAnswer, loadPlanState, savePlanState } from "$lib/plan/planstate";
import { DECIDE_TEXT, holdReply, type AnswerChoice, type InboxAnswer } from "$lib/plan/holdask";

export { DECIDE_TEXT, holdReply, type AnswerChoice, type InboxAnswer };

/** Where the old per-window Inbox was kept (before 24u part 4); brought into the database once. */
const OLD_KEY = "qanuvi.inbox.v1";

export type InboxType = "info" | "answer" | "problem";

/** The form a question is answered with. */
export interface AnswerForm {
  choices: { id: AnswerChoice; label: string; hint?: string }[];
  /** Placeholder of the optional text box; none means no text box. */
  text?: string;
  /** It asks for a login or password: answered in the run panel only, so the value is kept as a private detail. */
  secret?: boolean;
}

/** What a question belongs to; "hold" is a plan step a run set aside for the owner. */
export type InboxSource = HoldSource | ChangesFromSource;

export interface HoldSource {
  kind: "hold";
  projectId: string;
  file: string;
  runId: string;
  step: string;
  /** The held step's kind: a hold for the owner, a blocked step or one that did not finish. */
  held?: "hold" | "blocked" | "failed";
}

/** "Git cannot see some of the plan's files": the answer picks where this project's reviews get changes from. */
export interface ChangesFromSource {
  kind: "changes-from";
  projectId: string;
}

export interface InboxItem {
  id: string;
  at: number;
  type: InboxType;
  kind: "run" | "sign-in" | "knowledge" | "chats";
  title: string;
  text: string;
  read: boolean;
  /** Where the item's button leads. */
  open?: { label: string; view: Place; projectId?: string } | null;
  /** Items with the same key replace each other (the newest stays), so a lasting problem is listed once. */
  key?: string | null;
  form?: AnswerForm | null;
  source?: InboxSource | null;
  status: "open" | "answered" | "closed";
  answer?: InboxAnswer | null;
  answeredBy?: string | null;
  answeredAt?: number | null;
}

export type NewInboxItem = Pick<InboxItem, "kind" | "title" | "text"> &
  Partial<Pick<InboxItem, "type" | "open" | "key" | "form" | "source">>;

type Change = { change: "add" | "answer" | "close" | "read" | "remove"; item?: InboxItem | null };

/** A question still waiting for the owner. */
export const isOpenQuestion = (i: InboxItem) => i.type === "answer" && i.status === "open";

class InboxStore {
  items = $state<InboxItem[]>([]);
  #started = false;
  #answered = new Set<(item: InboxItem) => void>();

  get unread(): number {
    return this.items.filter((i) => !i.read).length;
  }

  /** Questions that still wait for the owner. */
  get questions(): InboxItem[] {
    return this.items.filter(isOpenQuestion);
  }

  /** Loads the list, brings the old per-window items in once and follows changes made anywhere. */
  async start(): Promise<void> {
    if (this.#started) return;
    this.#started = true;
    await listen<Change>("qanuvi-inbox", (e) => {
      const item = e.payload.item;
      if (e.payload.change === "answer" && item) for (const fn of this.#answered) fn(item);
      void this.refresh();
    });
    try {
      const raw = localStorage.getItem(OLD_KEY);
      const old = raw ? JSON.parse(raw) : null;
      if (Array.isArray(old) && old.length) await invoke("inbox_import", { items: old });
      if (raw !== null) localStorage.removeItem(OLD_KEY);
    } catch {
      // A broken old list is not worth keeping.
      localStorage.removeItem(OLD_KEY);
    }
    await this.refresh();
  }

  async refresh(): Promise<void> {
    try {
      this.items = await invoke<InboxItem[]>("inbox_list");
    } catch {
      // Keeps what it shows; the next change refreshes again.
    }
  }

  /** Called with every answered question, whichever window (or later device) answered it. */
  onAnswer(fn: (item: InboxItem) => void): () => void {
    this.#answered.add(fn);
    return () => this.#answered.delete(fn);
  }

  /** Adds an item; one with the same key is replaced (and stays read when nothing in it changed). */
  add(item: NewInboxItem): void {
    void invoke<InboxItem>("inbox_add", { item: { type: "info", ...item } }).catch(() => {});
  }

  /**
   * Answers a question. Only the first answer anywhere counts: false when another window (or device) answered or
   * closed it first. A plan step's answer is kept for its plan, so a run started later goes on with it too.
   */
  async answer(id: string, answer: InboxAnswer): Promise<boolean> {
    const done = await invoke<InboxItem | null>("inbox_answer", { id, answer }).catch(() => null);
    if (done?.source?.kind === "hold") {
      const { choice, text } = holdReply(answer);
      const s = done.source;
      savePlanState(s.projectId, s.file, keepAnswer(loadPlanState(s.projectId, s.file), s.step, choice, text));
    }
    return !!done;
  }

  /** The run panel answered a question: it closes everywhere (the run panel keeps the answer itself). */
  answerKey(key: string, answer: InboxAnswer): void {
    void invoke("inbox_answer", { key, answer }).catch(() => {});
  }

  /** The question is no longer asked (its step got done another way). */
  close(id: string): void {
    void invoke("inbox_close", { id }).catch(() => {});
  }

  /** Removes the item with this key, if any (the problem it told about is gone). */
  resolve(key: string): void {
    // Asked of the database even before the list is loaded (start-up checks resolve early).
    void invoke("inbox_resolve", { key }).catch(() => {});
  }

  /** Marks one item read, or all of them (open questions stay unread: they still need the owner). */
  markRead(id?: string): void {
    if (!this.items.some((i) => !i.read && (id === undefined ? !isOpenQuestion(i) : i.id === id))) return;
    void invoke("inbox_mark_read", { id: id ?? null }).catch(() => {});
  }

  /** Removes an item; open questions are answered or closed, not removed. */
  remove(id: string): void {
    void invoke("inbox_remove", { id }).catch(() => {});
  }

  /** Clears everything except open questions. */
  clear(): void {
    void invoke("inbox_clear").catch(() => {});
  }
}

export const inbox = new InboxStore();
