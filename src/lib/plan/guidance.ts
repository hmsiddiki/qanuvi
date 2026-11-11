/**
 * Messages the user sends while a plan step runs (the user is watching): guidance the agent weighs now. It may change
 * how the step is done, forbid something (even what the step asks), become a new step in the plan, or stop the work.
 * When nobody writes, the plan runs as it was set.
 */

/** Where the note starts in a sent message (the stored and shown message is the user's words only). */
export const GUIDE_MARK = "\n\n(Qanuvi: the user wrote this while you work on step ";

/** The note sent with the user's words. */
export function guidanceNote(step: string, file: string): string {
  return (
    `${GUIDE_MARK}${step} of ${file}. The user is watching. Take it in now and decide: if it changes how to do this ` +
    "step, follow it; if it says not to do something, do not do it, even when the step asks for it. If it asks for " +
    "something more: when it is small, do it now as part of this step and add a short note of it under the step in " +
    `the plan; when it is big, do not do it now: add it to the plan as a new open step right after ${step}, numbered ` +
    `${step}-1 (then ${step}-2), in the plan's format, and say so; it runs after this step. If it asks you to ` +
    "stop, stop and say where you left off. If it asks to go back to an earlier step (one that is blocked, skipped " +
    "or not finished), set that step back to [ ] in the plan and remove its Blocked or Skipped line; Qanuvi runs it " +
    "right after this step. Answer it in one short line before going on.)"
  );
}

export function withoutGuidance(text: string): string {
  const i = text.indexOf(GUIDE_MARK);
  return i >= 0 ? text.slice(0, i) : text;
}

/** A plain order to stop the run ("stop", "stop the run", "halt now"): Qanuvi stops it at once. */
export function isStopOrder(text: string): boolean {
  return /^\s*(stop|halt|cancel|abort)( (it|now|the run|the plan|everything|running|here))*[\s.!]*$/i.test(text);
}

/** Open steps in the plan now that the run did not know: added while it ran (by the agent or the user). */
export function addedSteps<T extends { number: string | null; title: string; status: string }>(
  now: T[],
  known: Set<string>,
): T[] {
  return now.filter((s) => s.status !== "x" && s.status !== "!" && !known.has(stepKey(s)));
}

export function stepKey(s: { number: string | null; title: string }): string {
  return `${s.number ?? ""}|${s.title}`;
}

/** Where a plan's run settings are kept (the Run dialog reads them). */
export function runSettingsKey(projectId: string, file: string): string {
  return `qanuvi.run.${projectId}.${file}`;
}

/** Changes some of a plan's kept run settings (a picker change while it ran). */
export function keepRunSetting(projectId: string, file: string, patch: Record<string, unknown>): void {
  try {
    const key = runSettingsKey(projectId, file);
    const now = JSON.parse(localStorage.getItem(key) ?? "{}");
    localStorage.setItem(key, JSON.stringify({ ...now, ...patch }));
  } catch {
    // Kept settings are a help; the run goes on without them.
  }
}

/** How an agent leaves something for the later steps of a run (a decision, a command), in its answer. */
export const NOTE_PREFIX = "Note for the next steps:";

/** The run's notes for a step's request, and how to leave new ones. */
export function notesBlock(notes: string[]): string {
  const lines = notes.length
    ? [
        "",
        "Notes from earlier in this run (the owner's words and what earlier steps left); follow them:",
        ...notes.map((n) => `- ${n}`),
      ]
    : [];
  return [
    ...lines,
    "",
    `If later steps need something you learned or decided, add a line starting with "${NOTE_PREFIX}" to your last ` +
      "answer. Never put a password or key in it: the project's secrets reach every step as environment variables.",
  ].join("\n");
}

/** Notes an agent left in its answer. */
export function notesIn(text: string): string[] {
  const out: string[] = [];
  for (const line of text.split("\n")) {
    const i = line.indexOf(NOTE_PREFIX);
    if (i < 0) continue;
    const note = line
      .slice(i + NOTE_PREFIX.length)
      .replace(/\*+/g, "")
      .trim();
    if (note && !looksSecret(note)) out.push(note.slice(0, 400));
  }
  return out;
}

/** A message that seems to hold a password or key (kept out of notes; Qanuvi offers project secrets instead). */
export function looksSecret(text: string): boolean {
  const key = /\b(sk|pk|ghp|gho|xox[abp])[-_][A-Za-z0-9_-]{16,}/;
  const hint = /\b(pass(word|wd)?|pwd|secret|token|api[ _-]?key|credentials?|login|db user)\b/i;
  // A value with letters and digits mixed, like most passwords (not plain words).
  const value = /(?<!\w)(?=[\w!@#$%^&*.-]*\d)(?=[\w!@#$%^&*.-]*[A-Za-z])[\w!@#$%^&*.-]{5,}/;
  return key.test(text) || (hint.test(text) && value.test(text));
}

/**
 * Steps the run already passed (blocked, failed or skipped) that are open in the plan again: the owner asked to go
 * back to them. Each comes back at most once per run.
 */
export function reopenedSteps<
  R extends { number: string | null; title: string; status: string },
  S extends { number: string | null; title: string; status: string },
>(runSteps: R[], now: S[], reopened: Set<string>): S[] {
  const passed = new Set(
    runSteps.filter((r) => ["blocked", "failed", "skipped"].includes(r.status)).map((r) => stepKey(r)),
  );
  const waiting = new Set(runSteps.filter((r) => r.status === "waiting").map((r) => stepKey(r)));
  return now.filter(
    (s) => s.status === " " && passed.has(stepKey(s)) && !waiting.has(stepKey(s)) && !reopened.has(stepKey(s)),
  );
}

/** A session's context size now: what its last model call read (new, cached and cache-written tokens). */
export function contextSize(lines: string[]): number {
  for (let i = lines.length - 1; i >= 0; i--) {
    if (!lines[i].includes('"usage"') || !lines[i].includes('"assistant"')) continue;
    try {
      const u = JSON.parse(lines[i])?.message?.usage;
      if (u) return (u.input_tokens ?? 0) + (u.cache_read_input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0);
    } catch {
      // Not a JSON line: look further back.
    }
  }
  return 0;
}

/** At most this many steps share one session, whatever its size says. */
export const CARRY_MAX_STEPS = 6;

/** Whether the next step continues the session: same model, same section, still small. */
export function canCarry(
  carry: { model: string; section: string; tokens: number; steps: number } | null,
  model: string,
  section: string,
  limit: number,
): boolean {
  return (
    !!carry &&
    carry.model === model &&
    carry.section === section &&
    carry.tokens < limit &&
    carry.steps < CARRY_MAX_STEPS
  );
}

/**
 * How many of the steps still to run need the blocked one, directly or through others. Many: the run stops and asks,
 * rather than skipping them all.
 */
export function needingCount(
  blocked: string,
  steps: { number: string | null; needs?: string[] }[],
  waiting: Set<string>,
): number {
  const chain = new Set([blocked]);
  let n = 0;
  for (const s of steps) {
    if (!s.number || !s.needs?.some((x) => chain.has(x.trim()))) continue;
    chain.add(s.number);
    if (waiting.has(s.number)) n++;
  }
  return n;
}

/** The plan's private details for a step's request, with the rule for them. */
export function privateBlock(details: string[]): string {
  if (!details.length) return "";
  return [
    "",
    "Private details the owner gave earlier in this plan (logins, passwords). Use them where the work needs them; " +
      "never write them into files, notes, commits, plans or your answers:",
    ...details.map((d) => `- ${d}`),
  ].join("\n");
}

/** Where a plan's notes are kept: the owner's instructions belong to the plan, for every run of it. */
function planNotesKey(projectId: string, file: string): string {
  return `qanuvi.plannotes.${projectId}.${file}`;
}

export function loadPlanNotes(projectId: string, file: string): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(planNotesKey(projectId, file)) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function savePlanNotes(projectId: string, file: string, notes: string[]): void {
  try {
    localStorage.setItem(planNotesKey(projectId, file), JSON.stringify(notes.slice(-60)));
  } catch {
    // Notes are a help; the run goes on without them.
  }
}
