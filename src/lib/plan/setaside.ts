/**
 * Runs that keep going (24u part 1): a step that waits for the owner (a hold from the check before the run, blocked,
 * or not finished by any model) is set aside, and so are the steps that need it; the run goes on with the rest and
 * waits, spending nothing, when only set-aside steps are left.
 */

export interface DepStep {
  number: string | null;
  title: string;
  details?: string[];
  needs?: string[];
  /** Files the step changed so far (a failed or blocked step may have started). */
  changed?: string[];
}

/** Why a step waits for the owner. */
export type HeldKind = "hold" | "blocked" | "failed";

/** A section's closing step (such as "95z"): it never waits for others, it does what it can and writes what is open. */
export function isClosingStep(number: string | null): boolean {
  return !!number && /^\d+z$/i.test(number.trim());
}

const FILE =
  /(?:[\w.-]+[/\\])*[\w-]+\.(?:php|ts|tsx|js|jsx|mjs|svelte|vue|rs|css|scss|html|sql|py|go|java|kt|json|toml|ya?ml|sh|ps1)\b/gi;

/** File names a text mentions (lower case, forward slashes). Plan notes (.md) are left out: every step names them. */
export function mentionedFiles(text: string): string[] {
  return [...new Set((text.match(FILE) ?? []).map((f) => f.replace(/\\/g, "/").toLowerCase()))];
}

const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);

/**
 * Whether the plan itself says `later` needs `held`: its Needs line, the same step number (95b after 95a), or a file
 * both touch. Null when the plan does not say; the decider is asked then.
 */
export function planDependence(held: DepStep, later: DepStep): string | null {
  const h = held.number?.trim();
  if (h && (later.needs ?? []).some((n) => n.trim() === h)) return `its Needs line names step ${h}`;
  const l = later.number?.trim();
  if (h && l && h !== l && /^\d+[a-y]$/i.test(h) && /^\d+[a-y]$/i.test(l) && parseInt(h, 10) === parseInt(l, 10))
    return `same step number (${parseInt(h, 10)})`;
  const heldFiles = [...mentionedFiles([held.title, ...(held.details ?? [])].join("\n")), ...(held.changed ?? [])].map(
    (f) => f.replace(/\\/g, "/").toLowerCase(),
  );
  const laterFiles = mentionedFiles([later.title, ...(later.details ?? [])].join("\n"));
  for (const f of laterFiles) {
    const hit = heldFiles.find((g) => g === f || g.endsWith(`/${f}`) || f.endsWith(`/${g}`) || base(g) === base(f));
    if (hit) return `both work on ${f.includes("/") ? f : hit}`;
  }
  return null;
}

const label = (s: DepStep) => (s.number ? `${s.number}. ${s.title}` : s.title);

/** One short question to the decider: which of these open steps need one of the set-aside steps? */
export function dependencePrompt(held: (DepStep & { why: string })[], open: DepStep[]): string {
  return [
    "A plan run set these steps aside (they wait for the owner):",
    ...held.map((s) => `- ${label(s)} (${s.why})`),
    "",
    "Which of these open steps cannot be done properly before one of them is done? A step only needs another when it",
    "uses what that step makes or changes; working on the same project is not enough.",
    ...open.map((s) => `- ${label(s)}${s.details?.length ? `: ${s.details.slice(0, 3).join(" ").slice(0, 300)}` : ""}`),
    "",
    'Answer with JSON only: {"needs":[{"step":"72a","on":"95b","why":"..."}]} (an empty list when none do).',
  ].join("\n");
}

/** Reads the decider's answer: open step number → the set-aside step it needs and why. Unreadable: none. */
export function parseDependence(
  text: string,
  open: string[],
  held: string[],
): Map<string, { on: string; why: string }> {
  const out = new Map<string, { on: string; why: string }>();
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start < 0 || end <= start) return out;
  try {
    const raw = JSON.parse(text.slice(start, end + 1)) as { needs?: unknown };
    if (!Array.isArray(raw.needs)) return out;
    for (const n of raw.needs as Record<string, unknown>[]) {
      const step = typeof n?.step === "string" ? n.step.trim() : "";
      const on = typeof n?.on === "string" ? n.on.trim() : "";
      const why = typeof n?.why === "string" ? n.why.trim().slice(0, 200) : "";
      if (open.includes(step) && held.includes(on) && !out.has(step)) out.set(step, { on, why });
    }
  } catch {
    // Not JSON: nothing is set aside on the decider's word.
  }
  return out;
}

/** The line the run shows while only set-aside steps are left. */
export function waitingLine(answers: number): string {
  return `Waiting for you (${answers} answer${answers === 1 ? "" : "s"}). Nothing is spent meanwhile; the run goes on by itself once you answer.`;
}

/** What a section's closing step is told about the steps set aside in this run. */
export function setAsideBlock(aside: { label: string; why: string }[]): string {
  if (!aside.length) return "";
  return [
    "",
    "",
    "Set aside in this run (they wait for the owner): " + aside.map((a) => `${a.label} (${a.why})`).join("; ") + ".",
    "Do what you can without them, and write in your log what is still open because of them.",
  ].join("\n");
}
