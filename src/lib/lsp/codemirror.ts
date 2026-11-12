/**
 * The editor's side of the language servers (15i): hover, completion, underlines from the server's problems, and the
 * lookups the editor's keys and menu start (go to definition, find references, rename).
 */
import type { Extension } from "@codemirror/state";
import { EditorState } from "@codemirror/state";
import { EditorView, hoverTooltip } from "@codemirror/view";
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import type { Diagnostic } from "@codemirror/lint";
import { renderMarkdown } from "$lib/agent/markdown";
import { lsp, type Doc, type Place, type Problem } from "$lib/stores/lsp.svelte";
import { completionType, markupText, relativePath, toOffset, toPosition, type Position, type Range } from "./protocol";

/** The shown file, or null when the editor shows something without a server (a config file, a binary). */
export type Current = () => Doc | null;

async function ask(view: EditorView, doc: Doc) {
  const c = await lsp.clientFor(doc);
  if (!c) return null;
  lsp.flush(doc, view.state.doc.toString());
  return c;
}

const atPos = (view: EditorView, pos: number) => toPosition(view.state.doc, pos);

function hover(current: Current): Extension {
  return hoverTooltip(
    async (view, pos) => {
      const doc = current();
      if (!doc) return null;
      const c = await ask(view, doc);
      if (!c?.client.capabilities.hoverProvider) return null;
      const r = await c.client
        .request<{ contents: unknown; range?: Range } | null>("textDocument/hover", {
          textDocument: { uri: c.uri },
          position: atPos(view, pos),
        })
        .catch(() => null);
      const text = markupText(r?.contents).trim();
      if (!text) return null;
      const from = r?.range ? toOffset(view.state.doc, r.range.start) : pos;
      const to = r?.range ? toOffset(view.state.doc, r.range.end) : pos;
      return {
        pos: from,
        end: to,
        above: true,
        create() {
          const dom = document.createElement("div");
          dom.className = "qx-lsp-hover";
          dom.innerHTML = renderMarkdown(text.length > 4000 ? `${text.slice(0, 4000)}…` : text);
          return { dom };
        },
      };
    },
    { hoverTime: 350 },
  );
}

interface LspCompletion {
  label: string;
  kind?: number;
  detail?: string;
  documentation?: unknown;
  sortText?: string;
  filterText?: string;
  insertText?: string;
  insertTextFormat?: number;
  textEdit?: { range?: Range; replace?: Range; newText: string };
}

/** Snippet text ($1, ${2:name}) as plain text: the placeholders' own words stay. */
function plainSnippet(text: string): string {
  return text.replace(/\$\{\d+(?::([^}]*))?\}/g, "$1").replace(/\$\d+/g, "");
}

function completion(current: Current): Extension {
  const source = async (ctx: CompletionContext): Promise<CompletionResult | null> => {
    const doc = current();
    if (!doc) return null;
    const word = ctx.matchBefore(/[\w$]+/);
    const before = ctx.state.sliceDoc(Math.max(0, ctx.pos - 2), ctx.pos);
    const trigger = /(\.|->|::|\$|:|\\)$/.test(before);
    if (!ctx.explicit && !word && !trigger) return null;
    const c = await lsp.clientFor(doc);
    if (!c?.client.capabilities.completionProvider || ctx.aborted) return null;
    lsp.flush(doc, ctx.state.doc.toString());
    const r = await c.client
      .request<LspCompletion[] | { items: LspCompletion[] } | null>("textDocument/completion", {
        textDocument: { uri: c.uri },
        position: toPosition(ctx.state.doc, ctx.pos),
        context: ctx.explicit
          ? { triggerKind: 1 }
          : { triggerKind: trigger ? 2 : 1, triggerCharacter: before.slice(-1) },
      })
      .catch(() => null);
    if (!r || ctx.aborted) return null;
    const items = Array.isArray(r) ? r : r.items;
    const from = word ? word.from : ctx.pos;
    const options: Completion[] = items.slice(0, 300).map((it, i) => {
      const edit = it.textEdit;
      const raw = edit?.newText ?? it.insertText ?? it.label;
      const text = it.insertTextFormat === 2 ? plainSnippet(raw) : raw;
      const range = edit?.range ?? edit?.replace;
      const doc = markupText(it.documentation);
      return {
        label: it.label,
        detail: it.detail,
        type: completionType(it.kind),
        // The server's order (its sortText) first; CodeMirror's own match score breaks ties.
        boost: Math.max(-99, 99 - i),
        info: doc
          ? () => {
              const dom = document.createElement("div");
              dom.className = "qx-lsp-hover";
              dom.innerHTML = renderMarkdown(doc);
              return dom;
            }
          : undefined,
        apply: (view, _c, f, t) => {
          const start = range ? toOffset(view.state.doc, range.start) : f;
          const end = range ? Math.max(t, toOffset(view.state.doc, range.end)) : t;
          view.dispatch({
            changes: { from: start, to: end, insert: text },
            selection: { anchor: start + text.length },
          });
        },
      };
    });
    return { from, options, validFor: /^[\w$]*$/ };
  };
  return EditorState.languageData.of(() => [{ autocomplete: source }]);
}

/** Hover and completion from the file's language server. */
export function lspExtensions(current: Current): Extension {
  return [hover(current), completion(current)];
}

/** The server's problems for a file, as CodeMirror's lint shows them. */
export function toDiagnostics(state: EditorState, problems: Problem[]): Diagnostic[] {
  return problems.map((p) => {
    const from = toOffset(state.doc, p.range.start);
    let to = toOffset(state.doc, p.range.end);
    // An empty range still needs a visible mark: the word (or character) at that point.
    if (to <= from) to = Math.min(state.doc.length, from + 1);
    return { from, to, severity: p.severity, message: p.message, source: p.source || undefined };
  });
}

interface Location {
  uri?: string;
  range?: Range;
  targetUri?: string;
  targetSelectionRange?: Range;
  targetRange?: Range;
}

function places(doc: Doc, root: string, result: unknown): Place[] {
  const list = (Array.isArray(result) ? result : result ? [result] : []) as Location[];
  const out: Place[] = [];
  for (const l of list) {
    const uri = l.targetUri ?? l.uri;
    const range = l.targetSelectionRange ?? l.targetRange ?? l.range;
    if (!uri || !range) continue;
    const path = relativePath(root, uri);
    // Definitions outside the project (a library, the language's own stubs) cannot open as a project file.
    if (path === null) continue;
    out.push({ projectId: doc.projectId, path, line: range.start.line + 1, column: range.start.character + 1 });
  }
  return out;
}

export interface Lookup {
  places: Place[];
  /** Why nothing was found, in words (no server, nothing there). */
  note: string;
}

async function locate(
  view: EditorView,
  doc: Doc,
  root: string,
  method: string,
  capability: "definitionProvider" | "referencesProvider",
  extra: object = {},
): Promise<Lookup | null> {
  const c = await ask(view, doc);
  if (!c?.client.capabilities[capability]) return null;
  const position: Position = atPos(view, view.state.selection.main.head);
  const r = await c.client.request(method, { textDocument: { uri: c.uri }, position, ...extra });
  const found = places(doc, root, r);
  const note = found.length ? "" : Array.isArray(r) && r.length ? "Only found outside this project." : "Nothing found.";
  return { places: found, note };
}

/** Go to definition through the server; null when it has none (the code index is used then). */
export function definition(view: EditorView, doc: Doc, root: string) {
  return locate(view, doc, root, "textDocument/definition", "definitionProvider");
}

export function references(view: EditorView, doc: Doc, root: string) {
  return locate(view, doc, root, "textDocument/references", "referencesProvider", {
    context: { includeDeclaration: true },
  });
}

/** The edits a rename makes, by file address; null when the server cannot rename. */
export async function renameEdits(view: EditorView, doc: Doc, pos: number, name: string): Promise<unknown> {
  const c = await ask(view, doc);
  if (!c?.client.capabilities.renameProvider) return null;
  return c.client.request("textDocument/rename", {
    textDocument: { uri: c.uri },
    position: atPos(view, pos),
    newName: name,
  });
}
