/**
 * Work packets (Milestone 12): what Qanuvi hands an agent with a step so it does not have to search for it, and what it
 * sends back after running the tests itself. Plain functions; the run uses them in stores/runner.svelte.ts.
 */

export interface Mentions {
  /** File paths written in the step, for example `src/price.js`. */
  paths: string[];
  /** Names that look like code: `backticked`, camelCase, PascalCase or snake_case words. */
  names: string[];
}

const PATH_RE =
  /(?:^|[\s`'"(])((?:[\w.-]+\/)*[\w.-]+\.(?:php|phtml|js|mjs|cjs|jsx|ts|tsx|svelte|vue|rs|py|dart|cs|go|java|kt|json|ya?ml|toml|css|scss|html|md|sql))(?=$|[\s`'"),.:;])/g;
const TICK_RE = /`([^`\n]{2,80})`/g;
const WORD_RE = /\b([A-Za-z_][A-Za-z0-9_]{3,})\b/g;
const STOP = new Set([
  "Model",
  "Validate",
  "Done",
  "This",
  "Then",
  "When",
  "With",
  "From",
  "Into",
  "JSON",
  "HTML",
  "TODO",
  "README",
]);

const looksLikeCode = (w: string) => /[a-z][A-Z]/.test(w) || /_/.test(w) || /^[A-Z][a-z]+[A-Z]/.test(w);

/** The files and code names a step's text mentions, in the order they appear, without repeats. */
export function mentions(text: string): Mentions {
  const paths: string[] = [];
  const names: string[] = [];
  const addName = (n: string) => {
    const name = n.replace(/\(\)$/, "").replace(/^[$@#]/, "");
    if (!/^[A-Za-z_$][\w$]*$/.test(name) || name.length < 3 || STOP.has(name)) return;
    if (!names.includes(name)) names.push(name);
  };
  for (const m of text.matchAll(PATH_RE)) {
    const p = m[1].replace(/^\.\//, "");
    if (!paths.includes(p)) paths.push(p);
  }
  for (const m of text.matchAll(TICK_RE)) {
    const inner = m[1].trim();
    if (paths.some((p) => inner.endsWith(p))) continue;
    // A call or a dotted name: take the last name before the brackets.
    const call = /([A-Za-z_$][\w$]*)\s*\(/.exec(inner);
    addName(call ? call[1] : (inner.split(/[.:]/).pop() ?? inner));
  }
  for (const m of text.matchAll(WORD_RE)) if (looksLikeCode(m[1])) addName(m[1]);
  return { paths, names };
}

export interface Excerpt {
  path: string;
  /** First line of the excerpt (1-based). */
  start: number;
  text: string;
  /** Why it is here, for example "definition of total". */
  reason: string;
}

/** Lines of a file around a definition: a few before, the definition, and the body up to `after` lines. */
export function excerptAround(fileText: string, line: number, before = 2, after = 30): { start: number; text: string } {
  const lines = fileText.split(/\r?\n/);
  const start = Math.max(1, line - before);
  const end = Math.min(lines.length, line + after);
  return { start, text: lines.slice(start - 1, end).join("\n") };
}

/** The code part of a packet, cut to `maxChars` (whole excerpts only, the first ones kept). */
export function contextBlock(excerpts: Excerpt[], maxChars = 6000): string {
  const parts: string[] = [];
  let used = 0;
  for (const e of excerpts) {
    const numbered = e.text
      .split("\n")
      .map((l, i) => `${String(e.start + i).padStart(4)}  ${l}`)
      .join("\n");
    const part = `${e.path} (${e.reason}):\n${numbered}`;
    if (used + part.length > maxChars && parts.length > 0) break;
    parts.push(part.length > maxChars ? part.slice(0, maxChars) : part);
    used += part.length;
  }
  return parts.join("\n\n");
}

const FAIL_RE =
  /\b(fail(ed|ure|ing)?|error|errors|assert\w*|expected|exception|panicked|not ok|traceback)\b|✗|✖|×|FAIL|ERR!/i;

/** The useful part of a failed test run: lines that report failures with a little context, then the last lines. */
/** Lines that never help: passing tests, zero counters, and stack frames inside the test runner or the language. */
const NOISE_RE =
  /^\s*(✔|✓|√|ok \d|PASS\b)|^\s*ℹ (cancelled|skipped|todo|duration_ms) |^\s*at .*(node:internal|node:async_hooks|internal\/|node_modules[\\/](jest|vitest|mocha))/;

export function failureSummary(output: string, maxChars = 3000): string {
  const lines = output
    .replace(/\x1b\[[0-9;]*m/g, "")
    .split(/\r?\n/)
    .filter((l) => !NOISE_RE.test(l));
  const keep = new Set<number>();
  lines.forEach((l, i) => {
    if (FAIL_RE.test(l)) for (let j = Math.max(0, i - 1); j <= Math.min(lines.length - 1, i + 3); j++) keep.add(j);
  });
  for (let j = Math.max(0, lines.length - 12); j < lines.length; j++) keep.add(j);
  const out: string[] = [];
  let last = -2;
  for (const i of [...keep].sort((a, b) => a - b)) {
    if (!lines[i].trim()) continue;
    if (i > last + 1 && out.length) out.push("…");
    out.push(lines[i]);
    last = i;
  }
  const text = out.join("\n");
  return text.length > maxChars ? `…\n${text.slice(text.length - maxChars)}` : text;
}

/** The step request with the packet added: the code it needs, and who runs the tests. */
export function packetPrompt(base: string, context: string, testCommand: string): string {
  const parts = [base];
  if (context) {
    parts.push(
      "",
      "Where the code this step mentions is, from Qanuvi's index (go straight there instead of searching):",
      context,
    );
  }
  if (testCommand) {
    parts.push(
      "",
      `Qanuvi runs \`${testCommand}\` itself when you finish and sends you only the failures. Do not run the whole test suite yourself; you may run a single test while working.`,
    );
  }
  return parts.join("\n");
}

/** The follow-up sent in the same session when the tests failed. */
export function fixPrompt(testCommand: string, summary: string, round: number, maxRounds: number): string {
  return [
    `\`${testCommand}\` failed (fix round ${round} of ${maxRounds}). The failures:`,
    "```",
    summary,
    "```",
    "Fix the cause, then mark the step done again only when it is complete.",
    'If a failure was not caused by this step, fix it only when the fix is small and clearly right; otherwise mark the step "- [!]" and say why.',
  ].join("\n");
}

export interface Location {
  name: string;
  kind: string;
  path: string;
  line: number;
  container: string | null;
}

/**
 * Where the code a step mentions is: one line per definition and a short outline per mentioned file. Measured on
 * 2026-11-10: pasting the code itself did not pay off with Claude Code, whose Edit tool needs the file read anyway; the
 * locations save searching for it at a few hundred tokens.
 */
export function locationBlock(
  defs: Location[],
  outlines: { path: string; items: Omit<Location, "path">[] }[],
  newFiles: string[],
  maxChars = 2500,
): string {
  const lines: string[] = [];
  for (const d of defs)
    lines.push(`${d.name}: ${d.kind}${d.container ? ` in ${d.container}` : ""} at ${d.path}:${d.line}`);
  for (const o of outlines) {
    lines.push(
      `${o.path}: ${o.items.map((i) => `${i.name} (${i.kind}, line ${i.line})`).join(", ") || "no definitions"}`,
    );
  }
  for (const f of newFiles) lines.push(`${f}: does not exist yet`);
  let text = "";
  for (const l of lines) {
    if (text.length + l.length + 1 > maxChars) break;
    text += (text ? "\n" : "") + l;
  }
  return text;
}
