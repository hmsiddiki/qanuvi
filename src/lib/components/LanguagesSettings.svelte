<script lang="ts">
  /**
   * Settings → Languages (15i): the language servers Qanuvi knows, whether each is on this computer, Install (the
   * click is the user's yes), on or off per language, how long an unused server keeps running; then the debuggers
   * and the PHP / Xdebug check (15i part 3).
   */
  import { onMount } from "svelte";
  import { lsp } from "$lib/stores/lsp.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import type { ServerId } from "$lib/lsp/protocol";
  import type { PhpInfo } from "$lib/dap/protocol";
  import { dap } from "$lib/stores/dap.svelte";

  let l = $derived(settings.value.languages);

  let xdebugDraft = $state(settings.value.languages.xdebugFile);

  onMount(() => {
    lsp.loadStatuses(true);
    dap.loadStatuses();
    dap.checkPhp();
  });

  function saveXdebug() {
    settings.set("languages", { ...l, xdebugFile: xdebugDraft.trim() });
    dap.checkPhp();
  }

  const phpLine = (p: PhpInfo | null) => {
    if (!p) return "Checking PHP…";
    if (!p.php) return "PHP not found";
    const x = p.xdebug
      ? `Xdebug ${p.xdebug} ${p.via === "ini" ? "(loaded by php.ini)" : "(from the file below, for debug runs only)"}`
      : "no Xdebug";
    return `PHP ${p.version} ${p.build} · ${x}`;
  };

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

  <label class="save">
    <input
      type="checkbox"
      checked={l.formatOnSave}
      onchange={(e) => settings.set("languages", { ...l, formatOnSave: e.currentTarget.checked })}
    />
    <span
      ><strong>Format on save</strong>
      <span class="muted"
        >Ctrl+S formats the file through its language server first (when it runs and can format; at most 3 seconds).
        Shift+Alt+F formats at any time.</span
      ></span
    >
  </label>

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

<h2 class="next">Debugging</h2>
<p class="intro">
  The debuggers VS Code uses, installed only when you click Install. One runs only while you debug (Run and debug in the
  sidebar, F5).
</p>
<div class="rows">
  {#each dap.statuses as s (s.id)}
    <div class="row">
      <div class="what">
        <strong>{s.label}</strong>
        <span class="muted"
          >{s.installed ? `Installed by Qanuvi${s.version ? ` · ${s.version}` : ""}` : "Not installed"}</span
        >
        {#if !s.installed}<span class="muted small">{s.install || s.missing}</span>{/if}
      </div>
      {#if !s.installed && s.install}
        <button type="button" disabled={dap.installing} onclick={() => dap.install(s.id)}
          >{dap.installing ? "Installing…" : "Install"}</button
        >
      {/if}
    </div>
  {/each}
  {#if dap.installError}<p class="error">{dap.installError}</p>{/if}

  <div class="row">
    <div class="what">
      <strong>PHP and Xdebug</strong>
      <span class="muted">{phpLine(dap.php)}</span>
      {#if dap.php?.php}<span class="muted small">{dap.php.php}</span>{/if}
      {#if dap.php?.note}<span class="muted small">{dap.php.note}</span>{/if}
      <label class="file">
        <span class="small"
          >Xdebug file, when php.ini does not load one (Qanuvi loads it for debug runs only and leaves php.ini as it is)</span
        >
        <span class="pick">
          <input
            type="text"
            spellcheck="false"
            placeholder="e.g. C:\tools\php_xdebug-3.5.3-8.2-nts-vs16-x86_64.dll"
            bind:value={xdebugDraft}
            onkeydown={(e) => e.key === "Enter" && saveXdebug()}
          />
          <button type="button" onclick={saveXdebug}>Check</button>
        </span>
      </label>
    </div>
  </div>
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

  .save {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 14px 20px 0;
  }

  .save .muted {
    display: block;
  }

  .idle {
    display: grid;
    gap: 6px;
    margin-top: 8px;
  }

  .file {
    display: grid;
    gap: 4px;
    margin-top: 6px;
  }

  .pick {
    display: flex;
    gap: 6px;
  }

  .pick input {
    flex: 1;
    min-width: 0;
    padding: 3px 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: inherit;
    font: inherit;
  }

  h2.next {
    margin-top: 26px;
  }

  .error {
    color: var(--qx-danger, #e5534b);
    white-space: pre-wrap;
  }
</style>
