<script lang="ts">
  import CaseSensitive from "@lucide/svelte/icons/case-sensitive";
  import Regex from "@lucide/svelte/icons/regex";
  import { api, errorText, type Project, type SearchMatch } from "$lib/api";
  import { editor } from "$lib/stores/editor.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
  }

  let { project }: Props = $props();

  let query = $state("");
  let regex = $state(false);
  let caseSensitive = $state(false);
  let busy = $state(false);
  let error = $state("");
  let summary = $state("");
  let groups = $state<{ path: string; matches: SearchMatch[] }[]>([]);
  let input: HTMLInputElement;
  let run = 0;

  export function focus() {
    input?.focus();
    input?.select();
  }

  async function search(event?: SubmitEvent) {
    event?.preventDefault();
    const mine = ++run;
    if (!query) {
      groups = [];
      summary = "";
      return;
    }
    busy = true;
    error = "";
    try {
      const result = await api.files.search(project.id, query, regex, caseSensitive);
      if (mine !== run) return;
      const byPath = new Map<string, SearchMatch[]>();
      for (const m of result.matches) {
        const list = byPath.get(m.path) ?? [];
        list.push(m);
        byPath.set(m.path, list);
      }
      groups = [...byPath].map(([path, matches]) => ({ path, matches }));
      summary = `${result.matches.length}${result.truncated ? "+" : ""} results in ${groups.length} files (${result.files_searched} searched)`;
    } catch (e) {
      if (mine === run) error = errorText(e);
    } finally {
      if (mine === run) busy = false;
    }
  }

  $effect(() => {
    void project.id;
    groups = [];
    summary = "";
  });
</script>

<div class="search">
  <form onsubmit={search}>
    <input bind:this={input} bind:value={query} placeholder="Search in project (Enter)" spellcheck="false" />
    <IconButton label="Match case" active={caseSensitive} onclick={() => ((caseSensitive = !caseSensitive), search())}>
      <CaseSensitive size={15} />
    </IconButton>
    <IconButton label="Regular expression" active={regex} onclick={() => ((regex = !regex), search())}>
      <Regex size={15} />
    </IconButton>
  </form>
  <p class="summary">
    {#if busy}Searching…{:else if error}<span class="error">{error}</span>{:else}{summary}{/if}
  </p>
  <div class="results">
    {#each groups as group (group.path)}
      <div class="file">{group.path}</div>
      {#each group.matches as m (m.line + ":" + m.column)}
        <button type="button" class="match" onclick={() => editor.open(project.id, m.path, m.line)}>
          <span class="line">{m.line}</span>
          <span class="text">{m.text.trim()}</span>
        </button>
      {/each}
    {/each}
  </div>
</div>

<style>
  .search {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  form {
    display: flex;
    gap: 2px;
    align-items: center;
    padding: var(--qx-space-2);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    outline: none;
    user-select: text;
  }

  input:focus {
    border-color: var(--qx-accent);
  }

  .summary {
    margin: 0;
    padding: 0 var(--qx-space-3) var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--qx-space-2) var(--qx-space-2);
  }

  .file {
    margin-top: var(--qx-space-2);
    padding: 2px var(--qx-space-1);
    color: var(--qx-text);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .match {
    width: 100%;
    display: flex;
    gap: var(--qx-space-2);
    padding: 2px var(--qx-space-1) 2px var(--qx-space-3);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .match:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .line {
    flex: none;
    min-width: 32px;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 12px;
    text-align: right;
  }

  .text {
    overflow: hidden;
    font-family: var(--qx-font-mono);
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
