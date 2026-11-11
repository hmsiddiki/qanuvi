<script lang="ts">
  /** This computer's remote access (Milestone 20), on the Servers page. */
  import MonitorSmartphone from "@lucide/svelte/icons/monitor-smartphone";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { remoteAccess } from "$lib/stores/remoteAccess.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ago } from "$lib/stores/git.svelte";

  let relay = $state(settings.value.remoteAccess.relay);
  let name = $state(settings.value.remoteAccess.name);
  let showLog = $state(false);

  let st = $derived(remoteAccess.status);
  let on = $derived(settings.value.remoteAccess.enabled);

  async function removeDevice(pairing: string, deviceName: string) {
    const ok = await ask(`Remove ${deviceName}? It can no longer reach this computer until it pairs again.`, {
      title: "Remove device",
      okLabel: "Remove",
      cancelLabel: "Keep",
    });
    if (ok) await remoteAccess.removeDevice(pairing);
  }

  async function openLog() {
    showLog = !showLog;
    if (showLog) await remoteAccess.loadActivity();
  }
</script>

<section class="panel">
  <h3><MonitorSmartphone size={14} /> This computer: remote access</h3>
  <p class="muted">
    Let your other devices reach this computer's projects from anywhere, through a relay. Everything between them is
    encrypted end to end; the relay only passes it on. Nothing is opened on this computer's network.
  </p>
  <div class="row">
    <input bind:value={relay} placeholder="Relay address, for example wss://relay.qanuvi.com" spellcheck="false" />
    <input class="short" bind:value={name} placeholder="This computer's name" />
  </div>
  <div class="row">
    {#if on}
      <button
        type="button"
        class="btn"
        disabled={remoteAccess.busy}
        onclick={() => remoteAccess.set(true, relay, name)}
      >
        Save
      </button>
      <button
        type="button"
        class="btn"
        disabled={remoteAccess.busy}
        onclick={() => remoteAccess.set(false, relay, name)}
      >
        Turn off
      </button>
    {:else}
      <button
        type="button"
        class="btn primary"
        disabled={remoteAccess.busy || !relay.trim()}
        onclick={() => remoteAccess.set(true, relay, name)}>Turn on</button
      >
    {/if}
    {#if on && st}
      <span class="state" class:ok={st.connected}
        >{st.connected ? "Connected to the relay" : st.error || "Connecting…"}</span
      >
    {/if}
  </div>
  {#if remoteAccess.error}<p class="error">{remoteAccess.error}</p>{/if}

  {#if on && st}
    <div class="row">
      <button type="button" class="btn" disabled={!st.connected} onclick={() => remoteAccess.pairStart()}>
        Pair a device
      </button>
      {#if st.code}
        <span class="code">{st.code[0]}</span>
        <span class="muted"
          >type this on the other device (Servers → Pair through the relay); good for {Math.ceil(st.code[1] / 60)} min, one
          try</span
        >
      {/if}
    </div>

    {#if st.sessions.length}
      <div class="live">
        <strong>In use now:</strong>
        {#each st.sessions as s (s.session)}<span>{s.device} (since {ago(s.since)})</span>{/each}
        <button type="button" class="btn stop" onclick={() => remoteAccess.stop()}>Stop all</button>
      </div>
    {/if}

    <h4>Paired devices</h4>
    {#each st.devices as [pairing, deviceName, added] (pairing)}
      <div class="device">
        <span>{deviceName}</span>
        <span class="muted">paired {ago(added)}</span>
        <button type="button" class="link" onclick={() => removeDevice(pairing, deviceName)}>Remove</button>
      </div>
    {:else}
      <p class="muted">None yet.</p>
    {/each}

    <button type="button" class="link" onclick={openLog}>{showLog ? "Hide" : "Show"} activity</button>
    {#if showLog}
      <ul class="log">
        {#each remoteAccess.activity as a, i (i)}
          <li><span class="muted">{new Date(a.at * 1000).toLocaleString()}</span> {a.device}: {a.what}</li>
        {:else}
          <li class="muted">Nothing yet.</li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .panel {
    margin-bottom: var(--qx-space-3);
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
  }

  h3,
  h4 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0;
    font-size: 13px;
  }

  h4 {
    margin-top: 10px;
    font-size: 12px;
  }

  .muted {
    margin: 2px 0;
    color: var(--qx-text-secondary);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
  }

  input {
    flex: 1;
    min-width: 200px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  input.short {
    flex: 0 1 200px;
    min-width: 140px;
  }

  input:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .btn {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .btn.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .btn.stop {
    background: #9a3412;
    border-color: #9a3412;
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .state {
    color: var(--qx-text-muted);
  }

  .state.ok {
    color: #2ea043;
  }

  .code {
    padding: 3px 10px;
    border-radius: 6px;
    background: var(--qx-bg-window);
    font-family: var(--qx-font-mono, monospace);
    font-size: 15px;
    letter-spacing: 1px;
    user-select: text;
  }

  .live {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 6px;
    background: rgba(194, 65, 12, 0.15);
  }

  .device {
    display: flex;
    gap: 10px;
    align-items: baseline;
    padding: 3px 0;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .log {
    max-height: 200px;
    overflow: auto;
    margin: 6px 0;
    padding-left: 16px;
  }
</style>
