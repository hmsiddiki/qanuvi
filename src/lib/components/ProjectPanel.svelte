<script lang="ts">
  /**
   * The project panel on the work area's left edge (15h): Explorer, Search and Git. The Explorer's sections (open
   * editors, files, outline, timeline, files the session touched) open and close, are dragged into another order,
   * resized and hidden; the panel remembers how it was left.
   */
  import Files from "@lucide/svelte/icons/files";
  import Search from "@lucide/svelte/icons/search";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import X from "@lucide/svelte/icons/x";
  import Bot from "@lucide/svelte/icons/bot";
  import { api, errorText, type FileTouch, type GitCommit, type Project } from "$lib/api";
  import { panel, SECTION_LABELS, type SectionId, type SectionState } from "$lib/stores/panel.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { scmView } from "$lib/stores/scmView.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { ago } from "$lib/stores/git.svelte";
  import Explorer from "./Explorer.svelte";
  import SearchPanel from "./SearchPanel.svelte";
  import SourceControl from "./SourceControl.svelte";

  /** views: false inside the sidebar, whose mode switch chooses Explorer, Search or Git. */
  let { project, views = true }: { project: Project; views?: boolean } = $props();

  let explorer: Explorer | undefined = $state();
  let search: SearchPanel | undefined = $state();

  // Search asked for (Ctrl+Shift+F): the Search view takes focus.
  $effect(() => {
    void panel.focusSearch;
    if (panel.focusSearch && panel.view === "search") queueMicrotask(() => search?.focus());
  });

  // ---------------- The file being worked on (for Outline and Timeline) ----------------
  let current = $derived(
    editor.active && !editor.active.global && editor.active.projectId === project.id ? editor.active.path : null,
  );

  // ---------------- Files: new file / new folder ----------------
  let creating = $state<"" | "file" | "folder">("");
  let newName = $state("");
  let createError = $state("");

  async function create(event: SubmitEvent) {
    event.preventDefault();
    const name = newName.trim();
    if (!name || !creating) return;
    try {
      await api.files.create(project.id, name, creating === "folder");
      const kind = creating;
      creating = "";
      newName = "";
      createError = "";
      await explorer?.refresh();
      if (kind === "file") await editor.open(project.id, name.replace(/^\/+/, ""));
    } catch (e) {
      createError = errorText(e);
    }
  }

  // ---------------- Outline ----------------
  let outline = $state<{ name: string; kind: string; line: number; container: string | null }[]>([]);
  let outlineNote = $state("");

  async function loadOutline() {
    const path = current;
    if (!path) {
      outline = [];
      outlineNote = "Open a file to see its outline.";
      return;
    }
    try {
      const list = await api.index.outline(project.id, path);
      if (path !== current) return;
      outline = list;
      outlineNote = list.length ? "" : "No symbols found in this file.";
    } catch (e) {
      outline = [];
      outlineNote = errorText(e);
    }
  }

  // ---------------- Timeline ----------------
  let touches = $state<FileTouch[]>([]);
  let commits = $state<GitCommit[]>([]);
  let timelineNote = $state("");

  async function loadTimeline() {
    const path = current;
    if (!path) {
      touches = [];
      commits = [];
      timelineNote = "Open a file to see its history.";
      return;
    }
    const [t, c] = await Promise.all([
      api.agent.fileHistory(project.id, path).catch(() => [] as FileTouch[]),
      api.git.log(project.id, path, 30).catch(() => [] as GitCommit[]),
    ]);
    if (path !== current) return;
    touches = t;
    commits = c;
    timelineNote = t.length || c.length ? "" : "No commits or agent changes for this file yet.";
  }

  const isOpen = (id: SectionId) => panel.sections.some((s) => s.id === id && s.open && !s.hidden);

  // Outline and Timeline follow the file being worked on (and an agent turn that may have changed it).
  $effect(() => {
    void current;
    void agent.turnsEnded;
    if (panel.view !== "explorer") return;
    if (isOpen("outline")) loadOutline();
    if (isOpen("timeline")) loadTimeline();
  });

  function openSession(id: string) {
    if (!layout.state.agentVisible) layout.toggleAgent();
    agent.select(id);
  }

  function showCommits() {
    if (!current) return;
    scmView.selected = null;
    scmView.historyFor = current;
    ui.show("diff");
  }

  // ---------------- Touched in this session ----------------
  let touched = $derived.by(() => {
    const out = new Map<string, boolean>();
    const root = project.path.replace(/\\/g, "/").replace(/\/$/, "");
    for (const item of agent.items) {
      if (item.kind !== "tool" || !item.file) continue;
      let p = item.file.replace(/\\/g, "/");
      if (p.toLowerCase().startsWith(root.toLowerCase() + "/")) p = p.slice(root.length + 1);
      const changed = !/^(read|grep|glob|search|ls|list|outline|find)/i.test(item.name.replace(/^mcp__[^_]+__/, ""));
      out.set(p, (out.get(p) ?? false) || changed);
    }
    return [...out].map(([path, changed]) => ({ path, changed }));
  });

  // ---------------- Sections: open, move, resize, hide ----------------
  let dragging = $state<SectionId | null>(null);

  function startResize(e: PointerEvent, s: SectionState) {
    e.preventDefault();
    const startY = e.clientY;
    const startH = s.height;
    const move = (m: PointerEvent) =>
      panel.update(s.id, { height: Math.min(800, Math.max(60, startH + m.clientY - startY)) });
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function toggle(s: SectionState) {
    panel.update(s.id, { open: !s.open });
  }

  function fileName(path: string) {
    return path.split("/").pop() ?? path;
  }
</script>

<div class="panel">
  <div class="bar" class:slim={!views} class:gone={!views && panel.view !== "explorer"}>
    {#if views}
      <button
        type="button"
        class:on={panel.view === "explorer"}
        title="Explorer"
        aria-label="Explorer"
        onclick={() => panel.show("explorer")}><Files size={15} /></button
      >
      <button
        type="button"
        class:on={panel.view === "search"}
        title="Search in the project"
        aria-label="Search"
        onclick={() => panel.show("search")}><Search size={15} /></button
      >
      <button
        type="button"
        class:on={panel.view === "git"}
        title="Git"
        aria-label="Git"
        onclick={() => panel.show("git")}><GitBranch size={15} /></button
      >
    {/if}
    <span class="spacer"></span>
    {#if panel.view === "explorer"}
      <div class="more">
        <button type="button" title="Sections" aria-label="Sections" onclick={() => (panel.menuOpen = !panel.menuOpen)}
          ><Ellipsis size={15} /></button
        >
        {#if panel.menuOpen}
          <div class="menu" role="menu">
            {#each panel.sections as s (s.id)}
              <label>
                <input
                  type="checkbox"
                  checked={!s.hidden}
                  disabled={s.id === "files"}
                  onchange={(e) => panel.update(s.id, { hidden: !e.currentTarget.checked })}
                />
                {SECTION_LABELS[s.id]}
              </label>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  {#if panel.view === "explorer"}
    <div class="sections">
      {#each panel.sections.filter((s) => !s.hidden) as s (s.id)}
        <section
          role="group"
          aria-label={SECTION_LABELS[s.id]}
          class="section"
          class:grow={s.id === "files" && s.open}
          class:drop={dragging && dragging !== s.id}
          ondragover={(e) => dragging && e.preventDefault()}
          ondrop={(e) => {
            e.preventDefault();
            if (dragging) panel.move(dragging, s.id);
            dragging = null;
          }}
        >
          <header
            role="group"
            aria-label="Drag to move this section"
            draggable="true"
            ondragstart={(e) => {
              dragging = s.id;
              e.dataTransfer?.setData("text/plain", s.id);
            }}
            ondragend={() => (dragging = null)}
          >
            <button type="button" class="head" aria-expanded={s.open} onclick={() => toggle(s)}>
              {#if s.open}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
              <span>{s.id === "files" ? project.name : SECTION_LABELS[s.id]}</span>
              {#if s.id === "timeline" && current}<span class="sub">{fileName(current)}</span>{/if}
            </button>
            <span class="actions">
              {#if s.id === "files"}
                <button
                  type="button"
                  title="New file"
                  aria-label="New file"
                  onclick={() => ((creating = "file"), panel.update("files", { open: true }))}
                  ><FilePlus size={13} /></button
                >
                <button
                  type="button"
                  title="New folder"
                  aria-label="New folder"
                  onclick={() => ((creating = "folder"), panel.update("files", { open: true }))}
                  ><FolderPlus size={13} /></button
                >
                <button type="button" title="Refresh" aria-label="Refresh" onclick={() => explorer?.refresh()}
                  ><RefreshCw size={13} /></button
                >
                <button
                  type="button"
                  title="Collapse folders"
                  aria-label="Collapse folders"
                  onclick={() => explorer?.collapseAll()}><ChevronsDownUp size={13} /></button
                >
              {:else if s.id === "outline"}
                <button type="button" title="Refresh" aria-label="Refresh outline" onclick={loadOutline}
                  ><RefreshCw size={13} /></button
                >
              {:else if s.id === "timeline"}
                <button type="button" title="Refresh" aria-label="Refresh timeline" onclick={loadTimeline}
                  ><RefreshCw size={13} /></button
                >
              {/if}
            </span>
          </header>

          {#if s.open}
            <div class="body" style:max-height={s.id === "files" ? undefined : `${s.height}px`}>
              {#if s.id === "editors"}
                {#each editor.visible(project.id) as tab (tab.key)}
                  <div class="row" class:active={editor.activeKey === tab.key}>
                    <button
                      type="button"
                      class="main"
                      title={tab.path}
                      onclick={() => {
                        editor.activate(tab.key);
                        ui.show("editor");
                      }}
                    >
                      {#if tab.dirty}<span class="dot"></span>{/if}
                      <span class="name">{tab.label ?? fileName(tab.path)}</span>
                      <span class="sub"
                        >{tab.path.includes("/") ? tab.path.slice(0, tab.path.lastIndexOf("/")) : ""}</span
                      >
                    </button>
                    <button type="button" class="x" aria-label="Close {tab.path}" onclick={() => editor.close(tab.key)}
                      ><X size={12} /></button
                    >
                  </div>
                {:else}
                  <p class="note">No files open.</p>
                {/each}
              {:else if s.id === "files"}
                {#if creating}
                  <form class="create" onsubmit={create}>
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      autofocus
                      placeholder={creating === "file"
                        ? "New file, for example src/new.ts"
                        : "New folder, for example src/lib"}
                      bind:value={newName}
                      onkeydown={(e) => e.key === "Escape" && ((creating = ""), (createError = ""))}
                    />
                  </form>
                  {#if createError}<p class="note error">{createError}</p>{/if}
                {/if}
                <Explorer bind:this={explorer} {project} headless />
              {:else if s.id === "outline"}
                {#each outline as o, i (i)}
                  <button
                    type="button"
                    class="row main"
                    onclick={() => current && editor.open(project.id, current, o.line)}
                  >
                    <span class="kind">{o.kind.slice(0, 3)}</span>
                    <span class="name">{o.name}</span>
                    {#if o.container}<span class="sub">{o.container}</span>{/if}
                  </button>
                {:else}
                  <p class="note">{outlineNote}</p>
                {/each}
              {:else if s.id === "timeline"}
                {#if touches.length}
                  <p class="group">Agent changes</p>
                  {#each touches as t (t.sessionId)}
                    <button
                      type="button"
                      class="row main"
                      title="Open this session"
                      onclick={() => openSession(t.sessionId)}
                    >
                      <Bot size={12} />
                      <span class="name">{t.title}</span>
                      <span class="sub">{t.edits} edit{t.edits === 1 ? "" : "s"} · {ago(t.updatedAt)}</span>
                    </button>
                  {/each}
                {/if}
                {#if commits.length}
                  <p class="group">Commits</p>
                  {#each commits as c (c.sha)}
                    <button
                      type="button"
                      class="row main"
                      title="{c.short} · {c.author} · {c.date}"
                      onclick={showCommits}
                    >
                      <GitBranch size={12} />
                      <span class="name">{c.subject}</span>
                      <span class="sub">{c.date}</span>
                    </button>
                  {/each}
                {/if}
                {#if timelineNote}<p class="note">{timelineNote}</p>{/if}
              {:else if s.id === "touched"}
                {#each touched as t (t.path)}
                  <button
                    type="button"
                    class="row main"
                    title={t.path}
                    onclick={() => !/^[a-z]:\/|^\//i.test(t.path) && editor.open(project.id, t.path)}
                  >
                    <span class="kind">{t.changed ? "M" : "R"}</span>
                    <span class="name">{fileName(t.path)}</span>
                    <span class="sub">{t.path}</span>
                  </button>
                {:else}
                  <p class="note">Files the agent reads or changes in this session show here.</p>
                {/each}
              {/if}
            </div>
            {#if s.id !== "files"}
              <div
                class="resize"
                role="separator"
                aria-orientation="horizontal"
                aria-label="Resize {SECTION_LABELS[s.id]}"
                onpointerdown={(e) => startResize(e, s)}
              ></div>
            {/if}
          {/if}
        </section>
      {/each}
    </div>
  {:else if panel.view === "search"}
    <div class="view"><SearchPanel bind:this={search} {project} /></div>
  {:else}
    <div class="view"><SourceControl {project} part="list" refreshToken={agent.turnsEnded} /></div>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    font-size: var(--qx-text-small, 12px);
  }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 6px;
    border-bottom: 1px solid var(--qx-border);
  }

  .bar button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 26px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .bar button:hover,
  .bar button.on {
    color: var(--qx-text);
  }

  .bar button.on {
    background: var(--qx-bg-raised);
  }

  .spacer {
    flex: 1;
  }

  /* In the sidebar: only the sections menu, small. */
  .bar.slim {
    padding: 0 2px;
    border-bottom: 0;
  }

  .bar.slim > button,
  .bar.slim .more > button {
    display: none;
  }

  .bar.slim {
    height: 0;
    position: relative;
  }

  .bar.slim .menu {
    top: 2px;
  }

  .bar.gone {
    display: none;
  }

  .more {
    position: relative;
  }

  .menu {
    position: absolute;
    top: 28px;
    right: 0;
    z-index: 20;
    display: grid;
    gap: 2px;
    min-width: 200px;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
    box-shadow: 0 6px 18px rgb(0 0 0 / 35%);
  }

  .menu label {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 3px 2px;
    color: var(--qx-text);
    white-space: nowrap;
  }

  .sections {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .section {
    flex: none;
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--qx-border);
  }

  .section.grow {
    flex: 1 1 auto;
    min-height: 120px;
  }

  .section.drop header {
    box-shadow: inset 0 2px 0 var(--qx-accent);
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    height: 24px;
    padding-right: 4px;
    cursor: grab;
  }

  .head {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 4px;
    align-items: center;
    padding: 0 4px;
    border: none;
    background: transparent;
    color: var(--qx-text-section, var(--qx-text-secondary));
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    white-space: nowrap;
    cursor: pointer;
  }

  .head .sub {
    overflow: hidden;
    text-transform: none;
    font-weight: 400;
    letter-spacing: 0;
    color: var(--qx-text-muted);
    text-overflow: ellipsis;
  }

  .actions {
    display: none;
    gap: 1px;
  }

  header:hover .actions,
  header:focus-within .actions {
    display: flex;
  }

  .actions button,
  .row .x {
    display: grid;
    place-items: center;
    width: 22px;
    height: 20px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .actions button:hover,
  .row .x:hover {
    color: var(--qx-text);
    background: var(--qx-bg-raised);
  }

  .body {
    min-height: 0;
    overflow: auto;
  }

  .section.grow .body {
    flex: 1;
  }

  .row {
    display: flex;
    align-items: center;
    width: 100%;
  }

  .row.active,
  .row:hover {
    background: var(--qx-bg-raised);
  }

  button.main,
  .row .main {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 3px 8px 3px 20px;
    border: none;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .name {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .sub {
    overflow: hidden;
    margin-left: auto;
    padding-left: 6px;
    color: var(--qx-text-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .kind {
    flex: none;
    width: 24px;
    color: var(--qx-accent);
    font-size: 10px;
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--qx-text-secondary);
  }

  .row .x {
    display: none;
  }

  .row:hover .x {
    display: grid;
  }

  .group {
    margin: 6px 0 2px 20px;
    color: var(--qx-text-muted);
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .note {
    margin: 4px 0;
    padding: 0 10px 0 20px;
    color: var(--qx-text-muted);
  }

  .note.error {
    color: var(--qx-danger, #e5534b);
  }

  .create {
    padding: 4px 8px 4px 20px;
  }

  .create input {
    width: 100%;
    box-sizing: border-box;
    padding: 3px 6px;
    border: 1px solid var(--qx-accent);
    border-radius: 4px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
  }

  .resize {
    flex: none;
    height: 4px;
    margin-top: -2px;
    cursor: row-resize;
  }

  .resize:hover {
    background: var(--qx-accent);
  }

  .view {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  /* The tree inside the Files section uses its own rows; the section header replaces its header. */
  .section :global(.explorer) {
    height: auto;
  }

  /* One scrollbar: the section's, not the tree's own. */
  .section :global(.explorer .tree) {
    overflow: visible;
  }
</style>
