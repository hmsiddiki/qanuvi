<script lang="ts">
  /** The debug console (15i part 3): the program's output, and expressions evaluated where it is paused. */
  import { dap } from "$lib/stores/dap.svelte";
  import { tick } from "svelte";

  let text = $state("");
  let list: HTMLDivElement | undefined = $state();
  let history: string[] = [];
  let back = -1;

  // Follow new lines unless the reader scrolled up.
  $effect(() => {
    void dap.console.length;
    const el = list;
    if (!el) return;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    if (atEnd) tick().then(() => (el.scrollTop = el.scrollHeight));
  });

  function key(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      const t = text.trim();
      if (!t) return;
      history = [...history.filter((h) => h !== t), t].slice(-50);
      back = -1;
      text = "";
      dap.evaluate(t);
    } else if (e.key === "ArrowUp" && history.length) {
      e.preventDefault();
      back = back < 0 ? history.length - 1 : Math.max(0, back - 1);
      text = history[back];
    } else if (e.key === "ArrowDown" && back >= 0) {
      e.preventDefault();
      back = back + 1 >= history.length ? -1 : back + 1;
      text = back < 0 ? "" : history[back];
    }
  }
</script>

<div class="console">
  <div class="lines" bind:this={list} role="log" aria-label="Debug console">
    {#each dap.console as l, i (i)}
      <pre class={l.kind}>{l.kind === "in" ? `> ${l.text}` : l.text}</pre>
    {:else}
      <p class="empty">The program's output shows here while debugging (F5 starts).</p>
    {/each}
  </div>
  <div class="input">
    <span aria-hidden="true">›</span>
    <input
      bind:value={text}
      onkeydown={key}
      disabled={!dap.session}
      placeholder={dap.session?.status === "stopped"
        ? "Evaluate in the selected frame"
        : dap.session
          ? "Evaluate (pause for the frame's variables)"
          : "Start debugging to evaluate"}
      aria-label="Evaluate an expression"
    />
    {#if dap.console.length}
      <button type="button" onclick={() => dap.clearConsole()}>Clear</button>
    {/if}
  </div>
</div>

<style>
  .console {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: var(--qx-text-small);
  }

  .lines {
    flex: 1;
    overflow: auto;
    padding: 4px 10px;
  }

  pre {
    margin: 0;
    font-family: var(--qx-font-mono);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .err {
    color: var(--qx-danger);
  }

  .info {
    color: var(--qx-text-muted);
  }

  .in {
    color: var(--qx-text-secondary);
  }

  .result {
    color: var(--qx-accent);
  }

  .empty {
    margin: 4px 0;
    color: var(--qx-text-muted);
  }

  .input {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-top: 1px solid var(--qx-border);
  }

  .input input {
    flex: 1;
    border: none;
    background: transparent;
    color: inherit;
    font-family: var(--qx-font-mono);
    font-size: inherit;
    outline: none;
  }

  .input button {
    border: none;
    background: transparent;
    color: var(--qx-text-muted);
    font: inherit;
    cursor: pointer;
  }
</style>
