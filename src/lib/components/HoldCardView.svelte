<script lang="ts">
  import PlainText from "./PlainText.svelte";
  /**
   * n1: a blocked or unfinished plan step's question, laid out so it can be answered without the chat: the headline,
   * what happened, why it needs the owner, what is needed, the exact text to copy, and what Qanuvi can do itself.
   * Used by the Inbox question and the run panel, so a question reads the same everywhere.
   */
  import type { HoldCard } from "$lib/plan/holdask";
  import { copyText } from "$lib/stores/contextMenu.svelte";

  let { card }: { card: HoldCard } = $props();

  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function copy() {
    if (!card.snippet) return;
    copied = await copyText(card.snippet.text);
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 2000);
  }

  const parts = $derived(
    [
      ["What happened", card.happened],
      ["Why it needs you", card.whyYou],
      ["What is needed", card.wanted],
    ].filter((p): p is [string, string] => !!p[1]?.trim()),
  );
</script>

<div class="card">
  {#if card.reply?.trim()}<p class="reply"><PlainText text={card.reply} /></p>{/if}
  <p class="headline"><PlainText text={card.headline} /></p>
  {#each parts as [label, text] (label)}
    <p class="part"><span class="label">{label}:</span> <PlainText {text} /></p>
  {/each}
  {#if card.snippet?.text}
    <div class="snippet">
      <div class="snippet-head">
        <span class="label">{card.snippet.label || "Copy this"}</span>
        <button type="button" class="copy" onclick={() => void copy()}>{copied ? "Copied" : "Copy"}</button>
      </div>
      <pre>{card.snippet.text}</pre>
    </div>
  {/if}
  {#if card.canDo?.trim()}<p class="can-do"><PlainText text={card.canDo} /></p>{/if}
</div>

<style>
  .card {
    margin: 0 0 8px;
  }

  p {
    margin: 0 0 6px;
    white-space: pre-wrap;
  }

  .headline {
    font-weight: 600;
  }

  .reply {
    padding: 6px 8px;
    border-left: 2px solid var(--qx-accent);
    background: color-mix(in srgb, var(--qx-accent) 8%, transparent);
  }

  .part,
  .can-do {
    color: var(--qx-text-secondary);
  }

  .label {
    color: var(--qx-text);
    font-weight: 500;
  }

  .snippet {
    margin: 0 0 6px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    overflow: hidden;
  }

  .snippet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 3px 8px;
    border-bottom: 1px solid var(--qx-border);
    font-size: 0.92em;
  }

  pre {
    margin: 0;
    padding: 6px 8px;
    overflow-x: auto;
    background: var(--qx-bg-window);
    font-family: var(--qx-font-mono);
    font-size: 0.92em;
    white-space: pre;
  }

  .copy {
    padding: 1px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
</style>
