/**
 * What the editor does with language server answers that reach beyond the shown file (15i): opening a definition,
 * listing references, and applying a rename to every file it touches.
 */
import type { EditorView } from "@codemirror/view";
import { api, errorText } from "$lib/api";
import { editor, tabKey } from "$lib/stores/editor.svelte";
import { lsp, type Doc, type Place } from "$lib/stores/lsp.svelte";
import { layout } from "$lib/layout/layout.svelte";
import { projects } from "$lib/stores/projects.svelte";
import { definition, references, renameEdits } from "./codemirror";
import { applyEdits, editsByFile, linesOf, relativePath, toOffset } from "./protocol";

const rootOf = (projectId: string) => projects.list.find((p) => p.id === projectId)?.path ?? "";

/** Fills in each place's line text (open tabs from the editor, other files read once each). */
async function withText(places: Place[]): Promise<Place[]> {
  const texts = new Map<string, string | null>();
  for (const p of places.slice(0, 500)) {
    const key = tabKey(p.projectId, p.path);
    if (texts.has(key)) continue;
    const state = editor.states.get(key);
    const tab = editor.tabs.find((t) => t.key === key);
    texts.set(
      key,
      state
        ? state.doc.toString()
        : tab
          ? tab.savedText
          : await api.files
              .read(p.projectId, p.path)
              .then((f) => (f.binary || f.too_large ? null : f.text))
              .catch(() => null),
    );
  }
  return places.map((p) => {
    const text = texts.get(tabKey(p.projectId, p.path));
    if (!text) return p;
    const doc = linesOf(text);
    if (p.line > doc.lines) return p;
    const l = doc.line(p.line);
    return { ...p, text: text.slice(l.from, l.to).trim().slice(0, 200) };
  });
}

function showReferences(title: string, places: Place[]) {
  lsp.references = { title, places };
  lsp.bottom = "references";
  if (!layout.state.terminalVisible) layout.toggleTerminal();
  withText(places).then((full) => {
    if (lsp.references?.title === title) lsp.references = { title, places: full };
  });
}

/**
 * Go to definition. Answers true when the language server handled it (opened the place, listed several, or said
 * there is nothing); false sends the editor to the code index instead.
 */
export async function goToDefinition(view: EditorView, doc: Doc, name: string): Promise<string | true | false> {
  const r = await definition(view, doc, rootOf(doc.projectId)).catch(() => null);
  if (!r) return false;
  if (r.places.length === 1) {
    const p = r.places[0];
    await editor.open(p.projectId, p.path, p.line, p.column);
    return true;
  }
  if (r.places.length > 1) {
    showReferences(`Definitions of ${name}`, r.places);
    return true;
  }
  return r.note;
}

export async function findReferences(view: EditorView, doc: Doc, name: string): Promise<string> {
  try {
    const r = await references(view, doc, rootOf(doc.projectId));
    if (!r) return "The language server of this file cannot find references.";
    if (!r.places.length) return r.note;
    showReferences(`References to ${name}`, r.places);
    return "";
  } catch (e) {
    return errorText(e);
  }
}

/** Renames the symbol at `pos` everywhere the server says; answers what happened, in words. */
export async function renameSymbol(view: EditorView, doc: Doc, pos: number, name: string): Promise<string> {
  let edit: unknown;
  try {
    edit = await renameEdits(view, doc, pos, name);
  } catch (e) {
    return errorText(e);
  }
  if (edit === null) return "The language server of this file cannot rename.";
  const root = rootOf(doc.projectId);
  const files = editsByFile(edit);
  let changed = 0;
  const skipped: string[] = [];
  for (const [uri, edits] of files) {
    const path = relativePath(root, uri);
    if (path === null) {
      skipped.push(uri.split("/").pop() ?? uri);
      continue;
    }
    const key = tabKey(doc.projectId, path);
    const tab = editor.tabs.find((t) => t.key === key);
    const changes = (state: { doc: Parameters<typeof toOffset>[0] }) =>
      edits.map((e) => ({
        from: toOffset(state.doc, e.range.start),
        to: toOffset(state.doc, e.range.end),
        insert: e.newText,
      }));
    if (key === doc.key) {
      // The shown file: one undoable change.
      view.dispatch({ changes: changes(view.state) });
    } else if (tab) {
      // Open in another tab: changed there, left unsaved like any edit.
      const state = editor.states.get(key);
      if (state) {
        const next = state.update({ changes: changes(state) }).state;
        editor.states.set(key, next);
        tab.dirty = next.doc.toString() !== tab.savedText;
        lsp.changed({ key, projectId: doc.projectId, path }, () => next.doc.toString());
      } else {
        const text = applyEdits(tab.savedText, edits);
        try {
          tab.hash = await api.files.write(doc.projectId, path, text, tab.hash);
          tab.savedText = text;
        } catch (e) {
          skipped.push(`${path} (${errorText(e)})`);
          continue;
        }
      }
    } else {
      try {
        const f = await api.files.read(doc.projectId, path);
        await api.files.write(doc.projectId, path, applyEdits(f.text, edits), f.hash);
      } catch (e) {
        skipped.push(`${path} (${errorText(e)})`);
        continue;
      }
    }
    changed++;
  }
  const where = changed === 1 ? "1 file" : `${changed} files`;
  return skipped.length ? `Renamed in ${where}; not changed: ${skipped.join(", ")}` : `Renamed in ${where}.`;
}
