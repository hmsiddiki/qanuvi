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

export interface Preflight {
  secrets: { name: string; why: string; steps: string[] }[];
  questions: { step: string; question: string }[];
  risky: { step: string; why: string }[];
  setup: { what: string; why: string }[];
}

export const EMPTY_PREFLIGHT: Preflight = { secrets: [], questions: [], risky: [], setup: [] };

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
    '{"secrets":[{"name":"ENV_NAME","why":"...","steps":["82e"]}],',
    ' "questions":[{"step":"83c","question":"a decision only the owner can make"}],',
    ' "risky":[{"step":"84a","why":"deletes data / migrates / publishes / live payments / pushes"}],',
    ' "setup":[{"what":"something that must run or be set","why":"..."}]}',
    "Rules: secrets only for logins, keys and passwords not already known (use the names the code or tests read);",
    "questions only for real choices the steps leave open, not for things you can decide from the code; risky only",
    "for steps that are hard to undo or reach outside this computer. At most 8 items in each list.",
  ].join("\n");
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
  const str = (v: unknown) => (typeof v === "string" ? v.trim().slice(0, 300) : "");
  const okName = (n: string) => /^[A-Za-z_][A-Za-z0-9_]{0,99}$/.test(n);
  return {
    secrets: arr(raw.secrets)
      .map((s) => ({
        name: str(s.name),
        why: str(s.why),
        steps: Array.isArray(s.steps) ? (s.steps as unknown[]).map(str).filter(Boolean).slice(0, 20) : [],
      }))
      .filter((s) => okName(s.name))
      .slice(0, 8),
    questions: arr(raw.questions)
      .map((q) => ({ step: str(q.step), question: str(q.question) }))
      .filter((q) => q.question)
      .slice(0, 8),
    risky: arr(raw.risky)
      .map((r) => ({ step: str(r.step), why: str(r.why) }))
      .filter((r) => r.step)
      .slice(0, 8),
    setup: arr(raw.setup)
      .map((x) => ({ what: str(x.what), why: str(x.why) }))
      .filter((x) => x.what)
      .slice(0, 8),
  };
}

/** Whether the run pauses before this step (a risky step to wait at, or a question left open). */
export function waitsBefore(step: string | null, wait: { step: string; why: string }[] | undefined): string | null {
  if (!step || !wait?.length) return null;
  return wait.find((w) => w.step === step)?.why ?? null;
}
