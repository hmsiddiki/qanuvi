<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    /** Shown in the tooltip after the label, for example "Ctrl+B". */
    shortcut?: string;
    active?: boolean;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
    children: Snippet;
  }

  let { label, shortcut, active = false, disabled = false, onclick, children }: Props = $props();
</script>

<button
  type="button"
  class="icon-button"
  class:active
  aria-label={label}
  aria-pressed={active}
  title={shortcut ? `${label} (${shortcut})` : label}
  {disabled}
  {onclick}
>
  {@render children()}
</button>

<style>
  .icon-button {
    width: 28px;
    height: 28px;
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .icon-button:hover:not(:disabled) {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .icon-button.active {
    color: var(--qx-text);
  }

  .icon-button:disabled {
    color: var(--qx-text-muted);
    opacity: 0.5;
    cursor: default;
  }

  .icon-button:focus-visible {
    outline: 1px solid var(--qx-accent);
    outline-offset: 1px;
  }
</style>
