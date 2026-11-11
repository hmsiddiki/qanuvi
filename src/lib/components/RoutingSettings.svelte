<script lang="ts">
  /** Model routing: monthly budget, this project's rules, and what Qanuvi has learned so far (Milestone 13). */
  import { api, type Project } from "$lib/api";
  import type { AgentProvider } from "$lib/agents/providers";
  import { modelLabel } from "$lib/agents/providers";
  import { formatCost } from "$lib/agent/events";
  import {
    JOB_LABELS,
    JOB_TYPES,
    MIN_SUCCESS,
    MIN_TRIES,
    jobType,
    learn,
    monthStart,
    type ModelStats,
  } from "$lib/plan/routing";
  import type { PlanRun } from "$lib/plan/runner";
  import { settings } from "$lib/stores/settings.svelte";

  interface Props {
    provider: AgentProvider;
    project: Project | null;
  }

  let { provider, project }: Props = $props();

  let spent = $state(0);
  let stats = $state<Map<string, ModelStats>>(new Map());

  $effect(() => {
    api.usage
      .since(monthStart())
      .then((u) => (spent = u.cost_usd))
      .catch(() => {});
    api.runs
      .listAll(300)
      .then((records) => {
        const runs: PlanRun[] = [];
        for (const r of records) {
          try {
            runs.push(JSON.parse(r.data));
          } catch {
            // skip unreadable records
          }
        }
        stats = learn(runs, jobType);
      })
      .catch(() => {});
  });

  let rules = $derived(project ? (settings.value.rules[project.id] ?? {}) : {});
  let budget = $derived(settings.value.monthlyBudgetUsd);
  let used = $derived(budget > 0 ? Math.min(1, spent / budget) : 0);

  let rows = $derived(
    JOB_TYPES.flatMap((type) =>
      provider.models
        .map((m) => ({ type, model: m.id, s: stats.get(`${type}|${m.id}`) }))
        .filter((r) => r.s && r.s.tries > 0),
    ),
  );

  const trusted = (s: ModelStats) => s.tries >= MIN_TRIES && s.successes / s.tries >= MIN_SUCCESS;
</script>

<section class="card">
  <h3>Model routing</h3>
  <p class="note">
    A step starts on its own model if it names one; otherwise on the project's rule for its kind of work; otherwise on
    the cheapest model that kept succeeding at that kind of work ({MIN_TRIES}+ tries, {Math.round(MIN_SUCCESS * 100)}%+
    done); otherwise on the default. Money and security work is never moved to a cheaper model.
  </p>

  <div class="budget">
    <label>
      Monthly budget for all agent work (USD, 0 = none)
      <input
        type="number"
        min="0"
        step="1"
        value={budget}
        onchange={(e) => settings.set("monthlyBudgetUsd", Number(e.currentTarget.value) || 0)}
      />
    </label>
    <div class="meter" title="Spent this month by all agent sessions">
      <span>This month: {formatCost(spent)}{budget > 0 ? ` of ${formatCost(budget)}` : ""}</span>
      {#if budget > 0}
        <div class="bar">
          <span class:tight={used >= 0.8} class:over={used >= 1} style:width="{used * 100}%"></span>
        </div>
        <span class="dim">From 80%: one model cheaper. At 100%: runs stop and ask you.</span>
      {/if}
    </div>
  </div>

  {#if project}
    <h4>Rules for {project.name}</h4>
    <div class="rules">
      {#each JOB_TYPES as type (type)}
        <label>
          {JOB_LABELS[type]}
          <select
            value={rules[type] ?? ""}
            onchange={(e) => settings.setRule(project!.id, type, e.currentTarget.value)}
          >
            <option value="">Learned or default</option>
            {#each provider.models as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
          </select>
        </label>
      {/each}
    </div>
  {/if}

  <h4>Learned so far</h4>
  {#if rows.length === 0}
    <p class="note">Nothing yet. Every plan run adds to this.</p>
  {:else}
    <table>
      <thead>
        <tr><th>Kind of work</th><th>Model</th><th>Tries</th><th>Done</th><th>Cost per done step</th><th></th></tr>
      </thead>
      <tbody>
        {#each rows as r (r.type + r.model)}
          <tr>
            <td>{JOB_LABELS[r.type]}</td>
            <td>{modelLabel(provider, r.model)}</td>
            <td>{r.s!.tries}</td>
            <td>{Math.round((r.s!.successes / r.s!.tries) * 100)}%</td>
            <td>{r.s!.successes ? formatCost(r.s!.costUsd / r.s!.successes) : "–"}</td>
            <td class="dim">{trusted(r.s!) ? "trusted" : ""}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .card {
    max-width: 760px;
    margin-bottom: var(--qx-space-4);
    padding: var(--qx-space-4);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  h3 {
    margin: 0 0 var(--qx-space-1);
    font-size: 14px;
  }

  h4 {
    margin: var(--qx-space-4) 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .note,
  .dim {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .budget {
    display: grid;
    grid-template-columns: 1fr 1.4fr;
    gap: var(--qx-space-4);
    align-items: end;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  input,
  select {
    height: 28px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  .meter {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--qx-text-small);
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--qx-border);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--qx-status-done);
  }

  .bar span.tight {
    background: var(--qx-status-waiting);
  }

  .bar span.over {
    background: var(--qx-status-failed);
  }

  .rules {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--qx-space-2) var(--qx-space-4);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--qx-text-small);
  }

  th,
  td {
    padding: 4px var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
    text-align: left;
  }

  th {
    color: var(--qx-text-muted);
    font-weight: 500;
  }
</style>
