<script lang="ts">
  import { agent } from "$lib/stores/agent.svelte";
  import Play from "@lucide/svelte/icons/play";
  import Square from "@lucide/svelte/icons/square";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import X from "@lucide/svelte/icons/x";
  import type { Project } from "$lib/api";
  import type { Plan } from "$lib/plan/tasklist";
  import { modelLabel, providerById } from "$lib/agents/providers";
  import { formatCost, formatTokens } from "$lib/agent/events";
  import { runner } from "$lib/stores/runner.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { freeSavings, isFree, DECIDER } from "$lib/plan/runner";

  interface Props {
    project: Project;
    file: string;
    plan: Plan;
    /** The setup form is shown (the user pressed Run). */
    setup: boolean;
    onclose: () => void;
  }

  let { project, file, plan, setup, onclose }: Props = $props();

  let provider = $derived(providerById(agent.runProviderId));
  let openSteps = $derived(plan.steps.filter((s) => s.status !== "x" && s.status !== "!"));
  let fromLine = $state(-1);
  let onlyOne = $state(false);
  let mode = $state("");
  let budget = $state(1);
  let perModel = $state(2);
  let lean = $state(true);
  let indexTools = $state(settings.agent(agent.runProviderId).indexTools);
  let packets = $state(true);
  let replan = $state(true);
  /** Free models can go first when the run's agent is a paid one and Qanuvi core has models. */
  let canFree = $derived(provider.id !== "qanuvi" && settings.value.brain.models.length > 0);
  let freeFirst = $state(true);
  /** A model's name in the run's lines: the free agent's attempts read as Qanuvi core. */
  function label(model: string) {
    if (model.startsWith(DECIDER)) return `Decider (${modelLabel(provider, model.slice(DECIDER.length))})`;
    return isFree(model) ? "Qanuvi core (free)" : modelLabel(provider, model);
  }
  let error = $state("");

  $effect(() => {
    if (setup) {
      fromLine = openSteps[0]?.line ?? -1;
      // Unattended work needs a mode that does not stop at every edit; the user can still choose Manual.
      mode = provider.modes.some((m) => m.id === "auto") ? "auto" : settings.agent(provider.id).mode;
    }
  });

  async function start(event: SubmitEvent) {
    event.preventDefault();
    error = "";
    if (!layout.state.agentVisible) layout.toggleAgent();
    onclose();
    try {
      await runner.start(project, file, {
        fromLine: Math.max(0, fromLine),
        onlyOne,
        mode,
        budgetUsd: Math.max(0, Number(budget) || 0),
        attemptsPerModel: Math.min(3, Math.max(1, Math.round(perModel))),
        lean,
        indexTools,
        packets,
        replan,
        freeFirst: canFree && freeFirst,
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
      Each step runs in a fresh {provider.name} session on its own model (the step's model, else
      {modelLabel(provider, settings.agent(provider.id).model)}). A step counts as done when the agent marks it done in
      the file.
    </p>
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
        Failed tries before a stronger model
        <select bind:value={perModel}>
          <option value={1}>1</option>
          <option value={2}>2</option>
          <option value={3}>3</option>
        </select>
      </label>
    </div>
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
    {#if canFree}
      <label
        class="check"
        title="Each step first goes to Qanuvi's own agent, which uses your free and local models; if it does not finish the step, the paid agent takes over as usual. Money and security steps go straight to the paid agent."
      >
        <input type="checkbox" bind:checked={freeFirst} /> Free models first: the paid agent only when they do not finish
        a step
      </label>
    {/if}
    {#if mode === "default"}<p class="hint warn">In Manual mode the run waits for your approval at every edit.</p>{/if}
    <div class="row">
      <span class="spacer"></span>
      <button type="submit" class="primary" disabled={openSteps.length === 0}><Play size={13} /> Start</button>
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
          {:else}
            Starting…
          {/if}
        </span>
        <button type="button" class="stop" onclick={() => runner.stop()}><Square size={11} /> Stop</button>
      {:else}
        <span class="main">
          {run.status === "done"
            ? "Run finished"
            : run.status === "needs-you"
              ? "Run paused: it needs you"
              : "Run stopped"}
          · {run.steps.filter((s) => s.status === "done").length}/{run.steps.length} steps done
          {#if run.message}<span class="dim"> · {run.message}</span>{/if}
        </span>
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
