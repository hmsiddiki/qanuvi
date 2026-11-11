<script lang="ts">
  /**
   * The composer's + menu: add to the next message a file from this computer (images the agent sees, text files it
   * reads), a file from the project (a mention), or a web page read as text.
   */
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import Plus from "@lucide/svelte/icons/plus";
  import Upload from "@lucide/svelte/icons/upload";
  import FileText from "@lucide/svelte/icons/file-text";
  import Globe from "@lucide/svelte/icons/globe";
  import { api, errorText } from "$lib/api";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  let { disabled = false }: { disabled?: boolean } = $props();

  let open = $state(false);
  let asking = $state(false);
  let url = $state("");
  let busy = $state("");
  let error = $state("");
  let root: HTMLDivElement;

  function close() {
    open = false;
    asking = false;
    url = "";
  }

  async function upload() {
    close();
    error = "";
    const picked = await openDialog({ multiple: true, directory: false }).catch(() => null);
    const paths = picked === null ? [] : Array.isArray(picked) ? picked : [picked];
    for (const path of paths) {
      busy = "Reading…";
      try {
        agent.attachments = [...agent.attachments, await api.attachments.file(path)];
      } catch (e) {
        error = errorText(e);
      }
    }
    busy = "";
    agent.input?.focus();
  }

  function mention() {
    close();
    ui.pickFile = (path) => {
      const m = `@${path} `;
      agent.draft = agent.draft && !agent.draft.endsWith(" ") ? `${agent.draft} ${m}` : agent.draft + m;
      agent.input?.focus();
    };
    ui.quickOpen = true;
  }

  async function readPage(event: SubmitEvent) {
    event.preventDefault();
    const address = url.trim();
    if (!address) return;
    close();
    error = "";
    busy = "Reading the page…";
    try {
      agent.attachments = [...agent.attachments, await api.attachments.page(address)];
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
      agent.input?.focus();
    }
  }

  function outside(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) close();
  }
</script>

<svelte:window onpointerdown={outside} />

<div class="attach" bind:this={root}>
  <button
    type="button"
    title="Add files, project files or a web page"
    aria-label="Add to the message"
    aria-haspopup="menu"
    aria-expanded={open}
    {disabled}
    onclick={() => (open = !open)}
  >
    <Plus size={15} />
  </button>
  {#if open}
    <div class="menu" role="menu">
      {#if asking}
        <form onsubmit={readPage}>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="url"
            placeholder="https://… (the page is read as text)"
            bind:value={url}
            autofocus
            onkeydown={(e) => e.key === "Escape" && close()}
          />
        </form>
      {:else}
        <button type="button" role="menuitem" onclick={upload}><Upload size={14} /> Upload from computer</button>
        <button type="button" role="menuitem" onclick={mention}
          ><FileText size={14} /> Add context from the project</button
        >
        <button type="button" role="menuitem" onclick={() => (asking = true)}><Globe size={14} /> Browse the web</button
        >
      {/if}
    </div>
  {/if}
  {#if busy || error}
    <span class="status" class:error={!!error} title={error || busy}>{error || busy}</span>
  {/if}
</div>

<style>
  .attach {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .attach > button {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .attach > button:hover:not(:disabled) {
    color: var(--qx-text-primary);
  }

  .menu {
    position: absolute;
    bottom: 32px;
    left: 0;
    z-index: 20;
    min-width: 240px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
    box-shadow: 0 6px 18px rgb(0 0 0 / 35%);
  }

  .menu button {
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-primary);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .menu button:hover {
    background: var(--qx-bg-hover, rgb(255 255 255 / 6%));
  }

  .menu input {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg);
    color: var(--qx-text-primary);
    font: inherit;
    font-size: 12.5px;
  }

  .status {
    overflow: hidden;
    max-width: 220px;
    color: var(--qx-text-secondary);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .status.error {
    color: var(--qx-danger, #e5534b);
  }
</style>
