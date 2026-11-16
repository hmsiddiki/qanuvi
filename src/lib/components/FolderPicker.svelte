<script lang="ts">
  /**
   * Qanuvi's own folder picker for a server's folders (stores/folderPicker.svelte.ts): a path box (Enter goes there),
   * Up, the folders of the folder listed (a click opens one), Escape or a click outside cancels.
   */
  import Folder from "@lucide/svelte/icons/folder";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Server from "@lucide/svelte/icons/server";
  import { folderPicker } from "$lib/stores/folderPicker.svelte";

  let c = $derived(folderPicker.current);
  let box = $state<HTMLInputElement>();

  $effect(() => {
    if (c) queueMicrotask(() => box?.focus());
  });

  function keydown(e: KeyboardEvent) {
    if (!c) return;
    if (e.key === "Escape") {
      e.preventDefault();
      folderPicker.cancel();
    }
  }
</script>

<svelte:window onkeydown={keydown} />

{#if c}
  <div
    class="backdrop"
    role="presentation"
    onpointerdown={(e) => e.target === e.currentTarget && folderPicker.cancel()}
  >
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Add a project folder on {c.name}">
      <div class="head">
        <span class="icon"><Server size={18} /></span>
        <h2>Add a project folder on {c.name}</h2>
      </div>
      <form
        class="path"
        onsubmit={(e) => {
          e.preventDefault();
          void folderPicker.go(folderPicker.typed.trim());
        }}
      >
        <button
          type="button"
          class="up"
          title="The folder above"
          aria-label="The folder above"
          disabled={!folderPicker.folder?.parent}
          onclick={() => folderPicker.up()}><ArrowUp size={14} /></button
        >
        <input
          bind:this={box}
          value={folderPicker.typed}
          oninput={(e) => (folderPicker.typed = e.currentTarget.value)}
          placeholder="Folder on {c.name}, for example /home/deploy/sites/shop"
          spellcheck="false"
          aria-label="Folder path"
        />
      </form>
      {#if folderPicker.error}<p class="error">{folderPicker.error}</p>{/if}
      <ul class="list" aria-label="Folders" aria-busy={folderPicker.loading}>
        {#if !folderPicker.folder}
          <li class="note">{folderPicker.loading ? "Reading the folders…" : ""}</li>
        {:else if !folderPicker.folder.dirs.length}
          <li class="note">No folders in here.</li>
        {:else}
          {#each folderPicker.folder.dirs as d (d.path)}
            <li>
              <button type="button" class="dir" title={d.path} onclick={() => folderPicker.go(d.path)}>
                <Folder size={14} />
                <span>{d.name}</span>
              </button>
            </li>
          {/each}
        {/if}
      </ul>
      <div class="actions">
        <button type="button" onclick={() => folderPicker.cancel()}>Cancel</button>
        <button
          type="button"
          class="primary"
          disabled={!folderPicker.typed.trim() || folderPicker.loading}
          onclick={() => folderPicker.choose()}>Add this folder</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 300;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 40%);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: min(560px, calc(100% - 32px));
    /* A fixed height (not one that follows the folder count), so Up and the path box stay where they were after a
       folder opens or an error line shows; the list takes what is left. */
    height: min(600px, calc(100% - 48px));
    padding: 18px 20px 16px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    background: var(--qx-bg-raised);
    box-shadow: 0 18px 48px rgb(0 0 0 / 50%);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .icon {
    display: flex;
    color: var(--qx-accent);
  }

  h2 {
    margin: 0;
    color: var(--qx-text);
    font-size: 14.5px;
  }

  .path {
    display: flex;
    gap: 6px;
  }

  input {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-family: var(--qx-font-mono);
    font-size: 12.5px;
  }

  input:focus {
    border-color: var(--qx-accent);
    outline: none;
  }

  .error {
    margin: 0;
    color: var(--qx-status-failed);
    font-size: 12.5px;
    user-select: text;
  }

  .list {
    flex: 1;
    min-height: 180px;
    margin: 0;
    padding: 4px;
    overflow: auto;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    list-style: none;
  }

  .note {
    padding: 8px;
    color: var(--qx-text-muted);
    font-size: 12.5px;
  }

  .dir {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .dir:hover,
  .dir:focus-visible {
    background: color-mix(in srgb, var(--qx-text) 8%, transparent);
    outline: none;
  }

  .dir :global(svg) {
    flex: none;
    color: var(--qx-accent);
  }

  .dir span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .actions button,
  .up {
    height: 30px;
    padding: 0 14px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }

  .up {
    display: grid;
    place-items: center;
    width: 30px;
    padding: 0;
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .actions button:not(:disabled):hover,
  .up:not(:disabled):hover {
    border-color: var(--qx-text-muted);
  }

  .actions button.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }
</style>
