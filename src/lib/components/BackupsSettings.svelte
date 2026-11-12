<script lang="ts">
  /**
   * Backups (1d): a daily copy of Qanuvi's data (database, core records, agent histories, the window's settings) in a
   * folder the user chooses, the last 7 kept. A restore is applied when Qanuvi starts again; the data in use is kept
   * beside it as a "before restore" backup.
   */
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { errorText } from "$lib/api";

  interface Backup {
    path: string;
    name: string;
    created: number;
    kind: "daily" | "manual" | "beforeRestore";
    version: string;
    from: string;
    files: number;
    bytes: number;
    window: boolean;
  }

  interface Info {
    data: string;
    dir: string;
    defaultDir: string;
    custom: boolean;
    off: boolean;
    lastAt: number;
    lastError: string | null;
    keep: number;
    restore: string | null;
    backups: Backup[];
  }

  let info = $state<Info | null>(null);
  let folder = $state("");
  let error = $state("");
  let note = $state("");
  let busy = $state(false);

  async function load() {
    info = await invoke<Info>("backups_info").catch((e) => {
      error = errorText(e);
      return null;
    });
    if (info) folder = info.custom ? info.dir : "";
  }
  onMount(load);

  const when = (secs: number) => new Date(secs * 1000).toLocaleString();
  const size = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${Math.max(0.1, b / 1e6).toFixed(1)} MB`);
  const KIND = { daily: "Daily", manual: "Made by you", beforeRestore: "Before a restore" } as const;
  const sameDrive = $derived(
    !!info && /^[a-z]:/i.test(info.dir) && info.dir.slice(0, 2).toLowerCase() === info.data.slice(0, 2).toLowerCase(),
  );

  async function save(dir: string, off: boolean) {
    error = "";
    try {
      await invoke("backups_set", { folder: dir, off });
    } catch (e) {
      error = errorText(e);
    }
    await load();
  }

  async function choose() {
    const picked = await open({ directory: true, title: "Where should Qanuvi keep its backups?" }).catch(() => null);
    if (typeof picked === "string") await save(picked, info?.off ?? false);
  }

  async function now() {
    error = "";
    note = "";
    busy = true;
    try {
      await invoke<string>("backups_now");
      note = "Backed up.";
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
    await load();
  }

  async function restore(b: Backup) {
    const yes = await ask(
      `Restore the backup from ${when(b.created)}? It is applied when Qanuvi starts again. Your data as it is then is backed up first and kept beside the others, so you can go back to it.`,
      { title: "Restore a backup", kind: "warning", okLabel: "Restore at the next start", cancelLabel: "Cancel" },
    );
    if (!yes) return;
    error = "";
    try {
      await invoke("backups_restore", { path: b.path });
    } catch (e) {
      error = errorText(e);
    }
    await load();
  }

  async function cancelRestore() {
    await invoke("backups_cancel_restore").catch((e) => (error = errorText(e)));
    await load();
  }

  async function remove(b: Backup) {
    const yes = await ask(`Remove the backup from ${when(b.created)}? It cannot be brought back.`, {
      title: "Remove a backup",
      kind: "warning",
      okLabel: "Remove",
      cancelLabel: "Keep it",
    });
    if (!yes) return;
    try {
      await invoke("backups_remove", { path: b.path });
    } catch (e) {
      error = errorText(e);
    }
    await load();
  }
</script>

<h2>Backups</h2>
<p class="intro">
  Once a day Qanuvi copies its data: projects, sessions, memory, Qanuvi core's records, agent histories and the window's
  settings. The database is copied whole while Qanuvi runs. The last {info?.keep ?? 7} backups are kept.
</p>

{#if info}
  <div class="card">
    <div class="row">
      <label class="check">
        <input type="checkbox" checked={!info.off} onchange={(e) => save(folder, !e.currentTarget.checked)} />
        Back up every day
      </label>
      <span class="spacer"></span>
      <button type="button" class="btn primary" disabled={busy} onclick={now}>
        {busy ? "Backing up…" : "Back up now"}
      </button>
    </div>
    <p class="small">
      {#if info.lastAt}Last backup: {when(info.lastAt)}{:else}No backup yet{/if}
      {#if info.off}<span class="warn">· daily backups are off</span>{/if}
    </p>
    {#if info.lastError}<p class="error small">The last backup failed: {info.lastError}</p>{/if}
    {#if note}<p class="ok small">{note}</p>{/if}

    <p class="small">
      Folder: <code>{info.dir}</code>
      {#if !info.custom}<span class="muted">(the default)</span>{/if}
    </p>
    <div class="row">
      <input
        bind:value={folder}
        placeholder="A folder, for example D:\Qanuvi backups"
        spellcheck="false"
        aria-label="Backups folder"
      />
      <button type="button" class="btn" onclick={choose}>Choose…</button>
      <button
        type="button"
        class="btn"
        disabled={!folder.trim() || folder.trim() === info.dir}
        onclick={() => save(folder, info!.off)}>Use this folder</button
      >
      {#if info.custom}
        <button type="button" class="link" onclick={() => save("", info!.off)}>Back to the default</button>
      {/if}
    </div>
    {#if sameDrive}
      <p class="muted small">Another drive is safer: a backup on the same drive is lost with it.</p>
    {/if}
  </div>

  {#if info.restore}
    <div class="card">
      <p class="warn">
        <code>{info.restore.split(/[\\/]/).pop()}</code> is restored when Qanuvi starts again. Your data as it is then is
        backed up first.
      </p>
      <div class="row">
        <button type="button" class="btn primary" onclick={() => invoke("app_restart")}>Restart Qanuvi now</button>
        <button type="button" class="link" onclick={cancelRestore}>Cancel the restore</button>
      </div>
    </div>
  {/if}

  <h3>Backups in the folder</h3>
  {#each info.backups as b (b.path)}
    <div class="item">
      <div class="what">
        <strong>{when(b.created)}</strong>
        <span class="muted small">
          {KIND[b.kind]} · {size(b.bytes)} · {b.files} files · Qanuvi {b.version}{b.window
            ? ""
            : " · no window settings"}
        </span>
      </div>
      <button type="button" class="btn" disabled={!!info.restore} onclick={() => restore(b)}>Restore…</button>
      <button type="button" class="link" onclick={() => remove(b)}>Remove…</button>
    </div>
  {:else}
    <p class="muted small">None yet.</p>
  {/each}
{/if}
{#if error}<p class="error small">{error}</p>{/if}

<style>
  h2 {
    margin: 0 0 6px;
    font-size: 16px;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 13px;
  }

  .intro {
    margin: 0 0 12px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  p {
    margin: 4px 0;
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
    padding: 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    font-size: 12px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }

  label.check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
  }

  input:not([type="checkbox"]) {
    flex: 1;
    min-width: 220px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--qx-border);
    font-size: 12px;
  }

  .what {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }

  code {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--qx-bg-window);
    user-select: text;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11.5px;
  }

  .ok {
    color: var(--qx-status-done);
  }

  .warn {
    color: var(--qx-status-waiting);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
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
    opacity: 0.55;
    cursor: default;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }
</style>
