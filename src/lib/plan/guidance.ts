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

/** Default context size at which a step hands over to a new session in the middle of its work (24p). */
export const HANDOVER_DEFAULT = 200_000;
/** At most this many handovers in one step: past that, the step goes on in its session (it may be stuck). */
export const HANDOVER_MAX = 4;

/**
 * Whether a running step should hand over now: its context has grown past the limit (0 = never). Every further model
 * call re-reads the whole context, so a long step gets dearer with each call, and a very long context also makes the
 * model lose track of details; a new session that starts from a careful progress note is cheaper and keeps its focus.
 */
export function handoverDue(context: number, limit: number, handovers: number): boolean {
  return limit > 0 && context >= limit && handovers < HANDOVER_MAX;
}

/**
 * What the step's agent is told when its context is too big. Quality first: it finishes the edit in hand (nothing
 * left half-written) and writes a note good enough for a new session to go on without guessing.
 */
export function handoverRequest(log: string, context: number): string {
  return (
    `(Qanuvi: this session's context is now about ${Math.round(context / 1000)}k tokens. A new session takes over ` +
    `this step, so the work stays focused and the cost stays down. Finish the edit you are making so no file is left ` +
    `half-written or broken; do not start anything new. Then add a "Progress" note to ${log} that a new session can ` +
    `work from without guessing: what is done and checked, what is left (in order), the files you touched and why, ` +
    `anything half-way, decisions you made and their reasons, and facts about the code you learned (names, places, ` +
    `traps). Do not mark the step done unless it is. Then stop and answer "Handed over".)`
  );
}

/** The first words of the new session that takes a step over. */
export const HANDOVER_RESUME =
  "An earlier session worked on this step until its context grew too big, and handed it over to you. Read its Progress note in the step's log first. Before you go on, read the files it names and check that what it calls done really is (fix it if not). Then finish what is left; do not redo what is done and correct.";

/**
 * What the step's agent is told when the owner picks another model for the step while it works: finish the edit in
 * hand, write the progress note, stop; the chosen model goes on in a new session.
 */
export function modelSwitchRequest(log: string, model: string): string {
  return (
    `(Qanuvi: the owner picked ${model} for this step; it takes over in a new session. Finish the edit you are making ` +
    `so no file is left half-written or broken; do not start anything new. Then add a "Progress" note to ${log} that ` +
    `a new session can work from without guessing: what is done and checked, what is left (in order), the files you ` +
    `touched and why, anything half-way, decisions you made and their reasons, and facts about the code you learned ` +
    `(names, places, traps). Do not mark the step done unless it is. Then stop and answer "Handed over".)`
  );
}

/** The first words of the session on the model the owner switched the step to. */
export const SWITCH_RESUME =
  "The owner switched this step to you while another model was working on it; that model stopped after the edit in hand and wrote a Progress note in the step's log. Read the note first. Before you go on, read the files it names and check that what it calls done really is (fix it if not). Then finish what is left; do not redo what is done and correct.";

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

/** The folder of a plan's step logs, and one step's log. */
export function logDir(planFile: string): string {
  const stem = planFile
    .replace(/\\/g, "/")
    .replace(/\.md$/i, "")
    .replace(/[^A-Za-z0-9._-]+/g, "_");
  return `.qanuvi/plan-logs/${stem}`;
}
export function logPath(planFile: string, step: { number: string | null; title: string }): string {
  const name = (step.number ?? step.title.slice(0, 40)).replace(/[^A-Za-z0-9._-]+/g, "_");
  return `${logDir(planFile)}/${name}.md`;
}

/** One attempt in a step's log. */
export function attemptEntry(a: {
  when: Date;
  model: string;
  outcome: string;
  costUsd: number;
  seconds: number;
  testsPassed?: boolean | null;
  note?: string;
  failure?: string;
  answer?: string;
}): string {
  const cut = (t: string, n: number) => (t.length > n ? t.slice(0, n) + " …" : t);
  const lines = [
    "",
    `## Attempt ${a.when.toISOString().slice(0, 16).replace("T", " ")} · ${a.model} · ${a.outcome === "open" ? "turn ended" : a.outcome} · $${a.costUsd.toFixed(2)} · ${Math.round(a.seconds)} s`,
  ];
  if (a.testsPassed !== undefined && a.testsPassed !== null)
    lines.push(a.testsPassed ? "Tests: passed." : "Tests: failed.");
  if (a.note) lines.push(`Note: ${a.note}`);
  if (a.failure) lines.push("Failure:", "```", cut(a.failure, 1500), "```");
  if (a.answer) lines.push("The agent's last answer:", "", cut(a.answer.trim(), 1500));
  return lines.join("\n") + "\n";
}

/** A step's result in its log. */
export function resultEntry(status: string, reason?: string): string {
  return `
**Result: ${status}**${reason ? ` — ${reason}` : ""}
`;
}

/** What a step is told about the logs. */
export function logsBlock(planFile: string, own: string, ownExists: boolean): string {
  return [
    "",
    `Every step of this plan has a log in ${logDir(planFile)}/ (its attempts, failures and results). Read the log of a step you depend on or that failed, when it helps.`,
    ...(ownExists ? [`This step was tried before: read its log ${own} first and do not repeat what failed.`] : []),
  ].join("\n");
}

/** The plan map: what earlier steps built and where (kept by the steps themselves). */
export function mapPath(planFile: string): string {
  return `${logDir(planFile)}/MAP.md`;
}

/**
 * What a step gets so it does not have to find its way from nothing: the plan map, the files the last steps changed,
 * how to find code (Qanuvi's index), and the rule to keep the map current.
 */
export function contextBlock(
  planFile: string,
  map: string,
  recent: { number: string | null; files: string[] }[],
  indexTools: boolean,
): string {
  const path = mapPath(planFile);
  const lines = [""];
  if (map.trim()) {
    const cut = map.length > 8000 ? map.slice(0, 8000) + "\n…" : map;
    lines.push(`The plan map (${path}): what earlier steps built and where. Start from it.`, cut.trim(), "");
  }
  const shown = recent.filter((r) => r.files.length).slice(-3);
  if (shown.length) {
    lines.push("Files the last steps changed:");
    for (const r of shown)
      lines.push(`- ${r.number ?? "step"}: ${r.files.slice(0, 25).join(", ")}${r.files.length > 25 ? ", …" : ""}`);
    lines.push("");
  }
  if (indexTools)
    lines.push(
      "Finding your way: use Qanuvi's code index (find_symbol for where something is defined, outline for what a file " +
        "holds, read_lines for just the lines you need, project_map for the layout) rather than reading whole files " +
        "with cat or sed.",
    );
  lines.push(
    `When the step is done, update the plan map ${path} (create it if it is missing): under 80 lines, one line per file, "path — what it holds or does", grouped by area; add what this step created or changed, keep earlier lines unless they are wrong now.`,
  );
  return lines.join("\n");
}

/**
 * The plan's knowledge, shared by every step, session and model (the owner's design, 2026-11-12): MAP.md (what is
 * where), FACTS.md (decisions, conventions, how to run and test, gotchas), API.md (what the plan added, written by
 * Qanuvi from its code index) and a log per step. Small files, kept current, so no session rediscovers the project.
 */
export const MAP_LINES = 80;
export const FACTS_LINES = 60;

export function factsPath(planFile: string): string {
  return `${logDir(planFile)}/FACTS.md`;
}
export function apiPath(planFile: string): string {
  return `${logDir(planFile)}/API.md`;
}

function lineCount(text: string): number {
  return text.split("\n").filter((l) => l.trim()).length;
}

/**
 * What every step gets first (the same for steps in a row, so it is read from the cache): where the knowledge is, the
 * map and the facts, the files the last steps changed, how to find code, and the rule to keep the files current.
 */
export function knowledgeBlock(
  planFile: string,
  map: string,
  facts: string,
  recent: { number: string | null; files: string[] }[],
  indexTools: boolean,
): string {
  const dir = logDir(planFile);
  const cut = (t: string) => (t.length > 8000 ? t.slice(0, 8000) + "\n…" : t.trim());
  const lines = [
    `This plan's knowledge, shared by every step and model, is in ${dir}/: MAP.md (what is where), FACTS.md (decisions, conventions, how to run and test, gotchas), API.md (functions and classes the plan added, with file and line; read it before opening files) and a log per step. Start from it; do not rediscover what it says.`,
  ];
  if (map.trim()) lines.push("", "MAP.md:", cut(map));
  if (facts.trim()) lines.push("", "FACTS.md:", cut(facts));
  const shown = recent.filter((r) => r.files.length).slice(-3);
  if (shown.length) {
    lines.push("", "Files the last steps changed:");
    for (const r of shown)
      lines.push(`- ${r.number ?? "step"}: ${r.files.slice(0, 25).join(", ")}${r.files.length > 25 ? ", …" : ""}`);
  }
  if (indexTools)
    lines.push(
      "",
      "Finding your way: use Qanuvi's code index (find_symbol for where something is defined, outline for what a file " +
        "holds, read_lines for just the lines you need, project_map for the layout) rather than reading whole files " +
        "with cat or sed.",
    );
  const tidy: string[] = [];
  if (lineCount(map) > MAP_LINES) tidy.push(`MAP.md has ${lineCount(map)} lines: tidy it to under ${MAP_LINES}`);
  if (lineCount(facts) > FACTS_LINES)
    tidy.push(`FACTS.md has ${lineCount(facts)} lines: tidy it to under ${FACTS_LINES}`);
  lines.push(
    "",
    `Before you finish: update ${dir}/MAP.md (one line per file, "path — what it holds or does", under ${MAP_LINES} lines; create it if missing) and ${dir}/FACTS.md (what later steps must know: decisions, conventions, how to run things, gotchas; under ${FACTS_LINES} lines; never passwords or keys). Change or remove lines that are no longer right instead of adding more.` +
      (tidy.length ? " " + tidy.join("; ") + "." : ""),
    "",
  );
  return lines.join("\n");
}

const CODE = /\.(php|js|mjs|cjs|ts|tsx|jsx|svelte|vue|py|rs|go|java|kt|cs|rb|swift|dart|c|cc|cpp|h|hpp)$/i;

/** Whether a changed file is code the index outlines. */
export function isCode(path: string): boolean {
  return CODE.test(path) && !/(^|\/)(node_modules|vendor|dist|build)\//.test(path);
}

/** One file's part of API.md, from the code index's outline. */
export function apiSection(
  path: string,
  items: { name: string; kind: string; line: number; container: string | null }[],
): string {
  if (!items.length) return "";
  const rows = items
    .slice(0, 40)
    .map((i) => `- ${i.kind} ${i.container ? i.container + "::" : ""}${i.name} :${i.line}`);
  return [`### ${path}`, ...rows].join("\n");
}

/** API.md with the given files' parts replaced (an empty part removes the file); files in path order. */
export function mergeApi(existing: string, updates: Map<string, string>): string {
  const parts = new Map<string, string>();
  for (const chunk of existing.split(/\n(?=### )/)) {
    const m = chunk.match(/^### (.+)/);
    if (m) parts.set(m[1].trim(), chunk.trim());
  }
  for (const [path, section] of updates) {
    if (section) parts.set(path, section);
    else parts.delete(path);
  }
  const body = [...parts.keys()].sort().map((k) => parts.get(k));
  return [
    "# What this plan added (written by Qanuvi from its code index after each step)",
    "",
    ...body.flatMap((b) => [b, ""]),
  ].join("\n");
}

/**
 * Other plans this one builds on: those its steps need ("Needs: OTHER-PLAN.md#4") and those its text names. Their
 * knowledge is their own; a step is told where it is and reads it when it helps.
 */
export function referencedPlans(planText: string, planFile: string): string[] {
  const self = planFile.replace(/\\/g, "/").toLowerCase();
  const found = new Set<string>();
  for (const m of planText.matchAll(/Needs:[^\n]*/gi))
    for (const part of m[0].slice(6).split(","))
      if (part.includes("#")) found.add(part.slice(0, part.lastIndexOf("#")).trim());
  for (const m of planText.matchAll(/[\w./-]*(?:task[-_ ]?list|todo|plan|roadmap)[\w.-]*\.md/gi)) found.add(m[0]);
  return [...found]
    .map((p) => p.replace(/\\/g, "/").replace(/^\.\//, ""))
    .filter(
      (p) =>
        p &&
        p.toLowerCase() !== self &&
        !p.toLowerCase().endsWith("/" + self) &&
        !self.endsWith("/" + p.toLowerCase()) &&
        !p.startsWith(".qanuvi/"),
    );
}

/** The lines telling a step where referenced plans keep their knowledge. */
export function referencedBlock(plans: string[]): string {
  if (!plans.length) return "";
  return [
    "",
    "Plans this one builds on keep their own knowledge (read their MAP.md, FACTS.md and API.md when the step touches their work):",
    ...plans.slice(0, 8).map((p) => `- ${p}: ${logDir(p)}/`),
  ].join("\n");
}
