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

export function preflightPrompt(
  file: string,
  steps: PreflightStep[],
  known: { secrets: string[]; testCommand: string; previewUrl: string; notes: string[] },
): string {
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
    "",
    "Open steps:",
    list,
    "",
    "Answer with JSON only, in this shape (empty lists when nothing is needed):",
    '{"secrets":[{"name":"ENV_NAME","what":"what it is, in plain words","example":"an example value, never a real password","ifEmpty":"what happens if it is left empty","why":"...","steps":["82e"]}],',
    ' "questions":[{"step":"83c","question":"a choice the steps leave open","sort":"decide","recommended":"the answer you advise","because":"..."}],',
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
