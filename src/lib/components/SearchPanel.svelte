<script lang="ts">
  /**
   * Search across the project (15k), as in VS Code: results while typing, grouped by file with every match marked;
   * match case, whole word and regular expression; files to include and leave out; replace in one file or all.
   * Ignored files are skipped, except nested repositories' own files (files.rs).
   */
  import CaseSensitive from "@lucide/svelte/icons/case-sensitive";
  import Regex from "@lucide/svelte/icons/regex";
  import WholeWord from "@lucide/svelte/icons/whole-word";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import ReplaceAll from "@lucide/svelte/icons/replace-all";
  import Replace from "@lucide/svelte/icons/replace";
  import ListCollapse from "@lucide/svelte/icons/list-collapse";
  import X from "@lucide/svelte/icons/x";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { api, errorText, type Project, type SearchMatch, type SearchOptions } from "$lib/api";
  import { editor } from "$lib/stores/editor.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
  }

  let { project }: Props = $props();

  const KEY = "qanuvi.search.v1";
  const kept = (() => {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? "{}") as Partial<SearchOptions & { more: boolean }>;
    } catch {
      return {};
    }
  })();

  let query = $state("");
  let replacement = $state("");
  let showReplace = $state(false);
  let more = $state(!!kept.more);
  let regex = $state(!!kept.regex);
  let caseSensitive = $state(!!kept.caseSensitive);
  let wholeWord = $state(!!kept.wholeWord);
  let include = $state(kept.include ?? "");
  let exclude = $state(kept.exclude ?? "");
  let busy = $state(false);
  let error = $state("");
  let summary = $state("");
  let note = $state("");
  let groups = $state<{ path: string; matches: SearchMatch[]; count: number }[]>([]);
  let closed = $state<Set<string>>(new Set());
  let input: HTMLInputElement;
  let run = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const options = (): SearchOptions => ({ regex, caseSensitive, wholeWord, include, exclude });

  export function focus() {
    input?.focus();
    input?.select();
  }

  /** Searches again a moment after the last key (results while typing). */
  function soon() {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => search(), 250);
  }

  async function search(event?: SubmitEvent) {
    event?.preventDefault();
    if (timer) clearTimeout(timer);
    localStorage.setItem(KEY, JSON.stringify({ ...options(), more }));
    const mine = ++run;
    note = "";
    if (!query) {
      groups = [];
      summary = "";
      error = "";
      busy = false;
      return;
    }
    busy = true;
    error = "";
    try {
      const result = await api.files.find(project.id, query, options());
      if (mine !== run) return;
      const byPath = new Map<string, SearchMatch[]>();
      for (const m of result.matches) {
        const list = byPath.get(m.path) ?? [];
        list.push(m);
        byPath.set(m.path, list);
      }
      groups = [...byPath].map(([path, matches]) => ({
        path,
        matches,
        count: matches.reduce((t, m) => t + (m.count || 1), 0),
      }));
      const total = groups.reduce((t, g) => t + g.count, 0);
      summary = result.matches.length
        ? `${total}${result.truncated ? "+" : ""} result${total === 1 ? "" : "s"} in ${groups.length} file${groups.length === 1 ? "" : "s"}`
        : `No results (${result.files_searched} files searched)`;
    } catch (e) {
      if (mine === run) {
        error = errorText(e);
        groups = [];
        summary = "";
      }
    } finally {
      if (mine === run) busy = false;
    }
  }

  /** A line split into plain and matched parts, for marking the matches. */
  function parts(m: SearchMatch): { text: string; hit: boolean }[] {
    const chars = [...m.text];
    const out: { text: string; hit: boolean }[] = [];
    let at = 0;
    for (const [a, b] of m.spans ?? []) {
      if (a > at) out.push({ text: chars.slice(at, a).join(""), hit: false });
      out.push({ text: chars.slice(a, b).join(""), hit: true });
      at = b;
    }
    if (at < chars.length) out.push({ text: chars.slice(at).join(""), hit: false });
    // Long leading space only takes room.
    if (out[0] && !out[0].hit) out[0].text = out[0].text.replace(/^\s+/, "");
    return out;
  }

  function nameOf(path: string) {
    return path.slice(path.lastIndexOf("/") + 1);
  }
  function dirOf(path: string) {
    const i = path.lastIndexOf("/");
    return i > 0 ? path.slice(0, i) : "";
  }

  function toggle(path: string) {
    const next = new Set(closed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    closed = next;
  }

  /** Files open in the editor are read again after a replace, so they show the new text. */
  async function refreshOpen(paths: string[]) {
    for (const t of editor.tabs)
      if (t.projectId === project.id && paths.includes(t.path) && !t.dirty) await editor.reload(t.key);
  }

  async function replaceIn(paths: string[] | null) {
    if (!query) return;
    const files = paths ?? groups.map((g) => g.path);
    const count = groups.filter((g) => files.includes(g.path)).reduce((t, g) => t + g.count, 0);
    const what = `${count} match${count === 1 ? "" : "es"} in ${files.length} file${files.length === 1 ? "" : "s"}`;
    if (
      paths === null &&
      !(await ask(`Replace ${what} with "${replacement}"? Files open with unsaved changes are not reloaded.`, {
        title: "Replace all",
        kind: "warning",
        okLabel: "Replace",
        cancelLabel: "Cancel",
      }))
    )
      return;
    busy = true;
    try {
      const done = await api.files.replace(project.id, query, options(), replacement, files);
      await refreshOpen(files);
      await search();
      note = `Replaced ${done.replacements} in ${done.files} file${done.files === 1 ? "" : "s"}.`;
    } catch (e) {
      error = errorText(e);
      busy = false;
    }
  }

  function clear() {
    query = "";
    replacement = "";
    search();
    input?.focus();
  }

  $effect(() => {
    void project.id;
    groups = [];
    summary = "";
    if (query) soon();
  });
</script>

<div class="search">
  <div class="fields">
    <button
      type="button"
      class="twist"
      title={showReplace ? "Hide replace" : "Replace"}
      aria-label={showReplace ? "Hide replace" : "Replace"}
      onclick={() => (showReplace = !showReplace)}
    >
      {#if showReplace}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
    </button>
    <div class="stack">
      <form class="row" onsubmit={search}>
        <input
          bind:this={input}
          bind:value={query}
          oninput={soon}
          placeholder="Search"
          spellcheck="false"
          aria-label="Search in the project"
        />
        <IconButton
          label="Match case"
          active={caseSensitive}
          onclick={() => ((caseSensitive = !caseSensitive), search())}
        >
          <CaseSensitive size={15} />
        </IconButton>
        <IconButton label="Match whole word" active={wholeWord} onclick={() => ((wholeWord = !wholeWord), search())}>
          <WholeWord size={15} />
        </IconButton>
        <IconButton label="Use regular expression" active={regex} onclick={() => ((regex = !regex), search())}>
          <Regex size={15} />
        </IconButton>
      </form>
      {#if showReplace}
        <div class="row">
          <input
            bind:value={replacement}
            placeholder={regex ? "Replace ($1 for a group)" : "Replace"}
            spellcheck="false"
            aria-label="Replace with"
            onkeydown={(e) => e.key === "Enter" && e.ctrlKey && e.altKey && replaceIn(null)}
          />
          <IconButton
            label="Replace all (Ctrl+Alt+Enter)"
            disabled={!query || !groups.length || busy}
            onclick={() => replaceIn(null)}
          >
            <ReplaceAll size={15} />
          </IconButton>
        </div>
      {/if}
    </div>
  </div>
  <div class="more-row">
    <IconButton label="Files to include and leave out" active={more} onclick={() => (more = !more)}>
      <Ellipsis size={14} />
    </IconButton>
  </div>
  {#if more}
    <div class="filters">
      <label>
        Files to include
        <input bind:value={include} oninput={soon} placeholder="for example src/**, *.php" spellcheck="false" />
      </label>
      <label>
        Files to leave out
        <input bind:value={exclude} oninput={soon} placeholder="for example vendor, *.min.js" spellcheck="false" />
      </label>
    </div>
  {/if}
  <div class="summary">
    <span class="what">
      {#if busy}Searching…{:else if error}<span class="error">{error}</span>{:else}{summary}{#if note}
          · {note}{/if}{/if}
    </span>
    {#if groups.length}
      <IconButton
        label={closed.size ? "Expand all" : "Collapse all"}
        onclick={() => (closed = closed.size ? new Set() : new Set(groups.map((g) => g.path)))}
      >
        <ListCollapse size={14} />
      </IconButton>
    {/if}
    {#if query}
      <IconButton label="Clear" onclick={clear}><X size={14} /></IconButton>
    {/if}
  </div>
  <div class="results">
    {#each groups as group (group.path)}
      <div class="file" title={group.path}>
        <button type="button" class="file-head" onclick={() => toggle(group.path)}>
          {#if closed.has(group.path)}<ChevronRight size={13} />{:else}<ChevronDown size={13} />{/if}
          <span class="name">{nameOf(group.path)}</span>
          <span class="dir">{dirOf(group.path)}</span>
          <span class="badge">{group.count}</span>
        </button>
        {#if showReplace && query}
          <button
            type="button"
            class="file-replace"
            title="Replace in this file"
            aria-label="Replace in {group.path}"
            onclick={() => replaceIn([group.path])}><Replace size={13} /></button
          >
        {/if}
      </div>
      {#if !closed.has(group.path)}
        {#each group.matches as m (m.line)}
          <button
            type="button"
            class="match"
            title="{group.path}:{m.line}"
            onclick={() => editor.open(project.id, m.path, m.line, m.column)}
          >
            <span class="text"
              >{#each parts(m) as p, i (i)}{#if p.hit}{#if showReplace && replacement && !regex}<del>{p.text}</del><ins
                      >{replacement}</ins
                    >{:else}<mark>{p.text}</mark>{/if}{:else}{p.text}{/if}{/each}</span
            >
            <span class="line">{m.line}</span>
          </button>
        {/each}
      {/if}
    {/each}
  </div>
</div>

<style>
  .search {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .fields {
    display: flex;
    gap: 2px;
    padding: var(--qx-space-2) var(--qx-space-2) 0 2px;
  }

  .twist {
    flex: none;
    width: 18px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-text-muted);
    cursor: pointer;
    align-self: stretch;
    border-radius: 4px;
  }

  .twist:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .stack {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .row {
    display: flex;
    gap: 2px;
    align-items: center;
  }

  input {
    flex: 1;
    min-width: 0;
    height: 28px;
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

  .more-row {
    display: flex;
    justify-content: flex-end;
    padding: 0 var(--qx-space-2);
  }

  .filters {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 0 var(--qx-space-2) var(--qx-space-2) 20px;
  }

  .filters label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .summary {
    display: flex;
    align-items: center;
    gap: 2px;
    min-height: 24px;
    padding: 0 var(--qx-space-2) 0 var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .what {
    flex: 1;
    min-width: 0;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--qx-space-1) var(--qx-space-2);
  }

  .file {
    display: flex;
    align-items: center;
    border-radius: 4px;
  }

  .file:hover {
    background: var(--qx-bg-raised);
  }

  .file-head {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 4px;
    border: 0;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .name {
    flex: none;
    font-weight: 500;
  }

  .dir {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--qx-text-muted);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .badge {
    flex: none;
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--qx-bg-raised);
    color: var(--qx-text-secondary);
    font-size: 10.5px;
    text-align: center;
  }

  .file-replace {
    flex: none;
    width: 22px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--qx-text-muted);
    cursor: pointer;
    opacity: 0;
  }

  .file:hover .file-replace,
  .file-replace:focus-visible {
    opacity: 1;
  }

  .file-replace:hover {
    color: var(--qx-text);
  }

  .match {
    width: 100%;
    display: flex;
    gap: var(--qx-space-2);
    padding: 1px var(--qx-space-1) 1px 24px;
    border: 0;
    border-radius: 4px;
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

  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-family: var(--qx-font-mono);
    font-size: 12px;
    white-space: pre;
    text-overflow: ellipsis;
  }

  mark {
    border-radius: 2px;
    background: color-mix(in srgb, #e2a33a 38%, transparent);
    color: var(--qx-text);
  }

  del {
    background: color-mix(in srgb, #e5534b 30%, transparent);
    color: var(--qx-text);
  }

  ins {
    background: color-mix(in srgb, #3fb950 30%, transparent);
    color: var(--qx-text);
    text-decoration: none;
  }

  .line {
    flex: none;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 11px;
  }
</style>
