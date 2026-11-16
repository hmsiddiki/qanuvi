<script lang="ts">
  /**
   * The shown master cannot be reached (24z3 Part E). Lost while shown: a see-through cover over its screen, which
   * stays as it was and cannot be used, "connection lost, reconnecting…", gone by itself when it answers. Not
   * connected: an opaque screen with nothing of it, "not connected" and Reconnect. Its work goes on on the machine.
   * The Servers page (the switcher's own list) is never covered.
   */
  import { onDestroy } from "svelte";
  import Unplug from "@lucide/svelte/icons/unplug";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { masters, displayName } from "$lib/stores/masters.svelte";
  import { switcher } from "$lib/stores/switcher.svelte";
  import { pick } from "$lib/stores/machines";
  import { projects } from "$lib/stores/projects.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let held = $derived(switcher.held);
  let m = $derived(held ? (masters.list.find((x) => x.id === held.id) ?? null) : null);
  let name = $derived(m ? displayName(m) : "The server");
  let trying = $derived(!!held && masters.statusOf(held.id).state === "connecting");

  // The countdown to the next try.
  let now = $state(Date.now());
  const tick = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(tick));
  let wait = $derived(held && held.nextAt > 0 ? Math.max(0, Math.ceil((held.nextAt - now) / 1000)) : 0);

  let busy = $state(false);
  async function reconnect() {
    busy = true;
    try {
      await masters.retryNow();
    } finally {
      busy = false;
    }
  }
</script>

{#if held && m && ui.settings !== "servers"}
  <div class="held" class:lost={held.mode === "lost"} role="alert" aria-live="polite">
    <div class="card">
      {#if held.mode === "lost"}
        <h2><LoaderCircle size={16} class="spin" /> {name}: connection lost, reconnecting…</h2>
        <p class="muted">
          Its screen stays as it was and cannot be used until it is back. Work on it goes on.
          {#if trying}Trying now…{:else if wait > 0}Next try in {wait} s.{/if}
        </p>
        {#if held.error}<p class="why">Last try: {held.error}</p>{/if}
      {:else}
        <h2><Unplug size={16} /> {name} is not connected</h2>
        <p class="muted">Nothing of it is shown until it answers. Work on it goes on.</p>
        {#if held.error}<p class="why">{held.error}</p>{/if}
      {/if}
      <div class="actions">
        <button type="button" class="btn primary" disabled={busy || trying} onclick={reconnect}
          >{busy || trying ? "Connecting…" : held.mode === "lost" ? "Try now" : "Reconnect"}</button
        >
        <button type="button" class="btn" onclick={() => pick(null, projects.persist)}>Show This PC</button>
        <button type="button" class="btn" onclick={() => ui.openSettings("servers")}>Servers…</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .held {
    position: absolute;
    inset: 0;
    z-index: 40;
    display: grid;
    place-items: center;
    padding: var(--qx-space-4, 16px);
    background: var(--qx-bg-window);
  }

  /* Lost: its screen shows through, dimmed, and takes no clicks. */
  .held.lost {
    background: color-mix(in srgb, var(--qx-bg-window) 55%, transparent);
  }

  .card {
    max-width: 460px;
    padding: 18px 20px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 8px;
    font-size: 15px;
  }

  p {
    margin: 0 0 8px;
    line-height: 1.5;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .why {
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .btn.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.6;
    cursor: progress;
  }

  :global(.held .spin) {
    animation: held-spin 1s linear infinite;
  }

  @keyframes held-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
