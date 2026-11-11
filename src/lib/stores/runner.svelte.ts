/**
 * Runs a task list step by step (Milestone 9). Each step gets a fresh agent session with only what it needs, on the
 * step's own model; a step that keeps failing moves to a stronger model; the run stops for the user when the strongest
 * model fails, a step is blocked, the budget is reached or the user presses Stop.
 */
import { api, errorText, type AgentEvent, type Project, type Session } from "$lib/api";
import { reduceAll } from "$lib/agent/events";
import {
  addedSteps,
  canCarry,
  contextSize,
  needingCount,
  keepRunSetting,
  reopenedSteps,
  looksSecret,
  notesBlock,
  notesIn,
  privateBlock,
  attemptEntry,
  logPath,
  logsBlock,
  resultEntry,
  loadPlanNotes,
  savePlanNotes,
  stepKey,
} from "$lib/plan/guidance";
import { providerById, type AgentProvider } from "$lib/agents/providers";
import { freeDecide, setupFor } from "./brain.svelte";
import {
  FREE_MODEL,
  isFree,
  nextModel,
  nextModelFreeFirst,
  freeFirstFor,
  ladder,
  paidPerStep,
  runTotals,
  stepOutcome,
  stepPrompt,
  stepsToRun,
  type Attempt,
  type PlanRun,
  type RunOptions,
  type StepRun,
  parseRanges,
  unmetNeed,
  runSummary,
} from "$lib/plan/runner";
import { clearStaleSkips, parsePlan, setStepModel, type Step } from "$lib/plan/tasklist";
import { budgetState, chooseModel, jobType, learn, monthStart, type ModelStats } from "$lib/plan/routing";
import {
  assignPrompt,
  freeWorks,
  parseAssignment,
  parseReview,
  reviewPrompt,
  type Assignment,
  type DecisionRecord,
} from "$lib/core/decider";
import { setStepSkipped, setStepStatus } from "$lib/plan/tasklist";
import { extractSteps, labelRange, lastNumber, renumber, replaceStep, replanPrompt } from "$lib/plan/planner";
import { defaultPlannerModel, planner } from "./planner.svelte";
import { changesSince, snapshotChanges } from "./changes";
import { failureSummary, fixPrompt, locationBlock, mentions, packetPrompt, type Location } from "$lib/plan/packet";

/** Fix rounds in the same session when the test run Qanuvi does fails. */
const MAX_FIX_ROUNDS = 2;
const CHECK_TIMEOUT_SECS = 600;

interface Turn {
  sessionId: string | null;
  last: Session | null;
  outcome: "idle" | "error" | "stopped";
  note: string;
}
import { agent } from "./agent.svelte";
import { settings } from "./settings.svelte";
import { inbox } from "./inbox.svelte";
import { projects } from "./projects.svelte";
import { memoryContext } from "./memory.svelte";

const FINISHED = ["idle", "failed", "ended", "stopped"];

class RunnerStore {
  run = $state<PlanRun | null>(null);
  /** Goes up whenever the task list may have changed, so the Plan view reloads it. */
  changed = $state(0);
  /** Model of the attempt in progress. */
  currentModel = $state("");
  /** Qanuvi is running the project's tests for the current step. */
  checking = $state(false);
  /** Average paid cost of a finished step per kind of job, from earlier runs (to show what free models saved). */
  paidPerStep = $state<Partial<Record<string, number>>>({});
  #stopRequested = false;
  #sessionId: string | null = null;

  get running(): boolean {
    return this.run?.status === "running";
  }

  get totals() {
    return this.run ? runTotals(this.run) : null;
  }

  /** The last run of a project, to show its summary. */
  async loadLast(projectId: string): Promise<void> {
    if (this.running) return;
    try {
      const list = await api.runs.list(projectId);
      // A run may have started while the list was loading; never replace it with an old record.
      if (this.running) return;
      this.run = list.length ? (JSON.parse(list[0].data) as PlanRun) : null;
      if (this.run && list[0].status !== this.run.status) this.run.status = list[0].status as PlanRun["status"];
    } catch {
      if (!this.running) this.run = null;
    }
  }

  async #save(run: PlanRun | null = this.run): Promise<void> {
    const r = run;
    if (!r) return;
    try {
      await api.runs.save(r.projectId, r.id, r.file, r.status, JSON.stringify(r));
    } catch {
      // Keeping the record is helpful, not essential; the run goes on.
    }
  }

  stop(): void {
    this.#stopRequested = true;
    if (this.#sessionId) api.agent.stop(this.#sessionId).catch(() => {});
    // Stop always works, also when the turn's own end never arrives (a lost event, an agent that hangs).
    this.#endTurn?.();
  }

  /** Adds an attempt to the step's log: model, outcome, cost, tests, the failure and the agent's last answer. */
  async #logAttempt(project: Project, file: string, step: StepRun, a: Attempt): Promise<void> {
    const path = logPath(file, step);
    if (!step.attempts.length) {
      const head = await api.files.read(project.id, path).catch(() => null);
      if (!head)
        await api.files
          .logAppend(
            project.id,
            path,
            `# Step ${step.number ? step.number + ". " : ""}${step.title}\n\nPlan: ${file}\n`,
          )
          .catch(() => {});
    }
    const lines = a.sessionId ? await api.agent.events(a.sessionId).catch(() => [] as string[]) : [];
    const texts = reduceAll(lines)
      .filter((i) => i.kind === "text")
      .map((i) => (i as { text: string }).text);
    const entry = attemptEntry({
      when: new Date(),
      model: a.model,
      outcome: a.outcome,
      costUsd: a.costUsd,
      seconds: a.seconds,
      testsPassed: a.testsPassed,
      note: a.note,
      failure: a.failure,
      answer: texts[texts.length - 1] ?? "",
    });
    await api.files.logAppend(project.id, path, entry).catch(() => {});
  }

  /** Adds the step's result (done, blocked, skipped with why) to its log. */
  async #logResult(project: Project, file: string, step: StepRun): Promise<void> {
    if (step.status === "waiting" || step.status === "running") return;
    await api.files.logAppend(project.id, logPath(file, step), resultEntry(step.status, step.reason)).catch(() => {});
  }

  /** Something the owner said during the run: every later step gets it. */
  addNote(text: string): void {
    const run = this.run;
    const note = text.replace(/\s+/g, " ").trim().slice(0, 400);
    if (!run || !note || looksSecret(note)) return;
    run.notes = [...(run.notes ?? []), note];
    savePlanNotes(run.projectId, run.file, run.notes);
    void this.#save(run);
  }

  /** A login or password given during the run: kept for this plan (credential store), for every later step. */
  async addPrivate(projectId: string, file: string, text: string): Promise<void> {
    await api.agent.planPrivateAdd(projectId, file, text).catch(() => {});
    this.changed++;
  }

  removeNote(index: number): void {
    const run = this.run;
    if (!run?.notes) return;
    run.notes = run.notes.filter((_, i) => i !== index);
    savePlanNotes(run.projectId, run.file, run.notes);
    void this.#save(run);
  }

  /** Notes the step's agent left for later steps ("Note for the next steps: ..."). */
  async #collectNotes(run: PlanRun, step: StepRun): Promise<void> {
    const last = step.attempts[step.attempts.length - 1];
    if (!last?.sessionId) return;
    const lines = await api.agent.events(last.sessionId).catch(() => [] as string[]);
    const texts = reduceAll(lines)
      .filter((i) => i.kind === "text")
      .map((i) => (i as { text: string }).text);
    const found = notesIn(texts.slice(-3).join("\n")).filter((n) => !(run.notes ?? []).includes(n));
    if (found.length) {
      run.notes = [...(run.notes ?? []), ...found];
      savePlanNotes(run.projectId, run.file, run.notes);
      await this.#save(run);
    }
  }

  /** Puts steps added to the plan while a step ran right after it in the run, in the plan's order. */
  async #takeAdded(
    project: Project,
    file: string,
    run: PlanRun,
    after: StepRun,
    steps: Step[],
    known: Set<string>,
    reopened: Set<string>,
  ) {
    const now = await api.files.read(project.id, file).catch(() => null);
    if (!now) return;
    const all = parsePlan(now.text).steps;
    const inRun = new Set(run.steps.map(stepKey));
    // Earlier steps the owner sent the run back to come first, then steps added while this one ran.
    const back = reopenedSteps(run.steps, all, reopened);
    for (const s of back) reopened.add(stepKey(s));
    const added = [...back, ...addedSteps(all, known).filter((s) => !inRun.has(stepKey(s)))];
    for (const s of all) known.add(stepKey(s));
    if (!added.length) return;
    steps.push(...added);
    run.steps.splice(
      run.steps.indexOf(after) + 1,
      0,
      ...added.map((s) => ({
        line: s.line,
        number: s.number,
        title: s.title,
        attempts: [],
        status: "waiting" as const,
      })),
    );
    this.changed++;
    await this.#save(run);
  }

  /** The session the next step may continue (same model and section, still small). */
  #carry: { sessionId: string; model: string; section: string; tokens: number; steps: number; base: Base } | null =
    null;
  #lastIdle = false;
  #lastTotals: Base = { cost: 0, input: 0, output: 0, ms: 0 };

  /** After an attempt: keep its session for the next step, or close it. */
  async #settleCarry(attempt: Attempt, section: string, options: RunOptions, continued: boolean): Promise<void> {
    const id = attempt.sessionId;
    if (!id || !this.#lastIdle || options.carryOn === false || attempt.outcome !== "open") {
      if (id && this.#lastIdle) await api.agent.stop(id).catch(() => {});
      this.#carry = null;
      return;
    }
    const lines = await api.agent.events(id).catch(() => [] as string[]);
    this.#carry = {
      sessionId: id,
      model: attempt.model,
      section,
      tokens: contextSize(lines),
      steps: (continued && this.#carry ? this.#carry.steps : 0) + 1,
      base: this.#lastTotals,
    };
  }

  /** Closes the carried session (the run ends, or the next step may not continue it). */
  async #dropCarry(): Promise<void> {
    if (this.#carry) await api.agent.stop(this.#carry.sessionId).catch(() => {});
    this.#carry = null;
  }

  /** A model the user picked for the step running now. */
  #liveModel: string | null = null;

  /** Writes the user's model choice into the step's Model line. */
  async #writeStepModel(project: Project, file: string, step: StepRun, model: string): Promise<void> {
    try {
      const now = await api.files.read(project.id, file);
      const found = parsePlan(now.text).steps.find((x) => x.number === step.number && x.title === step.title);
      if (!found) return;
      await api.files.write(project.id, file, setStepModel(now.text, found.line, model, found.title), now.hash);
      this.changed++;
    } catch {
      // The choice still holds for this run; only the file keeps its old line.
    }
  }

  /** Ends the turn the run is waiting for, as stopped. */
  #endTurn: (() => void) | null = null;

  /** The session of the step running now (a message sent there is guidance for the step). */
  get sessionId(): string | null {
    return this.running ? this.#sessionId : null;
  }

  async start(project: Project, file: string, options: RunOptions): Promise<void> {
    if (this.running) return;
    const provider = providerById(options.providerId || agent.runProviderId);
    const agentSettings = settings.agent(provider.id);
    let text: string;
    try {
      text = (await api.files.read(project.id, file)).text;
    } catch (e) {
      this.run = null;
      throw new Error(errorText(e));
    }
    const { ranges, error: rangeError } = parseRanges(options.ranges);
    if (rangeError) throw new Error(rangeError);
    const pause = options.pause ?? "decisions";
    const steps = stepsToRun(parsePlan(text).steps, options.fromLine, options.onlyOne, ranges);
    // What is done already, to check each step's "- Needs:" line (24f).
    // Skip lines from earlier runs whose needed steps are done now go first (they would say the wrong thing).
    const cleared = clearStaleSkips(text);
    if (cleared !== text) {
      const now = await api.files.read(project.id, file).catch(() => null);
      if (now?.text === text) {
        await api.files.write(project.id, file, cleared, now.hash).catch(() => {});
        text = cleared;
        this.changed++;
      }
    }
    const doneNumbers = new Set(
      parsePlan(text)
        .steps.filter((s) => s.status === "x" && s.number)
        .map((s) => s.number!),
    );
    this.#stopRequested = false;
    this.run = {
      id: crypto.randomUUID(),
      projectId: project.id,
      file,
      provider: provider.id,
      options,
      steps: steps.map((s) => ({ line: s.line, number: s.number, title: s.title, attempts: [], status: "waiting" })),
      status: steps.length ? "running" : "done",
      message: steps.length ? "" : "No open steps to run.",
      startedAt: Date.now(),
      endedAt: steps.length ? null : Date.now(),
    };
    // Keep a reference to this run: the rest of the run works on it, whatever else is shown meanwhile.
    const run = this.run;
    // The plan's notes from earlier runs: the owner's instructions hold for every run of this plan.
    run.notes = loadPlanNotes(project.id, file);
    // The user watching a step can change its mode (for the rest of the run, kept for this plan) and its model (this
    // step, written into its Model line) from the chat pickers.
    let live = null as { step: StepRun; model: string; from: number } | null;
    agent.onPlanChange = (change) => {
      const current = run.steps.find((x) => x.status === "running");
      if (change.mode) {
        run.options.mode = change.mode;
        keepRunSetting(project.id, file, { mode: change.mode });
        void this.#save(run);
      }
      if (change.model && current) {
        // Counted from the next attempt, so a step can still fail over and never retries for ever.
        live = { step: current, model: change.model, from: current.attempts.length + 1 };
        this.#liveModel = change.model;
        void this.#writeStepModel(project, file, current, change.model);
      }
    };
    await this.#save(run);

    // What earlier runs (in every project) showed, and what this month has cost so far.
    const stats = await this.#learned();
    this.paidPerStep = paidPerStep(stats);
    // Free models first only makes sense on a paid agent, with free models set up.
    const team = settings.value.team;
    // The run's own Team choice (Team settings, paid only, or free first) decides; older runs set freeFirst.
    const canFree = provider.id !== "qanuvi" && settings.value.brain.models.length > 0;
    const freeFirst = options.team
      ? freeFirstFor(options.team, freeWorks(team, true, true), canFree)
      : !!options.freeFirst && canFree && freeWorks(team, true, true);
    // One model for every step (the user's choice for this run) wins over the steps' own Model lines.
    const oneModel = options.model?.trim() || "";
    const models = ladder(provider.models, options.strongerOnFail);
    // The decider (the paid agent) says once, for the whole plan, which steps free models do (24b).
    const assignment = new Map<string, Assignment>();
    if (freeFirst && steps.length) {
      const briefs = steps.map((s) => ({
        number: s.number ?? String(s.line),
        title: s.title,
        details: s.details ?? [],
        type: jobType([s.title, s.note ?? "", ...(s.details ?? [])].join("\n")),
      }));
      const priority = team.mode === "priority" ? team.priority : "save";
      let answer = "";
      let deciderBy = provider.id;
      try {
        const model = defaultPlannerModel(provider);
        const r = await planner.ask(project, assignPrompt(briefs, priority), model, "Decider: who does each step", {
          agentId: provider.id,
          lean: true,
        });
        answer = r.text;
        (run.deciderCalls ??= []).push({ model, costUsd: r.costUsd });
      } catch {
        // The paid agent cannot answer now (limit, sign-in, offline): the free models take the seat (24e).
        try {
          const f = await freeDecide(
            "You plan work for coding agents. Answer with JSON only.",
            assignPrompt(briefs, priority),
          );
          answer = f.text;
          deciderBy = `${f.by} (free, ${provider.name} could not answer)`;
        } catch {
          // No answer: Qanuvi core's rules decide (parseAssignment falls back).
        }
      }
      for (const a of parseAssignment(answer, briefs, priority)) {
        assignment.set(a.number, a);
        this.#decide(run, {
          kind: "assign",
          by: answer ? deciderBy : "Qanuvi core",
          subject: `Step ${a.number}`,
          decision: a.worker,
          why: a.why,
        });
      }
      await this.#save(run);
    }
    const spentBefore = (await api.usage.since(monthStart()).catch(() => ({ cost_usd: 0 }))).cost_usd;
    const budget = () => ({
      monthlyUsd: settings.value.monthlyBudgetUsd,
      spentUsd: spentBefore + runTotals(run).costUsd,
    });
    const rules = settings.value.rules[project.id] ?? {};

    const done: { number: string | null; title: string }[] = [];
    // Every step the run knows; one that turns up later was added while it ran (the user's guidance).
    const known = new Set(parsePlan(text).steps.map(stepKey));
    const reopened = new Set<string>();
    for (const stepRun of run.steps) {
      if (run.status !== "running") break;
      // A step whose needs are not done is skipped with the reason, and the run carries on (24f).
      {
        const now = await api.files.read(project.id, file).catch(() => null);
        const nowStep = now
          ? parsePlan(now.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title)
          : undefined;
        const unmet = unmetNeed(nowStep?.needs, doneNumbers, await this.#otherPlans(project, nowStep?.needs));
        if (unmet) {
          stepRun.status = "skipped";
          stepRun.reason = unmet;
          await this.#noteSkipped(project, file, stepRun, unmet);
          await this.#logResult(project, file, stepRun);
          await this.#save(run);
          continue;
        }
      }
      stepRun.status = "running";
      const planned = steps.find((s) => s.line === stepRun.line);
      const type = jobType([stepRun.title, planned?.note ?? "", ...(planned?.details ?? [])].join("\n"));
      const choice = chooseModel({
        stepModel: oneModel || (planned?.model ?? null),
        type,
        rules,
        stats,
        models: provider.models,
        defaultModel: agentSettings.model,
        budget: budget(),
      });
      if (oneModel) choice.reason = "one model for the whole run (your choice)";
      stepRun.jobType = type;
      const first = choice.model;
      // Free first, except money and security work and steps that name their model.
      const assigned = assignment.get(stepRun.number ?? String(stepRun.line));
      const free = freeFirst && type !== "security" && !planned?.model && (assigned?.worker ?? "free") === "free";
      stepRun.choice = free
        ? `free first (decider: ${assigned?.why || "free models can do it"}); then ${first}: ${choice.reason}`
        : freeFirst && assigned?.worker === "paid"
          ? `paid (decider: ${assigned.why}); ${choice.reason}`
          : choice.reason;
      for (;;) {
        if (this.#stopRequested) {
          this.#finish(run, "stopped", "Stopped by you.");
          stepRun.status = "waiting";
          break;
        }
        if (budgetState(budget()) === "over") {
          this.#finish(
            run,
            "needs-you",
            `The monthly budget of $${settings.value.monthlyBudgetUsd.toFixed(2)} is used up.`,
          );
          stepRun.status = "waiting";
          break;
        }
        const spent = runTotals(run).costUsd;
        if (options.budgetUsd > 0 && spent >= options.budgetUsd) {
          this.#finish(run, "stopped", `The run reached its budget of $${options.budgetUsd.toFixed(2)}.`);
          stepRun.status = "waiting";
          break;
        }
        const chosen = live && live.step === stepRun ? live : null;
        const model = chosen
          ? nextModel(models, stepRun.attempts.slice(chosen.from), chosen.model, options.attemptsPerModel)
          : free
            ? nextModelFreeFirst(models, stepRun.attempts, FREE_MODEL, first, options.attemptsPerModel)
            : nextModel(models, stepRun.attempts, first, options.attemptsPerModel);
        if (model === null && options.replan && !stepRun.replacedBy) {
          // Back to the planner, once: rewrite the step into smaller ones and carry on with those.
          const added = await this.#replan(project, file, run, stepRun, provider);
          if (added.length) {
            steps.push(...added);
            break;
          }
        }
        if (model === null) {
          stepRun.status = "failed";
          stepRun.reason = "not finished even by the strongest model";
          if (pause === "never") {
            // Runs to the end: this step is left with its reason; steps that need it skip themselves.
            await this.#noteSkipped(project, file, stepRun, stepRun.reason);
            break;
          }
          this.#finish(
            run,
            "needs-you",
            `Step ${stepRun.number ?? stepRun.title} was not finished even by the strongest model. It needs you.`,
          );
          break;
        }
        // Read the list again: earlier steps or the user may have changed it.
        const current = await api.files.read(project.id, file).catch(() => null);
        const fresh = current
          ? parsePlan(current.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title)
          : undefined;
        if (!fresh || !current) {
          stepRun.status = "skipped";
          break;
        }
        this.currentModel = model;
        const title = `Step ${stepRun.number ? `${stepRun.number}: ` : ""}${stepRun.title}${stepRun.attempts.length ? ` (try ${stepRun.attempts.length + 1})` : ""}`;
        const testCommand = options.packets ? project.settings.test_command.trim() : "";
        // The plan's private details (read each step: one given during the last step counts at once).
        const privateDetails = await api.agent.planPrivate(project.id, file).catch(() => [] as string[]);
        const ownLog = logPath(file, fresh);
        const ownLogExists = !!(await api.files.read(project.id, ownLog).catch(() => null));
        let prompt =
          stepPrompt(file, fresh, done) +
          notesBlock(run.notes ?? []) +
          privateBlock(privateDetails) +
          logsBlock(file, ownLog, ownLogExists);
        // The reviewer turned down a free model's work: the next model hears why.
        const rejected = [...stepRun.attempts].reverse().find((a) => a.note.startsWith("Review: "));
        if (rejected)
          prompt += `\n\nA free model already worked on this step, but the review turned it down: ${rejected.note.slice(8)}\nCheck its changes and finish the step properly.`;
        const before = isFree(model) ? await this.#snapshot(project) : null;
        if (options.packets) prompt = packetPrompt(prompt, await this.#context(project, fresh), testCommand);
        // Same model, same section, still small: the step continues the last step's session (it knows the work).
        const section = fresh.section ?? "";
        const carryOn = canCarry(this.#carry, model, section, options.carryLimit ?? 80_000);
        if (!carryOn) await this.#dropCarry();
        const resume = carryOn && this.#carry ? { sessionId: this.#carry.sessionId, base: this.#carry.base } : null;
        const attempt = await this.#attempt(
          project,
          prompt,
          model,
          options.mode,
          agentSettings,
          title,
          options.lean,
          options.indexTools,
          testCommand,
          resume,
        );
        await this.#settleCarry(attempt, section, options, carryOn);
        await this.#logAttempt(project, file, stepRun, attempt);
        const after = await api.files.read(project.id, file).catch(() => null);
        if (attempt.outcome === "open" && after) {
          const outcome = stepOutcome(after.text, stepRun);
          attempt.outcome = outcome === "missing" ? "open" : outcome;
          // With Qanuvi running the tests, "done" also needs them to pass.
          if (attempt.outcome === "done" && testCommand && attempt.testsPassed === false) {
            attempt.outcome = "open";
            attempt.note = "Marked done, but the tests still fail.";
          }
        }
        // Quality first: a free model's "done" is checked by the decider before it counts.
        if (attempt.outcome === "done" && before && freeFirst) {
          const review = await this.#review(project, provider, stepRun, fresh, before, attempt.testsPassed);
          this.#decide(run, {
            kind: "review",
            by: provider.id,
            subject: `Step ${stepRun.number ?? stepRun.title}`,
            decision: review.ok ? "accepted" : "rejected",
            why: review.why,
          });
          if (!review.ok) {
            attempt.outcome = "open";
            attempt.note = `Review: ${review.why}`;
            const now = await api.files.read(project.id, file).catch(() => null);
            if (now) {
              try {
                await api.files.write(
                  project.id,
                  file,
                  setStepStatus(now.text, fresh.line, " ", fresh.title),
                  now.hash,
                );
              } catch {
                // The list changed meanwhile: the next attempt reads it again.
              }
            }
          }
        }
        stepRun.attempts.push(attempt);
        this.changed++;
        await this.#save(run);
        if (attempt.outcome === "done") {
          stepRun.status = "done";
          done.push({ number: stepRun.number, title: stepRun.title });
          if (stepRun.number) doneNumbers.add(stepRun.number);
          // An earlier run's "Skipped:" line no longer applies.
          await this.#noteSkipped(project, file, stepRun, null);
          if (pause === "step" && run.steps.some((s) => s.status === "waiting"))
            this.#finish(
              run,
              "stopped",
              `Paused after step ${stepRun.number ?? stepRun.title}, as chosen. Run again to go on.`,
            );
          break;
        }
        if (attempt.outcome === "blocked") {
          stepRun.status = "blocked";
          stepRun.reason = attempt.note || (await this.#blockedReason(project, file, stepRun)) || "it needs you";
          // Runs to the end: the agent marked it blocked with its reason; the run carries on.
          if (pause === "never") break;
          this.#finish(run, "needs-you", `Step ${stepRun.number ?? stepRun.title} is blocked and needs you.`);
          break;
        }
        if (attempt.outcome === "stopped") {
          this.#finish(run, "stopped", "Stopped by you.");
          stepRun.status = "waiting";
          break;
        }
      }
      await this.#logResult(project, file, stepRun);
      // Steps added while this one ran (the user asked for them mid-step) come next.
      await this.#takeAdded(project, file, run, stepRun, steps, known, reopened);
      await this.#collectNotes(run, stepRun);
      if (run.status === "running" && (stepRun.status === "blocked" || stepRun.status === "failed") && stepRun.number) {
        const now = await api.files.read(project.id, file).catch(() => null);
        const waiting = new Set(run.steps.filter((x) => x.status === "waiting" && x.number).map((x) => x.number!));
        const held = now ? needingCount(stepRun.number, parsePlan(now.text).steps, waiting) : 0;
        if (held >= 3 && held * 2 >= waiting.size)
          this.#finish(
            run,
            "needs-you",
            `Step ${stepRun.number} is ${stepRun.status === "blocked" ? "blocked" : "not finished"} (${stepRun.reason ?? "it needs you"}). ${held} of the ${waiting.size} steps left need it, so the run stops here instead of skipping them. Sort it out (a missing login goes into Project settings, Secrets), then press Run.`,
          );
      }
    }
    if (run.status === "running") this.#finish(run, "done", runSummary(run));
    await this.#dropCarry();
    // The whole plan is done: its private details are deleted, unless the owner chose to keep them.
    if (options.forgetPrivateWhenDone !== false) {
      const end = await api.files.read(project.id, file).catch(() => null);
      if (end && parsePlan(end.text).steps.every((x) => x.status === "x"))
        await api.agent.planPrivateClear(project.id, file).catch(() => {});
    }
    agent.onPlanChange = null;
    agent.planSessionId = null;
    await this.#save(run);
  }

  /** The "Blocked: ..." line the agent wrote under a step it marked blocked, without the label. */
  async #blockedReason(project: Project, file: string, stepRun: StepRun): Promise<string> {
    const now = await api.files.read(project.id, file).catch(() => null);
    const step = now
      ? parsePlan(now.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title)
      : undefined;
    const line = step?.details.map((d) => d.trim()).find((d) => /^- Blocked:/i.test(d));
    return line ? line.replace(/^- Blocked:\s*/i, "").slice(0, 300) : "";
  }

  /** Writes (or with null removes) the step's "- Skipped:" line with the reason (24f). */
  async #noteSkipped(project: Project, file: string, stepRun: StepRun, reason: string | null): Promise<void> {
    const now = await api.files.read(project.id, file).catch(() => null);
    if (!now) return;
    const step = parsePlan(now.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title);
    if (!step) return;
    try {
      const next = setStepSkipped(now.text, step.line, reason, step.title);
      if (next !== now.text) await api.files.write(project.id, file, next, now.hash);
    } catch {
      // The list changed meanwhile: the reason stays in the run's record.
    }
  }

  /** The finished step numbers of other plans that a step's needs name ("file#number"), by file. */
  async #otherPlans(project: Project, needs: string[] | undefined): Promise<Map<string, Set<string>>> {
    const out = new Map<string, Set<string>>();
    for (const need of needs ?? []) {
      const hash = need.lastIndexOf("#");
      if (hash <= 0) continue;
      const other = need.slice(0, hash);
      if (out.has(other)) continue;
      const f = await api.files.read(project.id, other).catch(() => null);
      if (f)
        out.set(
          other,
          new Set(
            parsePlan(f.text)
              .steps.filter((s) => s.status === "x" && s.number)
              .map((s) => s.number!),
          ),
        );
    }
    return out;
  }

  #decide(run: PlanRun, d: Omit<DecisionRecord, "at">): void {
    (run.decisions ??= []).push({ at: Date.now(), ...d });
  }

  /**
   * The text of the files that differ from the last commit now, to see later what one step changed (earlier steps'
   * changes are not committed yet, so a diff against the commit would show them too).
   */
  async #snapshot(project: Project): Promise<Map<string, string>> {
    return snapshotChanges(project.id);
  }

  /** The decider's look at what a free model changed for a step (diff since the step started, tests). */
  async #review(
    project: Project,
    provider: AgentProvider,
    stepRun: StepRun,
    step: Step,
    before: Map<string, string>,
    testsPassed: boolean | null | undefined,
  ): Promise<{ ok: boolean; why: string }> {
    const file = this.run?.file ?? "";
    const parts = await changesSince(project.id, before, (path) => !!file && path.endsWith(file));
    const tests = testsPassed === true ? "passed" : testsPassed === false ? "failed" : "";
    try {
      // A short look at one step's diff: the agent's own default model, not the planner's bigger one.
      const model = settings.agent(provider.id).model || defaultPlannerModel(provider);
      const { text, costUsd } = await planner.ask(
        project,
        reviewPrompt({ title: step.title, details: step.details ?? [] }, parts.join("\n"), tests),
        model,
        `Decider: review step ${stepRun.number ?? stepRun.title}`,
        { agentId: provider.id, lean: true },
      );
      if (this.run) (this.run.deciderCalls ??= []).push({ model, costUsd });
      return parseReview(text);
    } catch (e) {
      // The paid agent cannot answer now: the free models review instead (24e).
      try {
        const f = await freeDecide(
          "You review code changes. Answer with JSON only.",
          reviewPrompt({ title: step.title, details: step.details ?? [] }, parts.join("\n"), tests),
        );
        const r = parseReview(f.text);
        return { ok: r.ok, why: `${r.why} (reviewed by ${f.by}: ${provider.name} could not answer)` };
      } catch {
        // Nobody could review.
      }
      // Tests decide when there are any, else the step stays accepted with a note.
      return testsPassed === false
        ? { ok: false, why: "tests failed and the review did not answer" }
        : { ok: true, why: `not reviewed (${errorText(e)})` };
    }
  }

  #finish(run: PlanRun, status: PlanRun["status"], message: string): void {
    run.status = status;
    run.message = message;
    run.endedAt = Date.now();
    if (message !== "Stopped by you.") {
      const done = run.steps.filter((s) => s.status === "done").length;
      const name = run.file.split("/").pop() ?? run.file;
      const project = projects.list.find((p) => p.id === run.projectId)?.name ?? "";
      inbox.add({
        kind: "run",
        title: `${project ? `${project}: ` : ""}${name} ${status === "done" ? "finished" : status === "needs-you" ? "needs you" : "stopped"} · ${done}/${run.steps.length} steps done`,
        text: [message, status === "done" ? "" : runSummary(run)].filter(Boolean).join("\n"),
        open: { label: "Open the plan", view: "plan", projectId: run.projectId },
      });
    }
  }

  /**
   * Asks the planner to rewrite a step that failed on every model, writes the new steps after it (the step is marked
   * blocked with a note), and adds them to the run. Returns the new steps, or none when the planner gave none.
   */
  async #replan(
    project: Project,
    file: string,
    run: PlanRun,
    stepRun: StepRun,
    provider: AgentProvider,
  ): Promise<Step[]> {
    const current = await api.files.read(project.id, file).catch(() => null);
    if (!current) return [];
    const step = parsePlan(current.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title);
    if (!step) return [];
    const last = stepRun.attempts[stepRun.attempts.length - 1];
    const what = [
      ...stepRun.attempts.map((a) => `Try on ${a.model}: ${a.outcome}${a.note ? ` (${a.note})` : ""}`),
      last?.failure ? `Last test failures:\n${last.failure}` : "",
    ].join("\n");
    stepRun.replacedBy = "(asking the planner)";
    try {
      const answer = await planner.ask(
        project,
        replanPrompt(file, step, what, provider.models),
        defaultPlannerModel(provider),
        `Planner: rewrite step ${step.number ?? step.title}`,
      );
      const { block } = extractSteps(answer.text);
      if (!block) return [];
      const numbered = step.number
        ? renumber(block, step.number.replace(/[a-z]$/, ""))
        : renumber(block, lastNumber(current.text) + 1);
      const labels = labelRange(numbered);
      const next = replaceStep(current.text, step, numbered, labels);
      await api.files.write(project.id, file, next, current.hash);
      const added = parsePlan(next).steps.filter((s) => parsePlan(numbered).steps.some((n) => n.number === s.number));
      stepRun.replacedBy = labels;
      stepRun.status = "blocked";
      const index = run.steps.indexOf(stepRun);
      run.steps.splice(
        index + 1,
        0,
        ...added.map((s) => ({
          line: s.line,
          number: s.number,
          title: s.title,
          attempts: [],
          status: "waiting" as const,
        })),
      );
      this.changed++;
      await this.#save(run);
      return added;
    } catch {
      stepRun.replacedBy = undefined;
      return [];
    }
  }

  /** Learning from the latest runs of every project. */
  async #learned(): Promise<Map<string, ModelStats>> {
    const records = await api.runs.listAll(300).catch(() => []);
    const runs: PlanRun[] = [];
    for (const r of records) {
      try {
        runs.push(JSON.parse(r.data) as PlanRun);
      } catch {
        // A record that cannot be read teaches nothing.
      }
    }
    return learn(runs, jobType);
  }

  /** Where the code the step mentions is (from the index): one line per definition, outlines of its files. */
  async #context(project: Project, step: Step): Promise<string> {
    const m = mentions([step.title, step.note, ...step.details].join("\n"));
    const defs: Location[] = [];
    for (const name of m.names.slice(0, 8)) {
      const hits = await api.index.find(project.id, name).catch(() => []);
      const exact = hits.filter((h) => h.name === name);
      for (const h of (exact.length ? exact : hits.slice(0, 1)).slice(0, 2)) defs.push(h);
    }
    const outlines: { path: string; items: Omit<Location, "path">[] }[] = [];
    const newFiles: string[] = [];
    for (const path of m.paths.slice(0, 6)) {
      const exists = await api.files.read(project.id, path).then(
        () => true,
        () => false,
      );
      if (!exists) newFiles.push(path);
      else outlines.push({ path, items: await api.index.outline(project.id, path).catch(() => []) });
    }
    return locationBlock(defs, outlines, newFiles);
  }

  /** One turn of an agent session (a new session, or a follow-up in a running one). Resolves when the turn ends. */
  #turn(
    project: Project,
    sessionId: string | null,
    prompt: string,
    model: string,
    mode: string,
    s: { effort: string; baseUrl: string; browserTools: boolean },
    title: string,
    lean: boolean,
    indexTools: boolean,
  ): Promise<Turn> {
    return new Promise((resolve) => {
      let id: string | null = sessionId;
      let last: Session | null = null;
      let settled = false;
      const show = agent.handler(() => id);
      const finish = (outcome: Turn["outcome"], note = "") => {
        if (settled) return;
        settled = true;
        this.#endTurn = null;
        resolve({ sessionId: id, last, outcome, note });
      };
      this.#endTurn = () => finish("stopped");
      const onEvent = (event: AgentEvent) => {
        show(event);
        if (event.kind === "session") {
          last = event.session;
          id = event.session.id;
          const st = event.session.status;
          if (FINISHED.includes(st))
            finish(st === "stopped" ? "stopped" : st === "failed" || st === "ended" ? "error" : "idle");
        } else if (event.kind === "exit") {
          finish(this.#stopRequested ? "stopped" : "error");
        }
      };
      if (!sessionId) agent.watch(null);
      // "qanuvi:auto": the step goes to Qanuvi's own agent, which lets Qanuvi core pick the free model.
      const agentId = isFree(model) ? "qanuvi" : (this.run?.provider ?? agent.runProviderId);
      const agentModel = isFree(model) ? model.slice("qanuvi:".length) : model;
      // A step's first turn gets the relevant part of the user's memory.
      (sessionId ? Promise.resolve(null) : memoryContext(project.id, prompt))
        .then((context) =>
          api.agent.start(
            project.id,
            sessionId,
            prompt,
            agentModel,
            mode,
            s.baseUrl,
            s.effort,
            onEvent,
            lean,
            indexTools,
            s.browserTools,
            context,
            agentId,
            setupFor(agentId),
          ),
        )
        .then((session) => {
          id = session.id;
          this.#sessionId = session.id;
          agent.planSessionId = session.id;
          agent.model = session.model;
          agent.mode = session.mode;
          agent.track(session);
          if (!sessionId) agent.rename(session.id, title);
          agent.watch(session.id);
          if (this.#stopRequested) api.agent.stop(session.id).catch(() => {});
        })
        .catch((e) => finish("error", errorText(e)));
    });
  }

  /**
   * One attempt at a step: a fresh session, and when Qanuvi runs the tests, up to two fix rounds in the same session
   * with only the failures. The session is closed at the end to free its memory.
   */
  async #attempt(
    project: Project,
    prompt: string,
    model: string,
    mode: string,
    s: { effort: string; baseUrl: string; browserTools: boolean },
    title: string,
    lean: boolean,
    indexTools: boolean,
    testCommand: string,
    resume: { sessionId: string; base: Base } | null = null,
  ): Promise<Attempt> {
    let turn = await this.#turn(project, resume?.sessionId ?? null, prompt, model, mode, s, title, lean, indexTools);
    let testRuns = 0;
    let testsPassed: boolean | null = null;
    let failure = "";
    if (testCommand) {
      for (let round = 0; turn.outcome === "idle" && !this.#stopRequested; round++) {
        this.checking = true;
        const check = await api.checks.run(project.id, testCommand, CHECK_TIMEOUT_SECS).catch((e) => ({
          ok: false,
          exit_code: null,
          timed_out: false,
          duration_ms: 0,
          output: errorText(e),
        }));
        this.checking = false;
        testRuns++;
        testsPassed = check.ok;
        if (check.ok || round >= MAX_FIX_ROUNDS) break;
        const summary = check.timed_out
          ? "The tests did not finish within the time limit."
          : failureSummary(check.output);
        failure = summary;
        const follow = fixPrompt(testCommand, summary, round + 1, MAX_FIX_ROUNDS);
        // A model or mode the user picked meanwhile stays (it is not switched back for the fix round).
        turn = await this.#turn(
          project,
          turn.sessionId,
          follow,
          this.#liveModel ?? model,
          this.run?.options.mode ?? mode,
          s,
          title,
          lean,
          indexTools,
        );
      }
    }
    this.#sessionId = null;
    this.#liveModel = null;
    agent.planSessionId = null;
    const last = turn.last as Session | null;
    // The session stays open: the runner decides whether the next step continues it (#settleCarry).
    this.#lastIdle = !!turn.sessionId && last?.status === "idle";
    this.#lastTotals = totalsOf(last);
    const base = resume?.base ?? { cost: 0, input: 0, output: 0, ms: 0 };
    return {
      model,
      sessionId: turn.sessionId ?? "",
      outcome: turn.outcome === "stopped" ? "stopped" : turn.outcome === "error" ? "error" : "open",
      costUsd: Math.max(0, (last?.cost_usd ?? 0) - base.cost),
      inputTokens: Math.max(0, (last?.input_tokens ?? 0) - base.input),
      outputTokens: Math.max(0, (last?.output_tokens ?? 0) - base.output),
      seconds: Math.max(0, (last?.duration_ms ?? 0) - base.ms) / 1000,
      note: turn.note || (testsPassed === false ? "Tests failed." : ""),
      testRuns,
      testsPassed,
      failure: testsPassed === false ? failure || failureSummary("") : undefined,
    };
  }
}

export const runner = new RunnerStore();

/** A session's running totals, to count each step's own share when steps continue one session. */
interface Base {
  cost: number;
  input: number;
  output: number;
  ms: number;
}
function totalsOf(s: Session | null): Base {
  return {
    cost: s?.cost_usd ?? 0,
    input: s?.input_tokens ?? 0,
    output: s?.output_tokens ?? 0,
    ms: s?.duration_ms ?? 0,
  };
}

export type { StepRun };
