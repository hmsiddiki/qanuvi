<script lang="ts">
  /** Settings, Start-up: commands Qanuvi starts with itself (Headroom, for example), their state and output. */
  import { invoke } from "@tauri-apps/api/core";
  import { settings, type StartupItem } from "$lib/stores/settings.svelte";
  import { startup } from "$lib/stores/startup.svelte";

  let items = $derived(settings.value.startup);
  let name = $state("");
  let command = $state("");
  let waitFor = $state("");
  let headroom = $state<string | null>(null);
  let note = $state("");

  invoke<string | null>("startup_headroom")
    .then((p) => (headroom = p))
    .catch(() => {});

  function save(next: StartupItem[]) {
    settings.set("startup", next);
    void startup.apply();
  }

  function add() {
    if (!command.trim()) return;
    save([
      ...items,
      {
        id: crypto.randomUUID(),
        name: name.trim() || command.trim().split(" ")[0],
        command: command.trim(),
        waitFor: waitFor.trim(),
        enabled: true,
      },
    ]);
    name = command = waitFor = "";
  }

  function update(id: string, patch: Partial<StartupItem>) {
    save(items.map((i) => (i.id === id ? { ...i, ...patch } : i)));
  }

  async function remove(id: string) {
    await startup.stop(id);
    save(items.filter((i) => i.id !== id));
  }

  /** Headroom in one click: started with Qanuvi, and Claude Code goes through it. */
  function addHeadroom() {
    if (!items.some((i) => /headroom/i.test(i.command)))
      save([
        ...items,
        {
          id: crypto.randomUUID(),
          name: "Headroom",
          command: "headroom proxy --port 8787",
          waitFor: "127.0.0.1:8787",
          enabled: true,
        },
      ]);
    if (!settings.agent("claude-code").baseUrl) settings.setAgent("claude-code", { baseUrl: "http://127.0.0.1:8787" });
    note = "Headroom starts with Qanuvi now, and Claude Code goes through it (Settings, Agents shows the address).";
  }

  const colour = (s?: string) =>
    s === "running" || s === "already running" ? "ok" : s === "failed" ? "bad" : s === "starting" ? "busy" : "off";
</script>

<h2>Start-up</h2>
<p class="intro">
  Commands Qanuvi starts with itself, hidden, for example a Headroom proxy that makes Claude Code use fewer tokens. When
  one has an address to wait for, agents wait until it answers; when it already answers (you started it), Qanuvi leaves
  it alone. Qanuvi stops only what it started, when it closes.
</p>

{#if headroom && !items.some((i) => /headroom/i.test(i.command))}
  <div class="card offer">
    <span><strong>Headroom is installed</strong> <span class="muted">({headroom})</span></span>
    <button type="button" class="btn primary" onclick={addHeadroom}>Start it with Qanuvi</button>
  </div>
{/if}
{#if note}<p class="note">{note}</p>{/if}

{#each items as it (it.id)}
  {@const st = startup.status(it.id)}
  <div class="card">
    <div class="row">
      <span class="dot {colour(st?.state)}"></span>
      <input class="name" value={it.name} onchange={(e) => update(it.id, { name: e.currentTarget.value })} />
      <span class="muted">{it.enabled ? (st?.state ?? "not started") : "off"}</span>
      <span class="spacer"></span>
      <label class="check"
        ><input
          type="checkbox"
          checked={it.enabled}
          onchange={(e) => update(it.id, { enabled: e.currentTarget.checked })}
        /> On</label
      >
      <button type="button" class="btn" disabled={!it.enabled} onclick={() => startup.restart(it)}>Restart</button>
      <button type="button" class="btn" onclick={() => remove(it.id)}>Remove</button>
    </div>
    <label
      >Command <input
        value={it.command}
        spellcheck="false"
        onchange={(e) => update(it.id, { command: e.currentTarget.value })}
      /></label
    >
    <label
      >Wait for (host:port) <input
        value={it.waitFor}
        placeholder="for example 127.0.0.1:8787"
        spellcheck="false"
        onchange={(e) => update(it.id, { waitFor: e.currentTarget.value })}
      /></label
    >
    {#if st?.detail}<p class="muted small">{st.detail}</p>{/if}
    {#if st?.output}
      <details>
        <summary class="muted small">Output</summary>
        <pre>{st.output}</pre>
      </details>
    {/if}
  </div>
{/each}

<h3>Add a command</h3>
<div class="card">
  <label>Name <input bind:value={name} placeholder="for example Headroom" /></label>
  <label
    >Command <input
      bind:value={command}
      placeholder="for example headroom proxy --port 8787"
      spellcheck="false"
    /></label
  >
  <label
    >Wait for (host:port) <input
      bind:value={waitFor}
      placeholder="for example 127.0.0.1:8787"
      spellcheck="false"
    /></label
  >
  <div class="actions">
    <button type="button" class="btn primary" disabled={!command.trim()} onclick={add}>Add</button>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 6px;
    font-size: 16px;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 13px;
  }

  .intro,
  .note {
    margin: 0 0 12px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 10px;
    padding: 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  .card.offer {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--qx-text-muted);
    font-size: 11.5px;
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 4px;
  }

  input:not([type="checkbox"]) {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  input.name {
    width: 160px;
    font-weight: 600;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--qx-text-muted);
  }

  .dot.ok {
    background: var(--qx-status-done);
  }

  .dot.bad {
    background: var(--qx-status-failed);
  }

  .dot.busy {
    background: var(--qx-status-running);
  }

  .muted {
    color: var(--qx-text-muted);
  }

  .small {
    margin: 0;
    font-size: 11.5px;
  }

  pre {
    max-height: 220px;
    overflow: auto;
    margin: 6px 0 0;
    padding: 8px;
    border-radius: 6px;
    background: var(--qx-bg-window);
    font-size: 11px;
    white-space: pre-wrap;
    user-select: text;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }

  .btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .btn.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
