<script lang="ts">
  /**
   * One agent step as a card: the tool and what it does on top; IN (the command or input) and OUT (the result) in mono
   * blocks below, a few lines with a fade and a click to show all. Shell steps open by themselves, others on a click.
   * A test-browser step shows the address or element and its screenshot.
   */
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Copy from "@lucide/svelte/icons/copy";
  import CopyCheck from "@lucide/svelte/icons/copy-check";
  import type { Item } from "$lib/agent/events";
  import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import ChatImage from "./ChatImage.svelte";
  import { untrack } from "svelte";
  import { diffCounts, editDiff } from "$lib/agent/diff";

  type Tool = Extract<Item, { kind: "tool" }>;
  let {
    step,
    onopen,
    onrevert,
  }: {
    step: Tool;
    onopen: (file: string | null) => void;
    /** Undoes the step's edits in the file; answers why not when it cannot. */
    onrevert?: (step: Tool) => Promise<string | null>;
  } = $props();

  // A file change shows as a diff (open from the start: the change is the point of the step).
  const diffs = $derived(step.edits?.map((e) => editDiff(e.old, e.new)) ?? []);
  const counts = $derived(step.edits ? diffCounts(step.edits) : null);
  /** Lines a folded diff shows. */
  const DIFF_FOLDED = 14;
  let diffAll = $state(false);
  const diffLong = $derived(diffs.reduce((n, d) => n + d.length, 0) > DIFF_FOLDED);
  let reverted = $state(false);
  let revertNote = $state("");
  async function revert() {
    if (!onrevert) return;
    const why = await onrevert(step);
    if (why === null) reverted = true;
    else revertNote = why;
  }

  /** Lines a folded IN or OUT block shows. */
  const FOLDED_LINES = 6;

  const SHELL = /^(Bash|PowerShell|run_command|shell)$/i;
  const shell = $derived(SHELL.test(step.name));
  const browser = $derived(step.name.startsWith("browser_"));
  // Shell commands are worth seeing as they run: open at first; the rest open on a click (a screenshot opens its card
  // when it arrives, below).
  let open = $state(untrack(() => SHELL.test(step.name) || !!step.edits?.length));
  let shotSeen = false;
  $effect(() => {
    if (step.image && !shotSeen) {
      shotSeen = true;
      open = true;
    }
  });

  let inAll = $state(false);
  let outAll = $state(false);
  let copied = $state("");

  const lines = (t: string) => t.split("\n").length;
  const longIn = $derived(lines(step.input) > FOLDED_LINES || step.input.length > 600);
  const longOut = $derived(lines(step.output) > FOLDED_LINES || step.output.length > 600);

  function copy(which: "in" | "out", text: string) {
    copyText(text);
    copied = which;
    setTimeout(() => copied === which && (copied = ""), 1200);
  }

  function menu(e: MouseEvent) {
    contextMenu.show(e, [
      [
        { label: "Open the file", disabled: !step.file, action: () => onopen(step.file) },
        { label: "Copy the file's path", disabled: !step.file, action: () => copyText(step.file ?? "") },
      ],
      [
        { label: "Copy the input", disabled: !step.input, action: () => copyText(step.input) },
        { label: "Copy the result", disabled: !step.output, action: () => copyText(step.output) },
      ],
    ]);
  }
</script>

<div class="card {step.state}" class:open role="presentation" oncontextmenu={menu}>
  <div class="head">
    <button type="button" class="toggle" aria-expanded={open} onclick={() => (open = !open)}>
      <span class="chev"><ChevronRight size={12} /></span>
      <span class="icon">
        {#if step.state === "running"}<LoaderCircle
            size={13}
            class="spin"
          />{:else if step.state === "done"}<CircleCheck size={13} />{:else}<CircleX size={13} />{/if}
      </span>
      <span class="name">{step.name}</span>
      <span class="summary">{step.summary}</span>
      {#if counts}<span class="counts"
          ><span class="plus">+{counts.added}</span> <span class="minus">−{counts.removed}</span></span
        >{/if}
    </button>
    {#if step.edits && !step.whole && step.state === "done" && onrevert}
      <button
        type="button"
        class="file"
        disabled={reverted}
        title={reverted ? "This change was undone" : "Undo this change in the file"}
        onclick={revert}>{reverted ? "Reverted" : "Revert"}</button
      >
    {/if}
    {#if step.file}
      <button type="button" class="file" title="Open {step.file}" onclick={() => onopen(step.file)}>Open</button>
    {/if}
  </div>
  {#if !open && step.result}
    <div class="peek">{step.result}</div>
  {/if}
  {#if open}
    <div class="body">
      {#if diffs.length}
        <div class="diff" class:folded={diffLong && !diffAll}>
          {#each diffs as lines, n (n)}
            {#if n > 0}<div class="sep"></div>{/if}
            {#each lines as l, i (i)}
              <div class="dl {l.type}">
                <span class="sign"
                  >{l.type === "add" ? "+" : l.type === "del" ? "−" : l.type === "gap" ? "⋯" : " "}</span
                >{l.type === "gap" ? l.text : l.text || " "}
              </div>
            {/each}
          {/each}
        </div>
        {#if diffLong}<button type="button" class="more" onclick={() => (diffAll = !diffAll)}
            >{diffAll ? "Show less" : "Show the whole change"}</button
          >{/if}
        {#if step.whole}<div class="waiting">
            The whole file was written; Source control shows what it replaced.
          </div>{/if}
        {#if revertNote}<div class="waiting">{revertNote}</div>{/if}
      {/if}
      {#if step.input && !diffs.length}
        <div class="block">
          <span class="label">IN</span>
          <pre class:folded={longIn && !inAll}>{step.input}</pre>
          <button type="button" class="copy" title="Copy" onclick={() => copy("in", step.input)}
            >{#if copied === "in"}<CopyCheck size={12} />{:else}<Copy size={12} />{/if}</button
          >
          {#if longIn}<button type="button" class="more" onclick={() => (inAll = !inAll)}
              >{inAll ? "Show less" : "Show all"}</button
            >{/if}
        </div>
      {/if}
      {#if step.output && (!diffs.length || step.state === "failed")}
        <div class="block" class:fail={step.state === "failed"}>
          <span class="label">OUT</span>
          <pre class:folded={longOut && !outAll}>{step.output}</pre>
          <button type="button" class="copy" title="Copy" onclick={() => copy("out", step.output)}
            >{#if copied === "out"}<CopyCheck size={12} />{:else}<Copy size={12} />{/if}</button
          >
          {#if longOut}<button type="button" class="more" onclick={() => (outAll = !outAll)}
              >{outAll ? "Show less" : "Show all"}</button
            >{/if}
        </div>
      {:else if step.state === "running" && shell}
        <div class="waiting">Running…</div>
      {/if}
      {#if step.image}
        <div class="shot">
          <ChatImage
            image={{ name: browser ? "Screenshot from the test browser" : "Screenshot", src: step.image }}
            size={300}
          />
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    min-width: 0;
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--qx-space-1);
    padding-right: var(--qx-space-2);
  }

  .toggle {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 5px var(--qx-space-2);
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .toggle:hover .summary {
    color: var(--qx-text);
  }

  .chev {
    display: flex;
    flex: none;
    color: var(--qx-text-muted);
    transition: transform 0.12s;
  }

  .open .chev {
    transform: rotate(90deg);
  }

  .icon {
    display: flex;
    flex: none;
    color: var(--qx-text-muted);
  }

  .done .icon {
    color: var(--qx-status-done);
  }

  .failed .icon {
    color: var(--qx-status-failed);
  }

  .name {
    flex: none;
    color: var(--qx-text);
    font-weight: 600;
  }

  .summary {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file {
    flex: none;
    padding: 1px 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .file:hover {
    color: var(--qx-text);
  }

  .peek {
    overflow: hidden;
    padding: 0 var(--qx-space-2) 5px 44px;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--qx-space-1);
    padding: 0 var(--qx-space-2) var(--qx-space-2);
  }

  .block {
    position: relative;
    display: grid;
    grid-template-columns: 34px 1fr;
    border-radius: 6px;
    background: var(--qx-bg-raised);
  }

  .label {
    padding: 6px 0 0 8px;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono, monospace);
    font-size: 10px;
    letter-spacing: 0.04em;
  }

  .block.fail .label {
    color: var(--qx-status-failed);
  }

  pre {
    margin: 0;
    padding: 6px 28px 6px 0;
    overflow-x: auto;
    color: var(--qx-text);
    font-family: var(--qx-font-mono, monospace);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  pre.folded {
    max-height: calc(1.45em * 6 + 12px);
    overflow: hidden;
    mask-image: linear-gradient(to bottom, #000 60%, transparent);
  }

  .copy {
    position: absolute;
    top: 4px;
    right: 4px;
    display: flex;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--qx-text-muted);
    opacity: 0;
    cursor: pointer;
  }

  .block:hover .copy,
  .copy:focus-visible {
    opacity: 1;
  }

  .copy:hover {
    color: var(--qx-text);
    background: var(--qx-bg-panel);
  }

  .more {
    grid-column: 2;
    justify-self: start;
    margin: 0 0 4px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--qx-accent);
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .counts {
    flex: none;
    font-family: var(--qx-font-mono, monospace);
    font-size: 11px;
  }

  .plus {
    color: var(--qx-status-done);
  }

  .minus {
    color: var(--qx-status-failed);
  }

  .file:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .diff {
    overflow-x: auto;
    padding: 4px 0;
    border-radius: 6px;
    background: var(--qx-bg-raised);
    font-family: var(--qx-font-mono, monospace);
    font-size: 12px;
    line-height: 1.45;
  }

  .diff.folded {
    max-height: calc(1.45em * 14 + 8px);
    overflow: hidden;
    mask-image: linear-gradient(to bottom, #000 70%, transparent);
  }

  .dl {
    padding: 0 8px 0 0;
    color: var(--qx-text);
    white-space: pre;
  }

  .dl.add {
    background: color-mix(in srgb, var(--qx-status-done) 16%, transparent);
  }

  .dl.del {
    background: color-mix(in srgb, var(--qx-status-failed) 16%, transparent);
  }

  .dl.ctx {
    color: var(--qx-text-secondary);
  }

  .dl.gap {
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .sign {
    display: inline-block;
    width: 22px;
    color: var(--qx-text-muted);
    text-align: center;
    user-select: none;
  }

  .sep {
    height: 1px;
    margin: 4px 0;
    background: var(--qx-border);
  }

  .waiting {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .shot {
    padding-top: 2px;
  }
</style>
