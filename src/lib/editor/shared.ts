/**
 * One file shown in two editor groups (15i part 4). Each group has its own CodeMirror view; a change typed in one is
 * copied into the other, so both always hold the same text. Each view keeps its own undo history (copied changes are
 * not added to it), as in VS Code where undo follows the editor you type in.
 */
import {
  Annotation,
  Compartment,
  Facet,
  StateEffect,
  Transaction,
  type EditorState,
  type Extension,
} from "@codemirror/state";
import type { EditorView } from "@codemirror/view";

/** Marks a change copied from the other view of the same file. */
export const fromOtherView = Annotation.define<boolean>();

/** Which editor component built a state: its extensions call back into that component. */
export const ownerFacet = Facet.define<symbol, symbol | null>({ combine: (v) => v[0] ?? null });

/** Shared by all editors, so a state handed from one group to the other keeps its language without parsing again. */
export const languageSlot = new Compartment();

interface Live {
  owner: symbol;
  view: EditorView;
}

const live = new Map<string, Live[]>();

/** A view now shows this file. */
export function showing(key: string, owner: symbol, view: EditorView): void {
  const list = (live.get(key) ?? []).filter((l) => l.owner !== owner);
  list.push({ owner, view });
  live.set(key, list);
}

/** A view no longer shows this file. */
export function hiding(key: string, owner: symbol): void {
  const list = (live.get(key) ?? []).filter((l) => l.owner !== owner);
  if (list.length) live.set(key, list);
  else live.delete(key);
}

/** Views showing this file now (the first is the one edits from outside go through). */
export function liveViews(key: string): EditorView[] {
  return (live.get(key) ?? []).map((l) => l.view);
}

/** Another view showing this file, if any. */
export function otherView(key: string, owner: symbol): EditorView | null {
  return live.get(key)?.find((l) => l.owner !== owner)?.view ?? null;
}

/**
 * Gives a state built by another editor (a file moved between groups, or opened in both) this editor's extensions.
 * Text, selection, undo history and language stay; the other editor's callbacks go.
 */
export function adopt(state: EditorState, owner: symbol, extensions: (language: Extension) => Extension): EditorState {
  if (state.facet(ownerFacet) === owner) return state;
  const language = languageSlot.get(state) ?? [];
  return state.update({ effects: StateEffect.reconfigure.of(extensions(language)) }).state;
}

/** Copies a change typed in one view into the other views of the same file. */
export function forward(key: string, owner: symbol, tr: Transaction): void {
  for (const l of live.get(key) ?? []) {
    if (l.owner === owner) continue;
    const doc = l.view.state.doc;
    const annotations = [fromOtherView.of(true), Transaction.addToHistory.of(false)];
    if (doc.length === tr.startState.doc.length) {
      try {
        l.view.dispatch({ changes: tr.changes, annotations });
        continue;
      } catch {
        // Out of step: fall through and copy the whole text.
      }
    }
    l.view.dispatch({ changes: { from: 0, to: doc.length, insert: tr.state.doc }, annotations });
  }
}
