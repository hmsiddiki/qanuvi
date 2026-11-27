<script lang="ts">
  import { shownError } from "$lib/stores/linkState.svelte";
  import { ask } from "$lib/stores/confirm.svelte";
  import Pause from "@lucide/svelte/icons/pause";
  import Square from "@lucide/svelte/icons/square";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import FileText from "@lucide/svelte/icons/file-text";
  import { SvelteSet } from "svelte/reactivity";
  import { untrack } from "svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { api, errorText, type Project } from "$lib/api";
  import {
    STATUS_LABELS,
    findTaskLists,
    nextStatus,
    parsePlan,
    setStepModel,
    setStepStatus,
    setStepTitle,
    type Plan,
    type Step,
  } from "$lib/plan/tasklist";
  import { agent } from "$lib/stores/agent.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import IconButton from "./IconButton.svelte";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import { layout } from "$lib/layout/layout.svelte";
  import { taskListPrompt } from "$lib/plan/prompts";
  import { listToSteps } from "$lib/plan/liststeps";
  import Play from "@lucide/svelte/icons/play";
  import RunPanel from "./RunPanel.svelte";
  import PlannerDialog from "./PlannerDialog.svelte";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import Archive from "@lucide/svelte/icons/archive";
  import ListPlus from "@lucide/svelte/icons/list-plus";
  import HousekeepingPanel from "./HousekeepingPanel.svelte";

  let showPlanner = $state(false);
  /** The housekeeping panel that is open (24j): archive finished sections, or add another plan's steps. */
  let housekeeping = $state<"archive" | "merge" | null>(null);

  async function plannedInto(path: string) {
    if (path !== file) {
      await discover();
      choose(path);
    } else await load();
  }
  import { runner } from "$lib/stores/runner.svelte";
  import { inbox } from "$lib/stores/inbox.svelte";
  import { openThings, stepKey } from "$lib/plan/openThings";
  import { modelLabel, providerById } from "$lib/agents/providers";
  import { chooseModel, jobType, monthStart, type ModelStats } from "$lib/plan/routing";
  import { loadPicks, pickFor } from "$lib/plan/picks";
  import { planData } from "$lib/plan/planData.svelte";
  import { keptModel, loadPlanState, savePlanState, setKeptModel } from "$lib/plan/planstate";
  import BrainCircuit from "@lucide/svelte/icons/brain-circuit";
  import { formatCost } from "$lib/agent/events";

  let showRun = $state(false);
  /** The start panel was opened by "Raise budget": its budget box gets the focus. */
  let raising = $state(false);
  /** The steps shown under the open start panel anyway ("Show steps"); folded again each time it opens. */
  let peekSteps = $state(false);
  $effect(() => {
    if (showRun) peekSteps = false;
  });
  // The plan's last run (also after a restart): the button says Resume, and a run paused in a step goes on in it.
  $effect(() => {
    if (file) void runner.restoreLast(project, file);
  });

  /** What the current or last run did on a step: the model of its last try, the result and the cost. */
  function stepRunInfo(step: Step) {
    const r = runner.run;
    if (!r || r.projectId !== project.id || r.file !== file) return null;
    const s = r.steps.find((x) => x.number === step.number && x.title === step.title);
    if (!s || (s.attempts.length === 0 && s.status !== "running")) return null;
    const last = s.attempts[s.attempts.length - 1];
    const cost = s.attempts.reduce((sum, a) => sum + a.costUsd, 0);
    return {
      status: s.status,
      model: last?.model ?? runner.currentModel,
      tries: s.attempts.length,
      cost,
      why: s.choice ?? "",
      tools: s.attempts.reduce(
        (t, a) => ({
          read: t.read + (a.tools?.read ?? 0),
          change: t.change + (a.tools?.change ?? 0),
          run: t.run + (a.tools?.run ?? 0),
        }),
        { read: 0, change: 0, run: 0 },
      ),
    };
  }

  /** Opens a new agent session with a ready-made request to write or extend the task list. */
  function askAgent() {
    if (!layout.state.agentVisible) layout.toggleAgent();
    agent.newSession();
    agent.prepare(taskListPrompt(file || "DEVELOPMENT-TASKLIST.md", agent.provider));
  }

  interface Props {
    project: Project;
  }

  let { project }: Props = $props();

  let candidates = $state<string[]>([]);
  let file = $state("");
  let text = $state("");
  let hash = $state("");
  let error = $state("");
  let filter = $state<"all" | "open" | "done">("all");
  let editing = $state<number | null>(null);
  let editText = $state("");
  const open = new SvelteSet<number>();

  let plan = $derived<Plan | null>(text ? parsePlan(text) : null);
  /** A run of this plan is going now. */
  let thisRunning = $derived(runner.running && runner.run?.projectId === project.id && runner.run?.file === file);
  /** The last run of this plan stopped or paused with steps still open: the button says Resume. */
  let canResume = $derived(
    !!runner.run &&
      runner.run.projectId === project.id &&
      runner.run.file === file &&
      runner.run.status !== "done" &&
      runner.run.status !== "running" &&
      !!plan?.steps.some((s) => s.status === " "),
  );

  const FILE_KEY = (id: string) => `qanuvi.plan.${id}`;

  // A step with no Model: line is routed when it runs (project rule, then what earlier runs taught, then the agent's
  // default); the list shows that likely pick, so "Auto" is not a blank.
  let learned = $state<Map<string, ModelStats>>(new Map());
  let spent = $state(0);
  async function loadRouting() {
    learned = await runner.learnedStats().catch(() => new Map());
    spent = (await api.usage.since(monthStart()).catch(() => ({ cost_usd: 0 }))).cost_usd;
  }
  let runProvider = $derived(providerById(agent.runProviderId));
  /** The decider's picks the owner asked for ("Pick models"), kept on this computer for this plan. */
  let picks = $derived.by(() => {
    void runner.picksSaved;
    void planData.changed;
    return file ? loadPicks(planData, project.id, file) : null;
  });
  /** The plan's own data with the models the owner chose for steps here (R2e i): not written into the plan file. */
  let planState = $derived.by(() => {
    void planData.changed;
    return file ? loadPlanState(project.id, file) : null;
  });
  /** The step's model: its own "- Model:" line (written by hand), else the one chosen in Qanuvi. */
  const modelOf = (s: Step): string | null => s.model ?? (planState ? keptModel(planState, s) : null);
  /** Keeps the owner's choice in the plan's data; a "- Model:" line the step has goes, as it would win over it. */
  async function chooseStepModel(step: Step, model: string | null): Promise<boolean> {
    if (step.model && !(await applyToStep(step, (t, line, title) => setStepModel(t, line, null, title)))) return false;
    savePlanState(project.id, file, setKeptModel(loadPlanState(project.id, file), step, model));
    return true;
  }
  let pickNote = $state("");
  async function pickModels() {
    pickNote = "";
    pickNote = await runner.pickModels(project, file);
  }

  function autoPick(step: Step): { label: string; why: string; picked: boolean } {
    const p = pickFor(picks, { number: step.number ?? String(step.line), title: step.title }, runProvider.id);
    const c = chooseModel({
      picked: p?.model ? { model: p.model, why: p.why } : null,
      stepModel: null,
      type: jobType([step.title, step.note ?? "", ...(step.details ?? [])].join("\n")),
      rules: settings.value.rules[project.id] ?? {},
      stats: learned,
      models: runProvider.models,
      defaultModel: settings.agent(runProvider.id).model,
      budget: { monthlyUsd: settings.value.monthlyBudgetUsd, spentUsd: spent },
    });
    const why = c.reason === "default model" ? "the agent's default model (Settings, Agents)" : c.reason;
    // The decider's pick is what the run will use (unless a project rule comes first): say so, not "Auto".
    const picked = !!p?.model && c.model === p.model;
    return { label: modelLabel(runProvider, c.model), why, picked };
  }

  async function discover() {
    try {
      // Any folder counts, also one the project's .gitignore leaves out (a plugin's own plan, for example).
      const found = findTaskLists(await api.files.markdown(project.id));
      // Same list: nothing to change (opening the list must not redraw it under the pointer).
      if (found.join("\n") !== candidates.join("\n")) candidates = found;
      // The plan open now stays open while it is still there; a fresh project opens its saved plan.
      const saved = localStorage.getItem(FILE_KEY(project.id));
      if (!(file && candidates.includes(file)))
        file = saved && candidates.includes(saved) ? saved : (candidates[0] ?? "");
    } catch (e) {
      error = errorText(e);
    }
  }

  /** The status dot: next status. Marking a step done by hand asks first (a stray click must not skip work). */
  async function cycleStatus(step: Step) {
    const next = nextStatus(step.status);
    if (
      next === "x" &&
      !(await ask(
        `Mark step ${step.number ? step.number + ". " : ""}${step.title} as done? A plan run then skips it.`,
        { title: "Mark as done", kind: "warning", okLabel: "Mark done", cancelLabel: "Cancel" },
      ))
    )
      return;
    void applyToStep(step, (t, line, title) => setStepStatus(t, line, next, title));
  }

  /** A step's own chat is working now: a session of this plan's run record for this step, shown running or waiting. */
  function stepChatWorking(step: Step): boolean {
    const r = runner.run;
    if (!r || r.projectId !== project.id || r.file !== file) return false;
    const s = r.steps.find((x) => x.number === step.number && x.title === step.title);
    if (!s) return false;
    const ids = new Set(s.attempts.map((a) => a.sessionId));
    return agent.sessions.some((x) => ids.has(x.id) && ["running", "waiting"].includes(x.status));
  }

  /** f1: a plan written as a plain list, and how many steps it would give; null when it has steps or no list. */
  let listed = $derived(plan && !plan.sections.some((s) => s.steps.length) ? listToSteps(text) : null);

  /** f1: turns the plan's plain list into checkbox steps, from the file as it is on disk now. */
  async function makeSteps() {
    try {
      const now = await api.files.read(project.id, file);
      const r = listToSteps(now.text);
      if (!r) return load();
      await api.files.write(project.id, file, r.text, now.hash);
      await load();
    } catch (e) {
      error = `The list was not turned into steps (${errorText(e)}).`;
    }
  }

  async function load() {
    if (!file) {
      text = "";
      return;
    }
    try {
      const content = await api.files.read(project.id, file);
      text = content.text;
      hash = content.hash;
      error = "";
    } catch (e) {
      error = errorText(e);
    }
  }

  /** The step in freshly read steps: by number and title, else by number, else by title at the same line. */
  function findStep(steps: Step[], step: Step): Step | undefined {
    return (
      steps.find((s) => s.number === step.number && s.title === step.title) ??
      (step.number ? steps.find((s) => s.number === step.number) : undefined) ??
      steps.find((s) => s.line === step.line && s.title === step.title)
    );
  }

  /**
   * Applies an edit to one step of the file as it is on disk now, not as it was shown: an agent may have added or
   * moved steps since (there is no file watcher). Tries twice in case the file changes between reading and writing;
   * returns whether the edit was saved. A failure stays on screen and the list shows the file as it is.
   */
  async function applyToStep(step: Step, edit: (current: string, line: number, title: string) => string) {
    let failure = "";
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const now = await api.files.read(project.id, file);
        const found = findStep(parsePlan(now.text).steps, step);
        if (!found) throw new Error(`Step ${step.number ?? step.title} is no longer in ${file}.`);
        const next = edit(now.text, found.line, found.title);
        hash = await api.files.write(project.id, file, next, now.hash);
        text = next;
        error = "";
        return true;
      } catch (e) {
        failure = errorText(e);
      }
    }
    await load();
    error = `Not saved: ${failure}`;
    return false;
  }

  // The list follows changes made outside the view (an agent adding steps), so it never shows an old file.
  $effect(() => {
    if (!file) return;
    const timer = setInterval(async () => {
      if (document.hidden || editing !== null) return;
      const now = await api.files.read(project.id, file).catch(() => null);
      if (now && now.hash !== hash) {
        text = now.text;
        hash = now.hash;
      }
    }, 4000);
    return () => clearInterval(timer);
  });

  // f8: with no plan found, look again when a chat of this project stops working (the agent may have written one,
  // "Ask the agent to write one"). Listing the folder on a timer would cost too much on a big project.
  let working = $derived(agent.sessions.filter((s) => ["running", "waiting"].includes(s.status)).length);
  let lastWorking = 0;
  $effect(() => {
    const now = working;
    untrack(() => {
      if (now < lastWorking && !file) void discover().then(load);
      lastWorking = now;
    });
  });

  // Only another project reopens the view. Saving this project's settings (the run rules on Start) hands in a new
  // project object with the same id; reopening then dropped the Run panel and the check's answer with it.
  let projectId = $derived(project.id);
  $effect(() => {
    const id = projectId;
    untrack(() => {
      open.clear();
      // Another project: its own saved plan opens, not a same-named file of the last one.
      file = "";
      candidates = [];
      discover().then(load);
      void loadRouting();
      // Untracked: this effect must not re-run (and reload an old record) whenever the run changes.
      runner.loadLast(id);
    });
  });

  // Reload the list while a run changes it.
  $effect(() => {
    if (runner.changed) load();
  });

  function choose(path: string) {
    file = path;
    localStorage.setItem(FILE_KEY(project.id), path);
    open.clear();
    load();
  }

  const visible = (s: Step) => filter === "all" || (filter === "done" ? s.status === "x" : s.status !== "x");

  function startEdit(step: Step) {
    editing = step.line;
    editText = step.title;
  }

  function finishEdit(step: Step, save: boolean) {
    const newTitle = editText;
    if (save && newTitle.trim() && newTitle.trim() !== step.title)
      void applyToStep(step, (t, line, title) => setStepTitle(t, line, newTitle, title));
    editing = null;
  }

  function done(steps: Step[]) {
    return steps.filter((s) => s.status === "x").length;
  }

  /** Ticked steps the plan's open review list still names: shown half-orange, counted apart. */
  let openByStep = $derived(file ? openThings(inbox.items, project.id, file) : new Map<string, string[]>());
  const openOf = (s: Step) => (s.status === "x" ? (openByStep.get(stepKey(s)) ?? []) : []);
  const doneOpen = (steps: Step[]) => steps.filter((s) => openOf(s).length).length;
  /** "2 done with things open" after a count, or "". */
  const openWords = (steps: Step[]) => {
    const n = doneOpen(steps);
    return n ? ` · ${n} done with things open` : "";
  };
</script>

<div class="plan">
  <header>
    <FileText size={14} />
    {#if candidates.length > 1}
      <!-- Looked up again as the list opens: a plan written after the project opened shows up (an agent's new PLAN-E). -->
      <select
        value={file}
        onfocus={() => discover()}
        onpointerdown={() => discover()}
        onchange={(e) => choose((e.target as HTMLSelectElement).value)}
        aria-label="Task list"
      >
        {#each candidates as c (c)}<option value={c}>{c}</option>{/each}
      </select>
    {:else}
      <span class="file">{file || "No task list"}</span>
    {/if}
    <div class="filters" role="group" aria-label="Show">
      {#each ["all", "open", "done"] as f (f)}
        <button type="button" class:on={filter === f} onclick={() => (filter = f as typeof filter)}>
          {f === "all" ? "All" : f === "open" ? "Open" : "Done"}
        </button>
      {/each}
    </div>
    {#if file}
      <button type="button" class="link" onclick={() => editor.open(project.id, file)}>Open file</button>
    {/if}
    {#if file && settings.value.ai}
      {#if thisRunning}
        <!-- Running: Pause (after the step it is on) and Stop. -->
        <button
          type="button"
          class="run-btn"
          disabled={runner.pausing}
          title={runner.pausing
            ? "Pausing: the agent is finishing the edit it is making"
            : "The agent finishes the edit it is making, notes where it is, and stops; Resume goes on in the same session"}
          onclick={() => runner.pause()}><Pause size={12} /> {runner.pausing ? "Pausing…" : "Pause"}</button
        >
        <button
          type="button"
          class="run-btn stop"
          title={runner.stopping
            ? "Click again to stop at once"
            : "The agent finishes the edit it is making, then the run stops"}
          onclick={() => runner.stop()}
          ><Square size={11} /> {runner.stopping ? "Stopping (finishing its edit)…" : "Stop"}</button
        >
      {:else}
        <!-- Not running: Resume when the last run stopped with steps left, else Run; both open the run's settings. -->
        <button
          type="button"
          class="run-btn"
          disabled={runner.running}
          title={canResume
            ? "Go on from the next open step (the settings open as they were, to change if you like)"
            : "Run the plan"}
          onclick={() => {
            raising = false;
            showRun = !showRun;
          }}><Play size={12} /> {canResume ? "Resume" : "Run"}</button
        >
        {#if canResume && runner.run?.status === "stopped" && runner.run.budgetReached}
          <!-- b1: the run stopped on its own budget; say so where Resume is, with the way out next to it. -->
          <button
            type="button"
            class="link"
            title="The last run stopped at its budget of {formatCost(
              runner.run.budgetReached,
            )}: open the run's settings with the budget ready to change"
            onclick={() => {
              raising = true;
              showRun = true;
            }}>Raise budget</button
          >
        {/if}
      {/if}
    {/if}
    {#if settings.value.ai}
      <IconButton
        label="Plan a request: a strong model turns it into steps"
        onclick={() => (showPlanner = !showPlanner)}
      >
        <WandSparkles size={13} />
      </IconButton>
      <IconButton label="Ask the agent to write or extend the task list" onclick={askAgent}>
        <Sparkles size={13} />
      </IconButton>
    {/if}
    {#if file && settings.value.ai}
      <!-- Only when the owner asks: a run never picks models by itself. Steps with a model of their own are left alone. -->
      <IconButton
        label={runner.picking
          ? "Picking models…"
          : "Pick models: the decider reads the open steps set to Auto and picks a model for each (quality first; models that burned many tokens before only where needed). Your own choices are kept."}
        disabled={runner.picking || thisRunning}
        onclick={pickModels}
      >
        <BrainCircuit size={13} />
      </IconButton>
    {/if}
    {#if file}
      <IconButton
        label={thisRunning
          ? "Archive finished sections (not while a run of this plan is going)"
          : "Archive finished sections: move them to a file next to the plan"}
        disabled={thisRunning}
        onclick={() => (housekeeping = housekeeping === "archive" ? null : "archive")}
      >
        <Archive size={13} />
      </IconButton>
      <IconButton
        label={thisRunning
          ? "Add steps from another plan (not while a run of this plan is going)"
          : "Add steps from another plan"}
        disabled={thisRunning}
        onclick={() => (housekeeping = housekeeping === "merge" ? null : "merge")}
      >
        <ListPlus size={13} />
      </IconButton>
    {/if}
    <IconButton
      label="Reload the plan and look for new plan files"
      onclick={() => {
        void loadRouting();
        void discover().then(load);
      }}><RefreshCw size={13} /></IconButton
    >
  </header>

  {#if shownError(error)}<p class="error">{shownError(error)}</p>{/if}
  {#if runner.picking}<p class="pick-note">Picking models for the open steps…</p>
  {:else if pickNote}<p class="pick-note">{pickNote}</p>{/if}

  <div class="body">
    {#if showPlanner && settings.value.ai}
      <PlannerDialog
        {project}
        file={file || "TASKLIST.md"}
        onclose={() => (showPlanner = false)}
        onadded={plannedInto}
      />
    {/if}
    {#if housekeeping && file && !thisRunning}
      <HousekeepingPanel
        {project}
        {file}
        {candidates}
        mode={housekeeping}
        onclose={() => (housekeeping = null)}
        ondone={() => {
          open.clear();
          void load();
        }}
      />
    {/if}
    {#if !file}
      <div class="empty">
        <p>No task list was found in this project.</p>
        <p class="muted">
          Qanuvi looks for Markdown files named like DEVELOPMENT-TASKLIST.md, TODO.md, PLAN.md or ROADMAP.md.
        </p>
        {#if settings.value.ai}
          <button type="button" class="ask" onclick={askAgent}><Sparkles size={13} /> Ask the agent to write one</button
          >
        {/if}
      </div>
    {:else if plan && !plan.sections.some((s) => s.steps.length)}
      <div class="empty">
        <p>No steps found in {file}.</p>
        <p class="muted">
          Qanuvi reads steps written as checkbox lines, for example <code>- [ ] 1. Add the export</code>.
        </p>
        {#if listed}
          <button type="button" class="ask" onclick={makeSteps}>
            Turn the {listed.count === 1 ? "list item" : `${listed.count} list items`} into steps
          </button>
          <p class="muted">Only the list lines change; the rest of the file stays as it is.</p>
        {:else if settings.value.ai}
          <button type="button" class="ask" onclick={askAgent}
            ><Sparkles size={13} /> Ask the agent to write steps</button
          >
        {/if}
      </div>
    {:else if plan}
      <RunPanel
        {project}
        {file}
        {plan}
        setup={showRun}
        focusBudget={raising}
        resume={canResume}
        onclose={() => {
          showRun = false;
          raising = false;
        }}
        onchanged={load}
      />
      {#if showRun}
        <!-- The start panel has the screen; the steps wait under one line (a range may need their numbers). -->
        {@const all = plan.sections.flatMap((s) => s.steps)}
        <div class="steps-hidden">
          {all.length} steps · {all.length - done(all)} open{openWords(all)}
          <button type="button" class="link" onclick={() => (peekSteps = !peekSteps)}
            >{peekSteps ? "Hide steps" : "Show steps"}</button
          >
        </div>
      {/if}
      <div class="steps" hidden={showRun && !peekSteps}>
        {#each plan.sections.filter((s) => s.steps.length) as section (section.line)}
          {@const total = section.steps.length}
          {@const finished = done(section.steps)}
          <section>
            <h3>
              <span>{section.title || "Steps"}</span>
              <span
                class="count"
                title={doneOpen(section.steps)
                  ? `${doneOpen(section.steps)} of the done steps have things open (see the Inbox list to look at)`
                  : undefined}
                >{finished}/{total}{#if doneOpen(section.steps)}<span class="open-count"
                    >{` · ${doneOpen(section.steps)} with things open`}</span
                  >{/if}</span
              >
            </h3>
            <div class="progress"><span style:width="{(finished / total) * 100}%"></span></div>
            {#each section.steps.filter(visible) as step (step.line)}
              {@const info = stepRunInfo(step)}
              <div class="step" class:expanded={open.has(step.line)}>
                <div class="row">
                  <button
                    type="button"
                    class="status s{step.status === ' '
                      ? 'open'
                      : step.status === '-'
                        ? 'doing'
                        : step.status === 'x'
                          ? 'done'
                          : 'blocked'}"
                    class:part={openOf(step).length > 0}
                    class:working={(runner.running &&
                      runner.run?.file === file &&
                      runner.run.steps.some(
                        (r) => r.status === "running" && r.number === step.number && r.title === step.title,
                      )) ||
                      // A step chat working outside a run: one of this plan's sessions for this very step, by the
                      // run record's session ids (a title like "Step 4:" also fits other plans' step 4).
                      (!thisRunning && stepChatWorking(step))}
                    title="{openOf(step).length
                      ? `Done, with things open: ${openOf(step).join('; ')}`
                      : STATUS_LABELS[step.status]} (click to change)"
                    aria-label="Status: {STATUS_LABELS[step.status]}"
                    onclick={() => cycleStatus(step)}
                  ></button>
                  <button
                    type="button"
                    class="toggle"
                    aria-label={open.has(step.line) ? "Hide details" : "Show details"}
                    onclick={() => (open.has(step.line) ? open.delete(step.line) : open.add(step.line))}
                  >
                    {#if open.has(step.line)}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
                  </button>
                  {#if step.number}<span class="num">{step.number}</span>{/if}
                  {#if editing === step.line}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      class="title-edit"
                      bind:value={editText}
                      autofocus
                      onkeydown={(e) => {
                        if (e.key === "Enter") finishEdit(step, true);
                        if (e.key === "Escape") finishEdit(step, false);
                      }}
                      onblur={() => finishEdit(step, true)}
                    />
                  {:else}
                    <button
                      type="button"
                      class="title"
                      title="Double-click to rename"
                      ondblclick={() => startEdit(step)}
                    >
                      {step.title}
                    </button>
                  {/if}
                  {#if info}
                    <span
                      class="run-chip {info.status}"
                      title="{info.tries} tr{info.tries === 1 ? 'y' : 'ies'} in the run{info.why
                        ? ` · started on this model: ${info.why}`
                        : ''}{info.tools.read + info.tools.change + info.tools.run
                        ? ` · tool calls: ${info.tools.read} reading, ${info.tools.change} changing, ${info.tools.run} running`
                        : ''}"
                    >
                      {modelLabel(runProvider, info.model)}{info.status === "running" ? " …" : ""} · {formatCost(
                        info.cost,
                      )}
                    </span>
                  {/if}
                  <select
                    class="model"
                    value={modelOf(step) ?? ""}
                    title={!modelOf(step)
                      ? autoPick(step).picked
                        ? `Picked by the decider (Pick models): ${autoPick(step).label}: ${autoPick(step).why}. The run uses it. Choose a model to pin this step to another one.`
                        : `Auto: picked when the step runs. Now that would be ${autoPick(step).label}: ${autoPick(step).why}. Choose a model to pin this step to it.`
                      : "Model for this step (Auto lets the run pick)"}
                    onchange={async (e) => {
                      const el = e.target as HTMLSelectElement;
                      const saved = await chooseStepModel(step, el.value || null);
                      // Not saved: show what is kept, not the choice that did not stick.
                      if (!saved) el.value = modelOf(findStep(plan?.steps ?? [], step) ?? step) ?? "";
                      // The step that is running hands over to the chosen model after the edit in hand; any other
                      // step starts on it when its turn comes.
                      else if (el.value) runner.switchStepModel(project.id, file, step.number, step.title, el.value);
                    }}
                  >
                    <option value=""
                      >{modelOf(step)
                        ? "Auto"
                        : `${autoPick(step).picked ? "Picked" : "Auto"} · ${autoPick(step).label}`}</option
                    >
                    <!-- f10: the run's agent, not the chat's: a step chat on the free agent showed raw ids here. -->
                    {#each runProvider.models as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
                    {#if modelOf(step) && !runProvider.models.some((m) => m.id === modelOf(step))}
                      <option value={modelOf(step)}>{modelLabel(runProvider, modelOf(step) ?? "")}</option>
                    {/if}
                  </select>
                </div>
                {#if step.note}<p class="note">{step.note}</p>{/if}
                {#if open.has(step.line)}
                  <div class="details">
                    {#each step.details as d, i (i)}<div>{d}</div>{/each}
                    {#if step.details.length === 0}<div class="muted">No details.</div>{/if}
                    <button type="button" class="link" onclick={() => editor.open(project.id, file, step.line + 1)}>
                      Show in the file
                    </button>
                  </div>
                {/if}
              </div>
            {/each}
          </section>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .plan {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 6px var(--qx-space-3);
    border-bottom: 1px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }

  .file {
    color: var(--qx-text);
  }

  select {
    max-width: 260px;
    height: 26px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    font-size: var(--qx-text-small);
  }

  .filters {
    display: flex;
    margin-left: auto;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    overflow: hidden;
  }

  .filters button {
    padding: 3px 10px;
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .filters button.on {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .run-btn {
    display: inline-flex;
    flex-shrink: 0;
    white-space: nowrap;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: 6px;
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .run-btn.stop {
    background: var(--qx-status-failed);
  }

  .run-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .run-chip {
    flex: none;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    font-size: 11px;
    white-space: nowrap;
  }

  .run-chip.running {
    color: var(--qx-status-running);
  }

  .run-chip.done {
    color: var(--qx-status-done);
  }

  .run-chip.failed,
  .run-chip.blocked {
    color: var(--qx-status-failed);
  }

  .link {
    border: 0;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .pick-note {
    margin: 0;
    padding: var(--qx-space-1) var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .error {
    margin: 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--qx-space-2) var(--qx-space-3) var(--qx-space-6);
  }

  section {
    margin-bottom: var(--qx-space-4);
  }

  .steps-hidden {
    display: flex;
    gap: var(--qx-space-2);
    align-items: center;
    margin: var(--qx-space-2) 0;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }
  h3 {
    display: flex;
    justify-content: space-between;
    margin: var(--qx-space-3) 0 var(--qx-space-1);
    font-size: 13px;
    font-weight: 600;
  }

  .count {
    color: var(--qx-text-muted);
    font-weight: 400;
    font-size: var(--qx-text-small);
  }

  .progress {
    height: 3px;
    margin-bottom: var(--qx-space-2);
    border-radius: 2px;
    background: var(--qx-border);
    overflow: hidden;
  }

  .progress span {
    display: block;
    height: 100%;
    background: var(--qx-status-done);
  }

  .step {
    padding: 4px var(--qx-space-1);
    border-radius: 8px;
  }

  .step:hover,
  .step.expanded {
    background: var(--qx-bg-raised);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status {
    flex: none;
    width: 14px;
    height: 14px;
    padding: 0;
    border: 2px solid var(--qx-text-muted);
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
  }

  /* The step a run is working on now: a ring that turns. */
  /* Placed before the status colours, so it is written with more weight: a step being worked on always shows the ring. */
  .status.working.working {
    border-color: color-mix(in srgb, var(--qx-status-running) 30%, transparent);
    border-top-color: var(--qx-status-running);
    background: transparent;
    animation: qx-step-spin 0.9s linear infinite;
  }

  @keyframes qx-step-spin {
    to {
      transform: rotate(360deg);
    }
  }

  .status.sdoing {
    border-color: var(--qx-status-running);
    background: conic-gradient(var(--qx-status-running) 0 50%, transparent 50% 100%);
  }

  .status.sdone {
    border-color: var(--qx-status-done);
    background: var(--qx-status-done);
  }

  /* Ticked, but with things open (left for later, checks that need eyes, things to look at): half orange. */
  .status.sdone.part {
    border-color: var(--qx-status-waiting);
    background: linear-gradient(90deg, var(--qx-status-waiting) 50%, transparent 50%);
  }

  .open-count {
    color: var(--qx-status-waiting);
  }

  .status.sblocked {
    border-color: var(--qx-status-failed);
    background: var(--qx-status-failed);
  }

  .toggle {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .num {
    flex: none;
    min-width: 26px;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 12px;
  }

  .title {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    cursor: default;
  }

  .title-edit {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--qx-accent);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  .model {
    flex: none;
    max-width: 130px;
    height: 22px;
    opacity: 0.75;
  }

  .step:hover .model {
    opacity: 1;
  }

  .note {
    margin: 2px 0 0 46px;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .details {
    margin: 6px 0 4px 46px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
    white-space: pre-wrap;
    user-select: text;
  }

  .ask {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: var(--qx-space-3);
    padding: 6px 12px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }

  .empty {
    margin-top: 80px;
    text-align: center;
    color: var(--qx-text-secondary);
  }

  .muted {
    color: var(--qx-text-muted);
  }
</style>
