/**
 * The window's view of plan runs (24z R2d part 2). The run itself goes on in Qanuvi's core (src-tauri/src/plan/run.rs):
 * this store starts it, sends the owner's asks (Stop, Pause, answers, messages, model and mode) to its plan_run_*
 * commands, and shows what the core tells: "qanuvi-run" brings the run's view, "qanuvi-run-agent" the steps' agent
 * events, which go to the chat like any session's. Closing the window never ends a run.
 *
 * A project on a master (a server without a window) runs there: its view and agent events come on the stream of the
 * master's plan_run_watch (24z R2d part 3), one per project while the connection lasts.
 *
 * What stays here: "Pick models" (before a run), what earlier runs taught about models, a plan's private details for
 * chats in its step sessions, and going on from a step the last run stopped at.
 */
import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { callMaster, ownerOf } from "$lib/remote";
import { api, errorText, type AgentEvent, type Project, type RunView, type Session } from "$lib/api";
import { modelLabel, providerById, type AgentProvider } from "$lib/agents/providers";
import { freeDecide, ownSetup } from "./brain.svelte";
import { paidPerStep, runTotals, type PlanRun, type RunOptions, type StepRun } from "$lib/plan/runner";
import { factsPath, looksSecret, privateBlock, savePlanNotes } from "$lib/plan/guidance";
import { parsePlan, setStepStatus, type Step } from "$lib/plan/tasklist";
import { jobType, learn, modelProfiles, type JobType, type ModelStats } from "$lib/plan/routing";
import { savePicks, type SavedPicks } from "$lib/plan/picks";
import { planData, type Entry } from "$lib/plan/planData.svelte";
import { freeWorks, parsePicks, pickPrompt, type Assignment, type Profiles, type StepBrief } from "$lib/core/decider";
import { defaultPlannerModel, planner } from "./planner.svelte";
import type { FinishLevel } from "$lib/plan/finishing";
import { agent } from "./agent.svelte";
import { settings } from "./settings.svelte";
import { runSessionInfo, type PlanSessionInfo } from "$lib/agent/handover";

/** Where the next run goes on in an existing session (a paused step, or a chat the owner said to go on in). */
interface Resume {
  projectId: string;
  file: string;
  sessionId: string;
  model: string;
  section: string;
  base: Base;
  text: string;
  /** Qanuvi's own words (a paused step's note), not the owner's. */
  isNote: boolean;
}

/** A session's running totals, so a step that goes on in a session counts only its own share. */
interface Base {
  cost: number;
  input: number;
  output: number;
  ms: number;
}

function totalsOf(s: Session | null | undefined): Base {
  return {
    cost: s?.cost_usd ?? 0,
    input: s?.input_tokens ?? 0,
    output: s?.output_tokens ?? 0,
    ms: s?.duration_ms ?? 0,
  };
}

/** What changes in a run that the Plan view must read again (statuses, steps added, the run's own status). */
function shape(run: PlanRun | null): string {
  if (!run) return "";
  return `${run.id}|${run.status}|${run.steps.map((s) => `${s.number ?? s.title}:${s.status}`).join(",")}`;
}

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
  /** Average paid cost of a finished step per kind of job, from earlier runs (to show what free models saved). */
  paidPerStep = $state<Partial<Record<string, number>>>({});
  /** Only set-aside steps are left: the run waits (spending nothing) for this many answers (24u). */
  waitingAnswers = $state(0);
  /** The finishing pass over a finished run is at work (24o). */
  finishing = $state(false);
  /** Pause asked for: the run stops after the edit in hand (or at once while it waits). */
  pausing = $state(false);
  /** Stop asked while a step works: the agent finishes the edit it is making first (a second Stop is at once). */
  stopping = $state(false);
  /** The model the owner switched the running step to, until its new session starts (the chat footer shows it). */
  switchingTo = $state<string | null>(null);
  /** Owner messages in the step's chat the runner has not taken yet. */
  waitingMessages = $state(0);
  /** The last run or pass could not start, or ended with an error. */
  error = $state("");
  /** "Pick models" is asking the decider now (the Plan view's button waits). */
  picking = $state(false);
  /** Goes up when picks are saved, so the Plan view reads them again. */
  picksSaved = $state(0);

  #running = $state(false);
  #sessionId = $state<string | null>(null);
  /**
   * The session the run last worked in: the core's own id goes empty between a step's turns (checks, a fix round),
   * and the chat handover must keep off that session the whole time (the runner hands it over at the run's limit).
   */
  #workingIn: string | null = null;

  constructor() {
    agent.planSessionInfo = (id) => this.#sessionInfo(id);
  }

  /** Whether the run works in a session now, and the run's handover limit when it is one of its step sessions. */
  #sessionInfo(id: string): PlanSessionInfo {
    return runSessionInfo(id, this.run, this.#running || this.finishing, this.#sessionId, this.#workingIn);
  }
  /** The project whose run is shown. */
  #project: string | null = null;
  /** Notices of the shown run already put into the chat. */
  #noticesSeen = 0;
  #noticesRun = "";
  #shape = "";
  #resume: Resume | null = null;
  #ready: Promise<void> | null = null;
  /** Projects on a master whose run this window follows, with that master. */
  #watched = new Map<string, string>();
  /** The master the shown run is on (null for this computer). */
  #shownOn: string | null = null;

  /** Follows the core's runs from the start (a run may already go on when the window opens). */
  follow(): Promise<void> {
    return this.#listen();
  }

  get running(): boolean {
    return this.#running;
  }

  /** The session of the step running now (a message sent there goes to the runner first). */
  get sessionId(): string | null {
    return this.#running ? this.#sessionId : null;
  }

  get totals() {
    return this.run ? runTotals(this.run) : null;
  }

  /** Listens to the core's run events once (the window may open while a run already goes on). */
  #listen(): Promise<void> {
    this.#ready ??= (async () => {
      try {
        await listen<RunView>("qanuvi-run", (e) => this.#take(e.payload));
        await listen<{ projectId: string; sessionId: string | null; event: AgentEvent }>("qanuvi-run-agent", (e) =>
          this.#agentEvent(e.payload),
        );
        // A run that went on while no window was open (the core keeps it): show it.
        const active = await api.planRun.active().catch(() => [] as string[]);
        if (active.length && !this.#running) this.#take(await api.planRun.state(active[0]));
      } catch {
        // No core events (tests, a browser preview): nothing to follow.
      }
    })();
    return this.#ready;
  }

  /**
   * A project on a master: follows its run on the master's stream (once per connection). This computer's projects
   * come by the "qanuvi-run" events already.
   */
  async #watch(projectId: string): Promise<void> {
    const master = ownerOf(projectId);
    if (!master || this.#watched.get(projectId) === master) return;
    this.#watched.set(projectId, master);
    type Streamed =
      | { kind: "view"; view: RunView }
      | { kind: "agent"; sessionId: string | null; event: AgentEvent }
      | { kind: "event"; event: string; value: unknown };
    const events = new Channel<Streamed>();
    events.onmessage = (m) => {
      if (m.kind === "view") this.#take(m.view);
      else if (m.kind === "agent") this.#agentEvent({ projectId, sessionId: m.sessionId, event: m.event });
      // The master runner's plan data (holds, state, notes) as it changes there; its Inbox changes come on its own
      // Inbox stream (inbox.svelte.ts).
      else if (m.kind === "event" && m.event === "qanuvi-plan-data") planData.heard(m.value as Entry);
    };
    try {
      this.#take(await callMaster<RunView>(master, "plan_run_watch", { projectId, onEvent: events }));
    } catch {
      // An older master: no run news from it.
      this.#watched.delete(projectId);
    }
  }

  /** A master connected: a run going on there (started from any computer) is shown when nothing runs here. */
  async masterConnected(master: string): Promise<void> {
    await this.#listen();
    const active = await callMaster<string[]>(master, "plan_run_active", {}).catch(() => [] as string[]);
    if (active.length && !this.#running && !this.finishing) await this.loadLast(active[0]);
  }

  /** The connection to a master ended: its runs go on there, but this window hears no more of them. */
  dropMaster(master: string): void {
    for (const [p, m] of this.#watched) if (m === master) this.#watched.delete(p);
    if (this.#shownOn !== master || !(this.#running || this.finishing)) return;
    this.#running = false;
    this.finishing = false;
    this.#sessionId = null;
    agent.planSessionId = null;
    this.#unhook();
    agent.notice("The connection to the server ended. The plan run goes on there; connect again to see it.");
  }

  /** The core's view of a project's run. */
  #take(v: RunView): void {
    const busy = v.running || v.finishing;
    // Another project's run: shown only when nothing runs here.
    if (this.#project && v.projectId !== this.#project && (this.#running || this.finishing || !busy)) return;
    const wasBusy = this.#running || this.finishing;
    this.#project = v.projectId;
    this.#shownOn = ownerOf(v.projectId);
    this.run = v.run;
    this.#running = v.running;
    this.finishing = v.finishing;
    this.currentModel = v.currentModel;
    this.checking = v.checking;
    this.waitingUntil = v.waitingUntil ? new Date(v.waitingUntil) : null;
    this.waitingAnswers = v.waitingAnswers;
    this.pausing = v.pausing;
    this.stopping = v.stopping;
    this.waitingMessages = v.waitingMessages;
    this.error = v.error ?? "";
    // What the run told the owner on the way, once each, in the chat.
    const runId = v.run?.id ?? "";
    if (runId !== this.#noticesRun) {
      this.#noticesRun = runId;
      this.#noticesSeen = busy ? 0 : v.notices.length;
    }
    for (const n of v.notices.slice(this.#noticesSeen)) agent.notice(n);
    this.#noticesSeen = v.notices.length;
    // The step session at work: the chat follows it and its pickers act on it.
    const sid = v.running ? v.sessionId : null;
    if (sid !== this.#sessionId) {
      this.#sessionId = sid;
      agent.planSessionId = sid;
      if (sid) this.#workingIn = sid;
      if (sid) {
        this.switchingTo = null;
        void this.#follow(v.projectId, sid);
      }
    }
    if (busy && !wasBusy) this.#hook(v.projectId);
    if (!busy && wasBusy) {
      this.#unhook();
      if (v.error) agent.notice(`The plan run ended with an error: ${v.error}`);
    }
    const now = shape(v.run);
    if (now !== this.#shape) {
      this.#shape = now;
      this.changed++;
    }
  }

  /** A step's agent events go to the chat like any session's (the session they belong to decides where). */
  #agentEvent(p: { projectId: string; sessionId: string | null; event: AgentEvent }): void {
    const id = p.event.kind === "session" ? p.event.session.id : p.sessionId;
    // A session not open in the chat (a review, the planner, a step while another chat is open) is only listed: its
    // turn ending must not end the open chat's.
    if (p.event.kind === "session" && id !== agent.selectedId) {
      agent.track(p.event.session);
      return;
    }
    agent.handler(() => id)(p.event);
  }

  /** The chat shows the new step session (with what it said so far) and its title from the core. */
  async #follow(projectId: string, sessionId: string): Promise<void> {
    const list = await api.agent.sessions(projectId).catch(() => [] as Session[]);
    const s = list.find((x) => x.id === sessionId);
    if (s) agent.track(s);
    if (this.#sessionId === sessionId) await agent.select(sessionId);
  }

  /** While a run goes on, the chat's pickers and "read my message now" act on the step at work. */
  #hook(projectId: string): void {
    agent.onPlanChange = (change) => {
      if (change.mode) api.planRun.mode(projectId, change.mode).catch(() => {});
      if (change.model) {
        if (change.model !== this.currentModel) this.switchingTo = change.model;
        api.planRun.model(projectId, change.model).catch(() => {});
      }
    };
    agent.onPlanInterrupt = () => {
      api.planRun.interrupt(projectId).catch(() => {});
    };
  }

  #unhook(): void {
    agent.onPlanChange = null;
    agent.onPlanInterrupt = null;
    agent.planSessionId = null;
    this.#sessionId = null;
    this.switchingTo = null;
  }

  /** What the core needs to run with this window's settings: the agent, its models, Team, rules, memory. */
  #startOf(project: Project, file: string, options: RunOptions, provider: AgentProvider, resume: Resume | null) {
    const s = settings.value;
    const a = settings.agent(provider.id);
    const labels: Record<string, string> = {};
    for (const m of provider.models) labels[m.id] = modelLabel(provider, m.id);
    const hasFree = s.brain.models.length > 0;
    return {
      projectName: project.name,
      file,
      options,
      provider: provider.id,
      providerName: provider.name,
      models: provider.models,
      defaultModel: a.model,
      plannerModel: defaultPlannerModel(provider),
      team: s.team,
      hasFree,
      monthlyBudgetUsd: s.monthlyBudgetUsd,
      rules: s.rules[project.id] ?? {},
      turns: {
        effort: a.effort || null,
        baseUrl: a.baseUrl?.trim() || null,
        browserTools: !!a.browserTools,
        lean: !!options.lean,
        indexTools: !!options.indexTools,
        // Qanuvi's own models (free steps, the free decider); the core fills in their keys.
        setups: hasFree ? { qanuvi: JSON.stringify(ownSetup()) } : {},
        memory: { on: !!s.memoryContext, budget: s.memoryBudget },
        labels,
      },
      resume: resume
        ? {
            sessionId: resume.sessionId,
            model: resume.model,
            section: resume.section,
            base: resume.base,
            text: resume.text,
            isNote: resume.isNote,
          }
        : null,
    };
  }

  async start(project: Project, file: string, options: RunOptions): Promise<void> {
    await this.#listen();
    if (this.#running || this.finishing) return;
    const provider = providerById(options.providerId || agent.runProviderId);
    const r = this.#resume;
    const resume = r && r.projectId === project.id && r.file === file ? r : null;
    this.#resume = null;
    // What earlier runs cost per kind of step (the Run panel shows what free models saved against it).
    this.paidPerStep = paidPerStep(await this.#learned());
    this.#project = project.id;
    this.error = "";
    await this.#watch(project.id);
    try {
      await api.planRun.start(project.id, this.#startOf(project, file, options, provider, resume));
    } catch (e) {
      this.error = errorText(e);
      throw new Error(this.error);
    }
    this.#take(await api.planRun.state(project.id));
  }

  /** The finishing pass on the project's last run (24o), in the core like the run. */
  async finishPass(project: Project, level: FinishLevel): Promise<void> {
    await this.#listen();
    const run = this.run;
    if (!run || run.projectId !== project.id || this.#running || this.finishing) return;
    const provider = providerById(agent.runProviderId);
    this.#project = project.id;
    await this.#watch(project.id);
    try {
      await api.planRun.finish(project.id, this.#startOf(project, run.file, run.options, provider, null), level);
    } catch (e) {
      this.error = errorText(e);
      agent.notice(`The finishing pass could not start: ${this.error}`);
      return;
    }
    this.#take(await api.planRun.state(project.id));
  }

  /** The last run of a project, to show its summary. */
  async loadLast(projectId: string): Promise<void> {
    await this.#listen();
    if ((this.#running || this.finishing) && this.#project !== projectId) return;
    try {
      await this.#watch(projectId);
      const v = await api.planRun.state(projectId);
      // Shown as it is now, also when another project was shown before.
      this.#project = projectId;
      this.#take(v);
    } catch {
      if (!this.#running) this.run = null;
    }
  }

  /**
   * The plan's last run, after a restart (or another plan shown): so the button says Resume and a run paused in a
   * step goes on in that step's session. A session that cannot be opened again gives a new one, which reads the
   * step's progress note in its log.
   */
  async restoreLast(project: Project, file: string): Promise<void> {
    await this.#listen();
    if (this.#running || this.finishing) return;
    if (this.run && this.run.projectId === project.id && this.run.file === file) {
      this.#keepPaused(this.run);
      return;
    }
    const records = await api.runs.list(project.id).catch(() => []);
    const last = records.filter((r) => r.file === file).sort((a, b) => b.updated_at - a.updated_at)[0];
    if (!last || this.#running || this.finishing) return;
    let run: PlanRun;
    try {
      run = JSON.parse(last.data);
    } catch {
      return;
    }
    if (run.status === "running") run.status = "stopped";
    for (const s of run.steps) if (s.status === "running") s.status = "waiting";
    // No pass runs after a restart; its result, when it got that far, is in the plan's FINISH.md log.
    if (run.finish?.status === "running") {
      run.finish.status = "stopped";
      run.finish.note = "Its saved state ended mid-pass; the result, if any, is in the plan's FINISH.md log.";
    }
    this.#project = project.id;
    this.run = run;
    this.#shape = shape(run);
    this.#keepPaused(run);
  }

  /** A run paused in the middle of a step goes on in that step's session at the next start. */
  #keepPaused(run: PlanRun): void {
    const p = run.pausedSession;
    if (run.status !== "paused" || !p) return;
    this.#resume = {
      projectId: run.projectId,
      file: run.file,
      sessionId: p.sessionId,
      model: p.model,
      section: p.section,
      base: totalsOf(agent.sessions.find((x) => x.id === p.sessionId)),
      text: "You paused this step at the owner's request and the run now resumes: go on from where you stopped (your Progress note is in the step's log).",
      isNote: true,
    };
  }

  /**
   * The owner answers a held step: "go" lets it run (with the answer, if any, told to it), "skip" leaves it and the
   * steps waiting for it. Works while the run goes on (picked up after the step in hand) and while it waits.
   */
  answer(step: StepRun, choice: "go" | "skip", text = ""): void {
    const run = this.run;
    if (!run || !this.#running || step.status !== "held") return;
    api.planRun.answer(run.projectId, step.number ?? step.title, choice, text.trim()).catch(() => {});
  }

  /** Ends a run that waits for answers: what is set aside stays open, the summary lists it. */
  finishWithout(): void {
    if (!this.waitingAnswers || !this.run) return;
    api.planRun.finishWithout(this.run.projectId).catch(() => {});
  }

  pause(): void {
    if (!this.#running || this.pausing || !this.run) return;
    this.pausing = true;
    api.planRun.pause(this.run.projectId).catch(() => {});
  }

  /** Gently first (the edit in hand is finished, so no file is left half-written); a second Stop is at once. */
  stop(): void {
    const run = this.run;
    if (!run || (!this.#running && !this.finishing)) return;
    if (this.#running) this.stopping = true;
    api.planRun.stop(run.projectId).catch(() => {});
  }

  /**
   * The owner picked a model for a step of the running plan (the Plan view writes its Model line). The step running
   * now hands over to it (its agent finishes the edit in hand and writes a progress note); any other step just starts
   * on it when its turn comes. Returns whether the running step is switching.
   */
  switchStepModel(projectId: string, file: string, number: string | null, title: string, model: string): boolean {
    const r = this.run;
    if (!this.#running || !r || r.projectId !== projectId || r.file !== file) return false;
    const current = r.steps.find((x) => x.status === "running");
    if (!current || current.number !== number || current.title !== title) return false;
    if (model !== this.currentModel) this.switchingTo = model;
    api.planRun.model(projectId, model).catch(() => {});
    return true;
  }

  /**
   * The owner typed into the running step's chat: the runner sorts it first (a note, a rule for the whole plan, a
   * question, a change, stop or pause words) and tells the agent as fits.
   */
  async message(projectId: string, text: string): Promise<void> {
    await api.planRun.message(projectId, text);
  }

  /** Words for the running step's agent as they are (the plan's private details), never sorted by a model. */
  async tell(projectId: string, text: string): Promise<void> {
    await api.planRun.tell(projectId, text);
  }

  /**
   * The step of the last run whose session this is, when that run stopped short (blocked, failed, stopped, waiting
   * for the owner): writing there is a chat with the owner, and the run goes on only when they say so (24x).
   */
  stoppedAt(projectId: string, sessionId: string): { number: string; file: string } | null {
    const last = this.run;
    if (!last || this.#running || last.projectId !== projectId || last.status === "done") return null;
    const number = agent.sessions.find((x) => x.id === sessionId)?.title.match(/^Step ([^:\s]+):/)?.[1];
    if (!number || !last.steps.some((s) => s.number === number)) return null;
    return { number, file: last.file };
  }

  /**
   * The plan's private details (logins) for a chat in one of the last run's step sessions, as the hidden block steps
   * get; empty for other chats. A new session of the chat (a handover) would else not know them and ask again.
   */
  async privateFor(projectId: string, sessionId: string): Promise<string> {
    const last = this.run;
    if (!last || last.projectId !== projectId) return "";
    const number = agent.sessions.find((x) => x.id === sessionId)?.title.match(/^Step ([^:\s]+):/)?.[1];
    if (!number || !last.steps.some((s) => s.number === number)) return "";
    return privateBlock(await api.agent.planPrivate(projectId, last.file).catch(() => [] as string[]));
  }

  /**
   * The owner said to go on (a yes to the agent's question, or "continue") in the session of a step the last run
   * stopped at: the run goes on from that step, in the same session (which has the chat), then carries on with the
   * next steps as it was set. Answers false when there is no such run.
   */
  async resumeFrom(project: Project, sessionId: string, text: string): Promise<boolean> {
    const last = this.run;
    if (!last || this.#running || last.projectId !== project.id) return false;
    const session = agent.sessions.find((x) => x.id === sessionId);
    const number = session?.title.match(/^Step ([^:\s]+):/)?.[1];
    if (!number) return false;
    const now = await api.files.read(project.id, last.file).catch(() => null);
    const step = now ? parsePlan(now.text).steps.find((x) => x.number === number) : undefined;
    if (!step) return false;
    // A blocked step is open again: the owner is unblocking it.
    if (step.status === "!" && now)
      await api.files
        .write(project.id, last.file, setStepStatus(now.text, step.line, " ", step.title), now.hash)
        .catch(() => {});
    this.#resume = {
      projectId: project.id,
      file: last.file,
      sessionId,
      model: session?.model ?? "",
      section: step.section ?? "",
      base: totalsOf(session),
      text,
      isNote: false,
    };
    void this.start(project, last.file, {
      ...last.options,
      fromLine: step.line,
      onlyOne: false,
      ranges: "",
      // The session keeps its model for this step; later steps go back to their own.
      model: last.options.model,
    }).catch((e) => agent.notice(`The plan run could not go on: ${errorText(e)}`));
    return true;
  }

  /**
   * Something the owner decided outside the step's chat (a changed choice in the Run panel): the plan's FACTS.md
   * keeps it, so every later step and model reads it. A run that is not going on keeps it in its notes too.
   */
  addNote(text: string): void {
    const run = this.run;
    const note = text.replace(/\s+/g, " ").trim().slice(0, 400);
    if (!run || !note || looksSecret(note)) return;
    void api.files.logAppend(run.projectId, factsPath(run.file), `- (the owner) ${note}\n`).catch(() => {});
    if (this.#running) return;
    run.notes = [...(run.notes ?? []), note];
    void this.#save(run);
  }

  /** A login or password given during the run: kept for this plan (credential store), for every later step. */
  async addPrivate(projectId: string, file: string, text: string): Promise<void> {
    await api.agent.planPrivateAdd(projectId, file, text).catch(() => {});
    this.changed++;
  }

  removeNote(index: number): void {
    const run = this.run;
    if (!run?.notes || this.#running) return;
    run.notes = run.notes.filter((_, i) => i !== index);
    savePlanNotes(run.projectId, run.file, run.notes);
    void this.#save(run);
  }

  /** Keeps the record of a run that is not going on (the core keeps the record of one that is). */
  async #save(run: PlanRun): Promise<void> {
    if (this.#running && run.id === this.run?.id) return;
    await api.runs.save(run.projectId, run.id, run.file, run.status, JSON.stringify(run)).catch(() => {});
  }

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
      const { list, by } = await this.#askPicks(project, provider, briefs, { priority, freeFirst }, stats);
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
      savePicks(planData, project.id, file, saved);
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
    opts: { priority: "save" | "balanced" | "solve"; freeFirst: boolean },
    stats: Map<string, ModelStats>,
  ): Promise<{ list: Assignment[]; by: string }> {
    const profiles: Profiles = {};
    for (const b of briefs) profiles[b.type] ??= modelProfiles(stats, b.type as JobType, provider.models);
    const prompt = pickPrompt(briefs, {
      priority: opts.priority,
      freeFirst: opts.freeFirst,
      models: provider.models.map((m) => ({ id: m.id, label: m.label })),
      profiles,
    });
    let answer = "";
    let by = provider.name;
    try {
      const r = await planner.ask(project, prompt, defaultPlannerModel(provider), "Decider: the model for each step", {
        agentId: provider.id,
        lean: true,
      });
      answer = r.text;
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
    const list = parsePicks(answer, briefs, { ...opts, modelIds: provider.models.map((m) => m.id) });
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
}

/** A step as the decider reads it. */
function briefOf(s: Step): StepBrief {
  return {
    number: s.number ?? String(s.line),
    title: s.title,
    details: s.details ?? [],
    type: jobType([s.title, s.note ?? "", ...(s.details ?? [])].join("\n")),
  };
}

export const runner = new RunnerStore();
void runner.follow();
agent.privateNote = (projectId, sessionId) => runner.privateFor(projectId, sessionId);

export type { StepRun };
