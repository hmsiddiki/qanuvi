/**
 * Formatting and fixing helpers (15i part 2) that need no running server: the indent a file uses (sent with a
 * format request), the order quick fixes are offered in, which problems touch a range, and the request "Fix with
 * agent" puts in the message box.
 */
import { editsByFile, type Range } from "./protocol";

/** The indent of a text as the protocol's FormattingOptions want it. */
export function guessIndent(text: string): { tabSize: number; insertSpaces: boolean } {
  let tabs = 0;
  let spaced = 0;
  let by4 = 0;
  let by2 = 0;
  for (const line of text.split("\n", 5000)) {
    if (line.startsWith("\t")) tabs++;
    else {
      const n = /^ +/.exec(line)?.[0].length ?? 0;
      // A line of only spaces, or a block comment's " * ", says nothing about the indent.
      if (!n || n === line.length || line[n] === "*") continue;
      spaced++;
      if (n % 4 === 0) by4++;
      if (n % 2 === 0) by2++;
    }
  }
  if (tabs > spaced) return { tabSize: 4, insertSpaces: false };
  if (!spaced) return { tabSize: 4, insertSpaces: true };
  // Mostly multiples of 4 is a 4-space file; mostly even (and not 4s) a 2-space one.
  if (by4 >= spaced * 0.9) return { tabSize: 4, insertSpaces: true };
  if (by2 >= spaced * 0.9) return { tabSize: 2, insertSpaces: true };
  return { tabSize: 4, insertSpaces: true };
}

/** A code action or a bare command, as servers answer textDocument/codeAction. */
export interface CodeAction {
  title: string;
  kind?: string;
  isPreferred?: boolean;
  disabled?: { reason: string };
  edit?: unknown;
  /** A Command object on a CodeAction, or the command's id on a bare Command. */
  command?: { title?: string; command: string; arguments?: unknown[] } | string;
  arguments?: unknown[];
  data?: unknown;
  diagnostics?: unknown[];
}

const kindRank = (kind = "") =>
  kind.startsWith("quickfix") ? 0 : kind.startsWith("refactor") ? 1 : kind.startsWith("source") ? 2 : 1;

/** Quick fixes first (the server's preferred one on top), then refactors, then whole-file actions; disabled last. */
export function orderActions(list: CodeAction[]): CodeAction[] {
  return list
    .filter((a) => a && typeof a.title === "string" && a.title)
    .map((a, i) => ({ a, i }))
    .sort(
      (x, y) =>
        Number(!!x.a.disabled) - Number(!!y.a.disabled) ||
        Number(!x.a.isPreferred) - Number(!y.a.isPreferred) ||
        kindRank(x.a.kind) - kindRank(y.a.kind) ||
        x.i - y.i,
    )
    .map((x) => x.a);
}

/** The first line (from 0) an action is about: its problem, else its first edit; null when it says neither. */
function actionLine(a: CodeAction): number | null {
  const d = a.diagnostics?.[0] as { range?: Range } | undefined;
  if (d?.range) return d.range.start.line;
  const first = editsByFile(a.edit).values().next().value?.[0];
  return first ? first.range.start.line : null;
}

/**
 * The menu labels of actions: a title that several actions share gets its line, so the two "remove the whole `use`
 * item" fixes rust-analyzer offers for two imports read apart ("... (line 2)").
 */
export function actionLabels(actions: CodeAction[]): string[] {
  const count = new Map<string, number>();
  for (const a of actions) count.set(a.title, (count.get(a.title) ?? 0) + 1);
  return actions.map((a) => {
    const line = (count.get(a.title) ?? 0) > 1 ? actionLine(a) : null;
    const title = line === null ? a.title : `${a.title} (line ${line + 1})`;
    return a.disabled ? `${title} (${a.disabled.reason})` : title;
  });
}

/** Whether a problem's range meets the lines from `from` to `to` (protocol lines, from 0). */
export function touches(
  range: Range,
  from: { line: number; character: number },
  to: { line: number; character: number },
) {
  const before = (a: typeof from, b: typeof from) =>
    a.line < b.line || (a.line === b.line && a.character < b.character);
  return !before(range.end, from) && !before(to, range.start);
}

export interface FixProblem {
  path: string;
  range: Range;
  severity: string;
  message: string;
  source: string;
}

/**
 * The message "Fix with agent" prepares: where, what the server said, and the lines around it. The user reads it,
 * changes it if they like, and sends it; nothing is sent by itself.
 */
export function fixText(problems: FixProblem[], fileText: string | null, language = ""): string {
  if (!problems.length) return "";
  const path = problems[0].path;
  const lines = fileText === null ? null : fileText.split("\n");
  const said = (p: FixProblem) =>
    `- line ${p.range.start.line + 1}: ${p.message.trim().replace(/\s*\n\s*/g, " ")}${p.source ? ` (${p.source})` : ""}`;
  const one = problems.length === 1;
  const head = one
    ? `Fix this ${problems[0].severity} reported in @${path}:${problems[0].range.start.line + 1}`
    : `Fix these ${problems.length} problems reported in @${path}`;
  const out = [`${head}:`, ...problems.slice(0, 40).map(said)];
  if (problems.length > 40) out.push(`- and ${problems.length - 40} more`);
  if (lines && one) {
    const p = problems[0];
    const first = Math.max(0, p.range.start.line - 3);
    const last = Math.min(lines.length - 1, p.range.end.line + 3);
    const width = String(last + 1).length;
    const code = lines.slice(first, last + 1).map((l, i) => `${String(first + i + 1).padStart(width)}  ${l}`);
    out.push("", "```" + language, ...code, "```");
  }
  out.push(
    "",
    "Change only what the fix needs, keep the file's style, and say in a line what caused it. If it is not a real problem, say why instead of changing the code.",
  );
  return out.join("\n");
}
