/**
 * Changes between two versions of a file, line by line (Milestone 15b): for staging, unstaging or undoing one change
 * at a time, the editor's gutter markers and merge conflicts. Plain functions; the diff compares whole lines, as git does.
 */
/** One change: character ranges in the old text (a) and the new text (b), whole lines. */
export interface Hunk {
  fromA: number;
  toA: number;
  fromB: number;
  toB: number;
  /** First line in b (1-based; for a removal, the line after it) and how many lines it has in a and b. */
  line: number;
  linesA: number;
  linesB: number;
}

/** The lines of a text, each with its own line break (the last may have none). */
function splitKeep(s: string): string[] {
  return s.match(/[^\n]*\n|[^\n]+$/g) ?? [];
}

/** A line with its break written as LF, so CRLF and LF lines compare equal (as in VS Code). */
const sameEnding = (l: string) => (l.endsWith("\r\n") ? l.slice(0, -2) + "\n" : l);

/** Bigger differences than this become one change (keeps the work bounded on huge rewrites). */
const MAX_EDITS = 4000;

/** Pairs of equal lines (index in a, index in b), in order: Myers' shortest edit script. */
function equalLines(a: string[], b: string[]): [number, number][] {
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }
  const pairs: [number, number][] = [];
  for (let i = 0; i < start; i++) pairs.push([i, i]);
  const n = endA - start;
  const m = endB - start;
  const middle: [number, number][] = [];
  if (n > 0 && m > 0) {
    const max = Math.min(n + m, MAX_EDITS);
    const off = max + 1;
    const v = new Int32Array(2 * max + 3);
    const trace: Int32Array[] = [];
    let found = false;
    for (let d = 0; d <= max && !found; d++) {
      trace.push(v.slice());
      for (let k = -d; k <= d; k += 2) {
        let x = k === -d || (k !== d && v[k - 1 + off] < v[k + 1 + off]) ? v[k + 1 + off] : v[k - 1 + off] + 1;
        let y = x - k;
        while (x < n && y < m && a[start + x] === b[start + y]) {
          x++;
          y++;
        }
        v[k + off] = x;
        if (x >= n && y >= m) {
          found = true;
          break;
        }
      }
    }
    if (found) {
      let x = n;
      let y = m;
      for (let d = trace.length - 1; d >= 0 && (x > 0 || y > 0); d--) {
        const t = trace[d];
        const k = x - y;
        const prevK = k === -d || (k !== d && t[k - 1 + off] < t[k + 1 + off]) ? k + 1 : k - 1;
        const prevX = t[prevK + off];
        const prevY = prevX - prevK;
        while (x > prevX && y > prevY) {
          x--;
          y--;
          middle.push([start + x, start + y]);
        }
        x = prevX;
        y = prevY;
      }
      middle.reverse();
    }
  }
  pairs.push(...middle);
  for (let i = 0; endA + i < a.length; i++) pairs.push([endA + i, endB + i]);
  return pairs;
}

export function hunks(a: string, b: string): Hunk[] {
  if (a === b) return [];
  const la = splitKeep(a);
  const lb = splitKeep(b);
  const posA = [0];
  for (const l of la) posA.push(posA[posA.length - 1] + l.length);
  const posB = [0];
  for (const l of lb) posB.push(posB[posB.length - 1] + l.length);
  const out: Hunk[] = [];
  let pa = 0;
  let pb = 0;
  for (const [ea, eb] of [
    ...equalLines(la.map(sameEnding), lb.map(sameEnding)),
    [la.length, lb.length] as [number, number],
  ]) {
    if (ea > pa || eb > pb) {
      out.push({
        fromA: posA[pa],
        toA: posA[ea],
        fromB: posB[pb],
        toB: posB[eb],
        line: pb + 1,
        linesA: ea - pa,
        linesB: eb - pb,
      });
    }
    pa = ea + 1;
    pb = eb + 1;
  }
  return out;
}

/** a with one change taken over from b (stage a change: a = staged text, b = the file). */
export function takeHunk(a: string, b: string, h: Hunk): string {
  return a.slice(0, h.fromA) + b.slice(h.fromB, h.toB) + a.slice(h.toA);
}

/** b with one change given back from a (undo a change in the file, or unstage: a = last commit, b = staged). */
export function giveBack(a: string, b: string, h: Hunk): string {
  return b.slice(0, h.fromB) + a.slice(h.fromA, h.toA) + b.slice(h.toB);
}

export type Mark = "added" | "changed" | "removed";

/** Gutter marks per line (1-based) of the new text, as in VS Code. A removal is marked on the line after it. */
export function lineMarks(base: string, doc: string): Map<number, Mark> {
  const marks = new Map<number, Mark>();
  for (const h of hunks(base, doc)) {
    if (h.linesB === 0) marks.set(Math.max(1, h.line), "removed");
    else {
      const kind: Mark = h.linesA === 0 ? "added" : "changed";
      for (let i = 0; i < h.linesB; i++) marks.set(h.line + i, kind);
    }
  }
  return marks;
}

/** A merge conflict block in a file: <<<<<<< current ======= incoming >>>>>>>. */
export interface Conflict {
  from: number;
  to: number;
  /** 1-based line of the <<<<<<< marker. */
  line: number;
  current: string;
  incoming: string;
  currentLabel: string;
  incomingLabel: string;
}

export function conflicts(text: string): Conflict[] {
  const out: Conflict[] = [];
  const re = /^<{7}(.*)\r?\n([\s\S]*?)^(?:\|{7}.*\r?\n[\s\S]*?)?={7}\r?\n([\s\S]*?)^>{7}(.*)(?:\r?\n|$)/gm;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    out.push({
      from: m.index,
      to: m.index + m[0].length,
      line: text.slice(0, m.index).split("\n").length,
      current: m[2],
      incoming: m[3],
      currentLabel: m[1].trim(),
      incomingLabel: m[4].trim(),
    });
  }
  return out;
}

export type Resolution = "current" | "incoming" | "both";

export function resolve(text: string, c: Conflict, how: Resolution): string {
  const keep = how === "current" ? c.current : how === "incoming" ? c.incoming : c.current + c.incoming;
  return text.slice(0, c.from) + keep + text.slice(c.to);
}
