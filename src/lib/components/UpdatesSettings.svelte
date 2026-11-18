<script lang="ts">
  /**
   * Settings → Updates (1b): the version, the update channel, a manual check, reload and restart. M8c: with a master
   * shown, its own update: the master looks, downloads and restarts itself; this page asks it and shows where it is,
   * and "Download updates by themselves" is that master's own setting.
   */
  import { updates } from "$lib/stores/updates.svelte";
  import { masterUpdate } from "$lib/stores/masterUpdate.svelte";
  import { switcher } from "$lib/stores/switcher.svelte";
  import { shownPlace } from "$lib/remote";
  import { ago } from "$lib/stores/git.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  const onMaster = $derived(!!switcher.shown);
  const up = $derived(onMaster ? masterUpdate : updates);
  const where = $derived(switcher.shown ? shownPlace() : "this computer");

  const status = $derived.by(() => {
    if (onMaster) {
      const m = masterUpdate;
      switch (m.state) {
        case "checking":
          return "Looking for a newer version…";
        case "none":
          return m.noBuild || "Qanuvi is up to date there.";
        case "available":
          return `Qanuvi ${m.version} is available.${m.error ? ` ${m.error}` : ""}`;
        case "downloading":
          return `Downloading ${m.version}… ${m.percent}%`;
        case "verifying":
          return `Checking ${m.version}: its signature, and that it starts there.`;
        case "ready":
          return `${m.version} is ready. Restart to use it; the restart waits for work running there.`;
        case "waiting":
          return `Update to ${m.version} waits: ${m.waitingFor} working. It goes in as soon as that ends; nothing is cut off.`;
        case "restarting":
          return `Restarting with ${m.version}…`;
        case "error":
          return `The update check failed: ${m.error}`;
        default:
          return "It looks for a newer version two minutes after its start, then every hour.";
      }
    }
    switch (updates.state) {
      case "checking":
        return "Looking for a newer version…";
      case "none":
        return "Qanuvi is up to date.";
      case "available":
        return `Qanuvi ${updates.version} is available.`;
      case "downloading":
        return `Downloading ${updates.version}… ${updates.percent}%`;
      case "ready":
        return `${updates.version} is ready. Restart to use it, or it installs when you close Qanuvi.`;
      case "error":
        return `The update check failed: ${updates.error}`;
      default:
        return "Qanuvi looks for a newer version at start, every hour and when you come back to it.";
    }
  });

  const working = $derived(["checking", "downloading", "verifying", "waiting", "restarting"].includes(up.state));
  const failed = $derived(up.state === "error" || (onMaster && !!masterUpdate.error && up.state === "available"));
</script>

<h2>Updates</h2>
<p class="intro">
  New versions come from the <strong>dev</strong> channel, signed by the Qanuvi team; a version with a wrong signature
  is refused.
  {#if onMaster}
    These are {where}'s own updates: it looks for them itself, and does the download and the restart. A newer version
    shows as a button in the status bar while it is shown: click it to download, then restart to install.
  {:else}
    A newer version shows as a button in the status bar: click it to download, then restart (or close Qanuvi) to
    install.
  {/if}
</p>

<div class="card">
  <div class="row"><span class="muted">This version</span><strong>{up.current || "…"}</strong></div>
  <div class="row"><span class="muted">Channel</span><span>dev</span></div>
  {#if up.checkedAt}
    <div class="row">
      <span class="muted">Last checked</span><span>{ago(Math.floor(up.checkedAt / 1000))}</span>
    </div>
  {/if}
  <p class="status" class:error={failed}>{status}</p>
  {#if up.state === "downloading"}
    <div class="bar"><span style:width="{up.percent}%"></span></div>
  {/if}
  {#if up.notes && ["available", "ready", "downloading", "verifying", "waiting"].includes(up.state)}
    <pre class="notes">{up.notes}</pre>
  {/if}
  <div class="actions">
    {#if up.state === "ready"}
      <button type="button" class="btn primary" onclick={() => up.restartToUpdate()}>Restart to update</button>
    {:else if up.state === "available"}
      <button type="button" class="btn primary" onclick={() => up.download()}>Download {up.version}</button>
    {:else}
      <button type="button" class="btn" disabled={working} onclick={() => up.check()}>Check now</button>
    {/if}
  </div>
  {#if onMaster && masterUpdate.note && !["available", "error"].includes(up.state)}
    <p class="status error">{masterUpdate.note}</p>
  {/if}
</div>

<label class="auto">
  <input
    type="checkbox"
    checked={settings.value.updates.autoDownload}
    onchange={(e) => settings.set("updates", { ...settings.value.updates, autoDownload: e.currentTarget.checked })}
  />
  <span>
    <strong>Download updates by themselves</strong>
    {#if onMaster}
      <span class="muted"
        >On: {where} puts a new version in by itself, once no agent or plan run is working there. Off: it only tells, here
        and in its terminal.</span
      >
    {:else}
      <span class="muted">Off: a new version waits for your click in the status bar before it downloads.</span>
    {/if}
  </span>
</label>

<h3>Reload and restart</h3>
<div class="card">
  <div class="row">
    <span>
      <strong>Reload the window</strong>
      <span class="muted"
        >The interface starts again; agents keep working in the background and terminals open again.</span
      >
    </span>
    <button type="button" class="btn" onclick={() => updates.reloadWindow()}>Reload <kbd>Ctrl+Shift+R</kbd></button>
  </div>
  <div class="row">
    <span>
      <strong>Restart Qanuvi</strong>
      <span class="muted">Closes and opens Qanuvi again (asks first when an agent is still working).</span>
    </span>
    <button type="button" class="btn" onclick={() => updates.restart()}>Restart</button>
  </div>
</div>

<style>
  .auto {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    margin: 12px 0 4px;
    cursor: pointer;
  }

  .auto > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  h3 {
    margin: 22px 20px 8px;
    font-size: 13px;
  }

  .intro {
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .card {
    display: grid;
    gap: 8px;
    margin: 0 20px;
    padding: 12px 14px;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
    font-size: 12.5px;
  }

  .row {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
  }

  .row > span {
    display: grid;
    gap: 2px;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .status {
    margin: 4px 0 0;
  }

  .status.error {
    color: var(--qx-danger, #e5534b);
  }

  .bar {
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--qx-border);
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--qx-accent);
    transition: width 0.3s;
  }

  .notes {
    max-height: 160px;
    margin: 0;
    overflow: auto;
    color: var(--qx-text-secondary);
    font: inherit;
    white-space: pre-wrap;
  }

  .actions {
    display: flex;
    gap: 8px;
  }

  .btn {
    flex: none;
    padding: 5px 14px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .btn.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  kbd {
    margin-left: 6px;
    color: var(--qx-text-muted);
    font: inherit;
    font-size: 11px;
  }
</style>
