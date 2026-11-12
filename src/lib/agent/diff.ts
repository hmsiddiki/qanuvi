/**
 * Edits shown as diffs in the chat (24n part 3): the lines an Edit, MultiEdit or Write step removed and added, with
 * a little context, and undoing an edit from the chat (Revert).
 */
import { hunks } from "$lib/git/hunks";

/** One change a step made: the text it replaced and the text it put there (a whole-file write has no old text). */
export interface Edit {
  old: string;
  new: string;
  /** replace_all: every place, not only the first. */
  all?: boolean;
}

export interface DiffLine {
  type: "ctx" | "add" | "del" | "gap";
  text: string;
}

/** Lines of context kept around each change. */
const CONTEXT = 2;
/** At most this many diff lines per edit in the chat (the rest is one "… more lines" line). */
export const DIFF_MAX = 400;

const lines = (t: string) => (t === "" ? [] : t.replace(/\n$/, "").split("\n"));

/** The diff of one edit, line by line, with a little context and gaps where unchanged lines are left out. */
export function editDiff(before: string, after: string): DiffLine[] {
  const out: DiffLine[] = [];
  const a = before.endsWith("\n") || !before ? before : `${before}\n`;
  const b = after.endsWith("\n") || !after ? after : `${after}\n`;
  let at = 0; // where in a the last change ended
  const all = hunks(a, b);
  for (const [i, h] of all.entries()) {
    const between = lines(a.slice(at, h.fromA));
    if (between.length > CONTEXT * (i === 0 ? 1 : 2)) {
      if (i > 0) out.push(...between.slice(0, CONTEXT).map((text) => ({ type: "ctx" as const, text })));
      out.push({ type: "gap", text: "" });
      out.push(...between.slice(-CONTEXT).map((text) => ({ type: "ctx" as const, text })));
    } else out.push(...between.map((text) => ({ type: "ctx" as const, text })));
    out.push(...lines(a.slice(h.fromA, h.toA)).map((text) => ({ type: "del" as const, text })));
    out.push(...lines(b.slice(h.fromB, h.toB)).map((text) => ({ type: "add" as const, text })));
    at = h.toA;
  }
  const rest = lines(a.slice(at));
  if (all.length) {
    out.push(...rest.slice(0, CONTEXT).map((text) => ({ type: "ctx" as const, text })));
    if (rest.length > CONTEXT) out.push({ type: "gap", text: "" });
  }
  if (out.length > DIFF_MAX) {
    const more = out.length - DIFF_MAX;
    return [...out.slice(0, DIFF_MAX), { type: "gap", text: `${more} more lines` }];
  }
  return out;
}

/** How many lines the edits added and removed, for the card's head. */
export function diffCounts(edits: Edit[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const e of edits)
    for (const l of editDiff(e.old, e.new)) {
      if (l.type === "add") added++;
      else if (l.type === "del") removed++;
    }
  return { added, removed };
}

/**
 * The file's text with the edits undone, or null when that cannot be done safely: a whole-file write (the old text is
 * not known), or the edited text is no longer in the file exactly as the step left it (changed since).
 */
export function revertEdits(text: string, edits: Edit[]): string | null {
  let out = text;
  for (const e of [...edits].reverse()) {
    if (!e.new) return null;
    const first = out.indexOf(e.new);
    if (first < 0) return null;
    if (e.all) out = out.split(e.new).join(e.old);
    else {
      // Only when the edited text is there once: otherwise it is not clear which place to undo.
      if (out.indexOf(e.new, first + 1) >= 0) return null;
      out = out.slice(0, first) + e.old + out.slice(first + e.new.length);
    }
  }
  return out;
}
