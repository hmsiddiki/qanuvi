<script lang="ts">
  /**
   * The bottom panel's Problems and References lists (15i): what the language servers report for the project, and
   * the last "find references" (or several definitions). A click opens the place; "Fix with agent" (on a problem,
   * a file, or its right-click menu) puts a request in the conversation's message box for the user to send.
   */
  import CircleX from "@lucide/svelte/icons/circle-x";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Info from "@lucide/svelte/icons/info";
  import { editor } from "$lib/stores/editor.svelte";
  import { lsp, type Place, type Problem } from "$lib/stores/lsp.svelte";
  import { fixWithAgent } from "$lib/lsp/actions";
  import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";

  interface Props {
    projectId: string | null;
    view: "problems" | "references";
  }

  let { projectId, view }: Props = $props();

  let filter = $state("");
  let all = $derived(lsp.projectProblems(projectId));
  let shown = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    return f ? all.filter((p) => `${p.path} ${p.message} ${p.source}`.toLowerCase().includes(f)) : all;
  });
  /** Grouped by file, in the sorted order (errors first). */
  let groups = $derived.by(() => {
    const map = new Map<string, Problem[]>();
    for (const p of shown) map.set(p.path, [...(map.get(p.path) ?? []), p]);
    return [...map.entries()];
  });
  let refGroups = $derived.by(() => {
    const map = new Map<string, Place[]>();
    for (const p of lsp.references?.places ?? []) map.set(p.path, [...(map.get(p.path) ?? []), p]);
    return [...map.entries()];
  });
  let running = $derived(lsp.running(projectId));

  function openProblem(p: Problem) {
    editor.open(p.projectId, p.path, p.range.start.line + 1, p.range.start.character + 1);
  }

  function problemMenu(e: MouseEvent, p: Problem, list: Problem[]) {
    contextMenu.show(e, [
      [
        { label: "Open", action: () => openProblem(p) },
        { label: "Fix with agent", action: () => fixWithAgent([p]) },
        list.length > 1 && {
          label: `Fix all ${list.length} in this file with agent`,
          action: () => fixWithAgent(list),
        },
      ],
      [
        { label: "Copy the message", action: () => copyText(p.message) },
        {
          label: "Copy with its place",
          action: () => copyText(`${p.path}:${p.range.start.line + 1}:${p.range.start.character + 1} ${p.message}`),
        },
      ],
    ]);
  }

  function openPlace(p: Place) {
    editor.open(p.projectId, p.path, p.line, p.column);
  }
</script>

<div class="problems">
  {#if view === "problems"}
    <div class="bar">
      <input type="search" placeholder="Filter (file, message)" bind:value={filter} />
      <span class="servers">
        {#each running as r (r.server)}
          <span class="server" title="Language server">{r.label}: {r.state}</span>
        {:else}
          <span class="muted">No language server running. One starts when a file of its language opens.</span>
        {/each}
      </span>
    </div>
    {#if lsp.lastError}<p class="error">{lsp.lastError}</p>{/if}
    {#if groups.length === 0}
      <p class="muted empty">{all.length ? "Nothing matches the filter." : "No problems found."}</p>
    {:else}
      <ul>
        {#each groups as [path, list] (path)}
          <li class="file">
            {path} <span class="muted">{list.length}</span>
            <button
              type="button"
              class="fix"
              title="Put a request to fix them in the message box"
              onclick={() => fixWithAgent(list)}>Fix all with agent</button
            >
          </li>
          {#each list as p, i (i)}
            <li class="item" oncontextmenu={(e) => problemMenu(e, p, list)}>
              <button type="button" class="row" onclick={() => openProblem(p)}>
                {#if p.severity === "error"}<CircleX size={13} class="sev error" />
                {:else if p.severity === "warning"}<TriangleAlert size={13} class="sev warning" />
                {:else}<Info size={13} class="sev info" />{/if}
                <span class="msg">{p.message}</span>
                {#if p.source}<span class="muted">{p.source}</span>{/if}
                <span class="muted pos">[{p.range.start.line + 1}, {p.range.start.character + 1}]</span>
              </button>
              <button
                type="button"
                class="fix"
                title="Put a request to fix it in the message box"
                onclick={() => fixWithAgent([p])}>Fix with agent</button
              >
            </li>
          {/each}
        {/each}
      </ul>
    {/if}
  {:else if lsp.references}
    <div class="bar">
      <strong>{lsp.references.title}</strong> <span class="muted">{lsp.references.places.length}</span>
    </div>
    <ul>
      {#each refGroups as [path, list] (path)}
        <li class="file">{path} <span class="muted">{list.length}</span></li>
        {#each list as p, i (i)}
          <li>
            <button type="button" class="row" onclick={() => openPlace(p)}>
              <span class="muted pos">{p.line}</span>
              <span class="msg mono">{p.text ?? ""}</span>
            </button>
          </li>
        {/each}
      {/each}
    </ul>
  {:else}
    <p class="muted empty">Find references (Shift+F12 in a file) lists them here.</p>
  {/if}
</div>

<style>
  .problems {
    height: 100%;
    overflow: auto;
    font-size: var(--qx-text-small);
  }

  .bar {
    position: sticky;
    top: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    background: var(--qx-bg-panel);
    border-bottom: 1px solid var(--qx-border);
  }

  .bar input {
    width: 220px;
    padding: 3px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: inherit;
    font: inherit;
  }

  .servers {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .server {
    color: var(--qx-text-secondary);
  }

  ul {
    margin: 0;
    padding: 4px 0;
    list-style: none;
  }

  .file {
    padding: 4px 10px 2px;
    font-weight: 600;
  }

  .file,
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .item .row {
    flex: 1;
    min-width: 0;
  }

  .fix {
    flex: none;
    margin-right: 10px;
    padding: 0 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    cursor: pointer;
    visibility: hidden;
  }

  .file .fix {
    margin-left: auto;
    font-weight: normal;
  }

  .file:hover .fix,
  .item:hover .fix,
  .fix:focus-visible {
    visibility: visible;
  }

  .fix:hover {
    color: inherit;
    border-color: var(--qx-accent);
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 2px 10px 2px 24px;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row:hover {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.12));
  }

  .row :global(.sev) {
    flex: none;
    align-self: center;
  }

  .row :global(.sev.error) {
    color: var(--qx-danger, #e5534b);
  }

  .row :global(.sev.warning) {
    color: var(--qx-warning, #d29922);
  }

  .row :global(.sev.info) {
    color: var(--qx-accent);
  }

  .msg {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mono {
    font-family: var(--qx-font-mono, monospace);
  }

  .pos {
    flex: none;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .empty {
    padding: 10px;
    margin: 0;
  }

  .error {
    margin: 6px 10px;
    color: var(--qx-danger, #e5534b);
    white-space: pre-wrap;
  }
</style>
