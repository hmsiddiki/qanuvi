/**
 * The Inbox (24f, 15f, 24u part 4): what Qanuvi has to tell the owner — plan run reports, questions a run waits on,
 * sign-ins that need renewing, newer knowledge. Kept in Qanuvi's database, so every window shows the same list and
 * the first answer anywhere closes a question everywhere. This store is the window's copy, kept fresh by the
 * "qanuvi-inbox" event.
 *
 * A connected master (a server without a window, 24z R2d part 3) has its own Inbox: a run there asks its questions
 * in it. Its items are shown in the same list, marked with the master, and what is done to one goes to that master.
 * The Inbox screen is the one view shared by all machines (24z3 C2): a master whose connection ended keeps its items
 * listed, marked offline, until it is back; nothing is answered or changed from that copy.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { callMaster, callOn, callThisPc, machineName } from "$lib/remote";
import { forgetNotifications, learnNotifications, notifyFor } from "./notify.svelte";
import { listen } from "@tauri-apps/api/event";
import type { Place } from "./ui.svelte";
import { keepAnswer, loadPlanState, savePlanState } from "$lib/plan/planstate";
import { DECIDE_TEXT, holdReply, type AnswerChoice, type HoldCard, type InboxAnswer } from "$lib/plan/holdask";

export { DECIDE_TEXT, holdReply, type AnswerChoice, type HoldCard, type InboxAnswer };

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
  /** n1: the question laid out (a blocked or unfinished plan step); `text` holds the same as plain text. */
  card?: HoldCard;
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
  kind: "run" | "sign-in" | "knowledge" | "chats" | "drafts";
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
  /** The master whose Inbox has it; absent for this computer's own. Set by the window only. */
  master?: string;
  /** Its master is not connected now: shown as it was, never answered or changed from this copy. Window only. */
  offline?: boolean;
}

export type NewInboxItem = Pick<InboxItem, "kind" | "title" | "text"> &
  Partial<Pick<InboxItem, "type" | "open" | "key" | "form" | "source">>;

type Change = {
  change: "add" | "answer" | "close" | "read" | "remove" | "resolve";
  item?: InboxItem | null;
  /** From a run: a new item, not one that replaced an item with the same key. */
  new?: boolean;
};

const newest = (a: InboxItem, b: InboxItem) => b.at - a.at;

/** A question still waiting for the owner. */
export const isOpenQuestion = (i: InboxItem) => i.type === "answer" && i.status === "open";

class InboxStore {
  /** This computer's items and every connected master's, newest first. */
  items = $state<InboxItem[]>([]);
  #local: InboxItem[] = [];
  #remote: Record<string, InboxItem[]> = {};
  /** Masters whose Inbox is followed now (a stream that ends with the connection). */
  #following = new Set<string>();
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
      if (Array.isArray(old) && old.length) await callThisPc("inbox_import", { items: old });
      if (raw !== null) localStorage.removeItem(OLD_KEY);
    } catch {
      // A broken old list is not worth keeping.
      localStorage.removeItem(OLD_KEY);
    }
    await this.refresh();
  }

  async refresh(): Promise<void> {
    try {
      this.#local = await callThisPc<InboxItem[]>("inbox_list");
      this.#merge();
    } catch {
      // Keeps what it shows; the next change refreshes again.
    }
  }

  #merge(): void {
    const remote = Object.values(this.#remote).flat();
    this.items = remote.length ? [...this.#local, ...remote].sort(newest) : this.#local;
  }

  #setRemote(master: string, list: InboxItem[]): void {
    if (!this.#following.has(master)) return;
    this.#remote[master] = list.map((i) => ({ ...i, master }));
    this.#merge();
  }

  /**
   * Shows a connected master's Inbox and follows its changes until the connection ends (masters.svelte.ts calls
   * this on connect and `dropMaster` when it ends). A new question there shows a desktop notification, once.
   */
  async followMaster(master: string): Promise<void> {
    if (this.#following.has(master)) return;
    this.#following.add(master);
    await learnNotifications(master);
    const events = new Channel<Change>();
    events.onmessage = (c) => {
      const item = c.item ? { ...c.item, master } : null;
      if (c.change === "answer" && item) for (const fn of this.#answered) fn(item);
      if (c.change === "add" && item && c.new !== false)
        void invoke("notify_item", {
          item: { ...c.item, machine: machineName(master) },
          settings: notifyFor(master),
        }).catch(() => {});
      void callMaster<InboxItem[]>(master, "inbox_list", {})
        .then((list) => this.#setRemote(master, list))
        .catch(() => {});
    };
    try {
      this.#setRemote(master, await callMaster<InboxItem[]>(master, "inbox_watch", { onEvent: events }));
    } catch {
      // An older master has no Inbox commands: its questions stay on it.
      this.#following.delete(master);
    }
  }

  /**
   * The connection to a master ended: its items stay listed, marked offline (they stay on the master), so the Inbox
   * shows "not connected" for them until it is back and its own list replaces them.
   */
  dropMaster(master: string): void {
    this.#following.delete(master);
    forgetNotifications(master);
    if (!(master in this.#remote)) return;
    this.#remote[master] = this.#remote[master].map((i) => ({ ...i, offline: true }));
    this.#merge();
  }

  /** The master was removed from the list: its items leave the Inbox. */
  forgetMaster(master: string): void {
    this.dropMaster(master);
    if (!(master in this.#remote)) return;
    delete this.#remote[master];
    this.#merge();
  }

  /** The master that has this item, or null for this computer's own. */
  #masterOf(id: string): string | null {
    return this.items.find((i) => i.id === id)?.master ?? null;
  }

  /** The item can be answered or changed now (its machine is connected). */
  #live(id: string): boolean {
    const item = this.items.find((i) => i.id === id);
    return !!item && !item.offline;
  }

  /** Calls the Inbox command on this computer, or on the master that has the item. */
  #call<T>(master: string | null, cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    return callOn<T>(master, cmd, args);
  }

  /** Called with every answered question, whichever window (or later device) answered it. */
  onAnswer(fn: (item: InboxItem) => void): () => void {
    this.#answered.add(fn);
    return () => this.#answered.delete(fn);
  }

  /**
   * Adds an item; one with the same key is replaced (and stays read when nothing in it changed). An item about a
   * master's own work goes into that master's Inbox, so it is listed (and filtered) under that machine; an older
   * master without the command gets it here instead, so it is never lost.
   */
  /** As add, answering once the item is in (a window that is going waits for it, i1). */
  async addNow(item: NewInboxItem, master: string | null = null): Promise<void> {
    const args = { item: { type: "info", ...item } };
    const here = () => callThisPc<InboxItem>("inbox_add", args).then(() => {});
    if (!master) return here();
    await callMaster<InboxItem>(master, "inbox_add", args).then(() => {}, here);
  }

  add(item: NewInboxItem, master: string | null = null): void {
    const args = { item: { type: "info", ...item } };
    const here = () => void callThisPc<InboxItem>("inbox_add", args).catch(() => {});
    if (!master) return here();
    void callMaster<InboxItem>(master, "inbox_add", args).catch(here);
  }

  /**
   * Answers a question. Only the first answer anywhere counts: false when another window (or device) answered or
   * closed it first. A plan step's answer is kept for its plan, so a run started later goes on with it too.
   */
  async answer(id: string, answer: InboxAnswer): Promise<boolean> {
    if (!this.#live(id)) return false;
    const master = this.#masterOf(id);
    const done = await this.#call<InboxItem | null>(master, "inbox_answer", { id, answer }).catch(() => null);
    // A master's run reads its answer from the master's Inbox; its plan's state is kept there.
    if (!master && done?.source?.kind === "hold") {
      const { choice, text } = holdReply(answer);
      const s = done.source;
      savePlanState(s.projectId, s.file, keepAnswer(loadPlanState(s.projectId, s.file), s.step, choice, text));
    }
    return !!done;
  }

  /** The run panel answered a question: it closes everywhere (the run panel keeps the answer itself). */
  answerKey(key: string, answer: InboxAnswer): void {
    void callThisPc("inbox_answer", { key, answer }).catch(() => {});
  }

  /** The question is no longer asked (its step got done another way). */
  close(id: string): void {
    void callThisPc("inbox_close", { id }).catch(() => {});
  }

  /** Removes the item with this key, if any (the problem it told about is gone). */
  resolve(key: string): void {
    // Asked of the database even before the list is loaded (start-up checks resolve early).
    void callThisPc("inbox_resolve", { key }).catch(() => {});
  }

  /** Marks one item read, or all of them (open questions stay unread: they still need the owner). */
  markRead(id?: string): void {
    const unread = this.items.filter(
      (i) => !i.read && !i.offline && (id === undefined ? !isOpenQuestion(i) : i.id === id),
    );
    for (const master of new Set(unread.map((i) => i.master ?? null)))
      void this.#call(master, "inbox_mark_read", { id: id ?? null }).catch(() => {});
  }

  /** Removes an item; open questions are answered or closed, not removed. */
  remove(id: string): void {
    if (!this.#live(id)) return;
    void this.#call(this.#masterOf(id), "inbox_remove", { id }).catch(() => {});
  }

  /**
   * Clears everything except open questions, here and on every connected master; or on one machine only (null is
   * This PC), when the Inbox shows one machine's items.
   */
  clear(machine?: string | null): void {
    if (machine === undefined || machine === null) void callThisPc("inbox_clear").catch(() => {});
    for (const master of this.#following)
      if (machine === undefined || machine === master) void callMaster(master, "inbox_clear", {}).catch(() => {});
  }
}

export const inbox = new InboxStore();
