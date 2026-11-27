/**
 * ST2 (held-step questions): a question that quotes a long shell command is heavy for a normal user. Quoted commands
 * (between backticks) over LONG characters show by their program ("npm run build") with the whole command behind
 * "Show command"; short ones and plain words stay as they are.
 */

export const LONG = 48;

export type Piece = { kind: "text"; text: string } | { kind: "command"; text: string; short: string };

/** The program a command runs, in a few words: `cd app && NODE_ENV=x npm run build -- --x` → "npm run build". */
export function shortCommand(command: string): string {
  const parts = command
    .split(/&&|\|\||;|\|/)
    .map((p) => p.trim())
    .filter((p) => p && !/^cd(\s|$)/.test(p));
  const first = parts[0] ?? command.trim();
  const words = first
    .split(/\s+/)
    .filter((w) => !/^[A-Za-z_][A-Za-z0-9_]*=/.test(w))
    .map((w) => w.replace(/^.*[\\/]/, ""));
  const out: string[] = [];
  for (const w of words) {
    if (out.length >= 3 || w.startsWith("-") || /["'`$<>]/.test(w)) break;
    out.push(w);
  }
  return out.join(" ") || "a command";
}

/** Splits text into plain words and long quoted commands. */
export function pieces(text: string): Piece[] {
  const out: Piece[] = [];
  let rest = text;
  const re = /`([^`\n]+)`/;
  for (;;) {
    const m = re.exec(rest);
    if (!m) break;
    const inner = m[1];
    if (inner.length > LONG) {
      if (m.index > 0) out.push({ kind: "text", text: rest.slice(0, m.index) });
      out.push({ kind: "command", text: inner, short: shortCommand(inner) });
    } else {
      out.push({ kind: "text", text: rest.slice(0, m.index + m[0].length) });
    }
    rest = rest.slice(m.index + m[0].length);
  }
  if (rest) out.push({ kind: "text", text: rest });
  // Neighbouring plain pieces as one.
  return out.reduce<Piece[]>((acc, p) => {
    const last = acc[acc.length - 1];
    if (p.kind === "text" && last?.kind === "text") last.text += p.text;
    else acc.push({ ...p });
    return acc;
  }, []);
}
