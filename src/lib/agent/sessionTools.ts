/**
 * Session abilities in the chat (24n part 6): edit an earlier message and send it again as a branch, rewind the file
 * changes made from a point on, and search in the conversation. Plain functions only; the pane does the rest.
 *
 * A branch is a new session that is told what was said before the edited message (the original session stays as it
 * was), so it works the same for every agent: Claude Code, Qanuvi's own and the free models.
 */
import type { Item, Row } from "./events";
import { revertEdits, type Edit } from "./diff";

type Tool = Extract<Item, { kind: "tool" }>;

/** At most this much of the earlier conversation goes into a branch's first message (the newest part is kept). */
export const BRANCH_CHARS = 24_000;

/** One line per earlier item, as a branch's agent reads it. */
function transcriptLine(item: Item): string {
  switch (item.kind) {
    case "user":
      return `Owner: ${item.text}`;
    case "text":
      return item.live ? "" : `Agent: ${item.text}`;
    case "tool":
      return `(Agent step: ${item.name}${item.summary ? ` ${item.summary}` : ""}${item.state === "failed" ? ", failed" : ""})`;
    case "todos":
      return `(Agent to-do list: ${item.todos.map((t) => `${t.status === "completed" ? "[x]" : "[ ]"} ${t.text}`).join("; ")})`;
    default:
      return "";
  }
}

/**
 * What goes in front of a branch's first message: what was said before the edited message (cut from the oldest end).
 * The edited message follows it. `before` holds the items before the edited message; empty when there were none.
 */
export function branchPreamble(before: Item[], rewound: boolean): string {
  const lines = before.map(transcriptLine).filter(Boolean);
  let text = lines.join("\n\n");
  let cut = false;
  if (text.length > BRANCH_CHARS) {
    text = text.slice(text.length - BRANCH_CHARS);
    text = text.slice(text.indexOf("\n\n") + 2);
    cut = true;
  }
  if (!lines.length) return "";
  return [
    "This conversation goes on from an earlier one: the owner went back to one of their messages, changed it and sent",
    "it again. What was said before that message (for context only; do not redo it):",
    "",
    "<earlier>",
    ...(cut ? ["(… the oldest part left out)", ""] : []),
    text,
    "</earlier>",
    "",
    rewound
      ? "The file changes made from that message on were undone, so the files are as they were before it."
      : "The files were not reset: changes the earlier conversation made after that message may still be in them.",
    "",
    "The owner's message, as changed:",
    "",
    "",
  ].join("\n");
}

/** What rewinding from an item on would undo, per file, and what it cannot. */
export interface Rewind {
  /** Files with the edits to undo there, oldest first. */
  files: { file: string; edits: Edit[] }[];
  /** Files a step wrote whole (the old text is not known), or changed by a step that failed. */
  cannot: { file: string; why: string }[];
  /** File-changing steps from that point on. */
  steps: number;
}

/** The file changes made by the steps at `from` and after (the reply to a message and everything later). */
export function rewindFrom(items: Item[], from: number): Rewind {
  const files = new Map<string, Edit[]>();
  const cannot = new Map<string, string>();
  let steps = 0;
  for (const item of items.slice(from)) {
    if (item.kind !== "tool" || !item.edits?.length || !item.file) continue;
    if (item.state === "failed") continue; // a failed edit changed nothing
    steps++;
    if (item.whole) {
      cannot.set(item.file, "written whole by a step (its earlier text is not known)");
      continue;
    }
    files.set(item.file, [...(files.get(item.file) ?? []), ...item.edits]);
  }
  for (const f of cannot.keys()) files.delete(f);
  return {
    files: [...files].map(([file, edits]) => ({ file, edits })),
    cannot: [...cannot].map(([file, why]) => ({ file, why })),
    steps,
  };
}

/** A file's text with the rewind's edits undone (newest first), or null when it changed since. */
export function rewindText(text: string, edits: Edit[]): string | null {
  return revertEdits(text, edits);
}

/** The text an item is searched by. */
export function searchText(item: Item): string {
  switch (item.kind) {
    case "user":
    case "text":
    case "thinking":
    case "notice":
      return item.text;
    case "tool":
      return [item.name, item.summary, item.input, item.output, ...(item.edits ?? []).map((e) => e.new)].join("\n");
    case "todos":
      return item.todos.map((t) => t.text).join("\n");
    case "approval":
      return [item.tool, item.summary, item.detail].join("\n");
    case "result":
      return item.text;
  }
}

/** The items that contain the words (case does not matter), in order. */
export function searchItems(items: Item[], query: string): string[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return items.filter((i) => searchText(i).toLowerCase().includes(q)).map((i) => i.id);
}

/** The groups and folds to open so the items show (by row id). */
export function rowsToOpen(rows: Row[], ids: Set<string>): string[] {
  const out: string[] = [];
  const has = (r: Row): boolean => {
    if (r.kind === "group") {
      const hit = r.steps.some((s) => ids.has(s.id));
      if (hit) out.push(r.id);
      return hit;
    }
    if (r.kind === "fold") {
      let hit = false;
      for (const inner of r.rows) if (has(inner)) hit = true;
      if (hit) out.push(r.id);
      return hit;
    }
    return ids.has(r.id);
  };
  for (const r of rows) has(r);
  return out;
}

/** Where the words are in a text (case does not matter): start offsets. */
export function matchesIn(text: string, query: string): number[] {
  const q = query.toLowerCase();
  if (!q) return [];
  const t = text.toLowerCase();
  const out: number[] = [];
  for (let at = t.indexOf(q); at >= 0; at = t.indexOf(q, at + q.length)) out.push(at);
  return out;
}

export type { Tool };
