/**
 * The finishing pass at the end of a plan run (24o). The steps followed their own instructions; one smarter pass then
 * looks at the whole. Light: the project's full checks and a review of the whole run's diff. Full: also a session
 * that fixes what was found, runs live tests where the project defines them (scratch copies only) and checks the task
 * list's Done notes against what was built. Both end with a short "still open" list. Plain functions only; the pass
 * itself runs in stores/runner.svelte.ts.
 */
import { jsonIn } from "$lib/agents/guides";
import type { PlanRun, StepRun } from "./runner";

export type FinishLevel = "light" | "full";
/** What a run does at its end: offer the pass, or run one by itself (a plan's run setting). */
export type FinishAuto = "ask" | FinishLevel;

export interface FinishPass {
  level: FinishLevel;
  status: "running" | "done" | "failed" | "stopped";
  /** The decider's model the pass ran on. */
  model: string;
  startedAt: number;
  endedAt: number | null;
  costUsd: number;
  seconds: number;
  /** The project's full checks, when it has a test command. */
  checks: { command: string; ok: boolean; summary: string } | null;
  /** What the review found (light), or what the full pass found and did. */
  findings: string[];
  /** Left for the owner. */
  stillOpen: string[];
  /** The full pass's session (to open it). */
  sessionId?: string;
  note?: string;
}

/** The key the pass's cost has in the run's lines per model. */
export const FINISHER = "finish:";

/** A finished step's cost and time in this run (all its attempts). */
function stepCost(s: StepRun): { cost: number; seconds: number } {
  return s.attempts.reduce((t, a) => ({ cost: t.cost + a.costUsd, seconds: t.seconds + a.seconds }), {
    cost: 0,
    seconds: 0,
  });
}

function median(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

/**
 * About what the pass costs and how long it takes, from the run's own finished steps: a light pass is about half a
 * typical step (one review of the whole diff, plus the checks), a full one about 1.5 typical steps, more for long runs.
 * Cost is null when no finished step was paid for (free steps tell nothing about the paid decider's price).
 */
export function finishEstimate(
  run: Pick<PlanRun, "steps">,
  level: FinishLevel,
): { costUsd: number | null; seconds: number | null } {
  const done = run.steps.filter((s) => s.status === "done" && s.attempts.length).map(stepCost);
  if (!done.length) return { costUsd: null, seconds: null };
  const paid = done.filter((d) => d.cost > 0).map((d) => d.cost);
  const factor = level === "light" ? 0.5 : Math.min(4, 1.5 + 0.1 * done.length);
  return {
    costUsd: paid.length ? median(paid) * factor : null,
    seconds: median(done.map((d) => d.seconds)) * factor,
  };
}

/** Whether a finished run gets the offer: it ended done, some step got done in it, and no pass ran yet. */
export function offersFinish(run: Pick<PlanRun, "status" | "steps"> & { finish?: FinishPass }): boolean {
  return run.status === "done" && !run.finish && run.steps.some((s) => s.status === "done" && s.attempts.length > 0);
}

const DIFF_LIMIT = 40_000;

/** The steps the run finished, with the Done notes they left in the task list. */
export interface DoneBrief {
  number: string | null;
  title: string;
  done: string[];
}

function briefLines(steps: DoneBrief[]): string[] {
  return steps.flatMap((s) => [
    `- ${s.number ? `${s.number}. ` : ""}${s.title}`,
    ...s.done.slice(0, 4).map((d) => `    ${d.slice(0, 400)}`),
  ]);
}

/** The light pass: one review of the whole run's diff, with the checks' result. */
export function finishReviewPrompt(
  file: string,
  steps: DoneBrief[],
  diff: string,
  checks: FinishPass["checks"],
): string {
  const shown = diff.length > DIFF_LIMIT ? `${diff.slice(0, DIFF_LIMIT)}\n… (cut: the diff is longer)` : diff;
  return [
    `A plan run in this project just finished the steps below of ${file}. Each step was checked on its own; you look`,
    "at the whole once, before the run is called done. Look for:",
    "- leftovers: debug output, commented-out code, temporary files, TODOs the steps added and did not resolve;",
    "- steps whose changes do not fit together (one changes what another relies on, names or formats that disagree);",
    "- a Done note that claims more than the diff shows;",
    "- bugs a reviewer of the whole would catch. Style alone is no finding.",
    'Answer JSON only: {"ok": true or false, "findings": ["file: what is wrong, and the fix"], "stillOpen": ["what',
    'the owner still has to do or decide"]}. Empty lists when there is nothing. Short, concrete items.',
    "",
    "Steps done in this run:",
    ...briefLines(steps),
    "",
    checks
      ? `The project's full checks (${checks.command}): ${checks.ok ? "passed" : `FAILED\n${checks.summary}`}`
      : "The project has no test command: no checks were run.",
    "",
    "The run's changes (git diff; the task list and its logs left out):",
    shown || "(no change)",
  ].join("\n");
}

/** Reads the review. Anything unreadable is a finding, never a quiet pass. */
export function parseFinishReview(answer: string): { ok: boolean; findings: string[]; stillOpen: string[] } {
  const v = jsonIn(answer) as { ok?: unknown; findings?: unknown; stillOpen?: unknown } | null;
  const list = (x: unknown) =>
    Array.isArray(x)
      ? x
          .map((i) => String(i ?? "").trim())
          .filter(Boolean)
          .slice(0, 20)
          .map((i) => i.slice(0, 400))
      : [];
  if (!v || typeof v.ok !== "boolean")
    return {
      ok: false,
      findings: ["The review's answer could not be read; its reply is in the agent pane."],
      stillOpen: [],
    };
  return { ok: v.ok, findings: list(v.findings), stillOpen: list(v.stillOpen) };
}

/** The full pass: a session that fixes what the review found and checks the rest. */
export function finishFullPrompt(
  file: string,
  steps: DoneBrief[],
  review: { findings: string[]; stillOpen: string[] },
  checks: FinishPass["checks"],
  diffFiles: string[],
): string {
  return [
    `A plan run in this project just finished the steps below of ${file}. Each step was checked on its own. You do the`,
    "finishing pass over the whole run before it is called done:",
    "1. Fix what the review below found, where it is right (say so where it is not). Keep each fix small.",
    "2. Remove leftovers the steps added: debug output, commented-out code, temporary files.",
    `3. Check each Done note below against the code: what a note claims must be there. Correct a note in ${file} that`,
    "   says more than was built; do not reopen or re-mark steps.",
    "4. Live tests: only where the project or the plan defines them (a live-test folder, script or a plan rule), and",
    "   only on a scratch copy or the test environment the plan names; never on the owner's real folders, data or live",
    "   sites. None defined: say so, and do not invent any.",
    "5. Run the project's full checks at the end.",
    "End your last message with a section headed exactly `Still open:` and one `- ` line per thing the owner still has",
    "to do or decide (`- none` when nothing is left).",
    "",
    "Steps done in this run:",
    ...briefLines(steps),
    "",
    "Files the run changed:",
    ...(diffFiles.length ? diffFiles.slice(0, 60).map((f) => `- ${f}`) : ["- (none found)"]),
    "",
    checks
      ? `The project's full checks (${checks.command}) before this pass: ${checks.ok ? "passed" : `FAILED\n${checks.summary}`}`
      : "The project has no test command set.",
    "",
    "The review of the whole diff found:",
    ...(review.findings.length ? review.findings.map((f) => `- ${f}`) : ["- nothing"]),
    ...(review.stillOpen.length ? ["", "It left open:", ...review.stillOpen.map((f) => `- ${f}`)] : []),
  ].join("\n");
}

/** The `Still open:` list at the end of the full pass's answer; null when the answer has none. */
export function stillOpenIn(text: string): string[] | null {
  const lines = text.split(/\r?\n/);
  let at = -1;
  for (let i = lines.length - 1; i >= 0; i--)
    if (/^\s*(?:#+\s*)?\**\s*still open\s*:?\s*\**\s*:?\s*(.*)$/i.test(lines[i])) {
      at = i;
      break;
    }
  if (at < 0) return null;
  const inline = /still open\s*:?\s*\**\s*:?\s*(.*)$/i.exec(lines[at])?.[1]?.trim() ?? "";
  const items: string[] = [];
  if (inline && !/^none\.?$/i.test(inline)) items.push(inline);
  for (const l of lines.slice(at + 1)) {
    const m = /^\s*(?:[-*•]|\d+[.)])\s+(.*)$/.exec(l);
    if (!m) {
      if (l.trim() && items.length) break;
      continue;
    }
    const item = m[1].trim();
    if (/^none\.?$/i.test(item)) continue;
    items.push(item.slice(0, 400));
  }
  return items.slice(0, 20);
}

/** Done notes under a step's line ("- Done: ..."). */
export function doneNotes(details: string[]): string[] {
  return details.map((d) => d.trim()).filter((d) => /^- Done:/i.test(d));
}

/** Plan files and their logs are no part of the run's code changes. */
export function isPlanFile(path: string, planFile: string, logDirPath: string): boolean {
  const p = path.replace(/\\/g, "/");
  const plan = planFile.replace(/\\/g, "/");
  const archive = plan.replace(/\.md$/i, ".archive.md");
  return p === plan || p.endsWith(`/${plan}`) || p === archive || p.startsWith(`${logDirPath}/`);
}

/** The pass as an entry for the plan's FINISH.md log. */
export function finishEntry(f: FinishPass): string {
  const when = new Date(f.startedAt).toISOString().slice(0, 16).replace("T", " ");
  return [
    `## Finishing pass (${f.level}), ${when}: ${f.status}`,
    `- Model: ${f.model}; cost $${f.costUsd.toFixed(2)}; ${Math.round(f.seconds)} s`,
    f.checks ? `- Checks (${f.checks.command}): ${f.checks.ok ? "passed" : "failed"}` : "- Checks: no test command",
    ...(f.findings.length ? ["- Found:", ...f.findings.map((x) => `  - ${x}`)] : ["- Found: nothing"]),
    ...(f.stillOpen.length ? ["- Still open:", ...f.stillOpen.map((x) => `  - ${x}`)] : ["- Still open: nothing"]),
    ...(f.note ? [`- Note: ${f.note}`] : []),
    "",
    "",
  ].join("\n");
}
