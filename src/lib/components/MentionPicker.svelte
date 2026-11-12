<script lang="ts" module>
  /** The project's file list, read once per project while the app runs (the picker opens often). */
  const fileLists = new Map<string, Promise<string[]>>();
</script>

<script lang="ts">
  /**
   * The @ picker over the message box: files of the project and its symbols that match what is typed after "@".
   * The keys are handled by the message box (it has the focus): ↑ ↓ move, Enter or Tab picks, Escape closes.
   */
  import { api } from "$lib/api";
  import { fuzzyFilter } from "$lib/fuzzy";
  import FileText from "@lucide/svelte/icons/file-text";
  import Braces from "@lucide/svelte/icons/braces";
  import { fileMention, symbolMention } from "$lib/agent/mention";

  export interface Choice {
    key: string;
    title: string;
    detail: string;
    mention: string;
    symbol: boolean;
  }

  let {
    projectId,
    query,
    selected = $bindable(0),
    choices = $bindable<Choice[]>([]),
    onpick,
  }: {
    projectId: string;
    query: string;
    selected?: number;
    choices?: Choice[];
    onpick: (c: Choice) => void;
  } = $props();

  const MAX = 8;

  let files = $state<string[]>([]);
  let symbols = $state<Choice[]>([]);

  $effect(() => {
    let p = fileLists.get(projectId);
    if (!p) {
      p = api.files
        .listAll(projectId)
        .then((r) => r.files)
        .catch(() => []);
      fileLists.set(projectId, p);
    }
    p.then((f) => (files = f));
  });

  // Symbols from the code index, a moment after typing stops (two letters at least).
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const q = query.trim();
    clearTimeout(timer);
    if (q.length < 2 || q.includes("/") || q.includes(".")) {
      symbols = [];
      return;
    }
    timer = setTimeout(() => {
      api.index
        .find(projectId, q)
        .then((hits) => {
          symbols = hits.slice(0, MAX).map((h) => ({
            key: `s:${h.path}:${h.line}:${h.name}`,
            title: h.name,
            detail: `${h.kind} · ${h.path}:${h.line}`,
            mention: symbolMention(h.path, h.name, h.line),
            symbol: true,
          }));
        })
        .catch(() => (symbols = []));
    }, 150);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const fileChoices: Choice[] = (query ? fuzzyFilter(query, files) : files).slice(0, MAX).map((path) => {
      const slash = path.lastIndexOf("/");
      return {
        key: `f:${path}`,
        title: path.slice(slash + 1),
        detail: slash > 0 ? path.slice(0, slash) : "",
        mention: fileMention(path),
        symbol: false,
      };
    });
    // Files first, then symbols; at most MAX in all, with room for a few symbols when both match.
    const sym = symbols.slice(0, Math.max(3, MAX - fileChoices.length));
    choices = [...fileChoices.slice(0, MAX - sym.length), ...sym];
    if (selected >= choices.length) selected = 0;
  });
</script>

{#if choices.length}
  <div class="picker" role="listbox" aria-label="Mention a file or symbol">
    {#each choices as c, i (c.key)}
      <button
        type="button"
        role="option"
        aria-selected={i === selected}
        class:selected={i === selected}
        onmousedown={(e) => e.preventDefault()}
        onmouseenter={() => (selected = i)}
        onclick={() => onpick(c)}
      >
        <span class="icon"
          >{#if c.symbol}<Braces size={13} />{:else}<FileText size={13} />{/if}</span
        >
        <span class="title">{c.title}</span>
        <span class="detail">{c.detail}</span>
      </button>
    {/each}
    <div class="hint">↑ ↓ to choose · Enter or Tab to mention · Esc to close</div>
  </div>
{/if}

<style>
  .picker {
    position: absolute;
    right: 0;
    bottom: 100%;
    left: 0;
    z-index: 20;
    margin-bottom: 4px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    box-shadow: 0 6px 20px rgb(0 0 0 / 25%);
  }

  button {
    display: flex;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 4px 6px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  button.selected {
    background: color-mix(in srgb, var(--qx-accent) 18%, transparent);
  }

  .icon {
    display: flex;
    flex: none;
    color: var(--qx-text-muted);
  }

  .title {
    flex: none;
  }

  .detail {
    overflow: hidden;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    padding: 4px 6px 2px;
    color: var(--qx-text-muted);
    font-size: 10px;
  }
</style>
