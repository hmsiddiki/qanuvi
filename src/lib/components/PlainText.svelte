<script lang="ts">
  /** Text for the owner with long quoted commands folded: their program shows, "Show command" opens the whole one. */
  import { pieces } from "$lib/longCommands";

  let { text }: { text: string } = $props();

  let shown = $state<Record<number, boolean>>({});
  const list = $derived(pieces(text));
</script>

{#each list as p, i (i)}{#if p.kind === "text"}{p.text}{:else if shown[i]}<code class="cmd">{p.text}</code><button
      type="button"
      class="toggle"
      onclick={() => (shown[i] = false)}>Hide</button
    >{:else}<span class="short">{p.short}</span><button
      type="button"
      class="toggle"
      title={p.text}
      onclick={() => (shown[i] = true)}>Show command</button
    >{/if}{/each}

<style>
  .cmd {
    display: block;
    margin: 4px 0;
    padding: 4px 6px;
    font-family: var(--qx-font-mono, monospace);
    font-size: 0.92em;
    white-space: pre-wrap;
    word-break: break-all;
    background: color-mix(in srgb, var(--qx-text) 6%, transparent);
    border-radius: 4px;
  }

  .short {
    font-family: var(--qx-font-mono, monospace);
    font-size: 0.92em;
  }

  .toggle {
    margin-left: 4px;
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    font-size: 0.9em;
    cursor: pointer;
  }

  .toggle:hover {
    text-decoration: underline;
  }
</style>
