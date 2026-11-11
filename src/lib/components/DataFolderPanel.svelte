<script lang="ts">
  /**
   * Where Qanuvi keeps its data (15e): any folder on any drive. The move happens when Qanuvi starts again (nothing is
   * in use then): everything is copied, checked and switched; the old copy is kept until the user removes it.
   */
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { errorText } from "$lib/api";

  interface Info {
    current: string;
    defaultDir: string;
    pending: string | null;
    old: string | null;
    note: string | null;
  }

  let info = $state<Info | null>(null);
  let folder = $state("");
  let error = $state("");
  let busy = $state(false);

  async function load() {
    info = await invoke<Info>("data_location_info").catch(() => null);
  }
  onMount(load);

  async function choose() {
    const picked = await open({ directory: true, title: "Where should Qanuvi keep its data?" }).catch(() => null);
    if (typeof picked === "string") folder = picked;
  }

  async function move(target: string) {
    error = "";
    busy = true;
    try {
      await invoke<string>("data_location_move", { folder: target });
      await load();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    await invoke("data_location_cancel").catch((e) => (error = errorText(e)));
    await load();
  }

  async function removeOld() {
    if (!info?.old) return;
    const yes = await ask(
      `Remove the old copy of Qanuvi's data in ${info.old}? Qanuvi now uses ${info.current}; the old copy cannot be brought back.`,
      { title: "Remove the old copy", kind: "warning", okLabel: "Remove", cancelLabel: "Keep it" },
    );
    if (!yes) return;
    try {
      await invoke("data_location_remove_old");
    } catch (e) {
      error = errorText(e);
    }
    await load();
  }
</script>

{#if info}
  <section class="data">
    <h3>Where Qanuvi keeps its data</h3>
    <p class="muted">
      Projects, sessions, memory, Qanuvi core's records, agent histories and the window's settings, all in one folder.
      Put it on any drive.
    </p>
    <p>
      Now: <code>{info.current}</code>
      {#if info.current === info.defaultDir}<span class="muted small">(the default place)</span>{/if}
    </p>
    {#if info.note}<p
        class="small"
        class:ok={!info.note.startsWith("The data was not moved")}
        class:error={info.note.startsWith("The data was not moved")}
      >
        {info.note}
      </p>{/if}

    {#if info.pending}
      <p class="warn">
        Moves to <code>{info.pending}</code> when Qanuvi starts again: copied, checked, then switched.
      </p>
      <div class="row">
        <button type="button" class="btn primary" onclick={() => invoke("app_restart")}>Restart Qanuvi now</button>
        <button type="button" class="link" onclick={cancel}>Cancel the move</button>
      </div>
    {:else}
      <div class="row">
        <input
          bind:value={folder}
          placeholder="A folder, for example E:\Qanuvi"
          spellcheck="false"
          aria-label="Folder"
        />
        <button type="button" class="btn" onclick={choose}>Choose…</button>
        <button type="button" class="btn primary" disabled={busy || !folder.trim()} onclick={() => move(folder)}
          >Move here</button
        >
        {#if info.current !== info.defaultDir}
          <button type="button" class="link" disabled={busy} onclick={() => move(info!.defaultDir)}
            >Back to the default place</button
          >
        {/if}
      </div>
      <p class="muted small">
        A folder with other things in it gets a "Qanuvi" folder inside. Qanuvi checks the drive has room first.
      </p>
    {/if}

    {#if info.old}
      <p class="small">
        The old copy is still in <code>{info.old}</code>.
        <button type="button" class="link" onclick={removeOld}>Remove it…</button>
      </p>
    {/if}
    {#if error}<p class="error small">{error}</p>{/if}
  </section>
{/if}

<style>
  .data {
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

  p {
    margin: 4px 0;
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
    min-width: 220px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
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
    font-size: 11px;
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
