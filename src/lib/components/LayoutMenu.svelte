<script lang="ts">
  import LayoutPanelLeft from "@lucide/svelte/icons/layout-panel-left";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import { layout } from "$lib/layout/layout.svelte";

  let open = $state(false);
  let newName = $state("");
  let message = $state("");
  let root: HTMLDivElement;

  function choose(name: string) {
    layout.apply(name);
    open = false;
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const name = layout.saveAs(newName);
    message = name ? `Saved as "${name}"` : "Type a name (up to 40 characters, 20 layouts at most).";
    if (name) newName = "";
  }

  function outside(event: PointerEvent) {
    if (open && root && !root.contains(event.target as Node)) open = false;
  }

  function keydown(event: KeyboardEvent) {
    if (open && event.key === "Escape") open = false;
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={keydown} />

<div class="layout-menu" bind:this={root}>
  <button
    type="button"
    class="trigger"
    aria-haspopup="menu"
    aria-expanded={open}
    title="Layouts"
    onclick={() => ((open = !open), (message = ""))}
  >
    <LayoutPanelLeft size={15} />
    <span>{layout.active}</span>
  </button>

  {#if open}
    <div class="menu" role="menu">
      <div class="section">Layouts</div>
      {#each Object.keys(layout.named) as name (name)}
        <div class="row">
          <button type="button" role="menuitemradio" aria-checked={name === layout.active} onclick={() => choose(name)}>
            <span class="tick"
              >{#if name === layout.active}<Check size={14} />{/if}</span
            >
            <span class="name">{name}</span>
          </button>
          <button
            type="button"
            class="remove"
            title={layout.isPreset(name) ? `Reset "${name}" to its original settings` : `Remove "${name}"`}
            aria-label={layout.isPreset(name) ? `Reset ${name}` : `Remove ${name}`}
            onclick={() => layout.remove(name)}
          >
            <X size={13} />
          </button>
        </div>
      {/each}
      <form class="save" onsubmit={save}>
        <input bind:value={newName} placeholder="Save current layout as…" maxlength="40" aria-label="Layout name" />
        <button type="submit">Save</button>
      </form>
      {#if message}<p class="message">{message}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .layout-menu {
    position: relative;
  }

  .trigger {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 0 var(--qx-space-2);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .trigger:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    width: 260px;
    padding: var(--qx-space-2);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    box-shadow: 0 12px 32px rgb(0 0 0 / 45%);
    z-index: 50;
  }

  .section {
    padding: var(--qx-space-1) var(--qx-space-2) var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .row {
    display: flex;
    align-items: center;
    border-radius: var(--qx-radius-button);
  }

  .row:hover {
    background: var(--qx-bg-panel);
  }

  .row button {
    border: 0;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .row button[role="menuitemradio"] {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 6px var(--qx-space-2);
    text-align: left;
  }

  .tick {
    width: 14px;
    display: inline-grid;
    color: var(--qx-accent);
  }

  .remove {
    width: 26px;
    height: 26px;
    display: grid;
    place-items: center;
    color: var(--qx-text-muted) !important;
    visibility: hidden;
  }

  .row:hover .remove {
    visibility: visible;
  }

  .remove:hover {
    color: var(--qx-text) !important;
  }

  .save {
    display: flex;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-2);
    padding-top: var(--qx-space-2);
    border-top: 1px solid var(--qx-border);
  }

  .save input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  .save input:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .save button {
    height: 28px;
    padding: 0 var(--qx-space-3);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }

  .message {
    margin: var(--qx-space-2) var(--qx-space-1) 0;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }
</style>
