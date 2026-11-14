/**
 * What the editor does with language server answers that reach beyond a plain lookup (15i): opening a definition,
 * listing references, applying a rename or a quick fix to every file it touches, formatting, and "Fix with agent".
 */
import { closeHoverTooltips, type EditorView } from "@codemirror/view";
import { api, errorText } from "$lib/api";
import { editor, tabKey } from "$lib/stores/editor.svelte";
import { lsp, type Doc, type Place, type Problem } from "$lib/stores/lsp.svelte";
import { layout } from "$lib/layout/layout.svelte";
import { projects } from "$lib/stores/projects.svelte";
import { agent } from "$lib/stores/agent.svelte";
import { contextMenu } from "$lib/stores/contextMenu.svelte";
import { definition, references, renameEdits } from "./codemirror";
import { liveViews } from "$lib/editor/shared";
import {
  applyEdits,
  editsByFile,
  linesOf,
  relativePath,
  serverFor,
  tightChanges,
  toPosition,
  type Range,
  type TextEdit,
} from "./protocol";
import { actionLabels, fixText, guessIndent, orderActions, touches, type CodeAction } from "./fix";

const rootOf = (projectId: string) => projects.list.find((p) => p.id === projectId)?.path ?? "";

/** An editor view showing this file now (either group), so edits to it go through the view. */
const shownView = (key: string): EditorView | null => liveViews(key)[0] ?? null;

/** A file's text: the open tab's editor text, its saved text, or the file read from disk; null if not text. */
async function textOf(projectId: string, path: string): Promise<string | null> {
  const key = tabKey(projectId, path);
  const shown = shownView(key);
  if (shown) return shown.state.doc.toString();
  const state = editor.states.get(key);
  if (state) return state.doc.toString();
  const tab = editor.tabs.find((t) => t.key === key);
  if (tab) return tab.savedText;
  return api.files
    .read(projectId, path)
    .then((f) => (f.binary || f.too_large ? null : f.text))
    .catch(() => null);
}

/** Fills in each place's line text (open tabs from the editor, other files read once each). */
async function withText(places: Place[]): Promise<Place[]> {
  const texts = new Map<string, string | null>();
  for (const p of places.slice(0, 500)) {
    const key = tabKey(p.projectId, p.path);
    if (texts.has(key)) continue;
    texts.set(key, await textOf(p.projectId, p.path));
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

const CANNOT_RENAME = "The language server of this file cannot rename.";

/** Why a rename cannot start, asked before the name prompt opens; "" when it can. */
export async function renameBlocked(doc: Doc): Promise<string> {
  const c = await lsp.clientFor(doc);
  if (c?.client.capabilities.renameProvider) return "";
  // Intelephense keeps rename for its paid licence; say so rather than leave the user guessing.
  return c?.client.server === "php" ? "Intelephense renames only with its paid licence." : CANNOT_RENAME;
}

/** Renames the symbol at `pos` everywhere the server says; answers what happened, in words. */
export async function renameSymbol(view: EditorView, doc: Doc, pos: number, name: string): Promise<string> {
  let edit: unknown;
  try {
    edit = await renameEdits(view, doc, pos, name);
  } catch (e) {
    return errorText(e);
  }
  if (edit === null) return CANNOT_RENAME;
  const { changed, skipped } = await applyWorkspaceEdit(doc.projectId, edit);
  const where = changed === 1 ? "1 file" : `${changed} files`;
  return skipped.length ? `Renamed in ${where}; not changed: ${skipped.join(", ")}` : `Renamed in ${where}.`;
}

/** Edits as one CodeMirror change set (positions in the text before the change; whole-file rewrites made tight). */
function changesFor(doc: { toString(): string }, edits: TextEdit[]) {
  return tightChanges(doc.toString(), edits);
}

/**
 * Applies a WorkspaceEdit (a rename, a quick fix, an edit a server sends): the shown file as one undoable change,
 * other open tabs changed and left unsaved, files not open written straight to disk. Files outside the project are
 * not touched.
 */
export async function applyWorkspaceEdit(
  projectId: string,
  edit: unknown,
): Promise<{ changed: number; skipped: string[] }> {
  const root = rootOf(projectId);
  let changed = 0;
  const skipped: string[] = [];
  for (const [uri, edits] of editsByFile(edit)) {
    const path = relativePath(root, uri);
    if (path === null) {
      skipped.push(uri.split("/").pop() ?? uri);
      continue;
    }
    if (!edits.length) continue;
    const key = tabKey(projectId, path);
    const tab = editor.tabs.find((t) => t.key === key);
    const shown = shownView(key);
    if (shown) {
      shown.dispatch({ changes: changesFor(shown.state.doc, edits) });
    } else if (tab) {
      const state = editor.states.get(key);
      if (state) {
        const next = state.update({ changes: changesFor(state.doc, edits) }).state;
        editor.states.set(key, next);
        tab.dirty = next.doc.toString() !== tab.savedText;
        lsp.changed({ key, projectId, path }, () => next.doc.toString());
      } else {
        const text = applyEdits(tab.savedText, edits);
        try {
          tab.hash = await api.files.write(projectId, path, text, tab.hash);
          tab.savedText = text;
        } catch (e) {
          skipped.push(`${path} (${errorText(e)})`);
          continue;
        }
      }
    } else {
      try {
        const f = await api.files.read(projectId, path);
        await api.files.write(projectId, path, applyEdits(f.text, edits), f.hash);
      } catch (e) {
        skipped.push(`${path} (${errorText(e)})`);
        continue;
      }
    }
    changed++;
  }
  return { changed, skipped };
}

// Edits a server sends by itself (workspace/applyEdit, after a quick fix runs a command).
lsp.editApplier = async (projectId, edit) => {
  const r = await applyWorkspaceEdit(projectId, edit);
  return r.skipped.length === 0;
};

/**
 * Formats the file (or only the selection) through its language server: one undoable change, the cursor kept.
 * Answers what happened in words; "" when formatting changed nothing.
 */
export async function formatDocument(
  view: EditorView,
  doc: Doc,
  onlySelection = false,
  timeout = 15_000,
  onSave = false,
): Promise<string> {
  const c = await lsp.clientFor(doc);
  if (!c) return onSave ? "" : "No language server is running for this file.";
  const caps = c.client.capabilities;
  const sel = view.state.selection.main;
  const range = onlySelection && !sel.empty;
  if (range ? !caps.documentRangeFormattingProvider : !caps.documentFormattingProvider)
    return onSave
      ? ""
      : range
        ? "The language server of this file cannot format a selection."
        : "The language server of this file cannot format.";
  const before = view.state.doc;
  lsp.flush(doc, before.toString());
  const params = {
    textDocument: { uri: c.uri },
    options: { ...guessIndent(before.toString()), trimTrailingWhitespace: true, insertFinalNewline: true },
    ...(range ? { range: { start: toPosition(before, sel.from), end: toPosition(before, sel.to) } } : {}),
  };
  let edits: TextEdit[] | null;
  try {
    edits = await c.client.request<TextEdit[] | null>(
      range ? "textDocument/rangeFormatting" : "textDocument/formatting",
      params,
      timeout,
    );
  } catch (e) {
    return errorText(e);
  }
  // Typed on (or switched tabs) while the server worked: its edits no longer fit.
  if (view.state.doc !== before) return "The file changed while it was formatted; nothing was changed.";
  if (!edits?.length) return "";
  view.dispatch({ changes: changesFor(before, edits), userEvent: "format" });
  return "Formatted.";
}

/** The problems at the selection (or, with none under the cursor, on the cursor's line). */
export function problemsAt(view: EditorView, doc: Doc): Problem[] {
  const all = lsp.problemsFor(doc.projectId, doc.path);
  const sel = view.state.selection.main;
  const from = toPosition(view.state.doc, sel.from);
  const to = toPosition(view.state.doc, sel.to);
  const here = all.filter((p) => touches(p.range, from, to));
  if (here.length || !sel.empty) return here;
  return all.filter((p) => p.range.start.line <= from.line && p.range.end.line >= from.line);
}

/** Problems without hints: hints still go to the server (its fixes hang on them), but are not counted or sent on. */
export function realProblems(problems: Problem[]): Problem[] {
  return problems.filter((p) => p.severity !== "hint");
}

type Client = NonNullable<Awaited<ReturnType<typeof lsp.clientFor>>>;

/** The server's code actions for the selection and the problems there; null when it offers none at all. */
async function codeActions(
  view: EditorView,
  doc: Doc,
  problems: Problem[],
): Promise<{ client: Client; actions: CodeAction[] } | null> {
  const c = await lsp.clientFor(doc);
  if (!c?.client.capabilities.codeActionProvider) return null;
  lsp.flush(doc, view.state.doc.toString());
  const sel = view.state.selection.main;
  const range: Range =
    sel.empty && problems.length
      ? problems[0].range
      : { start: toPosition(view.state.doc, sel.from), end: toPosition(view.state.doc, sel.to) };
  const list = await c.client
    .request<CodeAction[] | null>("textDocument/codeAction", {
      textDocument: { uri: c.uri },
      range,
      context: {
        diagnostics: problems.map((p) => p.raw ?? { range: p.range, message: p.message }),
        triggerKind: 1,
      },
    })
    .catch(() => null);
  return { client: c, actions: orderActions(list ?? []) };
}

/** Runs a code action: its edit first, then its command (whose edits come back as workspace/applyEdit). */
async function runAction(c: Client, projectId: string, action: CodeAction): Promise<string> {
  let a = action;
  const caps = c.client.capabilities.codeActionProvider;
  try {
    if (!a.edit && typeof caps === "object" && caps.resolveProvider && a.data !== undefined)
      a = (await c.client.request<CodeAction | null>("codeAction/resolve", a)) ?? a;
    let note = "";
    if (a.edit) {
      const r = await applyWorkspaceEdit(projectId, a.edit);
      if (r.skipped.length) note = `Not changed: ${r.skipped.join(", ")}`;
    }
    const cmd = typeof a.command === "string" ? { command: a.command, arguments: a.arguments } : a.command;
    if (cmd?.command)
      await c.client.request("workspace/executeCommand", { command: cmd.command, arguments: cmd.arguments ?? [] });
    return note;
  } catch (e) {
    return `${a.title}: ${errorText(e)}`;
  }
}

/** Why the menu has no fix: none here, or none from this server at all (free Intelephense keeps them paid). */
function noFix(found: unknown, doc: Doc): string {
  if (found) return "The language server has no fix for this";
  return serverFor(doc.path) === "php"
    ? "Intelephense offers quick fixes only with its paid licence"
    : "The language server of this file offers no quick fixes";
}

/**
 * Ctrl+. : a menu at the cursor with the server's fixes for what is there, and "Fix with agent" for its problems.
 * `note` shows what happened (nothing to fix, a fix that failed).
 */
export async function quickFixMenu(view: EditorView, doc: Doc, note: (text: string) => void): Promise<void> {
  const problems = problemsAt(view, doc);
  const found = await codeActions(view, doc, problems);
  const real = realProblems(problems);
  const actions = (found?.actions ?? []).slice(0, 25);
  if (!actions.length && !problems.length) {
    note(found ? "No quick fixes here." : noFix(found, doc));
    return;
  }
  const at = view.coordsAtPos(view.state.selection.main.head);
  const labels = actionLabels(actions);
  // The hover box (docs and problem) would stay over the code under the menu.
  view.dispatch({ effects: closeHoverTooltips });
  contextMenu.showAt(at?.left ?? 200, (at?.bottom ?? 200) + 2, [
    found &&
      actions.map((a, i) => ({
        label: labels[i],
        disabled: !!a.disabled,
        action: async () => {
          const r = await runAction(found.client, doc.projectId, a);
          if (r) note(r);
          view.focus();
        },
      })),
    !actions.length && [{ label: noFix(found, doc), disabled: true, action: () => {} }],
    real.length > 0 && [
      {
        label: real.length === 1 ? "Fix with agent" : `Fix these ${real.length} problems with agent`,
        action: () => fixWithAgent(real),
      },
    ],
  ]);
}

const fence = (path: string) => {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return /^[a-z0-9]{1,8}$/.test(ext) ? ext : "";
};

/**
 * Adds a request to fix the problems (those in the first one's file) to the conversation's message box, with the
 * lines around a single problem. The user reads and sends it; nothing goes to the agent by itself.
 */
export async function fixWithAgent(problems: Problem[]): Promise<void> {
  if (!problems.length) return;
  const first = problems[0];
  const same = problems.filter((p) => p.projectId === first.projectId && p.path === first.path);
  const text = await textOf(first.projectId, first.path);
  if (!layout.state.agentVisible) layout.toggleAgent();
  agent.attach(fixText(same, text, fence(first.path)));
}
