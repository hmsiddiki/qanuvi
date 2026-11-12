/**
 * Language Server Protocol helpers (15i) that need no running server: which server serves a file, file addresses
 * (URIs) and positions. Kept apart from the client so they can be tested on their own.
 */

/** The servers Qanuvi knows (src-tauri/src/lsp.rs has the same ids). */
export type ServerId = "php" | "typescript" | "svelte" | "rust" | "python";

const BY_EXTENSION: Record<string, ServerId> = {
  php: "php",
  phtml: "php",
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "typescript",
  jsx: "typescript",
  mjs: "typescript",
  cjs: "typescript",
  svelte: "svelte",
  rs: "rust",
  py: "python",
  pyi: "python",
};

/** The language id the protocol expects for a file (textDocument/didOpen). */
const LANGUAGE_ID: Record<string, string> = {
  php: "php",
  phtml: "php",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "typescriptreact",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascriptreact",
  svelte: "svelte",
  rs: "rust",
  py: "python",
  pyi: "python",
};

function extension(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

export function serverFor(path: string): ServerId | null {
  return BY_EXTENSION[extension(path)] ?? null;
}

export function languageId(path: string): string {
  return LANGUAGE_ID[extension(path)] ?? "plaintext";
}

/** `E:\Projects\x` + `src/a.php` → `file:///E:/Projects/x/src/a.php` (each part percent-encoded). */
export function fileUri(root: string, relative = ""): string {
  const parts = [...root.replace(/\\/g, "/").split("/"), ...relative.split("/")].filter((p) => p !== "");
  const windows = /^[A-Za-z]:$/.test(parts[0] ?? "");
  const enc = parts.map((p, i) => (i === 0 && windows ? p : encodeURIComponent(p)));
  return `file:///${enc.join("/")}`.replace(/^file:\/\/\/\//, "file:///");
}

/** Comparable form of a file address: decoded, forward slashes, and on Windows-style paths the case ignored. */
export function uriKey(uri: string): string {
  let p = uri.replace(/^file:\/\//i, "");
  try {
    p = decodeURIComponent(p);
  } catch {
    // Keep a badly encoded address as it is.
  }
  p = p.replace(/\\/g, "/").replace(/^\/+/, "/");
  return /^\/[A-Za-z]:/.test(p) ? p.toLowerCase() : p;
}

/** The project-relative path of an address, or null when it is outside the project. */
export function relativePath(root: string, uri: string): string | null {
  const base = uriKey(fileUri(root));
  const key = uriKey(uri);
  const prefix = base.endsWith("/") ? base : `${base}/`;
  if (!key.startsWith(prefix)) return null;
  // Take the original spelling of the rest (the key may be lower-cased).
  let full = uri.replace(/^file:\/\//i, "");
  try {
    full = decodeURIComponent(full);
  } catch {
    // As above.
  }
  full = full.replace(/\\/g, "/").replace(/^\/+/, "/");
  return full.slice(prefix.length);
}

/** A protocol position: line from 0, character in UTF-16 units (the same units CodeMirror counts in). */
export interface Position {
  line: number;
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

/** The lines of a text, as CodeMirror's Text gives them; kept minimal so tests can pass a plain string. */
export interface Lines {
  lines: number;
  line(n: number): { from: number; to: number };
  lineAt(pos: number): { number: number; from: number };
  length: number;
}

export function toPosition(doc: Lines, offset: number): Position {
  const l = doc.lineAt(Math.max(0, Math.min(offset, doc.length)));
  return { line: l.number - 1, character: offset - l.from };
}

export function toOffset(doc: Lines, p: Position): number {
  if (p.line >= doc.lines) return doc.length;
  const l = doc.line(Math.max(0, p.line) + 1);
  return Math.min(l.to, l.from + Math.max(0, p.character));
}

/** A simple line index over a string, matching the Lines shape (for tests and files that are not open). */
export function linesOf(text: string): Lines {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") starts.push(i + 1);
  return {
    lines: starts.length,
    length: text.length,
    line(n: number) {
      const from = starts[n - 1];
      const next = starts[n];
      return { from, to: next === undefined ? text.length : next - 1 };
    },
    lineAt(pos: number) {
      let lo = 0;
      let hi = starts.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (starts[mid] <= pos) lo = mid;
        else hi = mid - 1;
      }
      return { number: lo + 1, from: starts[lo] };
    },
  };
}

export interface TextEdit {
  range: Range;
  newText: string;
}

/** Applies edits to a text (from the end, so earlier offsets stay right). */
export function applyEdits(text: string, edits: TextEdit[]): string {
  const doc = linesOf(text);
  const list = edits
    .map((e) => ({ from: toOffset(doc, e.range.start), to: toOffset(doc, e.range.end), insert: e.newText }))
    .sort((a, b) => b.from - a.from || b.to - a.to);
  let out = text;
  for (const e of list) out = out.slice(0, e.from) + e.insert + out.slice(e.to);
  return out;
}

/** The edits of a WorkspaceEdit by file address (both forms servers send). */
export function editsByFile(edit: unknown): Map<string, TextEdit[]> {
  const out = new Map<string, TextEdit[]>();
  const e = (edit ?? {}) as {
    changes?: Record<string, TextEdit[]>;
    documentChanges?: { textDocument?: { uri: string }; edits?: TextEdit[] }[];
  };
  for (const [uri, list] of Object.entries(e.changes ?? {})) out.set(uri, [...(out.get(uri) ?? []), ...list]);
  for (const d of e.documentChanges ?? []) {
    if (!d.textDocument || !Array.isArray(d.edits)) continue;
    const uri = d.textDocument.uri;
    // Snippet edits carry { snippet } instead of newText; take the plain text.
    const list = d.edits.map((x) => ({ range: x.range, newText: x.newText ?? "" }));
    out.set(uri, [...(out.get(uri) ?? []), ...list]);
  }
  return out;
}

/** Hover and documentation contents → plain markdown text. */
export function markupText(contents: unknown): string {
  if (contents == null) return "";
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) return contents.map(markupText).filter(Boolean).join("\n\n");
  const c = contents as { kind?: string; value?: string; language?: string };
  if (typeof c.value !== "string") return "";
  if (c.language) return "```" + c.language + "\n" + c.value + "\n```";
  return c.value;
}

/**
 * An LSP snippet ($1, ${2:name}, ${3|a,b|}, $0, \$ escapes) as a CodeMirror snippet template, so Tab walks the
 * arguments and the cursor ends where the server meant. Placeholders inside placeholders keep only their words;
 * variables (${TM_FILENAME}) keep their default, or their name.
 */
export function snippetTemplate(text: string): string {
  let out = "";
  // A literal "{" right after "$" or "#" would start a CodeMirror field; escape it.
  const literal = (ch: string) => (out += ch === "{" && /[$#]$/.test(out) ? "\\{" : ch);
  // A placeholder's words from `from` to its closing brace, nested placeholders flattened: [words, index after].
  const inner = (from: number): [string, number] => {
    let words = "";
    let depth = 1;
    let j = from;
    while (j < text.length && depth > 0) {
      const rest = text.slice(j);
      const open = /^\$\{(\d+|[A-Za-z_]\w*):?/.exec(rest) ?? /^\$\d+/.exec(rest);
      if (rest[0] === "\\" && rest.length > 1) {
        words += rest[1];
        j += 2;
      } else if (open) {
        if (open[0].startsWith("${")) depth++;
        j += open[0].length;
      } else {
        if (rest[0] === "}") depth--;
        else words += rest[0];
        j++;
      }
    }
    return [words.replace(/[{}]/g, ""), j];
  };
  let i = 0;
  while (i < text.length) {
    const rest = text.slice(i);
    let m: RegExpExecArray | null;
    if (rest[0] === "\\" && rest.length > 1 && "$}\\,|".includes(rest[1])) {
      literal(rest[1]);
      i += 2;
    } else if ((m = /^\$(\d+)/.exec(rest) ?? /^\$\{(\d+)\}/.exec(rest))) {
      out += "${" + m[1] + "}";
      i += m[0].length;
    } else if ((m = /^\$\{(\d+)\|([^,|]*)[^}]*\}/.exec(rest))) {
      out += "${" + m[1] + ":" + m[2].replace(/[{}]/g, "") + "}";
      i += m[0].length;
    } else if ((m = /^\$\{(\d+):/.exec(rest))) {
      const [words, end] = inner(i + m[0].length);
      out += "${" + m[1] + ":" + words + "}";
      i = end;
    } else if ((m = /^\$\{([A-Za-z_]\w*)\}/.exec(rest))) {
      for (const c of m[1]) literal(c);
      i += m[0].length;
    } else if ((m = /^\$\{([A-Za-z_]\w*):/.exec(rest))) {
      const [words, end] = inner(i + m[0].length);
      for (const c of words) literal(c);
      i = end;
    } else {
      literal(rest[0]);
      i++;
    }
  }
  return out;
}

export type Severity = "error" | "warning" | "info" | "hint";

export function severity(n: number | undefined): Severity {
  return n === 1 || n === undefined ? "error" : n === 2 ? "warning" : n === 3 ? "info" : "hint";
}

/** The protocol's CompletionItemKind numbers → CodeMirror's completion types (for the icons). */
export function completionType(kind: number | undefined): string {
  switch (kind) {
    case 2:
    case 3:
    case 4:
      return "function";
    case 5:
    case 10:
      return "property";
    case 6:
      return "variable";
    case 7:
    case 22:
      return "class";
    case 8:
      return "interface";
    case 9:
      return "namespace";
    case 13:
    case 20:
      return "enum";
    case 14:
      return "keyword";
    case 21:
      return "constant";
    case 25:
      return "type";
    default:
      return "text";
  }
}
