<script lang="ts">
  /**
   * Settings → Languages (15i): the language servers Qanuvi knows, whether each is on this computer, Install (the
   * click is the user's yes), on or off per language, and how long an unused server keeps running.
   */
  import { onMount } from "svelte";
  import { lsp } from "$lib/stores/lsp.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import type { ServerId } from "$lib/lsp/protocol";

  let l = $derived(settings.value.languages);

  onMount(() => {
    lsp.loadStatuses(true);
  });

  const where = (found: string) =>
    found === "qanuvi" ? "Installed by Qanuvi" : found === "path" ? "Found on this computer" : "Not installed";

  function forget(server: string) {
    settings.set("languages", { ...l, never: l.never.filter((x) => x !== server) });
  }
</script>

<h2>Languages</h2>
<p class="intro">
  Language servers give errors and warnings, completion, hover, exact go to definition, find references and rename.
  Nothing comes with Qanuvi: a server is installed only when you click Install. One starts when the first file of its
  language opens, runs at low priority, and stops when its project closes or after a while unused.
</p>

<div class="rows">
  {#each lsp.statuses as s (s.id)}
    {@const id = s.id as ServerId}
    <div class="row">
      <div class="what">
        <strong>{s.serves}</strong>
        <span class="muted">{s.label} · {where(s.found)}</span>
        {#if !s.found}<span class="muted small">{s.install || s.missing}</span>{/if}
        {#if l.never.includes(s.id)}
          <button type="button" class="link" onclick={() => forget(s.id)}>Offer it again when a file opens</button>
        {/if}
      </div>
      {#if !s.found && s.install}
        <button type="button" disabled={lsp.installing !== null} onclick={() => lsp.install(id)}
          >{lsp.installing === s.id ? "Installing…" : "Install"}</button
        >
      {/if}
      <label class="toggle">
        <input
          type="checkbox"
          checked={lsp.enabled(id)}
          onchange={(e) => lsp.setEnabled(id, e.currentTarget.checked)}
        />
        On
      </label>
    </div>
  {:else}
    <p class="muted">Reading what is installed…</p>
  {/each}
  {#if lsp.installError}<p class="error">{lsp.installError}</p>{/if}

  <label class="idle">
    <span><strong>Stop an unused server after</strong> <span class="muted">{l.idleMinutes} min</span></span>
    <input
      type="range"
      min="1"
      max="60"
      value={l.idleMinutes}
      onchange={(e) => settings.set("languages", { ...l, idleMinutes: Number(e.currentTarget.value) })}
    />
  </label>
</div>

<style>
  h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  .intro {
    max-width: 640px;
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .rows {
    display: grid;
    gap: 12px;
    max-width: 640px;
    margin: 0 20px;
    font-size: 12.5px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
  }

  .what {
    display: grid;
    flex: 1;
    gap: 2px;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11.5px;
  }

  button {
    padding: 3px 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .link {
    justify-self: start;
    border: none;
    padding: 0;
    color: var(--qx-accent);
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .idle {
    display: grid;
    gap: 6px;
    margin-top: 8px;
  }

  .error {
    color: var(--qx-danger, #e5534b);
    white-space: pre-wrap;
  }
</style>
