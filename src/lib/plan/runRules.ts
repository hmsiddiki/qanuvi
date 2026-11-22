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
  /** wd1: folders outside the project the owner allowed a step to write into (kept by the core from an answer). */
  write_dirs?: string[];
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
    // wd1: kept as they are, so saving the card never drops a folder the owner allowed in an answer.
    ...(Array.isArray(r.write_dirs) && r.write_dirs.length
      ? { write_dirs: r.write_dirs.filter((p) => typeof p === "string") }
      : {}),
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

/**
 * What the rules say, for the check before the run (a question they answer is decided, not asked). In Auto the user
 * gave the agent room: what is not listed is its judgement, only the never-lists hold.
 */
export function rulesFacts(r: RunRules, auto = false): string {
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
  lines.push(
    auto
      ? "- Permissions are Auto: any other command the agent judges safe may run; nothing needs to be allowed first."
      : "- Any other command is refused while nobody is there: its step is set aside with the command named.",
  );
  return lines.join("\n");
}

/**
 * y9-3: what "Check: test build" means, for the check before the run (it once asked which command a test build is).
 * `planOwn`: the plan has its own "Test build" section, which wins for that plan.
 */
export function testBuildFact(
  recipe: { build: string; start: string; check: string } | undefined,
  planOwn: boolean,
): string {
  const what =
    'A step line "Check: test build" means Qanuvi itself test-builds the app after the step, into its own folder and on fresh data, never over the installed app';
  if (planOwn) return `${what}, with the plan's own "Test build" section. Nothing needs to be asked about it.`;
  const set = (s: string) => s.trim() || "none";
  if (recipe && (recipe.build.trim() || recipe.check.trim()))
    return `${what}, with the project's recipe (build: ${set(recipe.build)}; start: ${set(recipe.start)}; check: ${set(recipe.check)}). Nothing needs to be asked about it.`;
  return `${what}, with the project's test build recipe (Project settings, Test build). This project has none yet: a step that asks for one is set aside until it is set.`;
}

/** The one line shown once the card was answered. */
export function rulesLine(r: RunRules, git: boolean, auto = false): string {
  const parts = [
    auto ? "Auto: the agent judges the rest" : "tests and builds may run",
    "never publishes or touches main",
  ];
  if (git) parts.push(r.branch ? "own git branch" : "Qanuvi's own copies");
  const extra = r.allow.length + r.never.length;
  if (extra) parts.push(`${extra} rule${extra === 1 ? "" : "s"} of your own`);
  return parts.join(" · ");
}
