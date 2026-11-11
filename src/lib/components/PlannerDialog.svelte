<script lang="ts">
  import { agent } from "$lib/stores/agent.svelte";
  import X from "@lucide/svelte/icons/x";
  import WandSparkles from "@lucide/svelte/icons/wand-sparkles";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import type { Project } from "$lib/api";
  import { modelLabel, providerById } from "$lib/agents/providers";
  import { formatCost } from "$lib/agent/events";
  import { JOB_LABELS, jobType } from "$lib/plan/routing";
  import { defaultPlannerModel, planner } from "$lib/stores/planner.svelte";
  import { runner } from "$lib/stores/runner.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { layout } from "$lib/layout/layout.svelte";

  interface Props {
    project: Project;
    /** Task list the steps go into (created when it does not exist). */
    file: string;
    onclose: () => void;
    /** Called after the steps were written, so the Plan view reloads. */
    onadded: (file: string) => void;
  }

  let { project, file, onclose, onadded }: Props = $props();

  let provider = $derived(providerById(agent.runProviderId));
  let request = $state("");
  let model = $state(defaultPlannerModel(providerById(agent.runProviderId)));
  let sectionTitle = $state("");
  let error = $state("");

  $effect(() => {
    if (planner.preview && !sectionTitle) sectionTitle = planner.preview.title;
  });

  async function makePlan(event: SubmitEvent) {
    event.preventDefault();
    if (!request.trim() || planner.busy) return;
    if (!layout.state.agentVisible) layout.toggleAgent();
    sectionTitle = "";
    await planner.plan(project, file, request, model);
  }

  async function add(run: boolean) {
    error = "";
    try {
      const firstLine = await planner.accept(project, file, sectionTitle);
      onadded(file);
      if (run) {
        const s = settings.agent(provider.id);
        runner
          .start(project, file, {
            fromLine: firstLine,
            onlyOne: false,
            mode: provider.modes.some((m) => m.id === "auto") ? "auto" : s.mode,
            budgetUsd: 0,
            attemptsPerModel: 2,
            lean: true,
            indexTools: s.indexTools,
            packets: true,
            replan: true,
          })
          .catch((e) => (error = String(e)));
      }
      onclose();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="panel">
  <div class="head">
    <strong><WandSparkles size={14} /> Plan a request</strong>
    <button type="button" class="x" aria-label="Close" onclick={onclose}><X size={14} /></button>
  </div>

  {#if !planner.preview}
    <form onsubmit={makePlan}>
      <textarea
        bind:value={request}
        rows="4"
        placeholder="What should be done? For example: add VAT to the cart total, with tests"
        aria-label="Request"
        disabled={planner.busy}></textarea>
      <div class="row">
        <label>
          Planner
          <select bind:value={model} disabled={planner.busy}>
            {#each provider.models as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
          </select>
        </label>
        <span class="hint">Reads the project read-only and answers with steps for {file}.</span>
        <button type="submit" class="primary" disabled={!request.trim() || planner.busy}>
          {#if planner.busy}<LoaderCircle size={13} class="spin" /> Planning…{:else}Make the plan{/if}
        </button>
      </div>
    </form>
  {:else}
    {@const p = planner.preview}
    <p class="hint">
      {p.steps.length} steps from {modelLabel(provider, model)} ({formatCost(p.costUsd)}). They are added as a new
      section; nothing else in {file} changes.
    </p>
    <label class="title">
      Section title
      <input bind:value={sectionTitle} maxlength="80" />
    </label>
    <ol class="steps">
      {#each p.steps as s, i (i)}
        <li>
          <span class="t">{s.title}</span>
          <span class="meta">
            {JOB_LABELS[jobType([s.title, ...s.details].join("\n"))]}{s.model
              ? ` · ${modelLabel(provider, s.model)}`
              : ""}
          </span>
          {#if s.details.length}<span class="d">{s.details.filter((d) => !/^- Model:/i.test(d)).join(" ")}</span>{/if}
        </li>
      {/each}
    </ol>
    <div class="row">
      <button type="button" onclick={() => (planner.preview = null)}>Discard</button>
      <span class="spacer"></span>
      <button type="button" onclick={() => add(false)}>Add to {file}</button>
      <button type="button" class="primary" onclick={() => add(true)}>Add and run</button>
    </div>
  {/if}
  {#if planner.error || error}<p class="error">{planner.error || error}</p>{/if}
</div>

<style>
  .panel {
    margin-bottom: var(--qx-space-3);
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-accent) 40%, var(--qx-border));
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--qx-space-2);
  }

  .head strong {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .x {
    border: 0;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  textarea,
  input,
  select {
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  textarea {
    width: 100%;
    padding: var(--qx-space-2);
    resize: vertical;
  }

  select,
  input {
    height: 28px;
    padding: 0 var(--qx-space-2);
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-2);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  label.title {
    margin-bottom: var(--qx-space-2);
  }

  .hint {
    flex: 1;
    margin: 0 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .row .hint {
    margin: 0 0 6px;
  }

  .spacer {
    flex: 1;
  }

  button {
    height: 28px;
    padding: 0 var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  button.primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border-color: transparent;
    background: var(--qx-accent);
    color: #fff;
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .steps {
    margin: 0;
    padding-left: var(--qx-space-6);
  }

  .steps li {
    margin-bottom: var(--qx-space-2);
  }

  .t {
    font-weight: 500;
  }

  .meta {
    margin-left: var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .d {
    display: block;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  .error {
    margin: var(--qx-space-2) 0 0;
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }
</style>
