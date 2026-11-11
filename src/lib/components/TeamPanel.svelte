<script lang="ts">
  /** How the team works (24b): who decides, who works, and what comes first. */
  import { settings } from "$lib/stores/settings.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import type { Priority, TeamMode } from "$lib/core/decider";
  import { paidAgent } from "$lib/agents/providers";

  /** The paid agent's name (Claude Code today; whichever paid agent the user has). */
  const paidName = paidAgent()?.name ?? "The paid agent";

  const MODES: { id: TeamMode; label: string; text: string }[] = [
    {
      id: "paid-decides",
      label: "Paid decides, free works",
      text: `${paidName} decides who does what; free models do the work; ${paidName} does what they could not, or steps in when Qanuvi core sees them in trouble. Every free result is reviewed before it counts.`,
    },
    {
      id: "priority",
      label: "Priority",
      text: `Like the above, leaning the way you choose below: more to free models to save tokens, or more to ${paidName} to solve problems first.`,
    },
    {
      id: "direct",
      label: "Direct",
      text: `Only ${paidName} works. Qanuvi core still gives it context (memory, the right files).`,
    },
    {
      id: "free-decides",
      label: "Free decides, paid works",
      text: `A free model decides what to send to ${paidName} and which of its models, so it gets fewer, better requests.`,
    },
  ];
  const PRIORITIES: { id: Priority; label: string }[] = [
    { id: "save", label: "Save tokens" },
    { id: "balanced", label: "Balanced" },
    { id: "solve", label: "Solve problems first" },
  ];

  let team = $derived(settings.value.team);
  let paid = $derived(!!agent.info?.available);
  let free = $derived(settings.value.brain.models.length > 0);
</script>

<section class="team">
  <h3>How the team works</h3>
  <p class="muted">
    {#if paid}
      {paidName} is here, so it decides. {free
        ? "Free models work alongside it as set here."
        : "Add free models (Qanuvi agent below) for it to hand work to."}
    {:else if free}
      No paid agent here: Qanuvi core's free models decide and work, best first, one after another.
    {:else}
      Add free models (Qanuvi agent below) or a paid agent.
    {/if}
  </p>
  {#if paid}
    <div class="modes">
      {#each MODES as m (m.id)}
        <label class="mode" class:on={team.mode === m.id}>
          <input
            type="radio"
            name="team-mode"
            checked={team.mode === m.id}
            onchange={() => settings.set("team", { ...team, mode: m.id })}
          />
          <span>
            <strong>{m.label}</strong>
            <span class="muted small">{m.text}</span>
          </span>
        </label>
      {/each}
    </div>
    {#if team.mode === "priority"}
      <div class="prio">
        {#each PRIORITIES as p (p.id)}
          <label
            ><input
              type="radio"
              name="team-priority"
              checked={team.priority === p.id}
              onchange={() => settings.set("team", { ...team, priority: p.id })}
            />
            {p.label}</label
          >
        {/each}
      </div>
    {/if}
  {/if}
</section>

<style>
  .team {
    margin-bottom: var(--qx-space-4, 16px);
    padding: 10px 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card, 8px);
    background: var(--qx-bg-raised);
    font-size: 12px;
  }

  h3 {
    margin: 0 0 4px;
    font-size: 13px;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .modes {
    display: grid;
    gap: 6px;
    margin-top: 6px;
  }

  .mode {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    cursor: pointer;
  }

  .mode.on {
    border-color: var(--qx-accent);
  }

  .mode > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .prio {
    display: flex;
    gap: 14px;
    margin-top: 8px;
  }
</style>
