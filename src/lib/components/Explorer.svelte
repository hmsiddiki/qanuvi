<script lang="ts">
  import { contextMenu } from "$lib/stores/contextMenu.svelte";
  import * as files from "$lib/fileActions";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import File from "@lucide/svelte/icons/file";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { api, errorText, type Entry, type Project } from "$lib/api";
  import { editor, tabKey } from "$lib/stores/editor.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { decorations, type Decorations } from "$lib/git/decorations";
  import { gitState } from "$lib/stores/git.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
    /** In the project panel: the panel's section header has the buttons. */
    headless?: boolean;
  }

  let { project, headless = false }: Props = $props();

  const children = new SvelteMap<string, Entry[]>();
  const expanded = new SvelteSet<string>();
  const loading = new SvelteSet<string>();
  let error = $state("");
  let deco = $state<Decorations>({ files: new Map(), folders: new Set() });

  /** Git badges for the tree. Cheap (one git status), so it is refreshed often. */
  async function loadGit() {
    try {
      const status = await api.git.status(project.id);
      deco = decorations(status.is_repo ? status.changes : []);
    } catch {
      deco = { files: new Map(), folders: new Set() };
    }
  }

  // After an agent turn, a git action, on saves, and every 15 seconds while the window is shown (terminal commands change files too).
  $effect(() => {
    void agent.turnsEnded;
    void gitState.changed;
    void editor.tabs.map((t) => t.hash).join();
    loadGit();
  });
  $effect(() => {
    const timer = setInterval(() => document.visibilityState === "visible" && loadGit(), 15000);
    return () => clearInterval(timer);
  });

  async function load(path: string) {
    loading.add(path);
    try {
      children.set(path, await api.files.listDir(project.id, path));
      error = "";
    } catch (e) {
      error = errorText(e);
    } finally {
      loading.delete(path);
    }
  }

  async function toggle(entry: Entry) {
    if (expanded.has(entry.path)) {
      expanded.delete(entry.path);
      return;
    }
    expanded.add(entry.path);
    if (!children.has(entry.path)) await load(entry.path);
  }

  export async function refresh() {
    const open = ["", ...expanded];
    await Promise.all([...open.map((p) => load(p)), loadGit()]);
  }

  /** Right-click on a file or folder. */
  function rowMenu(e: MouseEvent, entry: Entry, letter: string | undefined) {
    const p = project;
    const rel = entry.path;
    const dir = entry.is_dir ? rel : files.parentOf(rel);
    const after = () => refreshAround(rel);
    contextMenu.show(e, [
      entry.is_dir
        ? [
            { label: "New file…", action: () => files.newItem(p, dir, false, after) },
            { label: "New folder…", action: () => files.newItem(p, dir, true, after) },
          ]
        : [
            { label: "Open", action: () => editor.open(p.id, rel) },
            { label: "Add to the message", action: () => files.addToMessage(rel) },
          ],
      [{ label: "Open in terminal", action: () => files.openTerminal(p, dir) }],
      [
        { label: "Rename…", action: () => files.rename(p, rel, after) },
        { label: "Delete", danger: true, action: () => files.trash(p, rel, entry.is_dir, after) },
      ],
      [
        { label: "Copy path", action: () => files.copyPath(p, rel) },
        { label: "Copy relative path", action: () => files.copyRelativePath(rel) },
        { label: "Reveal in File Explorer", action: () => files.reveal(p, rel) },
      ],
      !entry.is_dir &&
        !!letter && [
          { label: "Git: stage", action: () => files.stage(p, rel) },
          { label: "Git: discard changes", danger: true, action: () => files.discard(p, rel) },
        ],
    ]);
  }

  /** Right-click on the tree's empty space: the project's top. */
  function rootMenu(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('[role="treeitem"]')) return;
    const p = project;
    contextMenu.show(e, [
      [
        { label: "New file…", action: () => files.newItem(p, "", false, () => refresh()) },
        { label: "New folder…", action: () => files.newItem(p, "", true, () => refresh()) },
      ],
      [{ label: "Open in terminal", action: () => files.openTerminal(p, "") }],
      [
        { label: "Refresh", action: () => refresh() },
        { label: "Collapse folders", action: () => collapseAll() },
        { label: "Reveal in File Explorer", action: () => files.reveal(p, "") },
      ],
    ]);
  }

  /** After a change: the folder it was in (and the top) load again. */
  async function refreshAround(rel: string) {
    await Promise.all([load(files.parentOf(rel)), load(""), loadGit()]);
    if (expanded.has(rel)) await load(rel);
  }

  export function collapseAll() {
    expanded.clear();
  }

  $effect(() => {
    void project.id;
    children.clear();
    expanded.clear();
    load("");
  });

  interface Row {
    entry: Entry;
    depth: number;
    /** Ignored by git itself or inside an ignored folder (greyed, as in VS Code). */
    ignored: boolean;
  }

  function rows(path: string, depth: number, out: Row[], inIgnored = false): Row[] {
    for (const entry of children.get(path) ?? []) {
      const ignored = inIgnored || entry.ignored;
      out.push({ entry, depth, ignored });
      if (entry.is_dir && expanded.has(entry.path)) rows(entry.path, depth + 1, out, ignored);
    }
    return out;
  }

  let flat = $derived(rows("", 0, []));
  let activeKey = $derived(editor.activeKey);
</script>

<div class="explorer">
  {#if !headless}
    <header>
      <span class="title" title={project.path}>{project.name}</span>
      <IconButton label="Refresh" onclick={refresh}><RefreshCw size={13} /></IconButton>
      <IconButton label="Collapse folders" onclick={collapseAll}><ChevronsDownUp size={13} /></IconButton>
    </header>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
  <div class="tree" role="tree" aria-label="Files" tabindex="-1" oncontextmenu={rootMenu}>
    {#each flat as { entry, depth, ignored } (entry.path)}
      {@const open = expanded.has(entry.path)}
      {@const letter = deco.files.get(entry.path)}
      <button
        type="button"
        role="treeitem"
        aria-expanded={entry.is_dir ? open : undefined}
        aria-selected={activeKey === tabKey(project.id, entry.path)}
        class="row"
        class:ignored
        class:active={activeKey === tabKey(project.id, entry.path)}
        style:padding-left="{8 + depth * 14}px"
        title={entry.path}
        onclick={() => (entry.is_dir ? toggle(entry) : editor.open(project.id, entry.path))}
        oncontextmenu={(e) => rowMenu(e, entry, letter)}
      >
        {#if entry.is_dir}
          <span class="chev"
            >{#if open}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}</span
          >
          <span class="icon folder"
            >{#if open}<FolderOpen size={14} />{:else}<Folder size={14} />{/if}</span
          >
        {:else}
          <span class="chev"></span>
          <span class="icon"><File size={14} /></span>
        {/if}
        <span class="name {letter ? `g${letter === '!' ? 'C' : letter}` : deco.folders.has(entry.path) ? 'gdir' : ''}"
          >{entry.name}</span
        >
        {#if loading.has(entry.path)}<span class="busy">…</span>{/if}
        {#if letter}<span class="badge g{letter === '!' ? 'C' : letter}">{letter}</span>
        {:else if entry.is_dir && deco.folders.has(entry.path)}<span class="dot"></span>{/if}
      </button>
    {/each}
    {#if !error && children.has("") && flat.length === 0}
      <p class="empty">This folder is empty.</p>
    {/if}
  </div>
</div>

<style>
  .explorer {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 34px;
    padding: 0 var(--qx-space-1) 0 var(--qx-space-3);
  }

  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .tree {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding-bottom: var(--qx-space-2);
  }

  .row {
    width: 100%;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: var(--qx-space-2);
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
  }

  .row:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .row.active {
    background: color-mix(in srgb, var(--qx-accent) 18%, transparent);
    color: var(--qx-text);
  }

  .chev,
  .icon {
    flex: none;
    width: 14px;
    display: inline-grid;
    place-items: center;
    color: var(--qx-text-muted);
  }

  .icon.folder {
    color: #c8a45a;
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.gM,
  .badge.gM {
    color: #e2c08d;
  }

  .name.gU,
  .badge.gU,
  .name.gA,
  .badge.gA {
    color: #73c991;
  }

  .name.gD,
  .badge.gD {
    color: #e5604b;
  }

  .name.gR,
  .badge.gR {
    color: #7fb4ff;
  }

  .name.gC,
  .badge.gC {
    color: #e5604b;
  }

  .name.gdir {
    color: #e2c08d;
  }

  .badge {
    margin-left: auto;
    padding-left: var(--qx-space-2);
    font-family: var(--qx-font-mono);
    font-size: 11px;
    font-weight: 600;
  }

  .dot {
    flex: none;
    width: 6px;
    height: 6px;
    margin-left: auto;
    border-radius: 50%;
    background: #e2c08d;
    opacity: 0.8;
  }

  .busy {
    color: var(--qx-text-muted);
  }

  .error,
  .empty {
    margin: 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    font-size: var(--qx-text-small);
    color: var(--qx-text-muted);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .row.ignored .name,
  .row.ignored .icon {
    opacity: 0.5;
  }
</style>
