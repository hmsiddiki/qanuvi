/**
 * Plan housekeeping (24j): move finished sections to an archive file next to the plan, and add another plan's steps
 * into this one. Both are pure: they return the new texts and what changed, so the Plan view can show a preview and
 * write only after the user confirms.
 */
import { parsePlan, splitLines, type Section, type Step } from "./tasklist";

/** The archive next to a plan: "docs/PLAN.md" becomes "docs/PLAN.archive.md". */
export function archivePath(planPath: string): string {
  return /\.md$/i.test(planPath) ? planPath.replace(/\.md$/i, ".archive.md") : `${planPath}.archive.md`;
}

/** Whether a file is a plan archive (never offered as a task list itself). */
export function isArchive(path: string): boolean {
  return /\.archive\.md$/i.test(path);
}

/** Step numbers marked done in a plan's text (its archive included, when given). */
export function doneNumbers(...texts: (string | null | undefined)[]): Set<string> {
  const out = new Set<string>();
  for (const t of texts) if (t) for (const s of parsePlan(t).steps) if (s.status === "x" && s.number) out.add(s.number);
  return out;
}

export interface ArchivedSection {
  title: string;
  steps: number;
}

export interface ArchiveResult {
  plan: string;
  archive: string;
  sections: ArchivedSection[];
}

/** Sections under a heading whose steps are all done. */
export function finishedSections(text: string): Section[] {
  return parsePlan(text).sections.filter(
    (s) => s.line >= 0 && s.steps.length > 0 && s.steps.every((x) => x.status === "x"),
  );
}

/**
 * Moves every finished section (heading, steps and any text under it, up to the next heading) to the end of the
 * archive. `archiveText` is the archive as it is now, or null when there is none yet. Returns null when nothing is
 * finished.
 */
export function archiveFinished(text: string, archiveText: string | null, planPath: string): ArchiveResult | null {
  const { lines, eol } = splitLines(text);
  const allHeadings = lines.map((l, i) => (/^#{1,6}\s/.test(l) ? i : -1)).filter((i) => i >= 0);
  const finished = finishedSections(text);
  if (!finished.length) return null;

  const blocks: { from: number; to: number }[] = finished.map((s) => {
    const next = allHeadings.find((h) => h > s.line);
    return { from: s.line, to: next ?? lines.length };
  });

  const moved: string[][] = blocks.map((b) => {
    const block = lines.slice(b.from, b.to);
    while (block.length && !block[block.length - 1].trim()) block.pop();
    return block;
  });

  const kept = [...lines];
  for (const b of [...blocks].reverse()) kept.splice(b.from, b.to - b.from);
  // The plan's end stays as it was: one final newline if it had one.
  while (kept.length > 1 && !kept[kept.length - 1].trim() && !kept[kept.length - 2].trim()) kept.pop();

  const base = archiveText ?? "";
  const aeol = archiveText ? splitLines(archiveText).eol : eol;
  const head = archiveText
    ? base.replace(/\s+$/, "")
    : [
        `# Archive of ${planPath.split("/").pop()}`,
        "",
        `Finished sections moved here from ${planPath}. Plan runs still count their steps as done.`,
      ].join(aeol);
  const archive = [head, ...moved.map((m) => m.join(aeol))].join(aeol + aeol) + aeol;

  return {
    plan: kept.join(eol),
    archive,
    sections: finished.map((s) => ({ title: s.title, steps: s.steps.length })),
  };
}

export interface MergeResult {
  text: string;
  /** Steps added. */
  added: number;
  /** Done steps left out (only with `onlyOpen`). */
  leftOut: number;
  /** Numbers that were taken in this plan and got a new one: old to new. */
  renumbered: [string, string][];
  /** Section titles added, with their step counts. */
  sections: ArchivedSection[];
}

const NUM_RE = /^(\d+)(.*)$/;
const TITLE_NUM_RE = /^( ?- \[[ xX!-]\] \*\*)(\d+[a-z]?(?:-\d+)?)(\.\s)/;
const NEEDS_LINE_RE = /^(\s*- Needs:\s*)(.+)$/i;

/**
 * Adds the steps of another plan (`source`, at `sourcePath`) to the end of this one, under their own headings.
 * Numbers this plan already uses get the next free ones (a step "45e" keeps its letter: "112e"), and the added
 * steps' "- Needs:" lines follow: a need on another added step takes its new number, a need on a step left out
 * points to the source plan ("PLAN.md#4"), and a need on this plan ("THIS.md#4") becomes plain "4". `taken`: numbers
 * used elsewhere that must not be reused (the plan's archive). The source file itself is not changed.
 */
export function mergePlans(
  target: string,
  targetPath: string,
  source: string,
  sourcePath: string,
  options: { onlyOpen?: boolean; taken?: Iterable<string> } = {},
): MergeResult | null {
  const tPlan = parsePlan(target);
  const { lines: tLines, eol } = splitLines(target);
  const sPlan = parsePlan(source);
  const { lines: sLines } = splitLines(source);

  const base = (n: string | null) => (n ? parseInt(n, 10) : NaN);
  // Numbers in use: this plan's, and those given as taken (its archive's).
  const tBases = new Set(
    [...tPlan.steps.map((s) => s.number), ...(options.taken ?? [])].map(base).filter((n) => !Number.isNaN(n)),
  );
  const sBases = [...new Set(sPlan.steps.map((s) => base(s.number)).filter((n) => !Number.isNaN(n)))].sort(
    (a, b) => a - b,
  );
  let free = Math.max(0, ...tBases, ...sBases) + 1;
  const baseMap = new Map<number, number>();
  for (const b of sBases) baseMap.set(b, tBases.has(b) ? free++ : b);
  const renumber = (n: string) =>
    n.replace(/\d+/g, (d) => {
      const m = baseMap.get(parseInt(d, 10));
      return m === undefined ? d : String(m);
    });

  const keep = (s: Step) => !options.onlyOpen || s.status !== "x";
  const kept = new Set(
    sPlan.steps
      .filter(keep)
      .map((s) => s.number)
      .filter(Boolean) as string[],
  );
  const renumbered: [string, string][] = [];
  for (const s of sPlan.steps)
    if (s.number && keep(s) && renumber(s.number) !== s.number) renumbered.push([s.number, renumber(s.number)]);

  const sameFile = (f: string) => f.replace(/^\.\//, "") === targetPath.replace(/^\.\//, "");
  const mapNeed = (need: string): string => {
    const hash = need.lastIndexOf("#");
    if (hash > 0) return sameFile(need.slice(0, hash)) ? need.slice(hash + 1) : need;
    if (!NUM_RE.test(need)) return need;
    return kept.has(need) ? renumber(need) : `${sourcePath}#${need}`;
  };

  const out: string[] = [];
  const sections: ArchivedSection[] = [];
  let added = 0;
  for (const sec of sPlan.sections) {
    const steps = sec.steps.filter(keep);
    if (!steps.length) continue;
    const heading = sec.line >= 0 ? sLines[sec.line] : `## From ${sourcePath}`;
    const block = [heading, ""];
    for (const st of steps) {
      for (let i = st.line; i < st.end; i++) {
        let line = sLines[i];
        if (i === st.line) line = line.replace(TITLE_NUM_RE, (_, a, n, c) => `${a}${renumber(n)}${c}`);
        else {
          const nm = NEEDS_LINE_RE.exec(line);
          if (nm) {
            const needs = nm[2]
              .split(/[,\s]+/)
              .map((x) => x.trim())
              .filter(Boolean);
            line = `${nm[1]}${needs.map(mapNeed).join(", ")}`;
          }
        }
        block.push(line);
      }
    }
    while (block.length && !block[block.length - 1].trim()) block.pop();
    out.push(block.join(eol));
    sections.push({ title: sec.title || `From ${sourcePath}`, steps: steps.length });
    added += steps.length;
  }
  if (!added) return null;

  const body = [...tLines];
  const hadFinalNewline = body.length > 1 && body[body.length - 1] === "";
  while (body.length && !body[body.length - 1].trim()) body.pop();
  const text = [body.join(eol), ...out].filter(Boolean).join(eol + eol) + (hadFinalNewline || !body.length ? eol : "");

  return {
    text,
    added,
    leftOut: sPlan.steps.length - sPlan.steps.filter(keep).length,
    renumbered,
    sections,
  };
}

/** Every step number in a plan's text, done or not. */
export function stepNumbers(text: string | null | undefined): string[] {
  return text
    ? (parsePlan(text)
        .steps.map((s) => s.number)
        .filter(Boolean) as string[])
    : [];
}
