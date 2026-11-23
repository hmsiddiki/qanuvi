<script lang="ts">
  import { shownError } from "$lib/stores/linkState.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { ANSWER_HINT } from "$lib/plan/holdask";
  import HoldCardView from "./HoldCardView.svelte";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import type { Project } from "$lib/api";
  import { addOrderNeeds, setStepModel, type Plan } from "$lib/plan/tasklist";
  import { modelLabel, providerById, PROVIDERS } from "$lib/agents/providers";
  import { api, errorText } from "$lib/api";
  import { untrack } from "svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import {
    DEFAULT_RULES,
    parsePatterns,
    rulesFacts,
    rulesLine,
    rulesOf,
    testBuildFact,
    type RunRules,
  } from "$lib/plan/runRules";
  import { callThisPc, shownMachine } from "$lib/remote";
  import { offerStartWithComputer, startSettingLabel } from "$lib/startWithComputer";
  import { formatCost, formatTokens } from "$lib/agent/events";
  import { runner } from "$lib/stores/runner.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import {
    costRatio,
    freeSavings,
    isFree,
    DECIDER,
    parseRanges,
    inRanges,
    type PauseMode,
    type StepRun,
  } from "$lib/plan/runner";
  import { freeWorks, type TeamMode } from "$lib/core/decider";
  import { loadPlanNotes, runSettingsKey, savePlanNotes } from "$lib/plan/guidance";
  import { planData } from "$lib/plan/planData.svelte";
  import {
    changeDecided,
    holdWording,
    keepDecided,
    keepHolds,
    keepChecked,
    alreadyChecked,
    keptModel,
    loadPlanState,
    savePlanState,
    decidedOpen,
    foldDecided,
    setKeptModel,
    type KeptDecided,
    type KeptHold,
  } from "$lib/plan/planstate";
  import {
    answersPrompt,
    decidedNote,
    firstUrl,
    maskedValue,
    parseAnswerReadings,
    parsePreflight,
    preflightPrompt,
    projectAnswers,
    readAnswers,
    siteAddressValues,
    recommendedRisk,
    settleCheck,
    typedAnswers,
    withReadHolds,
    END_TO_END,
    type AnswerReading,
    type AskSort,
    type OtherProject,
    type Preflight,
    type RiskChoice,
  } from "$lib/plan/preflight";
  import { budgetBelowFirstStep, finishedCost, planHistory, usualStepCost } from "$lib/plan/costs";
  import { jobType, type ModelStats } from "$lib/plan/routing";
  import { planner, defaultPlannerModel } from "$lib/stores/planner.svelte";
  import {
    finishEstimate,
    offersFinish,
    FINISHER,
    type FindingOutcome,
    type FinishAuto,
    type FinishLevel,
  } from "$lib/plan/finishing";

  interface Props {
    project: Project;
    file: string;
    plan: Plan;
    /** The setup form is shown (the user pressed Run). */
    setup: boolean;
    onclose: () => void;
    /** The plan file was changed here (the steps' models written in). */
    onchanged?: () => void;
    /** Opened by "Raise budget": the budget box gets the focus. */
    focusBudget?: boolean;
  }

  let { project, file, plan, setup, onclose, onchanged, focusBudget = false }: Props = $props();

  /** The agent for this run: any that works on this computer (Claude Code needs its command; Qanuvi's own, models). */
  let usable = $derived(
    PROVIDERS.filter((p) => (p.id === "qanuvi" ? settings.value.brain.models.length > 0 : !!agent.info?.available)),
  );
  let providerId = $state("");
  let provider = $derived(providerById(providerId || agent.runProviderId));
  /** One model for every step ("" = as the plan says), and whether a failing step may move to a stronger one. */
  let oneModel = $state("");
  let strongerOnFail = $state(true);
  /** This run's Team mode ("settings": the one in Settings, Team). */
  let team = $state<"settings" | TeamMode>("settings");
  const TEAM_LABELS: Record<TeamMode, string> = {
    "paid-decides": "Paid decides, free works",
    priority: "Priority",
    direct: "Direct (only the paid agent)",
    "free-decides": "Free decides, paid works",
    "free-only": "Free decides, free works",
  };
  /** Modes that need free models set up. */
  const NEEDS_FREE: TeamMode[] = ["paid-decides", "priority", "free-decides", "free-only"];
  /** Free decides, free works (here or in Settings): the run goes to Qanuvi's own agent. */
  let freeOnly = $derived(team === "free-only" || (team === "settings" && settings.value.team.mode === "free-only"));
  /** Free models are set up (Qanuvi's own agent can work). */
  let hasFree = $derived(settings.value.brain.models.length > 0);
  /** When the paid agent hits its usage limit. */
  let onLimit = $state<"wait" | "free">("wait");
  /** Per step at most this much and this long (0: no limit). */
  let stepUsd = $state(0);
  let stepMin = $state(0);
  /** Steps in a row on one model and section continue a session, up to a size (thousands of tokens). */
  let carryOn = $state(true);
  let carryK = $state(150);
  /** A step in the middle of its work hands over to a new session past this size (thousands of tokens). */
  let handoverOn = $state(true);
  let handoverK = $state(200);
  /** Sessions also go on into the next section; full checks per step or per section. */
  let carryAcross = $state(true);
  let fullChecks = $state<"step" | "section">("section");
  /** The plan's private details are deleted once every step is done. */
  let forgetPrivate = $state(true);
  /** At the end of a run that got done: offer the finishing pass, or run it by itself (24o). */
  let finishAuto = $state<FinishAuto>("ask");
  let openSteps = $derived(plan.steps.filter((s) => s.status !== "x" && s.status !== "!"));
  let fromLine = $state(-1);
  let onlyOne = $state(false);
  /** Which steps by number ("2-6, 10-15"); empty: from the start step to the end (24f). */
  let ranges = $state("");
  let rangeError = $derived(parseRanges(ranges).error);
  let pause = $state<PauseMode>("never");
  let mode = $state("");
  let budget = $state(1);
  let perModel = $state(2);
  let lean = $state(true);
  let indexTools = $state(settings.agent(agent.runProviderId).indexTools);
  let packets = $state(true);
  let replan = $state(true);
  /** Free models can go first when the run's agent is a paid one and Qanuvi core has models. */
  let canFree = $derived(provider.id !== "qanuvi" && settings.value.brain.models.length > 0);
  /** The plan's own data (the models chosen for steps in Qanuvi among it). */
  let keptState = $derived.by(() => {
    void planData.changed;
    return loadPlanState(project.id, file);
  });
  /** The steps the run would cover, with the model each would use by the plan. */
  let stepModels = $derived(
    openSteps
      .map((s) => s.model?.trim() || keptModel(keptState, s) || settings.agent(provider.id).model)
      .filter((m) => !!m),
  );
  let ratio = $derived(oneModel ? costRatio(provider.models, stepModels, oneModel) : null);

  /** Run settings are kept per plan file, so each plan keeps its own way of running. */
  const KEY = () => runSettingsKey(project.id, file);
  interface Kept {
    providerId: string;
    oneModel: string;
    strongerOnFail: boolean;
    team: "settings" | TeamMode;
    budget: number;
    perModel: number;
    pause: PauseMode;
    mode: string;
    lean: boolean;
    indexTools: boolean;
    packets: boolean;
    replan: boolean;
    carryOn: boolean;
    carryK: number;
    handoverOn: boolean;
    handoverK: number;
    carryAcross: boolean;
    fullChecks: "step" | "section";
    forgetPrivate: boolean;
    checkFirst: boolean;
    onLimit: "wait" | "free";
    stepUsd: number;
    stepMin: number;
    finishAuto: FinishAuto;
  }
  function restore(): boolean {
    try {
      const k = JSON.parse(planData.getItem(KEY()) ?? "null") as Partial<Kept> | null;
      if (!k) return false;
      if (k.providerId && usable.some((p) => p.id === k.providerId)) providerId = k.providerId;
      const p = providerById(providerId || agent.runProviderId);
      oneModel = k.oneModel && p.models.some((m) => m.id === k.oneModel) ? k.oneModel : "";
      strongerOnFail = k.strongerOnFail ?? !oneModel;
      if (k.team === "settings" || (typeof k.team === "string" && k.team in TEAM_LABELS)) team = k.team;
      if (typeof k.budget === "number") budget = k.budget;
      if (typeof k.perModel === "number") perModel = k.perModel;
      // "Only where a decision is needed" is gone (24u): held steps are set aside and asked about, the run goes on.
      if (k.pause) pause = k.pause === "decisions" ? "never" : k.pause;
      if (k.mode && p.modes.some((m) => m.id === k.mode)) mode = k.mode;
      if (typeof k.lean === "boolean") lean = k.lean;
      if (typeof k.indexTools === "boolean") indexTools = k.indexTools;
      if (typeof k.packets === "boolean") packets = k.packets;
      if (typeof k.replan === "boolean") replan = k.replan;
      if (typeof k.carryOn === "boolean") carryOn = k.carryOn;
      if (typeof k.carryK === "number") carryK = k.carryK;
      if (typeof k.handoverOn === "boolean") handoverOn = k.handoverOn;
      if (typeof k.handoverK === "number") handoverK = k.handoverK;
      if (typeof k.carryAcross === "boolean") carryAcross = k.carryAcross;
      if (k.fullChecks === "step" || k.fullChecks === "section") fullChecks = k.fullChecks;
      if (typeof k.forgetPrivate === "boolean") forgetPrivate = k.forgetPrivate;
      if (typeof k.checkFirst === "boolean") checkFirst = k.checkFirst;
      if (k.onLimit === "wait" || k.onLimit === "free") onLimit = k.onLimit;
      if (typeof k.stepUsd === "number") stepUsd = k.stepUsd;
      if (typeof k.stepMin === "number") stepMin = k.stepMin;
      if (k.finishAuto === "ask" || k.finishAuto === "light" || k.finishAuto === "full") finishAuto = k.finishAuto;
      return true;
    } catch {
      return false;
    }
  }
  function keep(): void {
    const k: Kept = {
      providerId,
      oneModel,
      strongerOnFail,
      team,
      budget,
      perModel,
      pause,
      mode,
      lean,
      indexTools,
      packets,
      replan,
      carryOn,
      carryK,
      handoverOn,
      handoverK,
      carryAcross,
      fullChecks,
      forgetPrivate,
      checkFirst,
      onLimit,
      stepUsd,
      stepMin,
      finishAuto,
    };
    planData.setItem(KEY(), JSON.stringify(k));
  }

  /** Another agent: its own models and modes. */
  function pickAgent(id: string) {
    providerId = id;
    const p = providerById(id);
    if (oneModel && !p.models.some((m) => m.id === oneModel)) oneModel = "";
    mode = p.modes.some((m) => m.id === "auto") ? "auto" : settings.agent(p.id).mode;
  }

  /** One model chosen: a failing step stops for you instead of quietly moving to another model (can be turned on). */
  function pickModel(id: string) {
    oneModel = id;
    strongerOnFail = !id;
  }

  /**
   * Keeps the chosen model for every open step in the plan's own data (R2e i), not in the plan file. "- Model:" lines
   * the steps have go, as they would win over it.
   */
  let writing = $state(false);
  async function writeIntoPlan() {
    if (!oneModel) return;
    writing = true;
    error = "";
    try {
      const now = await api.files.read(project.id, file);
      let text = now.text;
      // From the bottom up: a Model line taken out of a step moves the lines below it.
      for (const s of [...openSteps].sort((a, b) => b.line - a.line))
        if (s.model) text = setStepModel(text, s.line, null, s.title);
      if (text !== now.text) {
        await api.files.write(project.id, file, text, now.hash);
        onchanged?.();
      }
      let state = loadPlanState(project.id, file);
      for (const s of openSteps) state = setKeptModel(state, s, oneModel);
      savePlanState(project.id, file, state);
    } catch (e) {
      error = errorText(e);
    } finally {
      writing = false;
    }
  }
  /** A model's name in the run's lines: the free agent's attempts read as Qanuvi core. */
  function label(model: string) {
    if (model.startsWith(DECIDER)) return `Decider (${modelLabel(provider, model.slice(DECIDER.length))})`;
    if (model.startsWith(FINISHER))
      return `Finishing pass (${modelLabel(deciderProvider, model.slice(FINISHER.length))})`;
    return isFree(model) ? "Qanuvi core (free)" : modelLabel(provider, model);
  }
  let error = $state("");

  /** Private details kept for this plan (only how many shows here). */
  let privateCount = $state(0);
  $effect(() => {
    void runner.changed;
    api.agent
      .planPrivate(project.id, file)
      .then((d) => (privateCount = d.length))
      .catch(() => (privateCount = 0));
  });
  async function forgetPrivateNow() {
    await api.agent.planPrivateClear(project.id, file).catch(() => {});
    privateCount = 0;
  }

  /** What Qanuvi decided for the owner in this plan's check (24u part 3), each one changeable. */
  let decidedList = $state<KeptDecided[]>([]);
  let changing = $state<number | null>(null);
  let changeText = $state("");
  /** Open or folded as the owner left it for this plan (kept with the plan state). */
  let decidedShown = $state(false);
  $effect(() => {
    void runner.changed;
    void run?.id;
    const state = loadPlanState(project.id, file);
    decidedList = state.decided;
    decidedShown = decidedOpen(state);
  });
  function decidedToggled(open: boolean) {
    if (open === decidedShown) return;
    decidedShown = open;
    savePlanState(project.id, file, foldDecided(loadPlanState(project.id, file), open));
  }
  function saveChange(d: KeptDecided) {
    const text = changeText.trim();
    // The same answer again is no change.
    if (!text || text === (d.changed ?? d.answer)) {
      changing = null;
      return;
    }
    savePlanState(project.id, file, changeDecided(loadPlanState(project.id, file), d.step, d.what, text));
    const done = run?.steps.find((s) => (s.number ?? s.title) === d.step)?.status === "done";
    // Every later step and model reads it (the plan's FACTS.md).
    runner.addNote(
      `The owner changed a choice for step ${d.step}: ${d.what} → ${text} (instead of ${d.changed ?? d.answer}).`,
    );
    decidedList = loadPlanState(project.id, file).decided;
    changing = null;
    changeText = "";
    changeDone = done ? d.step : "";
  }
  /** The changed choice's step was already done: say it needs opening again to take the change. */
  let changeDone = $state("");

  /** The plan the step range was typed for: another plan starts with no range (pm1: "6" from one plan ran nothing in
   * a one-step plan). Reopening the same plan keeps it. */
  let rangesFile = "";
  $effect(() => {
    if (setup) {
      if (rangesFile !== file) {
        rangesFile = file;
        ranges = "";
        onlyOne = false;
      }
      fromLine = openSteps[0]?.line ?? -1;
      // Unattended work needs a mode that does not stop at every edit; the user can still choose Manual.
      mode = provider.modes.some((m) => m.id === "auto") ? "auto" : settings.agent(provider.id).mode;
      // This plan's own settings from its last run.
      restore();
    }
  });

  /** 24y: the run rules card (in full until answered once, then one line), kept in the project's settings. */
  let rules = $state<RunRules>({ ...DEFAULT_RULES });
  let rulesOpen = $state(false);
  let allowText = $state("");
  let neverText = $state("");
  let isGit = $state(false);
  // Read again for another project only, not when saving the rules hands in a new object for the same project.
  let projectId = $derived(project.id);
  $effect(() => {
    const id = projectId;
    const r = rulesOf(untrack(() => project.settings));
    rules = { ...r };
    allowText = r.allow.join("\n");
    neverText = r.never.join("\n");
    rulesOpen = false;
    isGit = false;
    api.git
      .status(id)
      .then((s) => {
        if (project.id === id) isGit = s.is_repo;
      })
      .catch(() => {});
  });

  /** The plan file has a section of this title (its own "Run rules" or "Test build", which win for this plan). */
  const planHas = (title: string) => plan.sections.some((s) => s.title.trim().toLowerCase() === title);

  function currentRules(): RunRules {
    return { ...rules, set: true, allow: parsePatterns(allowText), never: parsePatterns(neverText) };
  }

  /** Keeps the card's answers in the project (pressing Start is the answer, and leaving a Details list: a step set
   * aside for a command can then be tried again from the Inbox with it allowed); false when saving failed. */
  async function saveRules(): Promise<boolean> {
    const next = currentRules();
    if (JSON.stringify(next) === JSON.stringify(rulesOf(project.settings))) return true;
    const e = await projects.update(project.id, project.name, { ...project.settings, run_rules: next });
    if (e) {
      error = `The run rules could not be saved (${e}).`;
      return false;
    }
    rules = { ...next };
    return true;
  }

  /** The check before a run (24i): what the run will need, answered once. */
  let checkFirst = $state(true);
  let checking = $state(false);
  let check = $state<Preflight | null>(null);
  /** The steps the check in hand went through (kept once the run starts), and how many it left out as checked before. */
  let checkSteps: { number: string | null; title: string; details: string[] }[] = [];
  let checkedBefore = $state(0);
  /** Y10: the check's answer as it came (the core reads its grants, eyes, end check, promises and needs). */
  let checkAnswer = "";
  /** Y10 d5: what the steps need that is not on this machine: those steps wait. */
  let probeHolds = $state<{ step: string; why: string; by: "you" | "default" }[]>([]);
  /** e2e1: the real end-to-end check the run rules refuse is the owner's call, shown apart from what is missing. */
  const endToEndHolds = $derived(probeHolds.filter((h) => h.why.startsWith(END_TO_END)));
  const missingHolds = $derived(probeHolds.filter((h) => !h.why.startsWith(END_TO_END)));
  let secretValues = $state<Record<string, string>>({});
  let answers = $state<Record<number, string>>({});
  /** Answers the owner typed or changed (an untouched recommended answer is Qanuvi's decision). */
  let answersTouched = $state<Record<number, boolean>>({});
  let riskyChoice = $state<Record<number, RiskChoice>>({});
  /** Risky steps whose choice the owner set (a hold left at the default is Qanuvi's, not theirs). */
  let riskyTouched = $state<Record<number, boolean>>({});
  /** p2: the typed answers read by meaning (null: not read yet, or changed since). */
  let readings = $state<AnswerReading[] | null>(null);
  let readingAnswers = $state(false);
  /** The owner's yes or no to each value Qanuvi found for an instruction. */
  let used = $state<Record<string, boolean>>({});
  /** The other projects the reading could look in (kept for copying a secret from one by name). */
  let otherProjects: (OtherProject & { id: string })[] = [];

  /** The run's first step key (a secret that names no step holds it). */
  function firstStepKey(): string {
    const s = coveredSteps()[0];
    return s ? (s.number ?? s.title) : "";
  }
  /** What the read answers come to now (found values to offer, steps that will wait). */
  let readNow = $derived(
    check && readings
      ? readAnswers(check, secretValues, answers, answersTouched, readings, used, firstStepKey())
      : null,
  );
  /** Every found value, said yes or no to or not yet (the buttons stay to change one's mind). */
  let foundList = $derived(
    check && readings
      ? readAnswers(check, secretValues, answers, answersTouched, readings, {}, firstStepKey()).offered
      : [],
  );
  /** An answer changed: it is read again at Start. */
  function answerChanged() {
    readings = null;
    used = {};
  }

  /** p2: one short read-only call reads the typed answers together (and tries what an instruction asks). */
  async function readTyped(c: Preflight): Promise<AnswerReading[]> {
    const items = typedAnswers(c, secretValues, answers, answersTouched);
    if (!items.length) return [];
    readingAnswers = true;
    try {
      const all = await api.projects.list().catch(() => [] as Project[]);
      otherProjects = await Promise.all(
        all
          .filter((p) => p.id !== project.id)
          .slice(0, 20)
          .map(async (p) => ({
            id: p.id,
            name: p.name,
            path: p.path,
            secrets: await api.agent.secretNames(p.id).catch(() => [] as string[]),
          })),
      );
      const known = {
        secrets: await api.agent.secretNames(project.id).catch(() => [] as string[]),
        others: otherProjects,
      };
      const r = await planner.ask(
        project,
        answersPrompt(file, items, known),
        oneModel || settings.agent(provider.id).model || defaultPlannerModel(provider),
        "Before the run: reading your answers",
        { agentId: provider.id, lean: true, quiet: true },
      );
      return parseAnswerReadings(
        r.text,
        items.map((x) => x.id),
      );
    } catch {
      // Nobody could read them: sentences in secret boxes are not saved (their steps wait and ask), choices stand.
      return [];
    } finally {
      readingAnswers = false;
    }
  }

  /** The open steps the run would cover, for the check. */
  function coveredSteps() {
    const { ranges: r } = parseRanges(ranges.trim());
    const all = openSteps.filter((s) => s.line >= fromLine);
    const inR = r.length ? all.filter((s) => s.number && inRanges(s.number, r)) : all;
    return (onlyOne ? inR.slice(0, 1) : inR).slice(0, 40);
  }

  /** What earlier runs learned (cost per finished step per model), read when the start panel opens. */
  let learned = $state<Map<string, ModelStats>>(new Map());
  $effect(() => {
    if (setup)
      void runner.learnedStats().then(
        (m) => (learned = m),
        () => (learned = new Map()),
      );
  });
  /** The run's first step, its model and what a step usually costs there: a budget below it stops the run after it. */
  let firstStepCost = $derived.by(() => {
    void ranges;
    void fromLine;
    void onlyOne;
    if (freeOnly && hasFree) return null;
    const s = coveredSteps()[0];
    if (!s) return null;
    const model = oneModel || s.model?.trim() || keptModel(keptState, s) || settings.agent(provider.id).model;
    if (!model || isFree(model)) return null;
    const usual = usualStepCost(learned, model, jobType(s.title));
    return usual === null ? null : { step: s, model, usual };
  });
  let budgetInput = $state<HTMLInputElement | null>(null);
  $effect(() => {
    if (setup && focusBudget && budgetInput) {
      budgetInput.focus();
      budgetInput.select();
    }
  });

  async function runCheck() {
    checking = true;
    error = "";
    try {
      const known = {
        secrets: await api.agent.secretNames(project.id).catch(() => [] as string[]),
        testCommand: project.settings.test_command,
        previewUrl: project.settings.preview_url,
        // 24y: the run rules are standing answers, so the check decides what they cover instead of asking.
        notes: [
          ...loadPlanNotes(project.id, file),
          rulesFacts(currentRules(), mode === "auto"),
          ...(planHas("run rules")
            ? ["This plan file has its own Run rules section: it wins over the project's rules for this plan."]
            : []),
          testBuildFact(project.settings.test_build, planHas("test build")),
        ],
        // PM c: what the project memory knows is used, not asked again.
        memory: await api.memory.lines(project.id).catch(() => [] as string[]),
      };
      // Y10 d7: what earlier checks here missed.
      const lessons = await api.planRun.checkLessons(project.id).catch(() => [] as string[]);
      if (lessons.length)
        known.notes.push(
          `Earlier runs here were held for things this check could have found first; look for the same: ${lessons.join("; ")}`,
        );
      // A run started again (stopped by its budget, paused, ended): steps the check already went through, unchanged
      // since, are not asked about again; their answers, holds and extra checks are kept with the plan.
      const { checked, fresh } = alreadyChecked(loadPlanState(project.id, file), coveredSteps());
      checkedBefore = checked.length;
      const steps = fresh.map((s) => ({ number: s.number, title: s.title, details: s.details }));
      checkSteps = steps;
      if (!steps.length) {
        await go();
        return;
      }
      const r = await planner.ask(
        project,
        preflightPrompt(file, steps, known),
        // Y10 d6: the agent's strongest model reads the files the steps change; else the run's model.
        provider.models[0]?.id || oneModel || settings.agent(provider.id).model || defaultPlannerModel(provider),
        "Before the run: what it needs",
        {
          agentId: provider.id,
          lean: true,
          quiet: true,
        },
      );
      const found = parsePreflight(r.text);
      found.secrets = found.secrets.filter((s) => !known.secrets.includes(s.name));
      checkAnswer = r.text;
      // Y10 d5: what the steps need is tried on the machine now, so the card says what is missing before Start.
      probeHolds = await api.planRun
        .checkSettle(project.id, file, r.text, {}, mode, firstStepKey(), false)
        .then((x) => x.holds)
        .catch(() => []);
      check = found;
      recommendAll(found, false);
      if (
        !found.secrets.length &&
        !found.questions.length &&
        !found.risky.length &&
        !found.setup.length &&
        !found.order.length &&
        !probeHolds.length
      ) {
        check = null;
        savePlanState(project.id, file, keepChecked(loadPlanState(project.id, file), checkSteps));
        // Y10: its eyes, end check and promises are still kept (nothing to ask about them).
        const extra = await api.planRun
          .checkSettle(project.id, file, r.text, {}, mode, firstStepKey(), true)
          .catch(() => null);
        if (extra && (extra.notes.length || extra.said.length))
          savePlanNotes(project.id, file, [
            ...loadPlanNotes(project.id, file),
            ...extra.notes,
            ...extra.said.map((s) => `Settled before the run: ${s}.`),
          ]);
        if (extra) onchanged?.();
        await go(extra?.holds.map((h) => ({ step: h.step, why: holdWording(h) })) ?? [], []);
      }
    } catch (e) {
      error = `The check before the run did not answer (${errorText(e)}). Start without it, or try again.`;
    } finally {
      checking = false;
    }
  }

  /**
   * The check's recommended answers (24u part 3): Qanuvi's choices filled in, steps that truly need the owner left
   * open. `all`: "Accept all recommended", which fills those in too and counts as the owner's own answer.
   */
  function recommendAll(c: Preflight, all: boolean) {
    answers = Object.fromEntries(
      c.questions.map((q, i) => [i, all || q.sort !== "you" ? q.recommended : (answers[i] ?? "")]),
    );
    answersTouched = all ? Object.fromEntries(c.questions.map((q, i) => [i, q.sort === "you"])) : {};
    riskyChoice = Object.fromEntries(c.risky.map((x, i) => [i, recommendedRisk(x.sort)]));
    riskyTouched = {};
    answerChanged();
  }

  /**
   * PM a: the owner's answers about the whole project go into This project memory at once (they are theirs), so no
   * later plan or check asks again; a site named there fills Preview address when the project has none.
   */
  async function keepProjectAnswers(
    c: Preflight,
    given: Record<number, string>,
    touched: Record<number, boolean>,
    saved: { name: string; value: string }[],
  ) {
    const kept = projectAnswers(c, given, touched);
    // pm3: a site address the check asked for as a value to keep is a project fact too.
    const sites = siteAddressValues(c.secrets, saved);
    for (const a of [...kept, ...sites]) await api.memory.keep(project.id, a.title, a.body).catch(() => {});
    const url = kept.map((a) => firstUrl(a.body)).find((u) => u) ?? sites[0]?.url;
    if (url && !project.settings.preview_url.trim())
      await projects.update(project.id, project.name, { ...project.settings, preview_url: url });
  }

  const SORT_LABEL: Record<AskSort, string> = {
    decide: "Qanuvi decides",
    checks: "goes ahead with extra checks",
    you: "needs you",
  };

  /** Saves the answers (secrets, notes) and starts, holding at risky steps and open questions. */
  async function startChecked() {
    const c = check;
    if (!c) return go();
    // p2: the answers are read by meaning first; what Qanuvi found for an instruction waits for the owner's yes.
    const firstStep = firstStepKey();
    if (readings === null) {
      readings = await readTyped(c);
      if (readAnswers(c, secretValues, answers, answersTouched, readings, used, firstStep).offered.length) return;
    }
    const read = readAnswers(c, secretValues, answers, answersTouched, readings, used, firstStep);
    await keepProjectAnswers(c, read.answers, read.answersTouched, read.save);
    for (const s of read.save) await api.agent.setSecret(project.id, s.name, s.value).catch(() => {});
    for (const x of read.copy) {
      const from = otherProjects.find((p) => p.name.toLowerCase() === x.project.toLowerCase());
      const done = from
        ? await api.agent.copySecret(project.id, x.name, from.id, x.from).then(
            () => true,
            () => false,
          )
        : false;
      if (!done)
        read.holds.push({
          step: c.secrets.find((s) => s.name === x.name)?.steps[0] || firstStep,
          why: `it needs ${x.name}; Qanuvi could not copy ${x.from} from ${x.project}. Give the value, or say where Qanuvi can find it`,
          by: "default",
        });
    }
    // Sorted (24u part 3): Qanuvi's own choices, steps with extra checks, and holds only where the owner is needed.
    // Y10 d1-d5: grants, eyes, the end check and promises kept in the core; what is not on this machine waits.
    const extra = await api.planRun
      .checkSettle(project.id, file, checkAnswer, riskyChoice, mode, firstStep, true)
      .catch(() => null);
    for (const h of extra?.holds ?? []) {
      const had = read.holds.find((x) => x.step === h.step);
      // The real end-to-end check the run rules refuse is the owner's own call: its hold leads, the other reason after.
      if (had && h.why.startsWith(END_TO_END)) had.why = `${h.why}; also ${had.why}`;
      else if (!had) read.holds.push({ step: h.step, why: h.why, by: "default" });
    }
    const settled = withReadHolds(
      settleCheck(c, read.answers, read.answersTouched, riskyChoice, riskyTouched),
      read.holds.filter((h) => h.step),
    );
    const notes = [
      ...loadPlanNotes(project.id, file),
      ...(extra?.notes ?? []),
      ...(extra?.said ?? []).map((s) => `Settled before the run: ${s}.`),
      ...settled.notes,
      ...settled.decided.map(decidedNote),
    ];
    // Holds are kept per plan (24u part 2): a later run holds the same steps, saying who set each hold.
    const holds: Omit<KeptHold, "at">[] = settled.holds;
    const cleared = new Set(settled.cleared);
    savePlanNotes(project.id, file, notes);
    let state = keepDecided(loadPlanState(project.id, file), settled.decided, settled.extraChecks, settled.cleared);
    state = {
      ...state,
      holds: state.holds.filter((h) => !cleared.has(h.step) || holds.some((n) => n.step === h.step)),
    };
    savePlanState(project.id, file, keepChecked(keepHolds(state, holds), checkSteps));
    // The check's order decisions (R2e g) become Needs lines in the plan, so the run keeps that order.
    if (c.order.length) {
      try {
        const now = await api.files.read(project.id, file);
        const r = addOrderNeeds(now.text, c.order);
        if (r.added.length) {
          await api.files.write(project.id, file, r.text, now.hash);
          onchanged?.();
        }
      } catch (e) {
        error = `The order from the check was not written into the plan (${errorText(e)}).`;
      }
    }
    check = null;
    await go(
      holds.map((h) => ({ step: h.step, why: holdWording(h) })),
      settled.extraChecks,
    );
  }

  async function start(event: SubmitEvent) {
    event.preventDefault();
    if (!(await saveRules())) return;
    if (checkFirst && !check) return runCheck();
    if (check) return startChecked();
    await go();
  }

  async function go(
    waitBefore: { step: string; why: string }[] = [],
    extraChecks: { step: string; why: string }[] = [],
  ) {
    error = "";
    if (!layout.state.agentVisible) layout.toggleAgent();
    keep();
    onclose();
    try {
      await runner.start(project, file, {
        fromLine: Math.max(0, fromLine),
        onlyOne,
        ranges: ranges.trim(),
        pause,
        mode,
        budgetUsd: Math.max(0, Number(budget) || 0),
        attemptsPerModel: Math.min(3, Math.max(1, Math.round(perModel))),
        lean,
        indexTools,
        packets,
        replan,
        providerId: provider.id,
        model: oneModel,
        strongerOnFail,
        team: "settings",
        teamMode: team === "settings" ? undefined : team,
        carryOn,
        carryAcrossSections: carryAcross,
        fullChecks,
        forgetPrivateWhenDone: forgetPrivate,
        carryLimit: Math.max(20, Math.min(400, Number(carryK) || 150)) * 1000,
        handoverLimit: handoverOn ? Math.max(80, Math.min(800, Number(handoverK) || 200)) * 1000 : 0,
        waitBefore,
        extraChecks,
        onLimit,
        stepCostLimit: Math.max(0, Number(stepUsd) || 0),
        stepMinutes: Math.max(0, Number(stepMin) || 0),
        finishPass: finishAuto,
        // Free decides, free works: the whole run goes to Qanuvi's own agent with the free models.
        ...(freeOnly && hasFree ? { providerId: "qanuvi", model: "" } : {}),
      });
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    }
  }

  let run = $derived(runner.run && runner.run.projectId === project.id && runner.run.file === file ? runner.run : null);
  /**
   * M5b: a run This PC's shutdown cut off, while "Start Qanuvi when Windows starts" is off: offered each time it would
   * have helped (the run would have gone on by itself), until "Don't ask again".
   */
  let startOn = $state<boolean | null>(null);
  let startError = $state("");
  $effect(() => {
    if (run?.stoppedBy !== "shutdown" || shownMachine() !== null || settings.value.startOfferOff) return;
    callThisPc<boolean>("sign_in_start")
      .then((on) => (startOn = on))
      .catch(() => {});
  });
  let startOffer = $derived(
    shownMachine() === null && offerStartWithComputer(run?.stoppedBy, startOn, settings.value.startOfferOff),
  );
  async function turnOnStart() {
    startError = "";
    try {
      startOn = await callThisPc<boolean>("sign_in_start_set", { on: true });
    } catch (e) {
      startError = errorText(e);
    }
  }
  let totals = $derived(runner.totals);
  /** This plan's earlier runs, to compare the cost per finished step (the aim: Qanuvi spends less). */
  let history = $state<ReturnType<typeof planHistory> | null>(null);
  $effect(() => {
    void runner.changed;
    const id = run?.id ?? "";
    api.runs
      .list(project.id)
      .then((records) => (history = planHistory(records, file, id)))
      .catch(() => (history = null));
  });
  let compare = $derived.by(() => {
    if (!run) return "";
    const now = finishedCost(run);
    const tools = run.steps
      .flatMap((s) => s.attempts)
      .reduce(
        (t, a) => ({
          read: t.read + (a.tools?.read ?? 0),
          change: t.change + (a.tools?.change ?? 0),
          run: t.run + (a.tools?.run ?? 0),
        }),
        { read: 0, change: 0, run: 0 },
      );
    const parts: string[] = [];
    if (now.steps) parts.push(`this run ${formatCost(now.cost / now.steps)} per finished step`);
    if (history?.perStep != null) parts.push(`earlier runs ${formatCost(history.perStep)}`);
    if (history && history.runs) parts.push(`this plan so far ${formatCost(history.cost + now.cost)}`);
    const t = tools.read + tools.change + tools.run;
    if (t) parts.push(`tool calls: ${tools.read} reading, ${tools.change} changing, ${tools.run} running`);
    return parts.join(" · ");
  });
  let saved = $derived(run ? freeSavings(run, runner.paidPerStep) : { steps: 0, estimateUsd: 0 });
  let deciderUsd = $derived((run?.deciderCalls ?? []).reduce((t, c) => t + c.costUsd, 0));
  let current = $derived(run?.steps.find((s) => s.status === "running") ?? null);

  /** The finishing pass (24o) runs on the paid decider, whatever agent did the steps. */
  let deciderProvider = $derived(providerById(agent.runProviderId));
  let offer = $derived(!!run && !runner.finishing && offersFinish(run));
  /** FC: what the full pass did with a finding, in a word. */
  const OUTCOME: Record<FindingOutcome["state"], string> = {
    fixed: "Fixed",
    "not-fixed": "Not fixed",
    "needs-you": "Needs you",
  };
  /** FC: the test command the card offers to set, pre-filled from what the run used. */
  let testCommandDraft = $state<string | null>(null);
  let savingTestCommand = $state(false);
  async function setTestCommand(command: string) {
    const c = command.trim();
    if (!c) return;
    savingTestCommand = true;
    error = await projects.update(project.id, project.name, { ...project.settings, test_command: c });
    savingTestCommand = false;
    if (!error) testCommandDraft = null;
  }

  function estimate(level: FinishLevel): string {
    if (!run) return "";
    // Scaled by the pass model's price (output per million tokens): cheap steps under-guess a pass on a dear model.
    const byModel: Record<string, number> = {};
    for (const p of PROVIDERS) for (const m of p.models) if (m.price) byModel[m.id] = m.price[1];
    const pass = byModel[defaultPlannerModel(deciderProvider)] ?? 0;
    const e = finishEstimate(run, level, { pass, byModel });
    const cost = e.costUsd === null ? "cost not known yet" : `about ${formatCost(e.costUsd)}`;
    const time = e.seconds === null ? "" : `, about ${Math.max(1, Math.round(e.seconds / 60))} min`;
    return cost + time;
  }
  async function finishNow(level: FinishLevel) {
    error = "";
    if (!layout.state.agentVisible && level === "full") layout.toggleAgent();
    try {
      await runner.finishPass(project, level);
    } catch (e) {
      error = errorText(e);
    }
  }
  /** The try the step is on: once an attempt finished the step, its after-work (notes, checks) is still that try. */
  let currentAttempt = $derived(
    current ? current.attempts.length + (current.attempts.at(-1)?.outcome === "done" ? 0 : 1) : 0,
  );
  /** Steps set aside for the owner's answer (24u), and the steps waiting for each. */
  let heldSteps = $derived(run?.status === "running" ? run.steps.filter((s) => s.status === "held") : []);
  let replies = $state<Record<string, string>>({});
  const heldKey = (s: StepRun) => s.number ?? s.title;
  function waitingFor(s: StepRun): StepRun[] {
    return (run?.steps ?? []).filter((x) => x.status === "set-aside" && x.waitsFor?.step === heldKey(s));
  }
  function reply(s: StepRun, choice: "go" | "skip" | "end") {
    runner.answer(s, choice, replies[heldKey(s)] ?? "");
    delete replies[heldKey(s)];
  }
</script>

{#if setup}
  <form class="panel setup" onsubmit={start}>
    <div class="row head">
      <strong>Run the plan</strong>
      <button type="button" class="x" aria-label="Close" onclick={onclose}><X size={14} /></button>
    </div>
    <p class="hint">
      Each step runs in a fresh {provider.name} session{oneModel
        ? ` on ${modelLabel(provider, oneModel)}`
        : ` on its own model (the step's model, else ${modelLabel(provider, settings.agent(provider.id).model)})`}. A
      step counts as done when the agent marks it done in the file. These settings are kept for this plan.
    </p>
    <div class="grid">
      <label>
        Agent
        <select value={provider.id} onchange={(e) => pickAgent(e.currentTarget.value)}>
          {#each usable.length ? usable : [provider] as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
        </select>
      </label>
      <label>
        Model for every step
        <select value={oneModel} onchange={(e) => pickModel(e.currentTarget.value)}>
          <option value="">As the plan says (each step's own model)</option>
          {#each provider.models as m (m.id)}<option value={m.id}>{m.label} for every step</option>{/each}
        </select>
      </label>
    </div>
    {#if oneModel}
      <p class="hint">
        {#if ratio !== null}
          {modelLabel(provider, oneModel)} on all {stepModels.length} open steps costs about
          {ratio >= 1.05
            ? `${ratio.toFixed(1)}× as much as`
            : ratio <= 0.95
              ? `${ratio.toFixed(2)}× of`
              : "the same as"}
          the plan's own models.
        {/if}
        Only for this run.
        <button
          type="button"
          class="link"
          disabled={writing}
          title="Every open step keeps this model in the plan's own data (the plan file is not changed)"
          onclick={writeIntoPlan}>{writing ? "Keeping…" : "Keep it for every open step"}</button
        >
      </p>
    {/if}
    <div class="grid">
      <label>
        Start at
        <select bind:value={fromLine}>
          {#each openSteps as s (s.line)}
            <option value={s.line}>{s.number ? `${s.number}. ` : ""}{s.title}</option>
          {/each}
        </select>
      </label>
      <label>
        Permissions
        <select bind:value={mode}>
          {#each provider.modes as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
        </select>
      </label>
      <label>
        Budget for the run (USD, 0 = none)
        <input type="number" min="0" step="0.01" bind:value={budget} bind:this={budgetInput} />
      </label>
      <label title="A step that goes over pauses the run, so a runaway step cannot eat your limit. 0: no limit.">
        Per step at most (USD / minutes, 0 = none)
        <span class="pair">
          <input type="number" min="0" step="0.5" bind:value={stepUsd} />
          <input type="number" min="0" step="5" bind:value={stepMin} />
        </span>
      </label>
      <label>
        {strongerOnFail ? "Failed tries before a stronger model" : "Failed tries before it stops for you"}
        <select bind:value={perModel}>
          <option value={1}>1</option>
          <option value={2}>2</option>
          <option value={3}>3</option>
        </select>
      </label>
    </div>
    <div class="grid">
      <label title="Step numbers, for example 2-6, 10-15. Empty: from the start step to the end.">
        Steps (empty = from the start step on)
        <input type="text" placeholder="for example 2-6, 10-15" bind:value={ranges} disabled={onlyOne} />
      </label>
      <label>
        Stop for me
        <select bind:value={pause}>
          <option value="never"
            >Never: set aside what needs me and go on; wait for my answers only when nothing else is left</option
          >
          <option value="step">After every step</option>
        </select>
      </label>
    </div>
    {#if rangeError}<p class="hint warn">{rangeError}</p>{/if}
    {#if firstStepCost && budgetBelowFirstStep(Number(budget) || 0, firstStepCost.usual)}
      <p class="hint warn">
        The budget of {formatCost(Number(budget) || 0)} is below what a step on {modelLabel(
          provider,
          firstStepCost.model,
        )}
        usually costs (about {formatCost(firstStepCost.usual)}, from earlier runs): the run will likely stop after step
        {firstStepCost.step.number ?? firstStepCost.step.title}.
      </p>
    {/if}
    <p class="hint">
      A step can say what it needs first ("- Needs: 3, 5" or "- Needs: OTHER-PLAN.md#4"). When that is not done, the run
      skips it, writes why under it ("- Skipped: ..."), and carries on.
    </p>
    <label class="check"><input type="checkbox" bind:checked={onlyOne} /> Only this one step</label>
    <label
      class="check"
      title="No MCP servers, and only the file and command tools. About half the start-up tokens, and later steps read them from the cache."
    >
      <input type="checkbox" bind:checked={lean} /> Lean sessions (cheaper; no MCP servers or extra tools)
    </label>
    <label
      class="check"
      title="Find a definition, outline a file, read only the lines needed. Measured about 40% cheaper when a step has to find its way around the code."
    >
      <input type="checkbox" bind:checked={indexTools} /> Code index tools (the agent finds code without reading whole files)
    </label>
    <label
      class="check"
      title="Each step gets the code it mentions up front, and Qanuvi runs the project's test command itself and sends back only the failures (up to two fix rounds)."
    >
      <input type="checkbox" bind:checked={packets} /> Work packets: hand each step its code, and let Qanuvi run
      {project.settings.test_command
        ? project.settings.test_command
        : "the tests (set a test command in the project's settings)"}
    </label>
    <label
      class="check"
      title="When a step fails even on the strongest model, the planner rewrites it once into smaller steps and the run carries on with those."
    >
      <input type="checkbox" bind:checked={replan} /> Back to the planner when a step fails on every model
    </label>
    <label
      class="check"
      title="The next step keeps the session (it knows the work so far) while it uses the same model and is in the same section. Past the size below it starts a new one, with the run's notes; that keeps each step's cost down."
    >
      <input type="checkbox" bind:checked={carryOn} /> Continue the session across steps, up to
      <input type="number" class="small" min="20" max="400" step="10" bind:value={carryK} disabled={!carryOn} />k tokens
    </label>
    <label
      class="check"
      title="A long step re-reads its whole context on every call, and a very long context makes the model lose track of details. Past this size the step's agent finishes the edit in hand, writes a careful progress note in the step's log, and a new session checks that work and goes on. The step's tests and review still decide when it is done."
    >
      <input type="checkbox" bind:checked={handoverOn} /> Hand a long step over to a new session at
      <input type="number" class="small" min="80" max="800" step="10" bind:value={handoverK} disabled={!handoverOn} />k
      tokens
    </label>
    <label class="check" title="Steps of a plan build on each other: the session goes on into the next section too.">
      <input type="checkbox" bind:checked={carryAcross} disabled={!carryOn} /> Also into the next section
    </label>
    <label
      class="check"
      title="Each step tests what it changes; the full test suite and the plan's whole-project checks run once at the last step of each section."
    >
      Full test suite
      <select bind:value={fullChecks}>
        <option value="section">once per section (steps test what they change)</option>
        <option value="step">after every step</option>
      </select>
    </label>
    <label
      class="check"
      title="Logins and passwords given while this plan ran are kept for it in Windows' credential store. With this on they are deleted once every step of the plan is done."
    >
      <input type="checkbox" bind:checked={forgetPrivate} /> Forget the plan's private details (logins) when the plan is finished
    </label>
    <label class="check" title="After the failed tries above, the step goes to the next stronger model.">
      <input type="checkbox" bind:checked={strongerOnFail} /> A step that keeps failing moves to a stronger model
    </label>
    <div class="grid team">
      <label>
        Team for this run
        <select bind:value={team}>
          <option value="settings"
            >As in Settings: {TEAM_LABELS[settings.value.team.mode]}{!hasFree &&
            NEEDS_FREE.includes(settings.value.team.mode)
              ? " (no free models yet: paid only)"
              : ""}</option
          >
          {#each Object.entries(TEAM_LABELS) as [id, name] (id)}
            <option value={id} disabled={!hasFree && NEEDS_FREE.includes(id as TeamMode)}>{name}</option>
          {/each}
        </select>
      </label>
      <label>
        When {provider.name} hits its usage limit
        <select bind:value={onLimit}>
          <option value="wait">Wait for the reset, then go on</option>
          <option value="free" disabled={!hasFree}>Free models work until the reset</option>
        </select>
      </label>
    </div>
    {#if !hasFree}
      <p class="hint">
        Free models are not set up yet (Settings, Models and providers): with them a run can also go on while the paid
        agent is limited.
      </p>
    {:else if team !== "direct" && !freeOnly}
      <p class="hint">Money and security steps, and steps that name their own model, always go to {provider.name}.</p>
    {/if}
    {#if mode === "default"}<p class="hint warn">In Manual mode the run waits for your approval at every edit.</p>{/if}
    <label
      class="check"
      title="One smarter look at the whole run before it is called done, on the paid decider. Light: the project's full checks and a review of the run's whole diff (leftovers, debug code, steps that do not fit together). Full: also a session that fixes what was found, runs live tests where the project defines them (on a scratch copy only) and checks the Done notes against the code. Both end with a short still-open list."
    >
      At the end, the finishing pass
      <select bind:value={finishAuto}>
        <option value="ask">offer it, with its cost (I choose)</option>
        <option value="light">light, by itself (checks and a review of the whole diff)</option>
        <option value="full">full, by itself (also fix what is found, live tests, Done notes checked)</option>
      </select>
    </label>
    <label
      class="check"
      title="The agent reads the open steps once (read-only, a few cents) and lists what the run will need from you: logins, decisions, risky steps, things to set up. You answer once; then the run goes on alone."
    >
      <input type="checkbox" bind:checked={checkFirst} /> Check what the run needs from me first
    </label>
    {#if checking}
      <p class="hint">
        Checking what the run needs from you: one read-only pass on {modelLabel(
          provider,
          provider.models[0]?.id || oneModel || settings.agent(provider.id).model,
        )}, a few minutes (it reads the files the steps change). The run starts after you answer.
      </p>
    {/if}
    <div class="rules-card">
      {#if !rules.set || rulesOpen}
        <strong>While this plan runs alone</strong>
        <p class="hint">
          It may: change files in this project, run its tests and builds.<br />
          It never: publishes, deletes outside the project, uses your passwords, or changes your main branch.
          {#if mode === "auto"}
            Permissions are Auto: anything else the agent judges safe goes ahead; what it is unsure of is set aside with
            the command named, and the run goes on with the rest.
          {:else}
            A step that needs anything else is set aside with the command named, and the run goes on with the rest.
          {/if}
        </p>
        {#if isGit}
          <div class="rules-branch">
            <span>Keep the work on its own git branch?</span>
            <label
              ><input type="radio" name="rules-branch" value={false} bind:group={rules.branch} /> No, Qanuvi keeps its own
              copies</label
            >
            <label><input type="radio" name="rules-branch" value={true} bind:group={rules.branch} /> Yes</label>
          </div>
        {/if}
        {#if rulesOpen}
          <label class="rules-list">
            <span>May also run without asking (one per line; * stands for anything)</span>
            <textarea
              rows="3"
              bind:value={allowText}
              onchange={saveRules}
              placeholder={"make docs\nphp artisan migrate --env=testing"}></textarea>
          </label>
          <label class="rules-list">
            <span>Never runs (besides the fixed never-list)</span>
            <textarea rows="2" bind:value={neverText} onchange={saveRules} placeholder="npm run e2e"></textarea>
          </label>
          <p class="hint">
            Already allowed: the project's test command, its package.json scripts (not dev, start or publish), the usual
            test, check and build tools, reading commands. A plan file can add a "Run rules" section with Allow: and
            Never: lines; it wins for that plan.
          </p>
        {:else}
          <button type="button" class="link" onclick={() => (rulesOpen = true)}>Details</button>
        {/if}
      {:else}
        <p class="hint">
          Run rules: {rulesLine(rules, isGit, mode === "auto")}{planHas("run rules")
            ? " · this plan's own Run rules win"
            : ""} ·
          <button type="button" class="link" onclick={() => (rulesOpen = true)}>Change</button>
        </p>
      {/if}
    </div>
    {#if check}
      <div class="check-list">
        <strong>Before the run</strong>
        {#if checkedBefore}
          <p class="hint">
            {checkedBefore} step{checkedBefore === 1 ? " was" : "s were"} checked before and {checkedBefore === 1
              ? "is"
              : "are"} not asked about again (your answers stand); these questions are about new or changed steps only.
          </p>
        {/if}
        {#if check.secrets.length}
          <p class="hint">
            Logins and keys the steps need. A value is saved as this project's secret; no model sees it. Leave one empty
            to run without it. Not sure? Write a sentence instead (for example "find it in the config" or "the same as
            in another project"): Qanuvi reads it and tries it.
          </p>
          {#each check.secrets as s (s.name)}
            <label class="secret">
              <span class="secret-what">{s.what || s.why}</span>
              {#if s.example}<span class="dim">For example: <code>{s.example}</code></span>{/if}
              {#if s.ifEmpty}<span class="dim">If you leave it empty: {s.ifEmpty}</span>{/if}
              {#if s.what && s.why}<span class="dim small">Why: {s.why}</span>{/if}
              <span class="dim small"
                >Saved as <code>{s.name}</code>{s.steps.length ? ` · steps ${s.steps.join(", ")}` : ""}</span
              >
              <input
                type="password"
                autocomplete="off"
                placeholder={s.example ? `for example ${s.example}` : "value"}
                bind:value={secretValues[s.name]}
                oninput={answerChanged}
              />
            </label>
          {/each}
        {/if}
        {#if check.questions.length || check.risky.length}
          <p class="hint">
            Recommended answers are filled in: Qanuvi decides what has a sensible default (logged as "Decided for you",
            changeable later), lets steps go ahead with extra checks (review and tests) where it can, and waits only for
            what truly needs you.
            <button type="button" class="link" onclick={() => check && recommendAll(check, true)}
              >Accept all recommended</button
            >
          </p>
        {/if}
        {#if check.questions.length}
          <p class="hint">
            Choices the steps leave open (kept as notes for this plan; left empty, the run waits there):
          </p>
          {#each check.questions as q, i (i)}
            <label class="row-input"
              ><span
                ><strong>{q.step}</strong>
                {q.question}
                <span class="sort {q.sort}">{q.fromMemory ? "from memory" : SORT_LABEL[q.sort]}</span
                >{#if q.about === "project" && !q.fromMemory}<span
                    class="dim"
                    title="Your answer goes into This project memory, so later plans and checks do not ask again"
                  >
                    · kept for the project</span
                  >{/if}{#if q.because}<span class="dim">{" "}{q.because}</span>{/if}</span
              >
              <input
                type="text"
                placeholder={q.recommended ? `recommended: ${q.recommended}` : "your answer"}
                bind:value={answers[i]}
                oninput={() => {
                  answersTouched[i] = true;
                  answerChanged();
                }}
              /></label
            >
          {/each}
        {/if}
        {#if check.risky.length}
          <p class="hint">Risky steps (hard to undo, or they reach outside this computer):</p>
          {#each check.risky as x, i (i)}
            <div class="row-input">
              <span
                ><strong>{x.step}</strong>
                {x.why} <span class="sort {x.sort}">{SORT_LABEL[x.sort]}</span>{#if x.because}<span class="dim">
                    {x.because}</span
                  >{/if}{#if x.grant === "delete"}<span class="dim">
                    · Going ahead lets this step delete files inside this project (Qanuvi keeps a copy to undo it).</span
                  >{/if}</span
              >
              <select bind:value={riskyChoice[i]} onchange={() => (riskyTouched[i] = true)}>
                <option value="go">Go ahead</option>
                <option value="checks">Go ahead with extra checks (review and tests)</option>
                <option value="wait">Wait for me before it</option>
              </select>
            </div>
          {/each}
        {/if}
        {#if endToEndHolds.length}
          <p class="hint">Real end-to-end check (only this step waits for your answer; the rest runs):</p>
          <ul>
            {#each endToEndHolds as h, i (i)}<li>
                <strong>Step {h.step}</strong>
                <span class="dim">{h.why.slice(END_TO_END.length).replace(/^:\s*/, "")}</span>
              </li>{/each}
          </ul>
        {/if}
        {#if missingHolds.length}
          <p class="hint">Not on this machine (only these steps wait until it is there; the rest runs):</p>
          <ul>
            {#each missingHolds as h, i (i)}<li>
                <strong>Step {h.step}</strong> <span class="dim">{h.why}</span>
              </li>{/each}
          </ul>
        {/if}
        {#if check.setup.length}
          <p class="hint">To have running or set:</p>
          <ul>
            {#each check.setup as x, i (i)}<li><strong>{x.what}</strong> <span class="dim">{x.why}</span></li>{/each}
          </ul>
        {/if}
        {#if check.order.length}
          <p class="hint">Order (written into the plan as Needs lines when the run starts):</p>
          <ul>
            {#each check.order as x, i (i)}<li>
                <strong>Step {x.step} after {x.after.join(", ")}</strong>
                <span class="dim">{x.why}</span>
              </li>{/each}
          </ul>
        {/if}
        {#if readNow && (foundList.length || readNow.holds.length)}
          <div class="read-answers">
            <strong>Qanuvi read your answers</strong>
            {#each foundList as f (f.id)}
              <div class="row-input">
                <span
                  >{f.ask}:
                  <strong
                    >{f.copyFrom
                      ? `the same as ${f.copyFrom.name} in ${f.copyFrom.project}`
                      : f.secret
                        ? maskedValue(f.value)
                        : f.value}</strong
                  >{#if f.where}<span class="dim"> · found in {f.where}</span>{/if}</span
                >
                <span class="choice">
                  <button
                    type="button"
                    class:primary={used[f.id] === true}
                    onclick={() => (used = { ...used, [f.id]: true })}>Use this</button
                  >
                  <button
                    type="button"
                    class:primary={used[f.id] === false}
                    onclick={() => (used = { ...used, [f.id]: false })}>No</button
                  >
                </span>
              </div>
            {/each}
            {#each readNow.holds.filter((h) => h.step) as h (h.step)}
              <p class="dim small">
                Step {h.step} waits and asks you in the Inbox when the run gets there (the other steps go on): {h.why}.
              </p>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
    <div class="row">
      <span class="spacer"></span>
      <button type="submit" class="primary" disabled={openSteps.length === 0 || !!rangeError || readingAnswers}
        >{#if checking}<LoaderCircle size={13} class="spin" /> Checking…{:else if readingAnswers}<LoaderCircle
            size={13}
            class="spin"
          /> Reading your answers…{:else}<Play size={13} />
          {check ? "Start the run" : checkFirst ? "Check and start" : "Start"}{/if}</button
      >
    </div>
  </form>
{/if}

{#if shownError(error)}<p class="error">{shownError(error)}</p>{/if}

{#if run && !setup}
  <div class="panel status {run.status}">
    <div class="row">
      {#if run.status === "running"}
        <LoaderCircle size={14} class="spin" />
        <span class="main">
          {#if current}
            Step {current.number ?? ""} · {current.title} · {label(runner.currentModel)}
            {#if currentAttempt > 1}<span class="dim"> (try {currentAttempt})</span>{/if}
            {#if runner.waitingHealth}<span class="dim">
                · waiting: {runner.waitingHealth}; goes on by itself when it is back</span
              >{/if}
            {#if runner.waitingMemory}<span class="dim">
                · test build waits for {runner.waitingMemory} GB of free memory</span
              >{:else if runner.testBuilding}<span class="dim">
                · the agent is done; Qanuvi test-builds the app now (its own folder, can take minutes)</span
              >{:else if runner.checking}<span class="dim"> · the agent is done; Qanuvi runs the tests now</span>{/if}
            {#if runner.waitingUntil}<span class="dim">
                · usage limit reached: goes on by itself at {runner.waitingUntil.toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}</span
              >{/if}
          {:else if runner.waitingAnswers}
            {run.message}
          {:else}
            Starting…
          {/if}
        </span>
        {#if runner.waitingAnswers}
          <button
            type="button"
            onclick={() => runner.finishWithout()}
            title="Held steps stay open: answer them later in their chat or with Run">End the run here</button
          >
        {/if}
      {:else}
        <span class="main">
          {run.status === "done"
            ? "Run finished"
            : run.status === "stopped" && run.budgetReached
              ? `Stopped: budget ${formatCost(run.budgetReached)} reached`
              : run.status === "needs-you"
                ? "Run ended: it needs you"
                : run.status === "paused"
                  ? "Run paused"
                  : "Run stopped"}
          {#if run.report}
            <!-- Once the run ended, the counts are the end report's, so the line and the report never disagree. -->
            · This run: {run.report.done.length} of {run.steps.length} done{#if run.report.setAside.length},
              {run.report.setAside.length} set aside{/if}{#if run.report.waiting.length},
              {run.report.waiting.length} waiting for you{/if}
          {:else}
            · This run: {run.steps.filter((s) => s.status === "done").length} of {run.steps.length} done{#if run.steps.some((s) => s.status === "skipped")},
              {run.steps.filter((s) => s.status === "skipped").length} skipped{/if}{#if run.steps.some((s) => s.status === "blocked" || s.status === "failed")},
              {run.steps.filter((s) => s.status === "blocked" || s.status === "failed").length} blocked{/if}{#if run.steps.some((s) => s.status === "held" || s.status === "set-aside")},
              {run.steps.filter((s) => s.status === "held" || s.status === "set-aside").length} set aside{/if}
          {/if}
          · Plan: {plan.steps.filter((s) => s.status === "x").length} of {plan.steps.length} done
          {#if run.message && !(run.status === "stopped" && run.budgetReached)}<span class="dim">
              · {run.message}</span
            >{/if}
        </span>
      {/if}
    </div>
    {#if startOffer}
      <div class="start-offer">
        <span
          >This run was cut off when the computer shut down. With <strong>{startSettingLabel()}</strong> on, Qanuvi starts
          in the background with the computer and runs like this go on by themselves.</span
        >
        <div class="row">
          <button type="button" class="primary" onclick={() => void turnOnStart()}>Turn it on</button>
          <button type="button" onclick={() => settings.set("startOfferOff", true)}>Don't ask again</button>
        </div>
        {#if startError}<span class="bad">{startError}</span>{/if}
      </div>
    {/if}
    {#if run.report && run.status !== "running"}
      {@const rep = run.report}
      <!-- 24y Y5: the end report, in plain words: done; set aside and why; waiting for you, with what to do. -->
      {@const look = rep.look ?? []}
      <details class="report" open={rep.waiting.length > 0 || rep.setAside.length > 0 || look.length > 0}>
        <summary
          >Report: {rep.done.length} done{#if rep.setAside.length}, {rep.setAside.length} set aside{/if}{#if rep.waiting.length},
            {rep.waiting.length} waiting for you{/if}{#if look.length}, {look.length} to look at{/if}</summary
        >
        {#if rep.done.length}<div><strong>Done:</strong> {rep.done.join("; ")}</div>{/if}
        {#if rep.waiting.length}
          <div><strong>Waiting for you</strong></div>
          <ul>
            {#each rep.waiting as l, n (n)}
              <li>
                {l.step}: {l.why}{#if l.action}<br /><span class="act">{l.action}</span>{/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#if rep.setAside.length}
          <div><strong>Set aside</strong></div>
          <ul>
            {#each rep.setAside as l, n (n)}
              <li>{l.step}: {l.why}</li>
            {/each}
          </ul>
        {/if}
        {#if look.length}
          <!-- Y6: checks that need eyes; the run went on, the list waits in the Inbox. -->
          <div><strong>To look at</strong> <span class="dim">(the review list in the Inbox)</span></div>
          <ul>
            {#each look as l, n (n)}
              <li>{l.step}: {l.why}</li>
            {/each}
          </ul>
        {/if}
        {#if rep.branch}<div class="dim">{rep.branch}</div>{/if}
      </details>
    {/if}
    {#if heldSteps.length}
      <!-- Set aside for an answer (24u): answer while the run goes on; it goes back to them after the step in hand. -->
      <div class="asks">
        <strong>Needs your answer ({heldSteps.length})</strong>
        {#each heldSteps as s (heldKey(s))}
          {@const others = waitingFor(s)}
          {@const typed = !!replies[heldKey(s)]?.trim()}
          <div class="ask">
            <div>
              Step {s.number ? `${s.number}. ` : ""}{s.title}
              <span class="dim"
                >· {s.held?.kind === "hold" ? "held" : s.held?.kind === "blocked" ? "blocked" : "not finished"}: {s.held
                  ?.why}{others.length
                  ? ` · ${others.length} step${others.length === 1 ? "" : "s"} wait for it (${others.map(heldKey).join(", ")})`
                  : ""}</span
              >
            </div>
            <!-- n1: the question laid out, so it is answered without reading the chat. -->
            {#if s.held?.card}<HoldCardView card={s.held.card} />{/if}
            <div class="row">
              <textarea
                rows={typed ? 3 : 2}
                aria-label="Your answer"
                placeholder={s.held?.kind === "hold" ? "Your answer (optional)" : ANSWER_HINT}
                title="Enter sends, Shift+Enter starts a new line"
                bind:value={replies[heldKey(s)]}
                onkeydown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey && !e.isComposing && typed) {
                    e.preventDefault();
                    reply(s, "go");
                  }
                }}></textarea>
              <button type="button" class="primary" onclick={() => reply(s, "go")}
                >{typed ? "Send answer" : s.held?.kind === "hold" ? "Go ahead" : "Try again"}</button
              >
              <button type="button" onclick={() => reply(s, "skip")} title="Leave it, and the steps waiting for it"
                >Skip this step</button
              >
              {#if s.held?.kind !== "hold"}
                <button
                  type="button"
                  onclick={() => reply(s, "end")}
                  title="The run finishes what it still can, then ends; this step stays open for the next run"
                  >End the run</button
                >
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
    {#if totals}
      <div class="totals">
        {formatCost(totals.costUsd)} · in {formatTokens(totals.inputTokens)} · out {formatTokens(totals.outputTokens)} ·
        {totals.seconds.toFixed(0)} s
        {#each [...totals.byModel] as [model, t] (model)}
          <span class="model">{label(model)}: {t.attempts}× {formatCost(t.costUsd)}</span>
        {/each}
        {#if compare}
          <span class="saved compare">{compare}</span>
        {/if}
        {#if saved.steps}
          <span class="saved"
            >Free models finished {saved.steps} step{saved.steps === 1 ? "" : "s"}{saved.estimateUsd !== null
              ? `: about ${formatCost(saved.estimateUsd)} saved, going by what paid steps of the same kind cost before${deciderUsd ? `; the decider's checks cost ${formatCost(deciderUsd)}` : ""}`
              : " (what they saved shows once a paid step of the same kind has been measured)"}</span
          >
        {/if}
      </div>
    {/if}
    {#if privateCount}
      <p class="dim private">
        {privateCount} private detail{privateCount === 1 ? "" : "s"} (logins) kept for this plan in Windows' credential store;
        every step gets {privateCount === 1 ? "it" : "them"}.
        <button type="button" class="link" onclick={forgetPrivateNow}>Forget</button>
      </p>
    {/if}
    {#if decidedList.length}
      <details class="decisions" open={decidedShown} ontoggle={(e) => decidedToggled(e.currentTarget.open)}>
        <summary>Decided for you ({decidedList.length}): Qanuvi's choices from the check. Change?</summary>
        <ul>
          {#each decidedList as d, i (d.step + d.what)}
            <li>
              <strong>{d.step}</strong>
              {d.what} → {d.changed ?? d.answer}
              <span class="dim"
                >{d.changed ? `(you changed it from ${d.answer})` : d.because ? `because ${d.because}` : ""}</span
              >
              {#if changing === i}
                <span class="row-input">
                  <input
                    type="text"
                    placeholder="your answer"
                    bind:value={changeText}
                    onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), saveChange(d))}
                  />
                  <button
                    type="button"
                    onclick={() => saveChange(d)}
                    disabled={!changeText.trim() || changeText.trim() === (d.changed ?? d.answer)}>Save</button
                  >
                  <button type="button" class="link" onclick={() => (changing = null)}>Cancel</button>
                </span>
              {:else}
                <button
                  type="button"
                  class="link"
                  onclick={() => {
                    changing = i;
                    changeText = d.changed ?? d.answer;
                    changeDone = "";
                  }}>Change</button
                >
              {/if}
            </li>
          {/each}
        </ul>
        {#if changeDone}
          <p class="dim">
            Step {changeDone} is already done: open it again in the plan (set it back to [ ]) so the next run brings it in
            line. Later steps get the change at once.
          </p>
        {/if}
      </details>
    {/if}
    {#if run.notes?.length}
      <details class="decisions">
        <summary>Notes from this run ({run.notes.length}), kept in the plan's FACTS.md for every step and model</summary
        >
        <ul>
          {#each run.notes as n, i (i)}<li>{n}</li>{/each}
        </ul>
      </details>
    {/if}
    {#if run.decisions?.length}
      <details class="decisions">
        <summary>Decisions ({run.decisions.length})</summary>
        <ul>
          {#each run.decisions as d, i (i)}
            <li><strong>{d.subject}</strong>: {d.decision} <span class="dim">— {d.why} ({d.by})</span></li>
          {/each}
        </ul>
      </details>
    {/if}
    {#if offer}
      <div class="finish">
        <div>
          <strong>Finishing pass</strong>
          <span class="dim">
            one look at the whole run on {deciderProvider.name} before it is called done; it ends with a still-open list.
          </span>
        </div>
        <div class="row">
          <button
            type="button"
            onclick={() => finishNow("light")}
            title="The project's full checks and a review of the run's whole diff">Light · {estimate("light")}</button
          >
          <button
            type="button"
            onclick={() => finishNow("full")}
            title="Also fixes what is found, runs live tests where the project defines them (scratch copies only) and checks the Done notes against the code"
            >Full · {estimate("full")}</button
          >
          <span class="dim">or leave it</span>
        </div>
      </div>
    {:else if run.finish}
      {@const f = run.finish}
      <div class="finish {f.status}">
        <div class="row">
          {#if f.status === "running"}<LoaderCircle size={13} class="spin" />{/if}
          <strong
            >Finishing pass ({f.level}){f.status === "running"
              ? runner.checking
                ? ": running the full checks"
                : ": at work"
              : f.status === "done"
                ? ": done"
                : f.status === "stopped"
                  ? ": stopped"
                  : ": did not finish"}</strong
          >
          {#if f.status !== "running"}
            <span class="dim"
              >{formatCost(f.costUsd)} · {Math.round(f.seconds)} s{f.checks
                ? f.checks.ok
                  ? " · checks pass"
                  : " · checks fail"
                : ""}</span
            >
          {/if}
          <span class="spacer"></span>
          {#if f.status === "running"}
            <button type="button" onclick={() => runner.stop()}><Square size={11} /> Stop</button>
          {:else if f.sessionId}
            {@const sid = f.sessionId}
            <button type="button" class="link" onclick={() => agent.watch(sid)}>Open its session</button>
          {/if}
        </div>
        {#if f.note}<p class="dim">{f.note}</p>{/if}
        {#if f.outcomes?.length}
          {@const fixed = f.outcomes.filter((o) => o.state === "fixed").length}
          {@const more = f.findings.filter((x) => !x.startsWith("Review: "))}
          <details open>
            <summary>Fixed {fixed} of {f.outcomes.length}</summary>
            <ul class="outcomes">
              {#each f.outcomes as o, i (i)}
                <li>
                  <span class="outcome {o.state}">{OUTCOME[o.state]}</span>
                  {o.finding}{#if o.file}
                    · <button type="button" class="link" onclick={() => editor.open(project.id, o.file!)}
                      >{o.file}</button
                    >{/if}{#if o.note}<span class="dim"> · {o.note}</span>{/if}
                </li>
              {/each}
              {#each more as x, i (i)}<li>{x}</li>{/each}
            </ul>
          </details>
        {:else if f.findings.length}
          <details open={f.status !== "running"}>
            <summary>Found ({f.findings.length})</summary>
            <ul>
              {#each f.findings as x, i (i)}<li>{x}</li>{/each}
            </ul>
          </details>
          {#if f.level === "light" && f.status === "done"}
            <div class="row">
              <button
                type="button"
                onclick={() => finishNow("full")}
                title="A full pass fixes what this review found (it does not review again), then runs the full checks"
                >Fix these · {estimate("full")}</button
              >
            </div>
          {/if}
        {/if}
        {#if f.status !== "running" && !project.settings.test_command.trim()}
          {@const draft = testCommandDraft ?? run.testHint ?? ""}
          <div class="row set-test">
            <span>No test command, so the pass could not run the checks.</span>
            <input
              type="text"
              aria-label="Test command"
              placeholder="for example: npm test"
              value={draft}
              oninput={(e) => (testCommandDraft = e.currentTarget.value)}
            />
            <button
              type="button"
              disabled={savingTestCommand || !draft.trim()}
              title={run.testHint ? "The command the run's steps ran the tests with" : ""}
              onclick={() => setTestCommand(draft)}>Set the test command</button
            >
          </div>
        {/if}
        {#if f.status !== "running"}
          <p><strong>Still open</strong>{f.stillOpen.length ? "" : ": nothing"}</p>
          {#if f.stillOpen.length}
            <ul>
              {#each f.stillOpen as x, i (i)}<li>{x}</li>{/each}
            </ul>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .report {
    margin: var(--qx-space-2) 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    font-size: var(--qx-text-small);
    line-height: 1.5;
  }
  .report summary {
    cursor: pointer;
    font-weight: 600;
  }
  .report ul {
    margin: 2px 0 6px;
    padding-left: 18px;
  }
  .report .act {
    color: var(--qx-accent);
  }
  .start-offer {
    margin-top: 6px;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .start-offer .bad {
    color: var(--qx-status-failed);
  }
  .asks {
    margin-top: 6px;
    padding: 6px 8px;
    border: 1px solid var(--qx-status-waiting, var(--qx-border));
    border-radius: var(--qx-radius-card);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .ask .row textarea {
    flex: 1;
    min-width: 0;
    resize: vertical;
    padding: 4px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: inherit;
    font: inherit;
  }
  .finish {
    margin-top: 6px;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .outcomes li {
    margin: 2px 0;
  }
  .outcome {
    font-weight: 600;
    margin-right: 4px;
  }
  .outcome.fixed {
    color: var(--qx-status-done);
  }
  .outcome.not-fixed {
    color: var(--qx-text-secondary);
  }
  .outcome.needs-you {
    color: var(--qx-status-waiting);
  }
  .set-test {
    flex-wrap: wrap;
    gap: var(--qx-space-2);
  }
  .set-test input {
    flex: 1;
    min-width: 160px;
  }
  .finish.failed {
    border-color: var(--qx-status-failed);
  }
  .finish ul {
    margin: 2px 0 0;
    padding-left: 18px;
  }
  .finish p {
    margin: 0;
  }
  .decisions {
    margin-top: 6px;
    font-size: var(--qx-text-small, 11px);
  }

  .decisions ul {
    margin: 4px 0 0;
    padding-left: 16px;
  }

  .panel {
    margin-bottom: var(--qx-space-3);
    padding: var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  .panel.status.running {
    border-color: color-mix(in srgb, var(--qx-status-running) 50%, var(--qx-border));
  }

  .panel.status.needs-you {
    border-color: color-mix(in srgb, var(--qx-status-waiting) 55%, var(--qx-border));
  }

  .panel.status.done {
    border-color: color-mix(in srgb, var(--qx-status-done) 45%, var(--qx-border));
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
  }

  .head {
    justify-content: space-between;
  }

  .main {
    flex: 1;
    min-width: 0;
  }

  .dim {
    color: var(--qx-text-muted);
  }

  .spacer {
    flex: 1;
  }

  .sort {
    font-size: 0.85em;
    padding: 0 0.4em;
    border-radius: 3px;
    border: 1px solid var(--qx-border);
    white-space: nowrap;
  }
  .sort.you {
    color: var(--qx-status-waiting);
  }
  .hint {
    margin: var(--qx-space-2) 0;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  .hint.warn {
    color: var(--qx-status-waiting);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--qx-space-3);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-3);
  }

  select,
  input[type="number"] {
    height: 28px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  button {
    font: inherit;
    cursor: pointer;
  }

  input.small {
    width: 64px;
    height: 24px;
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }

  .compare {
    color: var(--qx-text-secondary);
  }

  .grid.team {
    margin-top: var(--qx-space-3);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
  }

  .link:disabled {
    opacity: 0.6;
  }

  .rules-card {
    margin-top: var(--qx-space-3);
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
  }
  .rules-card .hint {
    margin: var(--qx-space-1) 0;
  }
  .rules-branch {
    display: flex;
    flex-wrap: wrap;
    gap: var(--qx-space-1) var(--qx-space-3);
    align-items: center;
    margin: var(--qx-space-2) 0;
  }
  .rules-list {
    display: flex;
    flex-direction: column;
    gap: var(--qx-space-1);
    margin: var(--qx-space-2) 0;
  }
  .rules-list textarea {
    font-family: var(--qx-font-mono, monospace);
    resize: vertical;
  }
  .check-list {
    margin-top: var(--qx-space-3);
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-accent) 40%, var(--qx-border));
    border-radius: var(--qx-radius-card);
  }
  .read-answers {
    margin-top: var(--qx-space-3);
    padding-top: var(--qx-space-2);
    border-top: 1px solid var(--qx-border);
  }
  .read-answers .choice {
    display: inline-flex;
    gap: var(--qx-space-1);
    flex-shrink: 0;
  }

  .row-input {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 220px;
    align-items: center;
    gap: var(--qx-space-2);
    margin: 4px 0;
    color: var(--qx-text);
  }

  .secret {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 8px 0 10px;
    color: var(--qx-text);
  }

  .secret-what {
    font-weight: 600;
  }

  .secret .small {
    font-size: 0.9em;
  }

  .secret input {
    max-width: 420px;
    margin-top: 4px;
  }

  .row-input input,
  .secret input {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
  }

  .x {
    border: 0;
    background: transparent;
    color: var(--qx-text-muted);
  }

  .primary,
  .stop {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 var(--qx-space-3);
    border: 0;
    border-radius: var(--qx-radius-button);
    color: #fff;
  }

  .primary {
    background: var(--qx-accent);
  }

  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .stop {
    background: var(--qx-status-failed);
  }

  .pause {
    height: 28px;
    padding: 0 var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
  }

  .pause:disabled {
    opacity: 0.7;
    cursor: default;
  }

  .resume {
    height: 26px;
    margin-left: var(--qx-space-2);
    padding: 0 var(--qx-space-3);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
  }

  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: 4px var(--qx-space-3);
    margin-top: var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
    font-variant-numeric: tabular-nums;
  }

  .model {
    color: var(--qx-text-secondary);
  }

  .error {
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }

  .saved {
    display: block;
    margin-top: 2px;
    color: var(--qx-status-done);
  }
</style>
