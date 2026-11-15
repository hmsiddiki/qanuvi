/**
 * Messages the user sends while a plan step runs (the user is watching): guidance the agent weighs now. It may change
 * how the step is done, forbid something (even what the step asks), become a new step in the plan, or stop the work.
 * When nobody writes, the plan runs as it was set.
 */

import { planData } from "./planData.svelte";

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
  const i = [text.indexOf(GUIDE_MARK), text.indexOf(STOPPED_MARK)].filter((x) => x >= 0);
  return i.length ? text.slice(0, Math.min(...i)) : text;
}

/** Where the note starts in a message written in the chat of a step the run stopped at (shown: the words only). */
export const STOPPED_MARK = "\n\n(Qanuvi: the plan run is stopped at step ";

/** The question the agent ends with when the run could go on; the owner's yes to it resumes the run. */
export const RESUME_ASK = "Should I resume the plan run from step";

/**
 * Writing in the chat of a step the run stopped at (blocked, failed or stopped) is a chat, not the run: the agent
 * answers and does what the owner asks in this session, and asks before the run goes on. Only the owner's yes, or a
 * plain order such as "continue", resumes the run.
 */
export function stoppedChatNote(step: string, file: string): string {
  return (
    `${STOPPED_MARK}${step} of ${file}; this is a chat with the owner, not the run. Answer what they ask first, ` +
    "clearly. When they ask you to do something, do it here (with the usual approvals) and keep the plan file true: " +
    "the step's mark says what is really so. Do not go on with other steps. When the owner wants the run to go on, or " +
    "what blocked the step is settled, end your reply with this question on its own line: " +
    `"${RESUME_ASK} ${step}? (yes/no)". Qanuvi resumes the run only when the owner answers yes or says to continue.)`
  );
}

/** A plain order to go on with the run ("continue", "resume the run", "retry the step"). */
export function isGoOnOrder(text: string): boolean {
  return /^\s*(continue|resume|go on|carry on|proceed|retry|run it|run it again|run again|retry (it|the step|this step)|(continue|resume|run) the (run|plan|plan run|step)( again)?)[\s.!]*$/i.test(
    text,
  );
}

/** A plain yes ("yes", "ok", "go ahead"). */
export function isYes(text: string): boolean {
  return /^\s*(y|yes|yeah|yep|yup|ok|okay|sure|go|go ahead|do it|please do|yes please|yes,? (resume|continue|go on)( it)?)[\s.!]*$/i.test(
    text,
  );
}

/** Whether the agent's reply asked to resume the run (so a yes to it resumes). */
export function askedToResume(reply: string): boolean {
  return /resume the (plan )?run/i.test(reply) && reply.includes("?");
}

/** A plain order to stop the run ("stop", "stop the run", "halt now"): Qanuvi stops it at once. */
export function isStopOrder(text: string): boolean {
  return /^\s*(stop|halt|cancel|abort)( (it|now|the run|the plan|everything|running|here))*[\s.!]*$/i.test(text);
}

export function stepKey(s: { number: string | null; title: string }): string {
  return `${s.number ?? ""}|${s.title}`;
}

/** Where a plan's run settings are kept (the Run dialog reads them). */
export function runSettingsKey(projectId: string, file: string): string {
  return `qanuvi.run.${projectId}.${file}`;
}

/** A message that seems to hold a password or key (kept out of notes; Qanuvi offers project secrets instead). */
export function looksSecret(text: string): boolean {
  const key = /\b(sk|pk|ghp|gho|xox[abp])[-_][A-Za-z0-9_-]{16,}/;
  const hint = /\b(pass(word|wd)?|pwd|secret|token|api[ _-]?key|credentials?|login|db user)\b/i;
  // A value with letters and digits mixed, like most passwords (not plain words).
  const value = /(?<!\w)(?=[\w!@#$%^&*.-]*\d)(?=[\w!@#$%^&*.-]*[A-Za-z])[\w!@#$%^&*.-]{5,}/;
  return key.test(text) || (hint.test(text) && value.test(text));
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

/** The plan's private details for a step's request, with the rule for them. */
export function privateBlock(details: string[]): string {
  if (!details.length) return "";
  return [
    "",
    "Private details the owner gave earlier in this plan (logins, passwords). Use them where the work needs them; " +
      "never write them into files, notes, commits, plans or your answers:",
    // One line each: the stored copy masks each "- " line, so a detail must not run on to a next line.
    ...details.map((d) => `- ${d.replace(/\s*\n\s*/g, " ")}`),
  ].join("\n");
}

/** Where a plan's notes are kept: the owner's instructions belong to the plan, for every run of it. */
function planNotesKey(projectId: string, file: string): string {
  return `qanuvi.plannotes.${projectId}.${file}`;
}

export function loadPlanNotes(projectId: string, file: string): string[] {
  try {
    const v = JSON.parse(planData.getItem(planNotesKey(projectId, file)) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function savePlanNotes(projectId: string, file: string, notes: string[]): void {
  try {
    planData.setItem(planNotesKey(projectId, file), JSON.stringify(notes.slice(-60)));
  } catch {
    // Notes are a help; the run goes on without them.
  }
}

/** The folder of a plan's step logs, and one step's log. */
export function logDir(planFile: string): string {
  const stem = planFile
    .replace(/\\/g, "/")
    .replace(/\.md$/i, "")
    .replace(/[^A-Za-z0-9._-]+/g, "_");
  return `.qanuvi/plan-logs/${stem}`;
}

/** The plan map: what earlier steps built and where (kept by the steps themselves). */
export function mapPath(planFile: string): string {
  return `${logDir(planFile)}/MAP.md`;
}

export function factsPath(planFile: string): string {
  return `${logDir(planFile)}/FACTS.md`;
}
