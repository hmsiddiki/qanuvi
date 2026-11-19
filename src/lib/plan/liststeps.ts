/**
 * f1: a plan written as a plain list ("1. Do this" or "- Do this") has no steps Qanuvi can run, since steps are
 * checkbox lines. This turns the list's top-level items into "- [ ] ..." steps: numbered items keep their numbers,
 * a bulleted list is used only when there are no numbered items. A wrapped line of an item with no indent gets two
 * spaces so it stays part of that step. Code blocks and headings are left as they are.
 */

const NUMBERED_RE = /^(\d+)[.)]\s+(\S.*)$/;
const BULLET_RE = /^[-*+]\s+(?!\[[ xX!-]\])(\S.*)$/;
const CHECKBOX_RE = /^\s*[-*+]\s+\[[ xX!-]\]\s/;
const FENCE_RE = /^\s*(```|~~~)/;
const HEADING_RE = /^#{1,6}\s/;

/** The plan text with its list turned into steps, and how many steps were made; null when there is nothing to do. */
export function listToSteps(text: string): { text: string; count: number } | null {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(/\r?\n/);
  let fenced = false;
  const outside = lines.map((line) => {
    if (FENCE_RE.test(line)) {
      fenced = !fenced;
      return false;
    }
    return !fenced;
  });
  if (lines.some((l, i) => outside[i] && CHECKBOX_RE.test(l))) return null;
  const numbered = lines.some((l, i) => outside[i] && NUMBERED_RE.test(l));
  const item = numbered ? NUMBERED_RE : BULLET_RE;
  let count = 0;
  let inItem = false;
  const out = lines.map((line, i) => {
    if (!outside[i] || !line.trim() || HEADING_RE.test(line)) {
      inItem = false;
      return line;
    }
    const m = item.exec(line);
    if (m) {
      count++;
      inItem = true;
      return numbered ? `- [ ] ${m[1]}. ${m[2]}` : `- [ ] ${m[1]}`;
    }
    if (inItem && !/^\s/.test(line) && !NUMBERED_RE.test(line) && !BULLET_RE.test(line)) return `  ${line}`;
    if (/^\s/.test(line)) return line;
    inItem = false;
    return line;
  });
  return count ? { text: out.join(eol), count } : null;
}
