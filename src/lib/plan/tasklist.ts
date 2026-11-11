/**
 * Reads and edits Markdown task lists such as DEVELOPMENT-TASKLIST.md:
 *
 *   ## Section
 *   - [ ] **12. Step title** (a note that may continue
 *     on the next line)
 *     - detail
 *     - Model: claude-opus-5-5
 *
 * Edits change only the lines they must; everything else in the file stays exactly as it was, line endings included.
 */

export type StepStatus = " " | "-" | "x" | "!";

export const STATUS_ORDER: StepStatus[] = [" ", "-", "x", "!"];

export const STATUS_LABELS: Record<StepStatus, string> = {
  " ": "Not started",
  "-": "In progress",
  x: "Done",
  "!": "Blocked",
};

export interface Step {
  /** Index of the step's line in the file. */
  line: number;
  status: StepStatus;
  /** For example "82" or "45e"; null when the step has no number. */
  number: string | null;
  title: string;
  /** Text after the title, including continuation lines, for example "(Plan D, step 1 of 8)". */
  note: string;
  /** Detail lines under the step, without their leading indentation. */
  details: string[];
  model: string | null;
  /** First line after the step and its details. */
  end: number;
  section: string;
}

export interface Section {
  title: string;
  level: number;
  line: number;
  steps: Step[];
}

export interface Plan {
  sections: Section[];
  steps: Step[];
  eol: "\n" | "\r\n";
}

const STEP_RE = /^( ?)- \[([ xX!-])\] (.*)$/;
const HEADING_RE = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const MODEL_RE = /^\s+- Model:\s*(\S+)\s*$/i;
const TITLE_RE = /^\*\*(?:(\d+[a-z]?)\.\s+)?(.+?)\*\*\s*(.*)$/;

export function splitLines(text: string): { lines: string[]; eol: "\n" | "\r\n" } {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  return { lines: text.split(/\r?\n/), eol };
}

const indentOf = (line: string) => line.length - line.trimStart().length;
const isListItem = (trimmed: string) => /^([-*+]|\d+\.)\s/.test(trimmed);

export function parsePlan(text: string): Plan {
  const { lines, eol } = splitLines(text);
  const sections: Section[] = [];
  const steps: Step[] = [];
  let section: Section = { title: "", level: 0, line: -1, steps: [] };
  sections.push(section);

  for (let i = 0; i < lines.length; i++) {
    const heading = HEADING_RE.exec(lines[i]);
    if (heading) {
      section = { title: heading[2], level: heading[1].length, line: i, steps: [] };
      sections.push(section);
      continue;
    }
    const m = STEP_RE.exec(lines[i]);
    if (!m) continue;

    const status = (m[2] === "X" ? "x" : m[2]) as StepStatus;
    const t = TITLE_RE.exec(m[3]);
    let number: string | null = null;
    let title = m[3].trim();
    let note = "";
    if (t) {
      number = t[1] ?? null;
      title = t[2].trim();
      note = t[3].trim();
    }

    // Continuation of the step's own line, then details, until an unindented line or a heading.
    let j = i + 1;
    while (j < lines.length && lines[j].trim() && indentOf(lines[j]) >= 2 && !isListItem(lines[j].trim())) {
      note = `${note} ${lines[j].trim()}`.trim();
      j++;
    }
    const details: string[] = [];
    let model: string | null = null;
    let end = j;
    for (; j < lines.length; j++) {
      const line = lines[j];
      if (!line.trim()) continue;
      if (indentOf(line) < 2 || HEADING_RE.test(line)) break;
      const mm = MODEL_RE.exec(line);
      if (mm && model === null) model = mm[1];
      else details.push(line.replace(/^ {2}/, ""));
      end = j + 1;
    }

    const step: Step = { line: i, status, number, title, note, details, model, end, section: section.title };
    steps.push(step);
    section.steps.push(step);
    i = end - 1;
  }

  return { sections: sections.filter((s) => s.steps.length > 0 || s.line >= 0), steps, eol };
}

function join(lines: string[], eol: string): string {
  return lines.join(eol);
}

/** The step at `line` in freshly parsed text, refusing if the file no longer has a step there. */
function stepAt(text: string, line: number, expectTitle?: string): { plan: Plan; step: Step; lines: string[] } {
  const plan = parsePlan(text);
  const step = plan.steps.find((s) => s.line === line);
  if (!step || (expectTitle !== undefined && step.title !== expectTitle)) {
    throw new Error("The task list changed since it was shown. It has been reloaded; try again.");
  }
  return { plan, step, lines: splitLines(text).lines };
}

export function setStepStatus(text: string, line: number, status: StepStatus, expectTitle?: string): string {
  const { plan, lines } = stepAt(text, line, expectTitle);
  lines[line] = lines[line].replace(/^( ?- \[)[ xX!-](\])/, `$1${status}$2`);
  return join(lines, plan.eol);
}

export function setStepTitle(text: string, line: number, title: string, expectTitle?: string): string {
  const clean = title.replace(/\s+/g, " ").replace(/\*\*/g, "").trim();
  if (!clean) throw new Error("A step needs a title.");
  const { plan, step, lines } = stepAt(text, line, expectTitle);
  const prefix = /^( ?- \[[ xX!-]\] )/.exec(lines[line])![1];
  const rest = lines[line].slice(prefix.length);
  if (TITLE_RE.test(rest)) {
    const num = step.number ? `${step.number}. ` : "";
    const after = TITLE_RE.exec(rest)![3];
    lines[line] = `${prefix}**${num}${clean}**${after ? ` ${after}` : ""}`;
  } else {
    lines[line] = `${prefix}${clean}`;
  }
  return join(lines, plan.eol);
}

/** Sets, changes or (with null) removes the step's "- Model:" line. */
export function setStepModel(text: string, line: number, model: string | null, expectTitle?: string): string {
  const value = model?.trim() || null;
  if (value && /\s/.test(value)) throw new Error("A model name has no spaces.");
  const { plan, step, lines } = stepAt(text, line, expectTitle);
  const existing = lines.findIndex((l, i) => i > line && i < step.end && MODEL_RE.test(l));
  if (existing >= 0) {
    if (value) lines[existing] = lines[existing].replace(MODEL_RE, `  - Model: ${value}`);
    else lines.splice(existing, 1);
  } else if (value) {
    let at = line + 1;
    while (at < lines.length && lines[at].trim() && indentOf(lines[at]) >= 2 && !isListItem(lines[at].trim())) at++;
    lines.splice(at, 0, `  - Model: ${value}`);
  }
  return join(lines, plan.eol);
}

export function nextStatus(status: StepStatus): StepStatus {
  return STATUS_ORDER[(STATUS_ORDER.indexOf(status) + 1) % STATUS_ORDER.length];
}

/** Files in a project that look like task lists, the most likely first. */
export function findTaskLists(paths: string[]): string[] {
  const re = /(^|\/)[^/]*(task[-_ ]?list|todo|plan|roadmap)[^/]*\.md$/i;
  return paths
    .filter((p) => re.test(p) && !p.includes("node_modules/"))
    .sort((a, b) => a.split("/").length - b.split("/").length || a.localeCompare(b));
}
