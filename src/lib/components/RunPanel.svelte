<script lang="ts">
  import { agent } from "$lib/stores/agent.svelte";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import type { Project } from "$lib/api";
  import { setStepModel, type Plan } from "$lib/plan/tasklist";
  import { modelLabel, providerById, PROVIDERS } from "$lib/agents/providers";
  import { api, errorText } from "$lib/api";
  import { formatCost, formatTokens } from "$lib/agent/events";
  import { runner } from "$lib/stores/runner.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { costRatio, freeSavings, isFree, DECIDER, parseRanges, inRanges, type PauseMode } from "$lib/plan/runner";
  import { freeWorks, type TeamMode } from "$lib/core/decider";
  import { loadPlanNotes, runSettingsKey, savePlanNotes } from "$lib/plan/guidance";
  import { parsePreflight, preflightPrompt, type Preflight } from "$lib/plan/preflight";
  import { planner, defaultPlannerModel } from "$lib/stores/planner.svelte";

  interface Props {
    project: Project;
    file: string;
    plan: Plan;
    /** The setup form is shown (the user pressed Run). */
    setup: boolean;
    onclose: () => void;
    /** The plan file was changed here (the steps' models written in). */
    onchanged?: () => void;
    /** Resume: show the run's settings (as they were) to change and start from the next open step. */
    onresume?: () => void;
  }

  let { project, file, plan, setup, onclose, onchanged, onresume }: Props = $props();

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
  /** Steps in a row on one model and section continue a session, up to a size (thousands of tokens). */
  let carryOn = $state(true);
  let carryK = $state(150);
  /** Sessions also go on into the next section; full checks per step or per section. */
  let carryAcross = $state(true);
  let fullChecks = $state<"step" | "section">("section");
  /** The plan's private details are deleted once every step is done. */
  let forgetPrivate = $state(true);
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
  /** The steps the run would cover, with the model each would use by the plan. */
  let stepModels = $derived(
    openSteps.map((s) => s.model?.trim() || settings.agent(provider.id).model).filter((m) => !!m),
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
    carryAcross: boolean;
    fullChecks: "step" | "section";
    forgetPrivate: boolean;
    checkFirst: boolean;
    onLimit: "wait" | "free";
  }
  function restore(): boolean {
    try {
      const k = JSON.parse(localStorage.getItem(KEY()) ?? "null") as Partial<Kept> | null;
      if (!k) return false;
      if (k.providerId && usable.some((p) => p.id === k.providerId)) providerId = k.providerId;
      const p = providerById(providerId || agent.runProviderId);
      oneModel = k.oneModel && p.models.some((m) => m.id === k.oneModel) ? k.oneModel : "";
      strongerOnFail = k.strongerOnFail ?? !oneModel;
      if (k.team === "settings" || (typeof k.team === "string" && k.team in TEAM_LABELS)) team = k.team;
      if (typeof k.budget === "number") budget = k.budget;
      if (typeof k.perModel === "number") perModel = k.perModel;
      if (k.pause) pause = k.pause;
      if (k.mode && p.modes.some((m) => m.id === k.mode)) mode = k.mode;
      if (typeof k.lean === "boolean") lean = k.lean;
      if (typeof k.indexTools === "boolean") indexTools = k.indexTools;
      if (typeof k.packets === "boolean") packets = k.packets;
      if (typeof k.replan === "boolean") replan = k.replan;
      if (typeof k.carryOn === "boolean") carryOn = k.carryOn;
      if (typeof k.carryK === "number") carryK = k.carryK;
      if (typeof k.carryAcross === "boolean") carryAcross = k.carryAcross;
      if (k.fullChecks === "step" || k.fullChecks === "section") fullChecks = k.fullChecks;
      if (typeof k.forgetPrivate === "boolean") forgetPrivate = k.forgetPrivate;
      if (typeof k.checkFirst === "boolean") checkFirst = k.checkFirst;
      if (k.onLimit === "wait" || k.onLimit === "free") onLimit = k.onLimit;
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
      carryAcross,
      fullChecks,
      forgetPrivate,
      checkFirst,
      onLimit,
    };
    localStorage.setItem(KEY(), JSON.stringify(k));
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

  /** Writes the chosen model into every open step's Model line, so the plan keeps it. */
  let writing = $state(false);
  async function writeIntoPlan() {
    if (!oneModel) return;
    writing = true;
    error = "";
    try {
      const now = await api.files.read(project.id, file);
      let text = now.text;
      // From the bottom up: a Model line added to a step moves the lines below it.
      for (const s of [...openSteps].sort((a, b) => b.line - a.line))
        text = setStepModel(text, s.line, oneModel, s.title);
      await api.files.write(project.id, file, text, now.hash);
      onchanged?.();
    } catch (e) {
      error = errorText(e);
    } finally {
      writing = false;
    }
  }
  /** A model's name in the run's lines: the free agent's attempts read as Qanuvi core. */
  function label(model: string) {
    if (model.startsWith(DECIDER)) return `Decider (${modelLabel(provider, model.slice(DECIDER.length))})`;
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

  $effect(() => {
    if (setup) {
      fromLine = openSteps[0]?.line ?? -1;
      // Unattended work needs a mode that does not stop at every edit; the user can still choose Manual.
      mode = provider.modes.some((m) => m.id === "auto") ? "auto" : settings.agent(provider.id).mode;
      // This plan's own settings from its last run.
      restore();
    }
  });

  /** The check before a run (24i): what the run will need, answered once. */
  let checkFirst = $state(true);
  let checking = $state(false);
  let check = $state<Preflight | null>(null);
  let secretValues = $state<Record<string, string>>({});
  let answers = $state<Record<number, string>>({});
  let riskyChoice = $state<Record<number, "wait" | "go">>({});

  /** The open steps the run would cover, for the check. */
  function coveredSteps() {
    const { ranges: r } = parseRanges(ranges.trim());
    const all = openSteps.filter((s) => s.line >= fromLine);
    const inR = r.length ? all.filter((s) => s.number && inRanges(s.number, r)) : all;
    return (onlyOne ? inR.slice(0, 1) : inR).slice(0, 40);
  }

  async function runCheck() {
    checking = true;
    error = "";
    try {
      const known = {
        secrets: await api.agent.secretNames(project.id).catch(() => [] as string[]),
        testCommand: project.settings.test_command,
        previewUrl: project.settings.preview_url,
        notes: loadPlanNotes(project.id, file),
      };
      const steps = coveredSteps().map((s) => ({ number: s.number, title: s.title, details: s.details }));
      const r = await planner.ask(
        project,
        preflightPrompt(file, steps, known),
        settings.agent(provider.id).model || defaultPlannerModel(provider),
        "Before the run: what it needs",
        {
          agentId: provider.id,
          lean: true,
          quiet: true,
        },
      );
      const found = parsePreflight(r.text);
      found.secrets = found.secrets.filter((s) => !known.secrets.includes(s.name));
      check = found;
      riskyChoice = Object.fromEntries(found.risky.map((_, i) => [i, "wait"]));
      if (!found.secrets.length && !found.questions.length && !found.risky.length && !found.setup.length) {
        check = null;
        await go();
      }
    } catch (e) {
      error = `The check before the run did not answer (${errorText(e)}). Start without it, or try again.`;
    } finally {
      checking = false;
    }
  }

  /** Saves the answers (secrets, notes) and starts, holding at risky steps and open questions. */
  async function startChecked() {
    const c = check;
    if (!c) return go();
    for (const s of c.secrets) {
      const v = secretValues[s.name]?.trim();
      if (v) await api.agent.setSecret(project.id, s.name, v).catch(() => {});
    }
    const notes = loadPlanNotes(project.id, file);
    const wait: { step: string; why: string }[] = [];
    c.questions.forEach((q, i) => {
      const a = answers[i]?.trim();
      if (a) notes.push(`The owner's answer for step ${q.step}: ${q.question} → ${a}`);
      else if (q.step) wait.push({ step: q.step, why: `a question was left open (${q.question})` });
    });
    c.risky.forEach((x, i) => {
      if (riskyChoice[i] !== "go") wait.push({ step: x.step, why: `you asked to see it first (${x.why})` });
    });
    savePlanNotes(project.id, file, notes);
    check = null;
    await go(wait);
  }

  async function start(event: SubmitEvent) {
    event.preventDefault();
    if (checkFirst && !check) return runCheck();
    if (check) return startChecked();
    await go();
  }

  async function go(waitBefore: { step: string; why: string }[] = []) {
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
        waitBefore,
        onLimit,
        // Free decides, free works: the whole run goes to Qanuvi's own agent with the free models.
        ...(freeOnly && hasFree ? { providerId: "qanuvi", model: "" } : {}),
      });
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    }
  }

  let run = $derived(runner.run && runner.run.projectId === project.id && runner.run.file === file ? runner.run : null);
  let totals = $derived(runner.totals);
  let saved = $derived(run ? freeSavings(run, runner.paidPerStep) : { steps: 0, estimateUsd: 0 });
  let deciderUsd = $derived((run?.deciderCalls ?? []).reduce((t, c) => t + c.costUsd, 0));
  let current = $derived(run?.steps.find((s) => s.status === "running") ?? null);
  let currentAttempt = $derived(current ? current.attempts.length + 1 : 0);
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
        Only for this run; the plan file is not changed.
        <button type="button" class="link" disabled={writing} onclick={writeIntoPlan}
          >{writing ? "Writing…" : "Write it into the plan"}</button
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
        <input type="number" min="0" step="0.01" bind:value={budget} />
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
          <option value="never">Never: run to the end, skip what cannot be done (with why)</option>
          <option value="decisions">Only where a decision or my input is needed</option>
          <option value="step">After every step</option>
        </select>
      </label>
    </div>
    {#if rangeError}<p class="hint warn">{rangeError}</p>{/if}
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
          <option value="settings">As in Settings: {TEAM_LABELS[settings.value.team.mode]}</option>
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
      title="The agent reads the open steps once (read-only, a few cents) and lists what the run will need from you: logins, decisions, risky steps, things to set up. You answer once; then the run goes on alone."
    >
      <input type="checkbox" bind:checked={checkFirst} /> Check what the run needs from me first
    </label>
    {#if check}
      <div class="check-list">
        <strong>Before the run</strong>
        {#if check.secrets.length}
          <p class="hint">Logins and keys the steps need (saved as project secrets; no model sees them):</p>
          {#each check.secrets as s (s.name)}
            <label class="row-input"
              ><code>{s.name}</code><span class="dim">{s.why}{s.steps.length ? ` (${s.steps.join(", ")})` : ""}</span>
              <input type="password" autocomplete="off" placeholder="value" bind:value={secretValues[s.name]} /></label
            >
          {/each}
        {/if}
        {#if check.questions.length}
          <p class="hint">
            Decisions only you can make (kept as notes for this plan; left empty, the run waits there):
          </p>
          {#each check.questions as q, i (i)}
            <label class="row-input"
              ><span><strong>{q.step}</strong> {q.question}</span>
              <input type="text" placeholder="your answer" bind:value={answers[i]} /></label
            >
          {/each}
        {/if}
        {#if check.risky.length}
          <p class="hint">Risky steps (hard to undo, or they reach outside this computer):</p>
          {#each check.risky as x, i (i)}
            <div class="row-input">
              <span><strong>{x.step}</strong> {x.why}</span>
              <select bind:value={riskyChoice[i]}>
                <option value="wait">Wait for me before it</option>
                <option value="go">Go ahead</option>
              </select>
            </div>
          {/each}
        {/if}
        {#if check.setup.length}
          <p class="hint">To have running or set:</p>
          <ul>
            {#each check.setup as x, i (i)}<li><strong>{x.what}</strong> <span class="dim">{x.why}</span></li>{/each}
          </ul>
        {/if}
      </div>
    {/if}
    <div class="row">
      <span class="spacer"></span>
      <button type="submit" class="primary" disabled={openSteps.length === 0 || !!rangeError}
        >{#if checking}<LoaderCircle size={13} class="spin" /> Checking…{:else}<Play size={13} />
          {check ? "Start the run" : checkFirst ? "Check and start" : "Start"}{/if}</button
      >
    </div>
  </form>
{/if}

{#if error}<p class="error">{error}</p>{/if}

{#if run}
  <div class="panel status {run.status}">
    <div class="row">
      {#if run.status === "running"}
        <LoaderCircle size={14} class="spin" />
        <span class="main">
          {#if current}
            Step {current.number ?? ""} · {current.title} · {label(runner.currentModel)}
            {#if currentAttempt > 1}<span class="dim"> (try {currentAttempt})</span>{/if}
            {#if runner.checking}<span class="dim"> · running the tests</span>{/if}
            {#if runner.waitingUntil}<span class="dim">
                · usage limit reached: goes on by itself at {runner.waitingUntil.toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}</span
              >{/if}
          {:else}
            Starting…
          {/if}
        </span>
        <button
          type="button"
          class="pause"
          disabled={runner.pausing}
          title="Finish the step it is on, then pause; Resume lets you change the run's settings and go on"
          onclick={() => runner.pause()}>{runner.pausing ? "Pausing after this step…" : "Pause"}</button
        >
        <button type="button" class="stop" title="Stop now (the step is left as it is)" onclick={() => runner.stop()}
          ><Square size={11} /> Stop</button
        >
      {:else}
        <span class="main">
          {run.status === "done"
            ? "Run finished"
            : run.status === "needs-you"
              ? "Run paused: it needs you"
              : run.status === "paused"
                ? "Run paused"
                : "Run stopped"}
          · This run: {run.steps.filter((s) => s.status === "done").length} of {run.steps.length} done{#if run.steps.some((s) => s.status === "skipped")},
            {run.steps.filter((s) => s.status === "skipped").length} skipped{/if}{#if run.steps.some((s) => s.status === "blocked" || s.status === "failed")},
            {run.steps.filter((s) => s.status === "blocked" || s.status === "failed").length} blocked{/if}
          · Plan: {plan.steps.filter((s) => s.status === "x").length} of {plan.steps.length} done
          {#if run.message}<span class="dim"> · {run.message}</span>{/if}
        </span>
        {#if run.status !== "done" && plan.steps.some((s) => s.status === " ")}
          <button
            type="button"
            class="resume"
            title="Change the run's settings if you like, then go on"
            onclick={() => onresume?.()}>Resume</button
          >
        {/if}
      {/if}
    </div>
    {#if totals}
      <div class="totals">
        {formatCost(totals.costUsd)} · in {formatTokens(totals.inputTokens)} · out {formatTokens(totals.outputTokens)} ·
        {totals.seconds.toFixed(0)} s
        {#each [...totals.byModel] as [model, t] (model)}
          <span class="model">{label(model)}: {t.attempts}× {formatCost(t.costUsd)}</span>
        {/each}
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
    {#if run.notes?.length}
      <details class="decisions" open>
        <summary>Notes kept for this plan: every step and run gets them ({run.notes.length})</summary>
        <ul>
          {#each run.notes as n, i (i)}
            <li>
              {n}
              <button type="button" class="link" title="Remove this note" onclick={() => runner.removeNote(i)}>×</button
              >
            </li>
          {/each}
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
  </div>
{/if}

<style>
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

  .check-list {
    margin-top: var(--qx-space-3);
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-accent) 40%, var(--qx-border));
    border-radius: var(--qx-radius-card);
  }

  .row-input {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 220px;
    align-items: center;
    gap: var(--qx-space-2);
    margin: 4px 0;
    color: var(--qx-text);
  }

  .row-input input {
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
