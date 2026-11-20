/**
 * Run rules (24y Y1 + Y2): what a plan run may do while nobody is there, kept per project in its settings (the core
 * answers a run's command asks by them; see src-tauri/src/run_rules.rs). The window shows them as the card at the top
 * of "Before the run": in full until answered once, then as one line.
 */

export interface RunRules {
  /** The card was answered once. */
  set: boolean;
  /** Keep a plan's work on its own git branch (false: Qanuvi's own copies, nothing goes into git). */
  branch: boolean;
  /** More command patterns the run may use ("make test", "php artisan *"). */
  allow: string[];
  /** Command patterns the run never uses, besides the fixed never-list. */
  never: string[];
}

export const DEFAULT_RULES: RunRules = { set: false, branch: false, allow: [], never: [] };

/** The project's rules, filled in where an older record lacks them. */
export function rulesOf(settings: { run_rules?: Partial<RunRules> } | undefined): RunRules {
  const r = settings?.run_rules ?? {};
  return {
    set: !!r.set,
    branch: !!r.branch,
    allow: Array.isArray(r.allow) ? r.allow.filter((p) => typeof p === "string") : [],
    never: Array.isArray(r.never) ? r.never.filter((p) => typeof p === "string") : [],
  };
}

/** Patterns typed one per line: trimmed, empty lines and repeats left out. */
export function parsePatterns(text: string): string[] {
  const out: string[] = [];
  for (const line of text.split(/\r?\n/)) {
    const p = line.trim().replace(/^[-*]\s+/, "");
    if (p && !out.includes(p)) out.push(p);
  }
  return out;
}

/** What the rules say, for the check before the run (a question they answer is decided, not asked). */
export function rulesFacts(r: RunRules): string {
  const lines = [
    "Run rules for this project (standing answers: a question they answer is decided, not asked):",
    "- The run may change files in this project and run its tests, checks and builds.",
    "- It never publishes, deletes outside the project, uses passwords, pushes, or changes the main branch.",
    r.branch
      ? "- The plan's work is kept on its own git branch."
      : "- Nothing goes into git; Qanuvi keeps its own copies to undo a step.",
  ];
  if (r.allow.length) lines.push(`- Also allowed: ${r.allow.join("; ")}`);
  if (r.never.length) lines.push(`- Never: ${r.never.join("; ")}`);
  return lines.join("\n");
}

/** The one line shown once the card was answered. */
export function rulesLine(r: RunRules, git: boolean): string {
  const parts = ["tests and builds may run", "never publishes or touches main"];
  if (git) parts.push(r.branch ? "own git branch" : "Qanuvi's own copies");
  const extra = r.allow.length + r.never.length;
  if (extra) parts.push(`${extra} rule${extra === 1 ? "" : "s"} of your own`);
  return parts.join(" · ");
}
