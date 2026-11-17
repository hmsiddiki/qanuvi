<script lang="ts">
  /** Settings, Start-up: commands Qanuvi starts with itself (Headroom, for example), their state and output. */
  import { callThisPc, routed as invoke, shownMachine, shownPlace } from "$lib/remote";
  import { errorText } from "$lib/api";
  import { settings, type StartupItem } from "$lib/stores/settings.svelte";
  import { startup } from "$lib/stores/startup.svelte";

  let items = $derived(settings.value.startup);
  let name = $state("");
  let command = $state("");
  let waitFor = $state("");
  let headroom = $state<string | null>(null);
  let note = $state("");
  /** Claude Code's own settings already send it through a proxy (headroom wrap): Qanuvi keeps that address. */
  let claudeRoute = $state<string | null>(null);
  /** The machine this page shows (null: This PC), and its name for sentences. */
  let machine = $state<string | null>(null);
  let place = $state("this computer");
  /** M4c: signing in to this computer starts Qanuvi in the background (This PC only; off until turned on). */
  let atSignIn = $state<boolean | null>(null);
  let atSignInError = $state("");

  $effect(() => {
    callThisPc<boolean>("sign_in_start")
      .then((on) => (atSignIn = on))
      .catch((e) => (atSignInError = errorText(e)));
  });

  async function setAtSignIn(on: boolean) {
    atSignInError = "";
    try {
      atSignIn = await callThisPc<boolean>("sign_in_start_set", { on });
    } catch (e) {
      atSignInError = errorText(e);
    }
  }

  // Asked again when another machine is shown: its Headroom, its Claude Code settings (24z3 D4).
  $effect(() => {
    void settings.machine;
    const m = shownMachine();
    machine = m;
    place = shownPlace();
    headroom = claudeRoute = null;
    note = "";
    invoke<string | null>("claude_settings_base_url")
      .then((u) => m === shownMachine() && (claudeRoute = u))
      .catch(() => {});
    invoke<string | null>("startup_headroom")
      .then((p) => m === shownMachine() && (headroom = p))
      .catch(() => {});
    void startup.refresh();
  });

  /** "Qanuvi" on This PC; on a master, the service that runs there. */
  let starter = $derived(machine ? "the Qanuvi master" : "Qanuvi");

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
    if (claudeRoute) {
      note = `Headroom starts with ${starter} now. Claude Code already goes through it (its own settings send it to ${claudeRoute}), so that stays as it is.`;
      return;
    }
    if (!settings.agent("claude-code").baseUrl) settings.setAgent("claude-code", { baseUrl: "http://127.0.0.1:8787" });
    note = `Headroom starts with ${starter} now, and Claude Code goes through it (Settings, Agents shows the address).`;
  }

  const colour = (s?: string) =>
    s === "running" || s === "already running" ? "ok" : s === "failed" ? "bad" : s === "starting" ? "busy" : "off";
</script>

<h2>Start-up</h2>
{#if machine}
  <p class="intro">
    Commands the Qanuvi master on {place} starts with itself, hidden, on that machine, for example a Headroom proxy that makes
    Claude Code there use fewer tokens. They start when the master starts and keep running with no window connected. When
    one has an address to wait for, agents wait until it answers; when it already answers (started there by hand), the master
    leaves it alone. The master stops only what it started, when it stops.
  </p>
{:else}
  <p class="intro">
    Commands Qanuvi starts with itself, hidden, for example a Headroom proxy that makes Claude Code use fewer tokens.
    When one has an address to wait for, agents wait until it answers; when it already answers (you started it), Qanuvi
    leaves it alone. They keep running after the last window closes, while Qanuvi's icon shows in the tray; Qanuvi stops
    only what it started, when you choose Quit Qanuvi there.
  </p>
  <div class="card">
    <label class="check"
      ><input
        type="checkbox"
        checked={atSignIn === true}
        disabled={atSignIn === null}
        onchange={(e) => void setAtSignIn(e.currentTarget.checked)}
      /> Start Qanuvi in the background when I sign in</label
    >
    <p class="muted small">
      Qanuvi starts with no window, its icon in the tray: these commands, its daily backup and remote access run from
      then on. Open Qanuvi there shows the window.
    </p>
    {#if atSignInError}<p class="note bad">{atSignInError}</p>{/if}
  </div>
{/if}

{#if startup.problem}<p class="note bad">{startup.problem}</p>{/if}
{#if headroom && !items.some((i) => /headroom/i.test(i.command))}
  <div class="card offer">
    <span><strong>Headroom is installed</strong> <span class="muted">({headroom})</span></span>
    <button type="button" class="btn primary" onclick={addHeadroom}>Start it with {starter}</button>
  </div>
{/if}
{#if claudeRoute}
  <p class="note">
    Claude Code's own settings on {place} send every Claude Code there (Qanuvi's too) to <code>{claudeRoute}</code>.
    Start-up here only makes sure that proxy runs while {starter} runs.
  </p>
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
          onchange={(e) => void startup.setEnabled(it.id, e.currentTarget.checked)}
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

  .note.bad {
    color: var(--qx-status-failed);
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
