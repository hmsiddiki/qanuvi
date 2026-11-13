/**
 * Runs a task list step by step (Milestone 9). Each step gets a fresh agent session with only what it needs, on the
 * step's own model; a step that keeps failing moves to a stronger model; the run stops for the user when the strongest
 * model fails, a step is blocked, the budget is reached or the user presses Stop.
 */
import { api, errorText, type AgentEvent, type Project, type Session } from "$lib/api";
import { reduceAll } from "$lib/agent/events";
import { waitsBefore } from "$lib/plan/preflight";
import {
  closingRan,
  closingsDue,
  holdWording,
  keepAnswer,
  loadPlanState,
  markLate,
  pruneState,
  savePlanState,
} from "$lib/plan/planstate";
import {
  closingRerunNote,
  dependencePrompt,
  isClosingStep,
  parseDependence,
  planDependence,
  setAsideBlock,
  waitingLine,
  type HeldKind,
} from "$lib/plan/setaside";
import { isLimit, limitResetAt } from "$lib/plan/limits";
import { overLimit, toolKinds } from "$lib/plan/costs";
import {
  addedSteps,
  canCarry,
  contextSize,
  HANDOVER_DEFAULT,
  HANDOVER_RESUME,
  SWITCH_RESUME,
  handoverDue,
  handoverRequest,
  modelSwitchRequest,
  keepRunSetting,
  reopenedSteps,
  looksSecret,
  notesBlock,
  notesIn,
  privateBlock,
  attemptEntry,
  logPath,
  logsBlock,
  knowledgeBlock,
  referencedBlock,
  referencedPlans,
  logDir,
  mapPath,
  factsPath,
  apiPath,
  apiSection,
  mergeApi,
  isCode,
  resultEntry,
  loadPlanNotes,
  savePlanNotes,
  stepKey,
} from "$lib/plan/guidance";
import { modelLabel, providerById, type AgentProvider } from "$lib/agents/providers";
import { freeDecide, setupFor } from "./brain.svelte";
import {
  FREE_MODEL,
  isFree,
  nextModel,
  nextModelFreeFirst,
  freeFirstFor,
  freeWantedButMissing,
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
import { archivePath, doneNumbers as doneIn } from "$lib/plan/housekeeping";
import {
  budgetState,
  chooseModel,
  jobType,
  learn,
  modelProfiles,
  monthStart,
  type JobType,
  type ModelStats,
} from "$lib/plan/routing";
import { loadPicks, pickFor, savePicks, type SavedPicks } from "$lib/plan/picks";
import {
  assignPrompt,
  freeWorks,
  parseAssignment,
  parsePicks,
  pickPrompt,
  type Profiles,
  type StepBrief,
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
import {
  doneNotes,
  finishEntry,
  finishFullPrompt,
  finishReviewPrompt,
  isPlanFile,
  offersFinish,
  parseFinishReview,
  stillOpenIn,
  type DoneBrief,
  type FinishLevel,
  type FinishPass,
} from "$lib/plan/finishing";

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
/** After "Stop this command, read my message now" on a step: how long the runner waits for the agent to go on. */
const INTERRUPT_GRACE_MS = 30_000;

class RunnerStore {
  run = $state<PlanRun | null>(null);
  /** Goes up whenever the task list may have changed, so the Plan view reloads it. */
  changed = $state(0);
  /** Model of the attempt in progress. */
  currentModel = $state("");
  /** Qanuvi is running the project's tests for the current step. */
  checking = $state(false);
  /** The run waits for a usage limit to reset (shown in the run panel). */
  waitingUntil = $state<Date | null>(null);
  /** Free models work until this time (the paid agent's limit resets then). */
  #freeUntil: Date | null = null;
  /** Average paid cost of a finished step per kind of job, from earlier runs (to show what free models saved). */
  paidPerStep = $state<Partial<Record<string, number>>>({});
  #stopRequested = false;
  #sessionId: string | null = null;

  get running(): boolean {
    return this.run?.status === "running";
  }

  /** Only set-aside steps are left: the run waits (spending nothing) for this many answers (24u). */
  waitingAnswers = $state(0);
  /** Wakes a run waiting for answers (an answer, Finish without them, Pause or Stop). */
  #wake: (() => void) | null = null;
  /** The owner answered while the run went on: it goes back to the released steps after the step in hand. */
  #answered = false;
  /** The owner chose to end a waiting run without the answers. */
  #finishWithout = false;

  /** The step's key in set-aside notes: its number, else its title. */
  #key(s: StepRun): string {
    return s.number ?? s.title;
  }

  /**
   * The owner answers a held step: "go" lets it run (with the answer, if any, told to it), "skip" leaves it and the
   * steps waiting for it. Works while the run goes on (picked up after the step in hand) and while it waits.
   */
  answer(step: StepRun, choice: "go" | "skip", text = ""): void {
    const run = this.run;
    if (!run || !this.running || step.status !== "held") return;
    const key = this.#key(step);
    const waiting = run.steps.filter((x) => x.status === "set-aside" && x.waitsFor?.step === key);
    // Kept for this plan: a run started again later does not ask again (24u part 2).
    savePlanState(
      run.projectId,
      run.file,
      keepAnswer(loadPlanState(run.projectId, run.file), key, choice, text.trim()),
    );
    if (choice === "skip") {
      step.status = "skipped";
      step.reason = "you chose to skip it";
      for (const x of waiting) {
        x.status = "skipped";
        x.reason = `needs ${key}, which you skipped`;
      }
    } else {
      step.status = "waiting";
      step.answer = text.trim();
      step.retryFrom = step.attempts.length;
      for (const x of waiting) {
        x.status = "waiting";
        x.waitsFor = undefined;
      }
    }
    this.#answered = true;
    this.changed++;
    this.#wake?.();
  }

  /** Ends a run that waits for answers: what is set aside stays open, the summary lists it. */
  finishWithout(): void {
    if (!this.waitingAnswers) return;
    this.#finishWithout = true;
    this.#wake?.();
  }

  get totals() {
    return this.run ? runTotals(this.run) : null;
  }

  /** The finishing pass over a finished run is at work (24o). */
  finishing = $state(false);
  /** What differed from the last commit when the run started, so the pass sees only the run's own changes. */
  #runBefore: { id: string; snap: Map<string, string> } | null = null;
  /** The agent a turn goes to when it is not the run's own (the finishing pass runs on the paid decider). */
  #agentOverride: string | null = null;

  /** The last run of a project, to show its summary. */
  async loadLast(projectId: string): Promise<void> {
    if (this.running || this.finishing) return;
    try {
      const list = await api.runs.list(projectId);
      // A run may have started while the list was loading; never replace it with an old record.
      if (this.running || this.finishing) return;
      this.run = list.length ? (JSON.parse(list[0].data) as PlanRun) : null;
      if (this.run && list[0].status !== this.run.status) this.run.status = list[0].status as PlanRun["status"];
      // A pass that was at work when Qanuvi closed did not finish.
      if (this.run?.finish?.status === "running") {
        this.run.finish.status = "stopped";
        this.run.finish.note = "Qanuvi closed while it ran.";
      }
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

  /** Pause asked for: the run stops after the step it is on (or at once while it waits for a usage limit). */
  pausing = $state(false);

  /** Pauses after the current step; Resume opens the run's settings to change and go on. */
  /**
   * The owner picked a model for a step of the running plan. The step running now hands over to it (its agent
   * finishes the edit in hand and writes a progress note); any other step just starts on it when its turn comes (the
   * runner reads each step's Model line when it starts). Returns whether the running step is switching.
   */
  switchStepModel(projectId: string, file: string, number: string | null, title: string, model: string): boolean {
    const r = this.run;
    if (!this.running || !r || r.projectId !== projectId || r.file !== file) return false;
    const current = r.steps.find((x) => x.status === "running");
    if (!current || current.number !== number || current.title !== title) return false;
    this.#switchStep?.(model);
    return true;
  }

  pause(): void {
    if (!this.running || this.pausing) return;
    this.pausing = true;
    // Waiting for answers: it pauses at once (nothing is running).
    this.#wake?.();
    const run = this.run;
    const step = run?.steps.find((s) => s.status === "running");
    if (!run || !step || !this.#sendMore) return;
    // Now, not after the whole step: the agent ends the edit it is making and writes where it is.
    this.#pauseNow = true;
    const log = logPath(run.file, step);
    this.#sendMore(
      `(Qanuvi: the owner pauses the run now. Finish only the edit you are making; do not start anything new. Then add a short "Progress" note to ${log}: what is done, what is left, which files you touched and anything half-way. Then stop and answer "Paused". When the run resumes, this same session goes on from here.)`,
    );
    // An agent that does not stop is stopped (its session is kept all the same).
    setTimeout(() => {
      if (this.#pauseNow && this.#endTurn) {
        if (this.#sessionId) api.agent.stop(this.#sessionId).catch(() => {});
        this.#endTurn();
      }
    }, 180_000);
  }

  /**
   * Handover (24p): the running step's context is too big; its agent was asked to write a progress note and stop, and
   * a new session goes on with the step. Holds the context size that set it off.
   */
  #handover = 0;
  /**
   * The model the owner switched the running step to; the handover that follows is for that, not for size. Shown in the
   * chat footer ("switching after the edit in hand") until the new session starts.
   */
  switchingTo = $state<string | null>(null);
  /** Switches the running step to a model (chat picker or the step's model in the Plan view); set while a run goes. */
  #switchStep: ((model: string) => void) | null = null;
  /** Handovers in the current step. */
  #handovers = 0;
  /** The handover limit of the run going now (tokens; 0 = never). */
  #handoverLimit = HANDOVER_DEFAULT;
  /** The step log of the step running now (where its agent writes the progress note). */
  #stepLog = "";

  /** The step's agent is pausing now (not at the end of the step). */
  #pauseNow = false;
  /** Sends a message to the step session running now (set while a turn runs). */
  #sendMore: ((text: string) => void) | null = null;

  /** Stop asked while a step works: the agent finishes the edit it is making first (a second Stop is at once). */
  stopping = $state(false);

  stop(): void {
    // Gently first: the edit in hand is finished, so no file is left half-written.
    if (!this.stopping && this.#sendMore && this.#sessionId && !this.#stopRequested) {
      this.stopping = true;
      this.#stopRequested = true;
      this.#sendMore(
        '(Qanuvi: the owner stops the run. Finish only the edit you are making, start nothing new, then stop and answer "Stopped".)',
      );
      setTimeout(() => {
        if (this.stopping) this.#stopNow();
      }, 60_000);
      return;
    }
    this.#stopNow();
  }

  /** Stops at once: the agent and the turn the run waits for. */
  #stopNow(): void {
    this.#stopRequested = true;
    this.stopping = false;
    this.#wake?.();
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
    a.tools = toolKinds(lines);
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
    const files = step.changed?.length ? `Files changed: ${step.changed.join(", ")}\n` : "";
    await api.files
      .logAppend(project.id, logPath(file, step), resultEntry(step.status, step.reason) + files)
      .catch(() => {});
  }

  /**
   * The plan's last run, after a restart (or another plan shown): so the button says Resume and a run paused in a
   * step goes on in that step's session. A session that cannot be opened again gives a new one, which reads the
   * step's progress note in its log.
   */
  async restoreLast(project: Project, file: string): Promise<void> {
    if (this.running || (this.run && this.run.projectId === project.id && this.run.file === file)) return;
    const records = await api.runs.list(project.id).catch(() => []);
    const last = records.filter((r) => r.file === file).sort((a, b) => b.updated_at - a.updated_at)[0];
    if (!last || this.running) return;
    let run: PlanRun;
    try {
      run = JSON.parse(last.data);
    } catch {
      return;
    }
    if (run.status === "running") run.status = "stopped";
    for (const s of run.steps) if (s.status === "running") s.status = "waiting";
    this.run = run;
    const p = run.pausedSession;
    if (run.status === "paused" && p) {
      const s = agent.sessions.find((x) => x.id === p.sessionId) ?? null;
      this.#carry = {
        sessionId: p.sessionId,
        model: p.model,
        section: p.section,
        tokens: 0,
        steps: 1,
        base: totalsOf(s),
      };
      this.#resumeText =
        "You paused this step at the owner's request and the run now resumes: go on from where you stopped (your Progress note is in the step's log).";
    }
  }

  /** The step session is kept for Resume (the run was paused in the middle of a step). */
  #keepForResume = false;

  /** The resume text is Qanuvi's handover note, not the owner's words. */
  #resumeIsHandover = false;

  /** The owner's message that resumed the run (it goes first in the step's request). */
  #resumeText = "";

  /**
   * The owner wrote in the session of a step the last run stopped at (blocked, failed or stopped): the run goes on from
   * that step, in the same session, with the owner's words first, then carries on with the next steps as it was set.
   * Answers false when there is no such run, and the message goes to the chat as usual.
   */
  async resumeFrom(project: Project, sessionId: string, text: string): Promise<boolean> {
    const last = this.run;
    if (!last || this.running || last.projectId !== project.id) return false;
    const session = agent.sessions.find((x) => x.id === sessionId);
    const number = session?.title.match(/^Step ([^:\s]+):/)?.[1];
    if (!number) return false;
    const now = await api.files.read(project.id, last.file).catch(() => null);
    const step = now ? parsePlan(now.text).steps.find((x) => x.number === number) : undefined;
    if (!step || step.status === "x") return false;
    this.#carry = {
      sessionId,
      model: session?.model ?? "",
      section: step.section ?? "",
      tokens: 0,
      steps: 0,
      base: totalsOf(session ?? null),
    };
    this.#resumeText = text;
    // A blocked step is open again: the owner is unblocking it.
    if (step.status === "!" && now)
      await api.files
        .write(project.id, last.file, setStepStatus(now.text, step.line, " ", step.title), now.hash)
        .catch(() => {});
    void this.start(project, last.file, {
      ...last.options,
      fromLine: step.line,
      onlyOne: false,
      ranges: "",
      // The session keeps its model for this step; later steps go back to their own.
      model: last.options.model,
    });
    return true;
  }

  /** API.md: what the plan added, from Qanuvi's code index for the files the step changed (no model tokens). */
  async #updateApi(project: Project, file: string, step: StepRun): Promise<void> {
    const code = (step.changed ?? []).filter(isCode).slice(0, 30);
    if (!code.length) return;
    const updates = new Map<string, string>();
    for (const p of code) {
      const exists = await api.files.read(project.id, p).then(
        () => true,
        () => false,
      );
      updates.set(p, exists ? apiSection(p, await api.index.outline(project.id, p).catch(() => [])) : "");
    }
    const path = apiPath(file);
    const now = await api.files.read(project.id, path).catch(() => null);
    await api.files.write(project.id, path, mergeApi(now?.text ?? "", updates), now?.hash ?? null).catch(() => {});
  }

  /** Something the owner said during the run: every later step gets it. */
  addNote(text: string): void {
    const run = this.run;
    const note = text.replace(/\s+/g, " ").trim().slice(0, 400);
    if (!run || !note || looksSecret(note)) return;
    run.notes = [...(run.notes ?? []), note];
    void this.#save(run);
    void api.files.logAppend(run.projectId, factsPath(run.file), `- (the owner) ${note}\n`).catch(() => {});
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
      await this.#save(run);
      await api.files
        .logAppend(run.projectId, factsPath(run.file), found.map((n) => `- ${n}\n`).join(""))
        .catch(() => {});
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

  /** The step's limits and what it had spent before this attempt (the watchdog's view). */
  #limits: { usd?: number; minutes?: number } = {};
  #stepSpentBefore = 0;
  #attemptBase = 0;
  #stepStartedAt = Date.now();
  #overLimit: string | null = null;

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

  /** When the usage limit this attempt met resets, or null when it met none. */
  async #limitReset(a: Attempt): Promise<Date | null> {
    if (!a.sessionId) return null;
    const lines = await api.agent.events(a.sessionId).catch(() => [] as string[]);
    for (const l of lines.slice(-8).reverse()) {
      if (!isLimit(l)) continue;
      let text = l;
      try {
        const m = JSON.parse(l);
        text = String(m.result ?? m.text ?? m.message?.content?.[0]?.text ?? l);
      } catch {
        // The raw line.
      }
      const at = limitResetAt(text);
      if (at) return at;
    }
    return null;
  }

  /** Waits until the time (Stop ends it); answers false when the owner stopped the run. */
  async #waitUntil(at: Date): Promise<boolean> {
    this.waitingUntil = at;
    while (Date.now() < at.getTime()) {
      if (this.#stopRequested || this.pausing) {
        this.waitingUntil = null;
        return false;
      }
      await new Promise((r) => setTimeout(r, 5000));
    }
    this.waitingUntil = null;
    return !this.#stopRequested;
  }

  /** Ends the turn the run is waiting for, as stopped. */
  #endTurn: (() => void) | null = null;

  /** The session of the step running now (a message sent there is guidance for the step). */
  get sessionId(): string | null {
    return this.running ? this.#sessionId : null;
  }

  async start(project: Project, file: string, options: RunOptions): Promise<void> {
    if (this.running || this.finishing) return;
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
    // What is done already, to check each step's "- Needs:" line (24f); steps moved to the plan's archive (24j) count.
    const archived = doneIn((await api.files.read(project.id, archivePath(file)).catch(() => null))?.text);
    // Skip lines from earlier runs whose needed steps are done now go first (they would say the wrong thing).
    const cleared = clearStaleSkips(text, archived);
    if (cleared !== text) {
      const now = await api.files.read(project.id, file).catch(() => null);
      if (now?.text === text) {
        await api.files.write(project.id, file, cleared, now.hash).catch(() => {});
        text = cleared;
        this.changed++;
      }
    }
    const doneNumbers = new Set([
      ...archived,
      ...parsePlan(text)
        .steps.filter((s) => s.status === "x" && s.number)
        .map((s) => s.number!),
    ]);
    this.#stopRequested = false;
    this.pausing = false;
    this.stopping = false;
    this.#pauseNow = false;
    this.#answered = false;
    this.#finishWithout = false;
    this.waitingAnswers = 0;
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
    // The finishing pass compares with this, so changes from before the run are not taken for the run's (24o).
    this.#runBefore = steps.length ? { id: run.id, snap: await snapshotChanges(project.id) } : null;
    // The plan's notes from earlier runs: the owner's instructions hold for every run of this plan.
    run.notes = [];
    {
      const kept = loadPlanNotes(project.id, file);
      if (kept.length) {
        await api.files.logAppend(project.id, factsPath(file), kept.map((n) => `- ${n}\n`).join("")).catch(() => {});
        savePlanNotes(project.id, file, []);
      }
    }
    // What this plan kept from earlier runs (24u part 2): holds come back with who set them, answered steps run with
    // the owner's answer, skipped ones stay skipped. Entries of steps no longer open go.
    const waitList = [...(options.waitBefore ?? [])];
    {
      const planned = parsePlan(text).steps;
      const open = new Set(planned.filter((s) => s.status !== "x").map((s) => s.number ?? s.title));
      const kept = pruneState(loadPlanState(project.id, file), open, new Set(planned.map((s) => s.number ?? s.title)));
      savePlanState(project.id, file, kept);
      for (const h of kept.holds)
        if (!waitList.some((w) => w.step === h.step)) waitList.push({ step: h.step, why: holdWording(h) });
      for (const a of kept.answers) {
        const stepRun = run.steps.find((x) => this.#key(x) === a.step);
        if (!stepRun) continue;
        if (a.choice === "skip") {
          stepRun.status = "skipped";
          stepRun.reason = "you chose to skip it (in an earlier run)";
        } else {
          stepRun.answer = a.text;
          stepRun.held = { kind: "hold", why: "it was held in an earlier run" };
        }
      }
    }
    // The user watching a step can change its mode (for the rest of the run, kept for this plan) and its model (this
    // step, written into its Model line) from the chat pickers.
    let live = null as { step: StepRun; model: string; from: number } | null;
    // The step running now goes to another model: counted from the next attempt (so a step can still fail over and
    // never retries for ever); the agent at work finishes its edit and writes a progress note, then the chosen model
    // goes on in a new session. The model the step is already on changes nothing.
    const switchStep = (current: StepRun, model: string) => {
      void this.#writeStepModel(project, file, current, model);
      if (model === (this.#liveModel ?? this.currentModel)) return;
      live = { step: current, model, from: current.attempts.length + 1 };
      this.#liveModel = model;
      if (this.#sendMore && this.#stepLog && !this.#handover && !this.#pauseNow && !this.#stopRequested) {
        this.switchingTo = model;
        this.#handover = 1;
        this.#sendMore(modelSwitchRequest(this.#stepLog, modelLabel(provider, model)));
        agent.notice(
          `Step ${current.number ?? current.title}: ${modelLabel(provider, model)} takes over after the edit in hand.`,
        );
        // An agent that does not stop is stopped; the new session checks whatever it left.
        setTimeout(() => {
          if (this.switchingTo === model && this.#handover && this.#endTurn) {
            if (this.#sessionId) api.agent.stop(this.#sessionId).catch(() => {});
            this.#endTurn();
          }
        }, 180_000);
      }
    };
    this.#switchStep = (model) => {
      const current = run.steps.find((x) => x.status === "running");
      if (current) switchStep(current, model);
    };
    agent.onPlanChange = (change) => {
      const current = run.steps.find((x) => x.status === "running");
      if (change.mode) {
        run.options.mode = change.mode;
        keepRunSetting(project.id, file, { mode: change.mode });
        void this.#save(run);
      }
      if (change.model && current) switchStep(current, change.model);
    };
    await this.#save(run);

    // What earlier runs (in every project) showed, and what this month has cost so far.
    const stats = await this.#learned();
    this.paidPerStep = paidPerStep(stats);
    // Free models first only makes sense on a paid agent, with free models set up.
    // This run's Team mode, else the one in Settings.
    const team = options.teamMode ? { ...settings.value.team, mode: options.teamMode } : settings.value.team;
    // The run's own Team choice (Team settings, paid only, or free first) decides; older runs set freeFirst.
    const canFree = provider.id !== "qanuvi" && settings.value.brain.models.length > 0;
    const freeFirst = options.team
      ? freeFirstFor(options.team, freeWorks(team, true, true), canFree)
      : !!options.freeFirst && canFree && freeWorks(team, true, true);
    // The Team choice wanted free models but none are set up: say once that every step goes to the paid agent.
    if (
      freeWantedButMissing(options.team, team.mode, settings.value.brain.models.length > 0, provider.id === "qanuvi")
    ) {
      this.#decide(run, {
        kind: "assign",
        by: "Qanuvi",
        subject: "Team",
        decision: `No free models set up: every step on ${provider.name}`,
        why: "The Team mode works with free models, and Settings, Models and providers has none yet.",
      });
    }
    // One model for every step (the user's choice for this run) wins over the steps' own Model lines.
    const oneModel = options.model?.trim() || "";
    const models = ladder(provider.models, options.strongerOnFail);
    // With free models first, the decider (the paid agent) says once, for the whole plan, which steps they do (24b).
    // Models: only the picks the owner asked for before the run ("Pick models"); a run never picks them by itself.
    const assignment = new Map<string, Assignment>();
    if (steps.length) {
      const briefs = steps.map(briefOf);
      if (freeFirst) {
        const priority = team.mode === "priority" ? team.priority : "save";
        const { list, by } = await this.#askPicks(
          project,
          provider,
          briefs,
          { priority, freeFirst, workersOnly: true },
          stats,
          run,
        );
        for (const a of list) {
          assignment.set(a.number, a);
          this.#decide(run, { kind: "assign", by, subject: `Step ${a.number}`, decision: a.worker, why: a.why });
        }
      }
      const saved = oneModel ? null : loadPicks(localStorage, project.id, file);
      for (const b of briefs) {
        const p = pickFor(saved, b, provider.id);
        if (!p?.model) continue;
        const a = assignment.get(b.number);
        assignment.set(b.number, {
          number: b.number,
          worker: a?.worker ?? "paid",
          why: p.why,
          model: p.model,
        });
        this.#decide(run, {
          kind: "assign",
          by: "decider (Pick models, before the run)",
          subject: `Step ${b.number}`,
          decision: modelLabel(provider, p.model),
          why: p.why,
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
    // Plans this one builds on (needed or named) whose knowledge exists: every step is told where it is.
    const planRefs: string[] = [];
    for (const p of referencedPlans(text, file)) {
      const has = await api.files.read(project.id, `${logDir(p)}/MAP.md`).then(
        () => true,
        () =>
          api.files.read(project.id, `${logDir(p)}/FACTS.md`).then(
            () => true,
            () => false,
          ),
      );
      if (has) planRefs.push(p);
    }
    // Held and failed steps are set aside, not the end of the run (24u): the run goes on with the rest, then waits for
    // the owner's answers and goes back to the steps they release.
    let waitedOnce = false;
    // Finish without them was chosen: what is left to do before the run ends is the closing steps' catch-up.
    let ending = false;
    for (;;) {
      let again = false;
      for (const stepRun of run.steps) {
        if (run.status !== "running") break;
        if (stepRun.status !== "waiting") continue;
        // A step whose needs are not done is skipped with the reason, and the run carries on (24f).
        let freshModel: string | null | undefined;
        {
          const now = await api.files.read(project.id, file).catch(() => null);
          const nowStep = now
            ? parsePlan(now.text).steps.find((s) => s.number === stepRun.number && s.title === stepRun.title)
            : undefined;
          if (now)
            for (const x of parsePlan(now.text).steps) if (x.status === "x" && x.number) doneNumbers.add(x.number);
          // The step's Model line as it is now: the owner may have changed it while earlier steps ran.
          if (nowStep) freshModel = nowStep.model;
          const unmet = unmetNeed(nowStep?.needs, doneNumbers, await this.#otherPlans(project, nowStep?.needs));
          const asideNeed = run.steps.find(
            (x) => (x.status === "held" || x.status === "set-aside") && x.number && nowStep?.needs?.includes(x.number),
          );
          if (unmet && asideNeed) {
            stepRun.status = "set-aside";
            stepRun.waitsFor = {
              step:
                asideNeed.status === "set-aside" && asideNeed.waitsFor ? asideNeed.waitsFor.step : this.#key(asideNeed),
              why: `its Needs line names step ${asideNeed.number}`,
            };
            await this.#save(run);
            continue;
          }
          if (unmet) {
            stepRun.status = "skipped";
            stepRun.reason = unmet;
            await this.#noteSkipped(project, file, stepRun, unmet);
            await this.#logResult(project, file, stepRun);
            await this.#save(run);
            continue;
          }
        }
        // A step the owner chose to see first (risky, or a question left open in the check before the run). The step
        // the run starts at is held too: the owner left that answer open just before pressing Start.
        const holdWhy = waitsBefore(stepRun.number, waitList);
        if (holdWhy && stepRun.answer === undefined) {
          await this.#setAside(project, file, run, stepRun, "hold", holdWhy, provider, steps);
          continue;
        }
        stepRun.status = "running";
        // Released by the owner's answer: what it was held for and what they said go into its next try (24u).
        const released = stepRun.held;
        let answerNote = "";
        if (released) {
          stepRun.held = undefined;
          const said = stepRun.answer ? ` The owner answered: "${stepRun.answer}".` : "";
          answerNote =
            released.kind === "hold"
              ? `\n\nThis step was held for the owner (${released.why}); they said go ahead.${said}`
              : `\n\nEarlier tries did not finish this step (${released.why}).${said} Try it again with that in mind.`;
          // A blocked step is open again in the plan, so its next try can mark it done.
          if (released.kind === "blocked") {
            const now = await api.files.read(project.id, file).catch(() => null);
            const s = now
              ? parsePlan(now.text).steps.find((x) => x.line === stepRun.line && x.title === stepRun.title)
              : null;
            if (now && s?.status === "!")
              await api.files
                .write(project.id, file, setStepStatus(now.text, s.line, " ", s.title), now.hash)
                .catch(() => {});
          }
        }
        // A section's closing step hears what is set aside, does what it can and writes what is still open.
        const rerun = stepRun.rerunAfter;
        stepRun.rerunAfter = undefined;
        const asideNote = isClosingStep(stepRun.number)
          ? (rerun?.length ? closingRerunNote(rerun) : "") +
            setAsideBlock(
              run.steps
                .filter((x) => x.status === "held" || x.status === "set-aside")
                .map((x) => ({
                  label: this.#key(x),
                  why: x.status === "held" ? (x.held?.why ?? "it needs the owner") : `waits for ${x.waitsFor?.step}`,
                })),
            )
          : "";
        const planned = steps.find((s) => s.line === stepRun.line);
        const type = jobType([stepRun.title, planned?.note ?? "", ...(planned?.details ?? [])].join("\n"));
        const assigned = assignment.get(stepRun.number ?? String(stepRun.line));
        const choice = chooseModel({
          picked: assigned?.model ? { model: assigned.model, why: assigned.why } : null,
          stepModel: oneModel || (freshModel !== undefined ? freshModel : (planned?.model ?? null)),
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
        const free =
          freeFirst &&
          type !== "security" &&
          !(freshModel !== undefined ? freshModel : planned?.model) &&
          (assigned?.worker ?? "free") === "free";
        stepRun.choice = free
          ? `free first (decider: ${assigned?.why || "free models can do it"}); then ${first}: ${choice.reason}`
          : freeFirst && assigned?.worker === "paid"
            ? choice.reason.startsWith("decider:")
              ? `paid; ${choice.reason}`
              : `paid (decider: ${assigned.why}); ${choice.reason}`
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
          const tries = stepRun.attempts.slice(stepRun.retryFrom ?? 0);
          // The paid agent's limit is not reset yet and the owner chose free models meanwhile.
          const freeNow = !!this.#freeUntil && Date.now() < this.#freeUntil.getTime() && canFree && !isFree(first);
          const model = freeNow
            ? FREE_MODEL
            : chosen
              ? nextModel(models, stepRun.attempts.slice(chosen.from), chosen.model, options.attemptsPerModel)
              : free
                ? nextModelFreeFirst(models, tries, FREE_MODEL, first, options.attemptsPerModel)
                : nextModel(models, tries, first, options.attemptsPerModel);
          if (model === null && options.replan && !stepRun.replacedBy && stepRun.retryFrom === undefined) {
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
            // Set aside after its result is logged; the run goes on with the steps that do not need it (24u).
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
          // Done by hand (or by an agent in the chat) since the run started: it counts as done, nobody runs it again.
          if (fresh.status === "x" && !stepRun.attempts.length) {
            stepRun.status = "done";
            stepRun.reason = "marked done outside the run";
            done.push({ number: stepRun.number, title: stepRun.title });
            if (stepRun.number) doneNumbers.add(stepRun.number);
            break;
          }
          this.currentModel = model;
          const title = `Step ${stepRun.number ? `${stepRun.number}: ` : ""}${stepRun.title}${stepRun.attempts.length ? ` (try ${stepRun.attempts.length + 1})` : ""}`;
          // The full suite after every step, or only at the last step of a section (the others test what they changed).
          const lastOfSection = !run.steps
            .slice(run.steps.indexOf(stepRun) + 1)
            .some((x) => x.status === "waiting" && steps.find((s) => s.line === x.line)?.section === fresh.section);
          const fullNow = options.fullChecks !== "section" || lastOfSection;
          const testCommand = options.packets && fullNow ? project.settings.test_command.trim() : "";
          // The plan's private details (read each step: one given during the last step counts at once).
          const privateDetails = await api.agent.planPrivate(project.id, file).catch(() => [] as string[]);
          const ownLog = logPath(file, fresh);
          const ownLogExists = !!(await api.files.read(project.id, ownLog).catch(() => null));
          const resumedWith = this.#resumeText;
          const handedOver = this.#resumeIsHandover;
          this.#resumeText = "";
          this.#resumeIsHandover = false;
          // The plan's shared knowledge first (the same for steps in a row, so it is read from the cache), then the step.
          const read = async (p: string) => (await api.files.read(project.id, p).catch(() => null))?.text ?? "";
          let prompt =
            knowledgeBlock(
              file,
              await read(mapPath(file)),
              await read(factsPath(file)),
              run.steps
                .filter((x) => x !== stepRun && x.changed?.length)
                .map((x) => ({ number: x.number, files: x.changed ?? [] })),
              options.indexTools,
            ) +
            referencedBlock(planRefs) +
            "\n" +
            (handedOver
              ? `${resumedWith}\n\n`
              : resumedWith
                ? `The owner continued this step in the chat and wrote: "${resumedWith}". Take it into account first.\n\n`
                : "") +
            stepPrompt(file, fresh, done) +
            answerNote +
            asideNote +
            privateBlock(privateDetails) +
            logsBlock(file, ownLog, ownLogExists) +
            (fullNow
              ? ""
              : "\n\nChecks: run only the tests for what this step changes (and lint the files you touch). The full suite " +
                "and the plan's other whole-project checks run once, at the last step of this section; skip them here, " +
                "even where the plan's rules ask for them after every step.");
          // The reviewer turned down a free model's work: the next model hears why.
          const rejected = [...stepRun.attempts].reverse().find((a) => a.note.startsWith("Review: "));
          if (rejected)
            prompt += `\n\nA free model already worked on this step, but the review turned it down: ${rejected.note.slice(8)}\nCheck its changes and finish the step properly.`;
          const before = isFree(model) ? await this.#snapshot(project) : null;
          if (options.packets) prompt = packetPrompt(prompt, await this.#context(project, fresh, file), testCommand);
          // Same model, same section, still small: the step continues the last step's session (it knows the work).
          const section = fresh.section ?? "";
          const carryOn =
            (!!resumedWith && !handedOver && !!this.#carry) ||
            canCarry(
              this.#carry,
              model,
              // Across sections too (steps of a plan build on each other), unless the owner chose per section.
              options.carryAcrossSections === false ? section : (this.#carry?.section ?? section),
              options.carryLimit ?? 150_000,
            );
          if (!carryOn) await this.#dropCarry();
          const resume = carryOn && this.#carry ? { sessionId: this.#carry.sessionId, base: this.#carry.base } : null;
          const stepStarted = Date.now() - 2000;
          // The step's limits: what earlier attempts of it spent counts too.
          if (!stepRun.attempts.length) {
            this.#stepStartedAt = Date.now();
            this.#handovers = 0;
          }
          this.#handover = 0;
          this.#stepLog = ownLog;
          this.#handoverLimit = options.handoverLimit ?? HANDOVER_DEFAULT;
          this.#stepSpentBefore = stepRun.attempts.reduce((t, a) => t + a.costUsd, 0);
          this.#attemptBase = resume?.base.cost ?? 0;
          this.#limits = { usd: options.stepCostLimit, minutes: options.stepMinutes };
          this.#overLimit = null;
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
          // What the step changed, for the next steps (and its log).
          const changed = await api.files.changedSince(project.id, stepStarted).catch(() => [] as string[]);
          stepRun.changed = [...new Set([...(stepRun.changed ?? []), ...changed])];
          await this.#logAttempt(project, file, stepRun, attempt);
          // Handed over (context too big): no failure; unless the step got done, a new session goes on from the note.
          // A step marked done goes through the usual checks below (tests, review) like any other.
          if (this.#handover && !this.#pauseNow && !this.#overLimit && !this.#stopRequested) {
            const size = this.#handover;
            const switchTo = this.switchingTo;
            this.#handover = 0;
            this.switchingTo = null;
            const now = await api.files.read(project.id, file).catch(() => null);
            if (now && stepOutcome(now.text, stepRun) === "open" && switchTo) {
              // The owner's switch: no failed try, and not counted against the handover limit.
              attempt.outcome = "stopped";
              attempt.note = `handed over to ${switchTo} (the owner's choice)`;
              stepRun.attempts.push(attempt);
              await this.#dropCarry();
              if (attempt.sessionId) await api.agent.stop(attempt.sessionId).catch(() => {});
              this.#resumeText = SWITCH_RESUME;
              this.#resumeIsHandover = true;
              continue;
            }
            if (now && stepOutcome(now.text, stepRun) === "open" && !switchTo) {
              this.#handovers++;
              attempt.outcome = "stopped";
              attempt.note = `handed over to a new session at about ${Math.round(size / 1000)}k tokens of context`;
              stepRun.attempts.push(attempt);
              await this.#dropCarry();
              if (attempt.sessionId) await api.agent.stop(attempt.sessionId).catch(() => {});
              this.#resumeText = HANDOVER_RESUME;
              this.#resumeIsHandover = true;
              agent.notice(
                `Step ${stepRun.number ?? stepRun.title}: the context reached about ${Math.round(size / 1000)}k tokens, so a new session takes over from the progress note.`,
              );
              continue;
            }
          }
          // A usage limit is no failure: wait for the reset (or let free models work), then try the step again.
          // Paused now by the owner: the attempt is no failure, the session is kept, Resume goes on in it.
          if (this.#pauseNow) {
            this.#pauseNow = false;
            attempt.outcome = "stopped";
            attempt.note = "paused by the owner";
            stepRun.attempts.push(attempt);
            stepRun.status = "waiting";
            if (attempt.sessionId && (!this.#carry || this.#carry.sessionId !== attempt.sessionId))
              this.#carry = {
                sessionId: attempt.sessionId,
                model: attempt.model,
                section,
                tokens: 0,
                steps: 1,
                base: this.#lastTotals,
              };
            this.#keepForResume = true;
            // Saved with the run, so Resume finds the session after a restart (or a new one reads the progress note).
            if (attempt.sessionId)
              run.pausedSession = {
                sessionId: attempt.sessionId,
                model: attempt.model,
                section,
                number: stepRun.number,
              };
            this.#resumeText =
              "You paused this step at the owner's request and the run now resumes: go on from where you stopped (your Progress note is in the step's log).";
            this.#finish(
              run,
              "paused",
              `Paused in step ${stepRun.number ?? stepRun.title}. Resume goes on in the same session (you can change the run's settings first).`,
            );
            break;
          }
          // Over the step's limits: the run pauses and says so (Resume to go on, maybe with other limits or a model).
          if (this.#overLimit) {
            const why = this.#overLimit;
            this.#overLimit = null;
            await this.#dropCarry();
            stepRun.attempts.push(attempt);
            stepRun.status = "waiting";
            this.#finish(
              run,
              "paused",
              `Step ${stepRun.number ?? stepRun.title} was paused: ${why}. Resume to go on (raise the limit or choose another model).`,
            );
            break;
          }
          if (attempt.outcome === "error" || attempt.outcome === "open") {
            const resetAt = await this.#limitReset(attempt);
            if (resetAt) {
              await this.#dropCarry();
              if (options.onLimit === "free" && canFree) {
                this.#freeUntil = resetAt;
                agent.notice(
                  `The paid agent hit its limit (resets ${resetAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}): free models work until then.`,
                );
                continue;
              }
              if (!(await this.#waitUntil(resetAt))) {
                if (this.pausing)
                  this.#finish(
                    run,
                    "paused",
                    "Paused while waiting for the usage limit. Resume when you want to go on.",
                  );
                else this.#finish(run, "stopped", "Stopped by you while waiting for the usage limit to reset.");
                stepRun.status = "waiting";
                break;
              }
              continue;
            }
          }
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
            // Set aside after its result is logged; the run carries on (24u).
            break;
          }
          if (attempt.outcome === "stopped") {
            this.#finish(run, "stopped", "Stopped by you.");
            stepRun.status = "waiting";
            break;
          }
        }
        await this.#logResult(project, file, stepRun);
        // Paused by the owner: the run stops here, after a whole step; Resume goes on from the next open one.
        if (this.pausing && run.status === "running") {
          this.#finish(
            run,
            "paused",
            `Paused after step ${stepRun.number ?? stepRun.title}. Resume to go on (you can change the run's settings first).`,
          );
        }
        // Steps added while this one ran (the user asked for them mid-step) come next.
        await this.#takeAdded(project, file, run, stepRun, steps, known, reopened);
        await this.#collectNotes(run, stepRun);
        await this.#updateApi(project, file, stepRun);
        if (run.status === "running" && (stepRun.status === "blocked" || stepRun.status === "failed")) {
          const kind = stepRun.status;
          await this.#setAside(project, file, run, stepRun, kind, stepRun.reason ?? "it needs you", provider, steps);
        }
        // Finished after its section's closing step already ran (it was set aside): the closing step runs once more,
        // once the rest of the section is through.
        if (stepRun.status === "done" && run.status === "running") await this.#afterDone(project, file, run, stepRun);
        // Answered while the run went on: back to the released steps first (they come earlier in the plan).
        if (this.#answered && run.status === "running") {
          this.#answered = false;
          again = true;
          break;
        }
      }
      if (again) continue;
      if (run.status !== "running") break;
      // A step opened again behind the pass (a closing step's catch-up) gets its turn first.
      if (run.steps.some((x) => x.status === "waiting")) continue;
      const asks = run.steps.filter((x) => x.status === "held").length;
      if (!asks || ending) {
        // About to end: closing steps that missed late steps run once now, then the run ends.
        if (await this.#runClosings(project, file, run, true)) continue;
        break;
      }
      // Only set-aside steps are left: wait for the owner, spending nothing, and go on by itself once answered.
      this.waitingAnswers = asks;
      run.message = waitingLine(asks);
      if (!waitedOnce) {
        waitedOnce = true;
        const name = file.split("/").pop() ?? file;
        inbox.add({
          kind: "run",
          title: `${project.name}: ${name} waits for you · ${asks} answer${asks === 1 ? "" : "s"}`,
          text: run.steps
            .filter((x) => x.status === "held")
            .map((x) => `Step ${this.#key(x)}: ${x.held?.why ?? "it needs you"}`)
            .join("\n"),
          open: { label: "Open the plan", view: "plan", projectId: project.id },
        });
      }
      this.changed++;
      await this.#save(run);
      if (!this.#answered && !this.#finishWithout && !this.#stopRequested && !this.pausing)
        await new Promise<void>((resolve) => (this.#wake = resolve));
      this.#wake = null;
      this.waitingAnswers = 0;
      this.#answered = false;
      if (this.#stopRequested) {
        this.#finish(run, "stopped", "Stopped by you.");
        break;
      }
      if (this.pausing) {
        this.#finish(run, "paused", "Paused while waiting for your answers. Resume to go on.");
        break;
      }
      if (this.#finishWithout) {
        this.#finishWithout = false;
        ending = true;
      }
    }
    if (run.status === "running") this.#finish(run, "done", runSummary(run));
    this.#sendMore = null;
    this.stopping = false;
    // A run paused in a step keeps that step's session for Resume.
    if (!(run.status === "paused" && this.#keepForResume)) await this.#dropCarry();
    this.#keepForResume = false;
    // The whole plan is done: its private details are deleted, unless the owner chose to keep them.
    if (options.forgetPrivateWhenDone !== false) {
      const end = await api.files.read(project.id, file).catch(() => null);
      if (end && parsePlan(end.text).steps.every((x) => x.status === "x"))
        await api.agent.planPrivateClear(project.id, file).catch(() => {});
    }
    agent.onPlanChange = null;
    this.#switchStep = null;
    agent.planSessionId = null;
    await this.#save(run);
    // The owner chose to run the finishing pass by itself at the end (24o).
    if (options.finishPass && options.finishPass !== "ask" && offersFinish(run))
      await this.finishPass(project, options.finishPass);
  }

  /**
   * The finishing pass over a finished run (24o): the project's full checks, then the paid decider's review of the
   * whole run's diff; the full pass also has a session fix what was found, run live tests where the project defines
   * them and check the Done notes, ending with a "still open" list. Its cost counts in the run's.
   */
  async finishPass(project: Project, level: FinishLevel): Promise<void> {
    const run = this.run;
    if (!run || this.running || this.finishing || run.projectId !== project.id) return;
    // The paid agent on top decides and monitors (24e): intelligence paid for once, where it catches most.
    const provider = providerById(agent.runProviderId);
    const model = defaultPlannerModel(provider);
    const pass: FinishPass = {
      level,
      status: "running",
      model,
      startedAt: Date.now(),
      endedAt: null,
      costUsd: 0,
      seconds: 0,
      checks: null,
      findings: [],
      stillOpen: [],
    };
    this.finishing = true;
    this.#stopRequested = false;
    this.stopping = false;
    run.finish = pass;
    this.changed++;
    await this.#save(run);
    try {
      const command = project.settings.test_command.trim();
      if (command) {
        this.checking = true;
        const c = await api.checks.run(project.id, command, CHECK_TIMEOUT_SECS).catch((e) => ({
          ok: false,
          exit_code: null,
          timed_out: false,
          duration_ms: 0,
          output: errorText(e),
        }));
        this.checking = false;
        pass.checks = {
          command,
          ok: c.ok,
          summary: c.ok
            ? ""
            : c.timed_out
              ? "The checks did not finish within the time limit."
              : failureSummary(c.output),
        };
        await this.#save(run);
      }
      if (this.#stopRequested) throw new Error("stopped");
      const { diff, files } = await this.#runDiff(project, run);
      const briefs = await this.#doneBriefs(project, run);
      const prompt = finishReviewPrompt(run.file, briefs, diff, pass.checks);
      let review: { ok: boolean; findings: string[]; stillOpen: string[] };
      try {
        const r = await planner.ask(project, prompt, model, "Finishing pass: review the whole run", {
          agentId: provider.id,
          lean: true,
        });
        pass.costUsd += r.costUsd;
        review = parseFinishReview(r.text);
      } catch (e) {
        // The paid agent cannot answer now: the free models review instead, and the report says so.
        try {
          const f = await freeDecide("You review code changes. Answer with JSON only.", prompt);
          review = parseFinishReview(f.text);
          review.findings.push(`(Reviewed by ${f.by}: ${provider.name} could not answer: ${errorText(e)})`);
        } catch {
          review = { ok: false, findings: [`The review did not answer (${errorText(e)}).`], stillOpen: [] };
        }
      }
      if (pass.checks && !pass.checks.ok)
        review.findings.unshift(`The full checks fail: ${pass.checks.summary.slice(0, 300)}`);
      pass.findings = review.findings;
      pass.stillOpen = review.stillOpen;
      await this.#save(run);
      if (level === "full" && !this.#stopRequested) {
        // A session of its own: no handover, the run's own per-step limits.
        this.#limits = { usd: run.options.stepCostLimit, minutes: run.options.stepMinutes };
        this.#stepStartedAt = Date.now();
        this.#stepSpentBefore = 0;
        this.#attemptBase = 0;
        this.#overLimit = null;
        this.#handover = 0;
        this.#handoverLimit = 0;
        this.#stepLog = "";
        this.#agentOverride = provider.id;
        this.currentModel = model;
        const a = await this.#attempt(
          project,
          finishFullPrompt(run.file, briefs, review, pass.checks, files),
          model,
          run.options.mode,
          settings.agent(provider.id),
          "Finishing pass",
          run.options.lean,
          run.options.indexTools,
          command,
        );
        pass.sessionId = a.sessionId || undefined;
        pass.costUsd += a.costUsd;
        if (command && typeof a.testsPassed === "boolean")
          pass.checks = { command, ok: a.testsPassed, summary: a.testsPassed ? "" : (a.failure ?? "") };
        let answer = "";
        if (a.sessionId) {
          const lines = await api.agent.events(a.sessionId).catch(() => [] as string[]);
          answer = reduceAll(lines)
            .filter((i) => i.kind === "text")
            .map((i) => (i as { text: string }).text)
            .slice(-2)
            .join("\n");
          if (this.#lastIdle) await api.agent.stop(a.sessionId).catch(() => {});
        }
        const open = stillOpenIn(answer);
        pass.findings = [
          ...review.findings.map((f) => `Review: ${f}`),
          command
            ? `After the pass the full checks ${a.testsPassed === false ? "still fail" : "pass"}.`
            : "No test command: the pass could not run the checks itself.",
        ];
        pass.stillOpen = open ?? [...review.stillOpen, "The pass's answer had no Still open list: read its session."];
        if (a.outcome === "stopped" || this.#overLimit) {
          pass.status = "stopped";
          pass.note = this.#overLimit ?? "Stopped by you.";
        } else if (a.outcome === "error") {
          pass.status = "failed";
          pass.note = a.note || "The session ended with an error.";
        }
      }
      if (pass.status === "running") pass.status = this.#stopRequested ? "stopped" : "done";
    } catch (e) {
      pass.status = this.#stopRequested ? "stopped" : "failed";
      pass.note = pass.status === "failed" ? errorText(e) : "Stopped by you.";
    } finally {
      this.finishing = false;
      this.checking = false;
      this.stopping = false;
      this.#agentOverride = null;
      this.#limits = {};
      this.currentModel = "";
      agent.planSessionId = null;
      pass.endedAt = Date.now();
      pass.seconds = (pass.endedAt - pass.startedAt) / 1000;
      this.changed++;
      await this.#save(run);
      await api.files.logAppend(project.id, `${logDir(run.file)}/FINISH.md`, finishEntry(pass)).catch(() => {});
      const name = run.file.split("/").pop() ?? run.file;
      inbox.add({
        kind: "run",
        title: `${project.name ? `${project.name}: ` : ""}${name} finishing pass ${pass.status} · ${pass.stillOpen.length} still open`,
        text: [...pass.findings.slice(0, 6), ...pass.stillOpen.map((x) => `Still open: ${x}`)].join("\n"),
        open: { label: "Open the plan", view: "plan", projectId: run.projectId },
      });
    }
  }

  /** The run's own changes as one diff (the task list and its logs left out), and the files it changed. */
  async #runDiff(project: Project, run: PlanRun): Promise<{ diff: string; files: string[] }> {
    const dir = logDir(run.file);
    const skip = (path: string) => isPlanFile(path, run.file, dir);
    const files = [...new Set(run.steps.flatMap((s) => s.changed ?? []))].filter((f) => !skip(f));
    const parts =
      this.#runBefore?.id === run.id
        ? await changesSince(project.id, this.#runBefore.snap, skip)
        : // After a restart the run's start is not known: the files' diffs against the last commit.
          await Promise.all(files.slice(0, 40).map((f) => api.git.diff(project.id, f).catch(() => "")));
    return { diff: parts.filter(Boolean).join("\n"), files };
  }

  /** The steps the run finished, with the Done notes they left in the task list. */
  async #doneBriefs(project: Project, run: PlanRun): Promise<DoneBrief[]> {
    const now = await api.files.read(project.id, run.file).catch(() => null);
    const steps = now ? parsePlan(now.text).steps : [];
    return run.steps
      .filter((s) => s.status === "done")
      .map((s) => {
        const found = steps.find((x) => x.number === s.number && x.title === s.title);
        return { number: s.number, title: s.title, done: doneNotes(found?.details ?? []) };
      });
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
      if (f) {
        const a = await api.files.read(project.id, archivePath(other)).catch(() => null);
        out.set(other, doneIn(f.text, a?.text));
      }
    }
    return out;
  }

  /**
   * After a step is done (24u): a closing step records what of its section is still set aside; any other step is
   * noted as late for the closing steps that ran without it. Kept per plan, so a later run catches up too.
   */
  async #afterDone(project: Project, file: string, run: PlanRun, stepRun: StepRun): Promise<void> {
    const key = this.#key(stepRun);
    let state = loadPlanState(project.id, file);
    if (isClosingStep(stepRun.number)) {
      const plan = await api.files.read(project.id, file).catch(() => null);
      const parsed = plan ? parsePlan(plan.text).steps : [];
      const section = parsed.find((x) => x.number === stepRun.number && x.title === stepRun.title)?.section;
      const inSection = new Set(parsed.filter((x) => x.section === section).map((x) => x.number ?? x.title));
      const pending = run.steps
        .filter(
          (x) => x !== stepRun && (x.status === "held" || x.status === "set-aside") && inSection.has(this.#key(x)),
        )
        .map((x) => this.#key(x));
      state = closingRan(state, key, pending);
    } else state = markLate(state, key);
    savePlanState(project.id, file, state);
    await this.#runClosings(project, file, run, false);
  }

  /**
   * Opens closing steps again that missed steps done since they ran: once per section, when nothing they wait for is
   * still pending in this run, or when the run is about to end (`ending`). A closing step done in an earlier run is
   * added to this one. Says whether one was opened.
   */
  async #runClosings(project: Project, file: string, run: PlanRun, ending: boolean): Promise<boolean> {
    const state = loadPlanState(project.id, file);
    const pending = (step: string) =>
      run.steps.some(
        (x) =>
          this.#key(x) === step &&
          (x.status === "held" || x.status === "set-aside" || x.status === "waiting" || x.status === "running"),
      );
    let opened = false;
    for (const due of closingsDue(state, pending, ending)) {
      let closing = run.steps.find((x) => this.#key(x) === due.closing);
      if (closing && closing.status !== "done") continue; // Waiting, running, or failed in this run: not again.
      const now = await api.files.read(project.id, file).catch(() => null);
      const s = now ? parsePlan(now.text).steps.find((x) => (x.number ?? x.title) === due.closing) : null;
      if (!now || !s) continue;
      if (s.status === "x") {
        try {
          await api.files.write(project.id, file, setStepStatus(now.text, s.line, " ", s.title), now.hash);
        } catch {
          continue; // The plan changed meanwhile: the next look tries again.
        }
      }
      if (!closing) {
        closing = { line: s.line, number: s.number, title: s.title, attempts: [], status: "waiting" };
        const at = run.steps.findIndex((x) => x.line > s.line);
        run.steps.splice(at < 0 ? run.steps.length : at, 0, closing);
      }
      closing.status = "waiting";
      closing.retryFrom = closing.attempts.length;
      closing.rerunAfter = [...due.late];
      opened = true;
    }
    if (opened) {
      this.changed++;
      await this.#save(run);
    }
    return opened;
  }

  #decide(run: PlanRun, d: Omit<DecisionRecord, "at">): void {
    (run.decisions ??= []).push({ at: Date.now(), ...d });
  }

  /**
   * Sets a step aside for the owner's answer (24u), and the open steps that need it: the plan says so first (Needs,
   * same step number, shared files), then one short decider call for the rest, logged in Decisions. A section's
   * closing step is never set aside. The run then goes on with what is left.
   */
  async #setAside(
    project: Project,
    file: string,
    run: PlanRun,
    stepRun: StepRun,
    kind: HeldKind,
    why: string,
    provider: AgentProvider,
    steps: Step[],
  ): Promise<void> {
    stepRun.status = "held";
    stepRun.held = { kind, why };
    stepRun.answer = undefined;
    const key = this.#key(stepRun);
    const now = await api.files.read(project.id, file).catch(() => null);
    const planSteps = now ? parsePlan(now.text).steps : steps;
    const info = (s: StepRun) => {
      const p =
        planSteps.find((x) => x.number === s.number && x.title === s.title) ?? steps.find((x) => x.line === s.line);
      return { number: s.number, title: s.title, details: p?.details ?? [], needs: p?.needs, changed: s.changed };
    };
    const open = () => run.steps.filter((x) => x.status === "waiting" && !isClosingStep(x.number));
    const aside = (x: StepRun, by: string, reason: string) => {
      x.status = "set-aside";
      x.waitsFor = { step: key, why: reason };
      this.#decide(run, {
        kind: "set-aside",
        by,
        subject: `Step ${this.#key(x)}`,
        decision: `waits for ${key}`,
        why: reason,
      });
    };
    // The plan first, also for steps that need a step set aside just now (72b needs 72a, which needs 95b).
    const outs = [stepRun];
    for (let grew = true; grew;) {
      grew = false;
      for (const x of open()) {
        const later = info(x);
        for (const h of outs) {
          const reason = planDependence(info(h), later);
          if (!reason) continue;
          aside(x, "the plan", h === stepRun ? reason : `${reason} (step ${this.#key(h)}, which waits for ${key})`);
          outs.push(x);
          grew = true;
          break;
        }
      }
    }
    // Then the decider, once, for the open steps the plan says nothing about.
    const rest = open().slice(0, 30);
    if (rest.length) {
      const heldList = outs.map((h) => ({ ...info(h), why: h === stepRun ? why : `waits for ${key}` }));
      const prompt = dependencePrompt(heldList, rest.map(info));
      const keys = rest.map((x) => this.#key(x));
      const heldKeys = outs.map((h) => this.#key(h));
      let text = "";
      let by: string = provider.id;
      try {
        const model = settings.agent(provider.id).model || defaultPlannerModel(provider);
        const r = await planner.ask(project, prompt, model, `Decider: what waits for step ${key}`, {
          agentId: provider.id,
          lean: true,
          quiet: true,
        });
        (run.deciderCalls ??= []).push({ model, costUsd: r.costUsd });
        text = r.text;
      } catch {
        try {
          const f = await freeDecide("You read plan steps. Answer with JSON only.", prompt);
          text = f.text;
          by = f.by;
        } catch {
          // Nobody could answer: the plan's own word stands, the other steps go on.
          this.#decide(run, {
            kind: "set-aside",
            by: "Qanuvi",
            subject: `Step ${key}`,
            decision: "other steps go on",
            why: "the decider could not answer; only what the plan says waits",
          });
        }
      }
      for (const [step, d] of parseDependence(text, keys, heldKeys)) {
        const x = rest.find((r) => this.#key(r) === step);
        if (x) aside(x, `decider (${by})`, d.why || `needs step ${d.on}`);
      }
    }
    this.changed++;
    await this.#save(run);
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
        : renumber(
            block,
            Math.max(
              lastNumber(current.text),
              lastNumber((await api.files.read(project.id, archivePath(file)).catch(() => null))?.text ?? ""),
            ) + 1,
          );
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
  /** "Pick models" is asking the decider now (the Plan view's button waits). */
  picking = $state(false);
  /** Goes up when picks are saved, so the Plan view reads them again. */
  picksSaved = $state(0);

  /**
   * "Pick models" in the Plan view: the decider picks the model for each open step now, before a run, and the picks
   * are kept on this computer (not in the plan file). Steps with their own Model: line keep it and are not asked.
   */
  async pickModels(project: Project, file: string): Promise<string> {
    if (this.picking) return "";
    this.picking = true;
    try {
      const provider = providerById(agent.runProviderId);
      const steps = parsePlan((await api.files.read(project.id, file)).text).steps.filter(
        (s) => s.status !== "x" && !s.model?.trim(),
      );
      if (!steps.length) return "No open steps without a model of their own.";
      const team = settings.value.team;
      const canFree = provider.id !== "qanuvi" && settings.value.brain.models.length > 0;
      const freeFirst = canFree && freeWorks(team, true, true);
      const priority = team.mode === "priority" ? team.priority : "save";
      const briefs = steps.map(briefOf);
      const stats = await this.#learned();
      const { list, by } = await this.#askPicks(project, provider, briefs, { priority, freeFirst }, stats, null);
      const saved: SavedPicks = { at: Date.now(), providerId: provider.id, freeFirst, picks: {} };
      for (const a of list) {
        const b = briefs.find((x) => x.number === a.number);
        if (b)
          saved.picks[a.number] = {
            title: b.title,
            worker: a.worker,
            why: a.why,
            ...(a.model ? { model: a.model } : {}),
          };
      }
      savePicks(localStorage, project.id, file, saved);
      this.picksSaved++;
      return `Picked by ${by} for ${list.length} step${list.length === 1 ? "" : "s"}.`;
    } catch (e) {
      return errorText(e);
    } finally {
      this.picking = false;
    }
  }

  /** One decider call for these steps: the paid agent, else the free models, else Qanuvi core's rules. */
  async #askPicks(
    project: Project,
    provider: AgentProvider,
    briefs: StepBrief[],
    /** workersOnly: free or paid per step, as runs always asked (models only come from "Pick models"). */
    opts: { priority: "save" | "balanced" | "solve"; freeFirst: boolean; workersOnly?: boolean },
    stats: Map<string, ModelStats>,
    run: PlanRun | null,
  ): Promise<{ list: Assignment[]; by: string }> {
    const profiles: Profiles = {};
    for (const b of briefs) profiles[b.type] ??= modelProfiles(stats, b.type as JobType, provider.models);
    const prompt = opts.workersOnly
      ? assignPrompt(briefs, opts.priority)
      : pickPrompt(briefs, {
          priority: opts.priority,
          freeFirst: opts.freeFirst,
          models: provider.models.map((m) => ({ id: m.id, label: m.label })),
          profiles,
        });
    let answer = "";
    let by = provider.name;
    try {
      const model = defaultPlannerModel(provider);
      const r = await planner.ask(
        project,
        prompt,
        model,
        opts.workersOnly ? "Decider: who does each step" : "Decider: the model for each step",
        {
          agentId: provider.id,
          lean: true,
        },
      );
      answer = r.text;
      if (run) (run.deciderCalls ??= []).push({ model, costUsd: r.costUsd });
    } catch {
      // The paid agent cannot answer now (limit, sign-in, offline): the free models take the seat (24e).
      try {
        const f = await freeDecide("You plan work for coding agents. Answer with JSON only.", prompt);
        answer = f.text;
        by = `${f.by} (free, ${provider.name} could not answer)`;
      } catch {
        // No answer: Qanuvi core's rules decide (parsePicks falls back).
      }
    }
    const list = opts.workersOnly
      ? parseAssignment(answer, briefs, opts.priority)
      : parsePicks(answer, briefs, { ...opts, modelIds: provider.models.map((m) => m.id) });
    return { list, by: answer ? by : "Qanuvi core" };
  }

  /** What earlier runs taught about models (the Plan view shows each step's likely pick from it). */
  learnedStats(): Promise<Map<string, ModelStats>> {
    return this.#learned();
  }

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
  async #context(project: Project, step: Step, planFile = ""): Promise<string> {
    const m = mentions([step.title, step.note, ...step.details].join("\n"));
    const defs: Location[] = [];
    // The plan's own folder only (a plugin's plan means the plugin's code), and never vendor code.
    const home = planFile.includes("/") ? planFile.slice(0, planFile.lastIndexOf("/") + 1) : "";
    const vendor = /(^|\/)(vendor|node_modules|tinymce|dist|build|third[-_]?party)\/|\.min\.js$/i;
    for (const name of m.names.slice(0, 8)) {
      const found = (await api.index.find(project.id, name).catch(() => [])).filter((h) => !vendor.test(h.path));
      const hits = home ? found.filter((h) => h.path.startsWith(home)) : found;
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
      // "Stop this command, read my message now" on this step (24r): the turn that ends is not the step's end; the
      // agent reads the message and goes on. Nothing goes on within half a minute: the step is paused by the owner.
      let interrupted = false;
      let interruptWait: ReturnType<typeof setTimeout> | null = null;
      agent.onPlanInterrupt = () => (interrupted = true);
      const finish = (outcome: Turn["outcome"], note = "") => {
        if (settled) return;
        settled = true;
        if (interruptWait) clearTimeout(interruptWait);
        if (agent.onPlanInterrupt) agent.onPlanInterrupt = null;
        this.#endTurn = null;
        resolve({ sessionId: id, last, outcome, note });
      };
      this.#endTurn = () => finish("stopped");
      this.#sendMore = (more: string) => {
        if (!id) return;
        api.agent
          .start(
            project.id,
            id,
            more,
            agentModel,
            mode,
            s.baseUrl,
            s.effort,
            () => {},
            lean,
            indexTools,
            s.browserTools,
            null,
            agentId,
            setupFor(agentId),
          )
          .catch(() => {});
      };
      // The step's limits (cost and time): checked while the turn runs; over them, the turn stops and the run asks.
      const guard = setInterval(() => {
        if (settled) return clearInterval(guard);
        const spent = this.#stepSpentBefore + Math.max(0, (last?.cost_usd ?? 0) - this.#attemptBase);
        const why = overLimit(spent, (Date.now() - this.#stepStartedAt) / 60_000, this.#limits);
        if (why) {
          clearInterval(guard);
          this.#overLimit = why;
          if (id) api.agent.stop(id).catch(() => {});
          finish("stopped");
        }
      }, 15_000);
      const onEvent = (event: AgentEvent) => {
        show(event);
        // The context of the call just made: too big, and the step hands over (once per attempt).
        if (event.kind === "line" && !this.#handover && !this.#pauseNow && event.line.includes('"usage"')) {
          const size = event.line.includes('"assistant"') ? contextSize([event.line]) : 0;
          if (size && handoverDue(size, this.#handoverLimit, this.#handovers) && this.#sendMore && this.#stepLog) {
            this.#handover = size;
            this.#sendMore(handoverRequest(this.#stepLog, size));
            // An agent that does not stop is stopped; the new session checks whatever it left.
            setTimeout(() => {
              if (this.#handover && this.#endTurn) {
                if (id) api.agent.stop(id).catch(() => {});
                this.#endTurn();
              }
            }, 180_000);
          }
        }
        if (event.kind === "session") {
          last = event.session;
          id = event.session.id;
          const st = event.session.status;
          if (st === "running" && interruptWait) {
            clearTimeout(interruptWait);
            interruptWait = null;
          }
          if (st === "idle" && interrupted) {
            interrupted = false;
            interruptWait = setTimeout(() => {
              interruptWait = null;
              this.#pauseNow = true;
              finish("stopped");
            }, INTERRUPT_GRACE_MS);
            return;
          }
          if (FINISHED.includes(st))
            finish(st === "stopped" ? "stopped" : st === "failed" || st === "ended" ? "error" : "idle");
        } else if (event.kind === "exit") {
          finish(this.#stopRequested ? "stopped" : "error");
        }
      };
      if (!sessionId) agent.watch(null);
      // "qanuvi:auto": the step goes to Qanuvi's own agent, which lets Qanuvi core pick the free model.
      const agentId = isFree(model) ? "qanuvi" : (this.#agentOverride ?? this.run?.provider ?? agent.runProviderId);
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
      for (let round = 0; turn.outcome === "idle" && !this.#stopRequested && !this.#handover; round++) {
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
    this.switchingTo = null;
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

/** A step as the decider reads it. */
function briefOf(s: Step): StepBrief {
  return {
    number: s.number ?? String(s.line),
    title: s.title,
    details: s.details ?? [],
    type: jobType([s.title, s.note ?? "", ...(s.details ?? [])].join("\n")),
  };
}
