<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { agent } from "$lib/stores/agent.svelte";

  interface Props {
    disabled?: boolean;
  }

  let { disabled = false }: Props = $props();

  let open = $state(false);
  let root: HTMLDivElement;

  let modes = $derived(agent.provider.modes);
  let current = $derived(modes.find((m) => m.id === agent.mode) ?? modes[0]);

  function choose(id: string) {
    agent.pickMode(id);
    open = false;
  }
</script>

<svelte:window
  onpointerdown={(e) => open && root && !root.contains(e.target as Node) && (open = false)}
  onkeydown={(e) => open && e.key === "Escape" && (open = false)}
/>

<div class="mode-picker" bind:this={root}>
  <button
    type="button"
    class="trigger"
    title="{current.label}: {current.description} (Shift+Tab to switch)"
    aria-haspopup="menu"
    aria-expanded={open}
    {disabled}
    onclick={() => (open = !open)}
  >
    <current.icon size={14} />
    <span>{current.label}</span>
  </button>

  {#if open}
    <div class="menu" role="menu">
      <div class="head">
        <span>Modes</span>
        <span class="keys"><kbd>Shift</kbd> + <kbd>Tab</kbd> to switch</span>
      </div>
      {#each modes as m (m.id)}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={m.id === agent.mode}
          class:selected={m.id === agent.mode}
          onclick={() => choose(m.id)}
        >
          <span class="icon"><m.icon size={17} /></span>
          <span class="text">
            <strong>{m.label}</strong>
            <span>{m.description}</span>
          </span>
          <span class="tick"
            >{#if m.id === agent.mode}<Check size={15} />{/if}</span
          >
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .mode-picker {
    position: relative;
  }

  .trigger {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 var(--qx-space-2);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .trigger:hover:not(:disabled) {
    background: var(--qx-bg-panel);
    color: var(--qx-text);
  }

  .trigger:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .menu {
    position: absolute;
    right: 0;
    bottom: calc(100% + 8px);
    width: min(340px, 90vw);
    padding: var(--qx-space-1);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    box-shadow: 0 16px 40px rgb(0 0 0 / 50%);
    z-index: 60;
  }

  .head {
    display: flex;
    justify-content: space-between;
    padding: var(--qx-space-2) var(--qx-space-2) var(--qx-space-1);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  kbd {
    padding: 0 4px;
    border: 1px solid var(--qx-border);
    border-radius: 4px;
    font-family: inherit;
    font-size: 10px;
  }

  .menu button[role="menuitemradio"] {
    width: 100%;
    display: flex;
    align-items: flex-start;
    gap: var(--qx-space-3);
    padding: var(--qx-space-2);
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .menu button:hover,
  .menu button.selected {
    background: var(--qx-bg-panel);
  }

  .icon {
    flex: none;
    padding-top: 2px;
    color: var(--qx-text-secondary);
  }

  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .text strong {
    font-weight: 500;
  }

  .text span {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .tick {
    flex: none;
    width: 16px;
    padding-top: 2px;
    color: var(--qx-text);
  }
</style>
