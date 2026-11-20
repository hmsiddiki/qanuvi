/**
 * Before a run (24i): the paid agent reads the open steps once and lists what the run will need from the owner, so
 * the owner answers once and the run then goes unattended: logins (project secrets), decisions only the owner can
 * make, risky steps (wait or go ahead), things that must be running or set, and steps that cannot be checked.
 */

export interface PreflightStep {
  number: string | null;
  title: string;
  details: string[];
}

/**
 * How an ask is sorted (24u part 3): "decide": Qanuvi takes the recommended answer (a sensible default kept easy to
 * change) and says so; "checks": the step goes ahead, but the decider reviews its diff and the tests must pass before
 * it counts as done; "you": truly needs the owner (secrets, outside accounts, anything leaving this computer).
 */
export type AskSort = "decide" | "checks" | "you";

export interface PreflightQuestion {
  step: string;
  question: string;
  sort: AskSort;
  /** The answer the check recommends (empty when it has none). */
  recommended: string;
  /** Why that answer, or why the owner must answer. */
  because: string;
  /** PM a: "project" when the answer holds for the whole project (a test site, how tests run), so the owner's answer
   * goes into This project memory; "plan" when it is only this plan's choice. */
  about: "project" | "plan";
  /** PM c: the project memory already answers it: recommended is the memory's answer, shown "from memory". */
  fromMemory: boolean;
}

export interface PreflightRisk {
  step: string;
  why: string;
  sort: AskSort;
  because: string;
}

/**
 * A login, key or address the steps need. `what`, `example` and `ifEmpty` are written for someone who does not know
 * the code (owner, 2026-11-20: "if the dev building it cannot understand the question, how would a general user?").
 */
export interface PreflightSecret {
  name: string;
  /** What it is, in plain words. */
  what: string;
  /** An example value (never a real password). */
  example: string;
  /** What happens if it is left empty. */
  ifEmpty: string;
  why: string;
  steps: string[];
}

export interface Preflight {
  secrets: PreflightSecret[];
  questions: PreflightQuestion[];
  risky: PreflightRisk[];
  setup: { what: string; why: string }[];
  /** Steps that must run after steps the file lists after them (R2e g): each becomes a "- Needs:" line. */
  order: { step: string; after: string[]; why: string }[];
}

/** What a risky step does in the run: go ahead, go ahead with extra checks, or wait for the owner. */
export type RiskChoice = "go" | "checks" | "wait";

/** The choice a risky step starts on, from its sort (holds only where it truly needs the owner). */
export function recommendedRisk(sort: AskSort): RiskChoice {
  return sort === "decide" ? "go" : sort === "checks" ? "checks" : "wait";
}

export const EMPTY_PREFLIGHT: Preflight = { secrets: [], questions: [], risky: [], setup: [], order: [] };

/** f4: an example in the owner's questions never uses their own name (it once came from the folder path). */
const NEUTRAL_EXAMPLES =
  'Examples use made-up neutral names (for example "Example Shop Ltd"), never the owner\'s own name or a name taken from a folder path, user name or account.';

export function preflightPrompt(
  file: string,
  steps: PreflightStep[],
  known: { secrets: string[]; testCommand: string; previewUrl: string; notes: string[]; memory?: string[] },
): string {
  const memory = known.memory ?? [];
  const list = steps
    .map((s) =>
      [`- ${s.number ? s.number + ". " : ""}${s.title}`, ...s.details.slice(0, 8).map((d) => `    ${d}`)].join("\n"),
    )
    .join("\n");
  return [
    `Before an unattended run of ${file}, read these open steps (and the code only where you must, read-only: at most`,
    "about ten quick looks; this check should take a minute or two, not a step's work) and list",
    "what the run will need from the owner, so the owner answers once and the run then goes on alone. Change nothing.",
    "",
    "Already known:",
    `- Project secrets (environment variables every step gets): ${known.secrets.join(", ") || "none"}`,
    `- Test command: ${known.testCommand || "none set"}`,
    `- Preview address: ${known.previewUrl || "none set"}`,
    ...(known.notes.length ? ["- The owner's notes for this plan:", ...known.notes.map((n) => `  - ${n}`)] : []),
    ...(memory.length
      ? ["- This project's memory (the owner's own facts about the project):", ...memory.map((m) => `  - ${m}`)]
      : []),
    "",
    "Open steps:",
    list,
    "",
    "Answer with JSON only, in this shape (empty lists when nothing is needed):",
    '{"secrets":[{"name":"ENV_NAME","what":"what it is, in plain words","example":"an example value, never a real password","ifEmpty":"what happens if it is left empty","why":"...","steps":["82e"]}],',
    ' "questions":[{"step":"83c","question":"a choice the steps leave open","sort":"decide","recommended":"the answer you advise","because":"...","about":"plan","fromMemory":false}],',
    ' "risky":[{"step":"84a","why":"deletes data / migrates / publishes / live payments / pushes","sort":"checks","because":"..."}],',
    ' "setup":[{"what":"something that must run or be set","why":"..."}],',
    ' "order":[{"step":"85b","after":["85c"],"why":"..."}]}',
    "Rules: secrets only for logins, keys, passwords and addresses the steps cannot run without and that are not",
    "already known. Before asking, look at how the project's test command or test runner reads its settings (its docs",
    "say what to set): a variable the runner fills in by itself is not missing, ask for the one the runner reads; a",
    "need a known secret already covers is not asked again.",
    "Write every secret, question and risky item for someone who does not know the code: plain words, what it is for",
    "and what each answer changes, with one concrete example; never quote a plan line or a code name without saying",
    "what it means.",
    NEUTRAL_EXAMPLES,
    "Questions only for real choices the steps leave open, not for things you can decide from the code; risky only",
    "for steps that are hard to undo or reach outside this computer; order only when a step must run after steps",
    "listed below it (the run goes in file order otherwise; each becomes a Needs line in the plan). At most 8 items",
    "in each list.",
    "Sort every question and risky step (sort):",
    '- "decide": a sensible default exists that can be changed later (a setting, not hard-coded); give it as',
    "  recommended and say why. Most naming, format, layout and library choices are this.",
    '- "checks": it may go ahead if the decider reviews its diff and the tests pass first (migrations, deletions on',
    "  this computer or a test database, refactors of shared code).",
    '- "you": truly needs the owner: secrets, actions on outside accounts (real payments, publishing, pushing, mail',
    "  to real people), or anything that leaves this computer. Still give a recommended answer when you have one.",
    "Judge risk by where the step runs: money on a local or test database is not real money.",
    'A "decide" answer must be something the run may do under the run rules above (nothing they hold or refuse,',
    'such as a download, an install or an outside site); when only such a way exists, sort it "you".',
    '"about": "project" when the answer holds for the whole project beyond this plan (which site or address to test',
    'on, how the tests run, how to get a test login, where something is), else "plan".',
    'A question the project memory or a known secret already answers is not asked of the owner: give it as "decide"',
    'with the memory\'s answer as recommended and "fromMemory": true.',
  ].join("\n");
}

function sortOf(v: unknown, fallback: AskSort): AskSort {
  return v === "decide" || v === "checks" || v === "you" ? v : fallback;
}

/** A text longer than `n` characters ends at a word (when one ends at 200 or later) with "…". */
export function clipReason(s: string, n = 300): string {
  if (s.length <= n) return s;
  let head = s.slice(0, n - 1);
  const space = head.lastIndexOf(" ");
  if (space >= 200) head = head.slice(0, space);
  return head.trimEnd() + "…";
}

/** Reads the agent's answer; anything unreadable gives an empty check (the run is not held up by it). */
export function parsePreflight(text: string): Preflight {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end <= start) return { ...EMPTY_PREFLIGHT };
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(text.slice(start, end + 1));
  } catch {
    return { ...EMPTY_PREFLIGHT };
  }
  const arr = (v: unknown) =>
    Array.isArray(v) ? (v as Record<string, unknown>[]).filter((x) => x && typeof x === "object") : [];
  const str = (v: unknown) => (typeof v === "string" ? clipReason(v.trim()) : "");
  const okName = (n: string) => /^[A-Za-z_][A-Za-z0-9_]{0,99}$/.test(n);
  return {
    secrets: arr(raw.secrets)
      .map((s) => ({
        name: str(s.name),
        what: str(s.what),
        example: str(s.example),
        ifEmpty: str(s.ifEmpty),
        why: str(s.why),
        steps: Array.isArray(s.steps) ? (s.steps as unknown[]).map(str).filter(Boolean).slice(0, 20) : [],
      }))
      .filter((s) => okName(s.name))
      .slice(0, 8),
    questions: arr(raw.questions)
      .map((q) => {
        const recommended = str(q.recommended);
        // Unsorted, a question with no recommended answer needs the owner; one with an answer Qanuvi can decide.
        const sort = sortOf(q.sort, recommended ? "decide" : "you");
        return {
          step: str(q.step),
          question: str(q.question),
          // Nothing to go ahead with: only the owner can answer it.
          sort: !recommended ? ("you" as const) : sort,
          recommended,
          because: str(q.because),
          about: q.about === "project" ? ("project" as const) : ("plan" as const),
          fromMemory: q.fromMemory === true && !!recommended,
        };
      })
      .filter((q) => q.question)
      .slice(0, 8),
    risky: arr(raw.risky)
      // Unsorted risky steps wait, as before sorting.
      .map((r) => ({ step: str(r.step), why: str(r.why), sort: sortOf(r.sort, "you"), because: str(r.because) }))
      .filter((r) => r.step)
      .slice(0, 8),
    setup: arr(raw.setup)
      .map((x) => ({ what: str(x.what), why: str(x.why) }))
      .filter((x) => x.what)
      .slice(0, 8),
    order: arr(raw.order)
      .map((o) => {
        const step = str(o.step);
        const list = Array.isArray(o.after) ? (o.after as unknown[]) : typeof o.after === "string" ? [o.after] : [];
        const after = [...new Set(list.map(str).filter((a) => a && a !== step && !/[\s,]/.test(a)))].slice(0, 10);
        return { step, after, why: str(o.why) };
      })
      .filter((o) => o.step && o.after.length)
      .slice(0, 8),
  };
}

/** A choice Qanuvi made for the owner (24u part 3): shown and logged as "Decided for you", changeable. */
export interface DecidedForYou {
  step: string;
  /** The question, or the risky step's risk. */
  what: string;
  answer: string;
  because: string;
}

/** What the owner's answers to the check come to for the run. */
export interface SettledCheck {
  /** Steps the run holds, with who set the hold. */
  holds: { step: string; why: string; by: "you" | "default" }[];
  /** Steps the check no longer holds (answered, or let go ahead). */
  cleared: string[];
  /** The owner's own answers, kept as notes for the plan. */
  notes: string[];
  decided: DecidedForYou[];
  /** Steps that go ahead only after the decider's review of the diff and passing tests. */
  extraChecks: { step: string; why: string }[];
}

/** The plan note for a choice Qanuvi made, so every step and model follows it and keeps it easy to change. */
export function decidedNote(d: DecidedForYou): string {
  return `Decided for you for step ${d.step}: ${d.what} → ${d.answer}${d.because ? `, because ${d.because}` : ""}. The owner may change it later, so keep it in one place (a setting or constant), not hard-coded.`;
}

/**
 * Sorts the check's items into holds, decided-for-you choices and extra checks. `answers` and `risk` are the form's
 * values; `touched` marks the ones the owner changed (an untouched recommended answer is Qanuvi's decision, not theirs).
 */
export function settleCheck(
  check: Preflight,
  answers: Record<number, string>,
  answersTouched: Record<number, boolean>,
  risk: Record<number, RiskChoice>,
  riskTouched: Record<number, boolean>,
): SettledCheck {
  const out: SettledCheck = { holds: [], cleared: [], notes: [], decided: [], extraChecks: [] };
  const clear = (step: string) => step && !out.cleared.includes(step) && out.cleared.push(step);
  check.questions.forEach((q, i) => {
    const a = answers[i]?.trim() ?? "";
    if (!a) {
      if (q.step) out.holds.push({ step: q.step, why: `a question was left open (${q.question})`, by: "default" });
      return;
    }
    if (a === q.recommended && !answersTouched[i] && q.sort !== "you")
      out.decided.push({ step: q.step, what: q.question, answer: a, because: q.because });
    else out.notes.push(`The owner's answer for step ${q.step}: ${q.question} → ${a}`);
    if (q.sort === "checks" && q.step) out.extraChecks.push({ step: q.step, why: q.question });
    clear(q.step);
  });
  check.risky.forEach((x, i) => {
    const choice = risk[i] ?? recommendedRisk(x.sort);
    if (choice === "wait") {
      out.holds.push({ step: x.step, why: x.why, by: riskTouched[i] ? "you" : "default" });
      return;
    }
    if (choice === "checks") out.extraChecks.push({ step: x.step, why: x.why });
    if (!riskTouched[i])
      out.decided.push({
        step: x.step,
        what: x.why,
        answer: choice === "checks" ? "go ahead with extra checks" : "go ahead",
        because: x.because,
      });
    clear(x.step);
  });
  // A step held for one item stays held whatever another item cleared.
  out.cleared = out.cleared.filter((s) => !out.holds.some((h) => h.step === s));
  return out;
}

/**
 * p2 (owner, 2026-11-20): the check's answers are read by meaning before anything is saved. One short call reads
 * them all together and sorts each: a value (saved as now), an instruction ("find it yourself", "skip", "use the same
 * as qchmmart": tried at once, read-only, and what was found is shown for a one-click yes) or unclear / a question back
 * (not saved: the run starts anyway, the step that needs it waits and asks in the Inbox, the others go on).
 */
export interface TypedAnswer {
  /** "secret:NAME" or "question:<index>". */
  id: string;
  /** What was asked, in plain words. */
  ask: string;
  example: string;
  answer: string;
}

export type ReadingKind = "value" | "instruction" | "unclear";

export interface AnswerReading {
  id: string;
  kind: ReadingKind;
  /** A value: the value itself; an instruction: what Qanuvi found (empty when nothing). */
  value: string;
  /** Where an instruction's value was found. */
  where: string;
  /** The value is another project's known secret (copied by Qanuvi, never shown). */
  copyFrom: { project: string; name: string } | null;
  /** The owner said to leave it ("skip", "you decide"). */
  skip: boolean;
  /** For the owner: the plain answer to their question back, or where Qanuvi looked. */
  reply: string;
}

/** A project the reader may look in for "the same as <project>" (names of its secrets only). */
export interface OtherProject {
  name: string;
  path: string;
  secrets: string[];
}

/**
 * Whether a secret's typed answer is read by a model. One piece without spaces is the value itself and is saved as
 * typed, so a password never reaches a model or a session's record; anything with spaces (a sentence: an instruction
 * or a question back) is read.
 */
export function secretNeedsReading(typed: string): boolean {
  return /\s/.test(typed.trim());
}

function secretAsk(s: PreflightSecret): string {
  return `${s.what || s.why || s.name} (${s.name})`;
}

/** The answers the reading call reads: secrets with a sentence in them, and choices the owner typed or changed. */
export function typedAnswers(
  check: Preflight,
  secretValues: Record<string, string>,
  answers: Record<number, string>,
  answersTouched: Record<number, boolean>,
): TypedAnswer[] {
  const out: TypedAnswer[] = [];
  for (const s of check.secrets) {
    const v = secretValues[s.name]?.trim() ?? "";
    if (v && secretNeedsReading(v))
      out.push({ id: `secret:${s.name}`, ask: secretAsk(s), example: s.example, answer: clipReason(v, 600) });
  }
  check.questions.forEach((q, i) => {
    const a = answers[i]?.trim() ?? "";
    if (a && answersTouched[i] && a !== q.recommended)
      out.push({ id: `question:${i}`, ask: q.question, example: q.recommended, answer: clipReason(a, 600) });
  });
  return out;
}

export function answersPrompt(
  file: string,
  items: TypedAnswer[],
  known: { secrets: string[]; others: OtherProject[] },
): string {
  const list = items.flatMap((x) => [
    `- id: ${x.id}`,
    `  asked: ${x.ask}`,
    ...(x.example ? [`  example: ${x.example}`] : []),
    `  answer: ${JSON.stringify(x.answer)}`,
  ]);
  const others = known.others.slice(0, 20).map((p) => `- ${p.name} (${p.path}): ${p.secrets.join(", ") || "none"}`);
  return [
    `Before a run of ${file}, Qanuvi asked the owner a few things. Read each answer by what it means (any wording,`,
    "spelling or language, short or long), never word by word, and sort it:",
    '- "value": the answer itself (an address, a user name, a choice). Put it in value, without words around it',
    '  ("it is localhost:3306" gives "localhost:3306").',
    '- "instruction": the owner tells Qanuvi what to do instead (find it, look somewhere, use the same as another',
    "  project, leave it, you decide). Carry it out now, read-only and quick (a few looks at most: this project's",
    "  docs, config and example files, or the other projects below): put what you found in value and the file or",
    "  place in where. When it is a known secret of another project, give copyFrom with that project's name and the",
    "  secret's name and leave value empty (Qanuvi copies it; you never see it). skip true only when the owner clearly",
    "  says to go on without it or lets Qanuvi decide.",
    "  Found nothing: value empty, and reply says in one sentence where you looked.",
    '- "unclear": the owner asks back, is unsure, or you cannot tell what they mean. In reply, answer what they asked',
    "  in one or two plain sentences with an example, for someone who does not know code; it is asked again later.",
    "  Not now, later, not known yet, or anything you cannot place is unclear too (the step waits and asks again),",
    "  never skip; when unsure between skip and unclear, answer unclear.",
    "Change nothing. Never guess a password.",
    NEUTRAL_EXAMPLES,
    "",
    `Known secrets of this project (names only): ${known.secrets.join(", ") || "none"}`,
    "Other projects on this machine (their secrets, names only):",
    ...(others.length ? others : ["- none"]),
    "",
    "Answers:",
    ...list,
    "",
    "Answer with JSON only, one entry per id:",
    '{"answers":[{"id":"secret:DB_HOST","kind":"instruction","value":"localhost:3306","where":"config.php","copyFrom":null,"skip":false,"reply":""}]}',
  ].join("\n");
}

/** Reads the reading call's answer: one reading per known id; anything unreadable is left out. */
export function parseAnswerReadings(text: string, ids: string[]): AnswerReading[] {
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end <= start) return [];
  let raw: Record<string, unknown>;
  try {
    raw = JSON.parse(text.slice(start, end + 1));
  } catch {
    return [];
  }
  if (!raw || typeof raw !== "object") return [];
  const str = (v: unknown) => (typeof v === "string" ? clipReason(v.trim()) : "");
  const list = Array.isArray(raw.answers)
    ? (raw.answers as Record<string, unknown>[]).filter((x) => x && typeof x === "object" && !Array.isArray(x))
    : [];
  const out: AnswerReading[] = [];
  for (const r of list) {
    const id = str(r.id);
    if (!ids.includes(id) || out.some((o) => o.id === id)) continue;
    const kind = r.kind === "value" || r.kind === "instruction" || r.kind === "unclear" ? r.kind : null;
    if (!kind) continue;
    const c = r.copyFrom as Record<string, unknown> | null | undefined;
    const copyFrom =
      kind === "instruction" && c && typeof c === "object" && !Array.isArray(c) && str(c.project) && str(c.name)
        ? { project: str(c.project), name: str(c.name) }
        : null;
    out.push({
      id,
      kind,
      value: str(r.value),
      where: kind === "instruction" ? str(r.where) : "",
      copyFrom,
      skip: kind === "instruction" && r.skip === true,
      reply: str(r.reply),
    });
  }
  return out;
}

/** What Qanuvi found for an instruction, offered for a one-click yes. */
export interface FoundValue {
  id: string;
  /** What it is for (the ask). */
  ask: string;
  value: string;
  where: string;
  copyFrom: { project: string; name: string } | null;
  /** A secret: the value is never shown in full. */
  secret: boolean;
}

/** What the read answers come to: secrets to save or copy, the choices' answers, holds, and found values to offer. */
export interface ReadAnswers {
  save: { name: string; value: string }[];
  copy: { name: string; project: string; from: string }[];
  answers: Record<number, string>;
  answersTouched: Record<number, boolean>;
  /** Steps that wait (their question goes to the Inbox), replacing the check's default hold of the same step. */
  holds: { step: string; why: string; by: "default" }[];
  /** Found by an instruction and not yet said yes or no to. */
  offered: FoundValue[];
}

/** "The password ..." mid-sentence reads "the password ..."; a name such as MySQL, API or NOTES_DB stays as written. */
function midSentence(text: string): string {
  return /^[A-Z][a-z]+(?![A-Za-z0-9_])/.test(text) ? text[0].toLowerCase() + text.slice(1) : text;
}

/**
 * The hold's question for an answer that did not give what was needed: it stands on its own in the Inbox. `choice`:
 * an open choice (answered in words), else a login or value (given, or where Qanuvi can find it).
 */
export function unsettledWhy(ask: string, typed: string, reply: string, choice = false): string {
  const said = clipReason(typed, 120);
  const more = reply.replace(/[.\s]+$/, "");
  const end = choice ? "Answer it in your own words" : "Give the value, or say where Qanuvi can find it";
  const need = midSentence(ask);
  return `it needs ${need}${/[.?!]$/.test(need) ? "" : "."} Before the run you wrote "${said}"${more ? `; ${more}` : ""}. ${end}`;
}

/**
 * Applies the readings to the form. `used`: the owner's yes (true) or no (false) to each offered found value; one
 * not answered yet is offered (and counts as no if the run starts anyway). A secret's sentence with no reading (the
 * call failed) is not saved: its step waits and asks. `firstStep` is held for a secret that names no step.
 */
export function readAnswers(
  check: Preflight,
  secretValues: Record<string, string>,
  answers: Record<number, string>,
  answersTouched: Record<number, boolean>,
  readings: AnswerReading[],
  used: Record<string, boolean>,
  firstStep: string,
): ReadAnswers {
  const out: ReadAnswers = {
    save: [],
    copy: [],
    answers: { ...answers },
    answersTouched: { ...answersTouched },
    holds: [],
    offered: [],
  };
  const reading = (id: string) => readings.find((r) => r.id === id);
  // A step held twice (a login and a choice both unsettled) asks both: the second need is added, never dropped.
  const hold = (steps: string[], why: string) => {
    for (const step of steps) {
      if (!step) continue;
      const had = out.holds.find((h) => h.step === step);
      if (!had) out.holds.push({ step, why, by: "default" });
      else if (!had.why.includes(why)) had.why = `${had.why}; also ${why}`;
    }
  };
  for (const s of check.secrets) {
    const v = secretValues[s.name]?.trim() ?? "";
    if (!v) continue;
    if (!secretNeedsReading(v)) {
      out.save.push({ name: s.name, value: v });
      continue;
    }
    const id = `secret:${s.name}`;
    const r = reading(id);
    const steps = s.steps.length ? s.steps : [firstStep];
    const ask = secretAsk(s);
    if (r?.kind === "value") out.save.push({ name: s.name, value: r.value || v });
    else if (r?.kind === "instruction" && r.skip) continue;
    else if (r?.kind === "instruction" && (r.value || r.copyFrom)) {
      if (used[id] === true) {
        if (r.copyFrom) out.copy.push({ name: s.name, project: r.copyFrom.project, from: r.copyFrom.name });
        else out.save.push({ name: s.name, value: r.value });
      } else {
        if (used[id] === undefined)
          out.offered.push({ id, ask, value: r.value, where: r.where, copyFrom: r.copyFrom, secret: true });
        hold(steps, unsettledWhy(ask, v, r.reply));
      }
    } else hold(steps, unsettledWhy(ask, v, r?.reply ?? ""));
  }
  check.questions.forEach((q, i) => {
    const a = answers[i]?.trim() ?? "";
    const id = `question:${i}`;
    if (!a || !answersTouched[i] || a === q.recommended) return;
    const r = reading(id);
    // No reading (no model, or it failed): a one-piece answer ("dark", "1,234.00") stands as typed; a sentence is
    // never taken word by word (it may be "plus tard" or a question back): the step waits and asks again.
    if (!r) {
      if (!secretNeedsReading(a)) return;
      out.answers[i] = "";
      hold([q.step], unsettledWhy(`your answer to: ${q.question}`, a, "", true));
      return;
    }
    if (r.kind === "value") {
      out.answers[i] = r.value || a;
      return;
    }
    if (r.kind === "instruction" && r.skip) {
      // "You decide": the recommended answer, as Qanuvi's own choice; with none, the step waits.
      out.answers[i] = q.recommended;
      out.answersTouched[i] = false;
      return;
    }
    if (r.kind === "instruction" && r.value && used[id] === true) {
      out.answers[i] = r.value;
      return;
    }
    if (r.kind === "instruction" && r.value && used[id] === undefined)
      out.offered.push({ id, ask: q.question, value: r.value, where: r.where, copyFrom: null, secret: false });
    out.answers[i] = "";
    hold([q.step], unsettledWhy(`your answer to: ${q.question}`, a, r.reply, true));
  });
  return out;
}

/**
 * The settled check with the read answers' holds: each replaces Qanuvi's default hold of the same step (its wording
 * says what the owner wrote and what is needed); one the owner set stays theirs and gets that need added. A step held
 * this way is no longer cleared.
 */
export function withReadHolds(settled: SettledCheck, holds: ReadAnswers["holds"]): SettledCheck {
  const mine = new Map(holds.map((h) => [h.step, h.why]));
  const kept = settled.holds
    .filter((h) => h.by === "you" || !mine.has(h.step))
    .map((h) => (mine.has(h.step) ? { ...h, why: `${h.why}; also ${mine.get(h.step)}` } : h));
  return {
    ...settled,
    holds: [...kept, ...holds.filter((h) => !kept.some((k) => k.step === h.step))],
    cleared: settled.cleared.filter((s) => !mine.has(s)),
  };
}

/** A found secret as shown for the yes: its start only, never the whole value. */
export function maskedValue(v: string): string {
  return v.length <= 4 ? "•".repeat(v.length) : `${v.slice(0, 2)}${"•".repeat(Math.min(8, v.length - 2))}`;
}

/**
 * PM a (owner, 2026-11-21): the owner's answers to questions about the whole project (not only this plan) go into This
 * project memory. Only the owner's own answers: typed or changed, accepted with "Accept all recommended", or a question
 * only they could answer; an answer the memory gave and the owner left as it was is in the memory already.
 */
export function projectAnswers(
  check: Preflight,
  answers: Record<number, string>,
  answersTouched: Record<number, boolean>,
): { title: string; body: string }[] {
  return check.questions.flatMap((q, i) => {
    const body = (answers[i] ?? "").trim();
    const owners = answersTouched[i] || (q.sort === "you" && !q.fromMemory);
    if (q.about !== "project" || !body || !owners) return [];
    if (q.fromMemory && !answersTouched[i] && body === q.recommended.trim()) return [];
    return [{ title: clipReason(q.question.trim(), 200), body }];
  });
}

/**
 * pm3 (seen live): the check may ask for a site address as a value to keep (TEST_SITE_URL) rather than as a question.
 * A value that is only a plain web address (no login in it, no key-like query) is a fact about the project, so it is
 * kept in This project memory too; anything else stays a secret only.
 */
export function siteAddressValues(
  secrets: PreflightSecret[],
  saved: { name: string; value: string }[],
): { title: string; body: string; url: string }[] {
  return saved.flatMap((s) => {
    const v = s.value.trim();
    if (!/^https?:\/\/[^\s"'<>]+$/i.test(v)) return [];
    let u: URL;
    try {
      u = new URL(v);
    } catch {
      return [];
    }
    if (u.username || u.password) return [];
    if ([...u.searchParams.keys()].some((k) => /key|token|secret|pass|auth|sig|code/i.test(k))) return [];
    const what = secrets.find((x) => x.name === s.name)?.what.trim() || s.name;
    return [{ title: clipReason(what, 200), body: `${v} (also kept as ${s.name})`, url: v }];
  });
}

/** PM a: the first web address in an answer (a test site fills Preview address when it is empty). */
export function firstUrl(text: string): string | null {
  const m = /https?:\/\/[^\s"'<>()[\]]+/.exec(text);
  return m ? m[0].replace(/[.,;:!?]+$/, "") : null;
}
