<script lang="ts">
  import Globe from "@lucide/svelte/icons/globe";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import FileCode from "@lucide/svelte/icons/file-code";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SquareTerminal from "@lucide/svelte/icons/square-terminal";
  import Plus from "@lucide/svelte/icons/plus";
  import Maximize2 from "@lucide/svelte/icons/maximize-2";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import X from "@lucide/svelte/icons/x";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import Splitter from "./Splitter.svelte";
  import IconButton from "./IconButton.svelte";
  import { panel } from "$lib/stores/panel.svelte";
  import EditorGroup from "./EditorGroup.svelte";
  import FileTabs from "./FileTabs.svelte";
  import { tabDrag } from "$lib/stores/tabDrag.svelte";
  import PreviewTab from "./PreviewTab.svelte";
  import PlanView from "./PlanView.svelte";
  import SourceControl from "./SourceControl.svelte";
  import TerminalView from "./TerminalView.svelte";
  import ProblemsPanel from "./ProblemsPanel.svelte";
  import DebugConsole from "./DebugConsole.svelte";
  import Bug from "@lucide/svelte/icons/bug";
  import { dap } from "$lib/stores/dap.svelte";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import { lsp } from "$lib/stores/lsp.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { platform } from "$lib/platform.svelte";
  import { shortcutLabel } from "$lib/shortcuts";
  import { projects } from "$lib/stores/projects.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui, type WorkView } from "$lib/stores/ui.svelte";

  let project = $derived(projects.current);
  /** The editor groups with tabs for this project (15i part 4): one, or two when the editor is split. */
  let shown = $derived(editor.shownGroups(project?.id ?? null));
  let split = $derived(shown.length > 1);
  let down = $derived(editor.split.direction === "down");
  let groupsWidth = $state(0);
  let groupsHeight = $state(0);
  /** Room for the two groups (the handle between them takes none). */
  let room = $derived(Math.max(1, down ? groupsHeight : groupsWidth));
  let maximized = $derived(layout.state.terminalVisible && layout.state.terminalMaximized);
  let termTabs = $derived(terminals.forProject(project?.id ?? null));
  let activeTerm = $derived(terminals.active(project?.id ?? null));
  let problemCount = $derived(lsp.counts(project?.id ?? null));
  let bottom = $derived(lsp.bottom);

  const fixed: { id: WorkView; label: string; icon: typeof Globe }[] = [
    { id: "preview", label: "Preview", icon: Globe },
    { id: "plan", label: "Plan", icon: ListChecks },
  ];

  const key = (action: Parameters<typeof shortcutLabel>[0]) => shortcutLabel(action, platform.isMac);

  /** Shows the sidebar, expanded, in this mode. */
  function openSidebar(view: "explorer" | "search") {
    if (!layout.state.sidebarVisible) layout.toggleSidebar();
    if (layout.state.sidebarCollapsed) layout.toggleSidebarCollapsed();
    panel.show(view);
  }

  // Search asked for: the sidebar shows Search.
  $effect(() => {
    if (ui.searchWanted) openSidebar("search");
  });

  // Opening a file shows the editor.
  $effect(() => {
    if (editor.activeKey) ui.view = "editor";
  });
</script>

<section class="work-pane" aria-label="Work">
  {#if !maximized}
    <div class="top">
      <div class="main">
        <div class="tabs" role="tablist">
          <IconButton
            label={panel.view === "explorer" ? "Back to the workspace" : "Show the files (Explorer)"}
            active={panel.view === "explorer" && layout.state.sidebarVisible && !layout.state.sidebarCollapsed}
            onclick={() => {
              if (panel.view === "explorer" && layout.state.sidebarVisible && !layout.state.sidebarCollapsed)
                panel.show("workspace");
              else openSidebar("explorer");
            }}
          >
            <FolderTree size={14} />
          </IconButton>
          {#if split}
            <!-- Split: each group has its own tabs; this one brings the editor back from Preview or Plan. -->
            <button
              type="button"
              role="tab"
              class="tab fixed"
              class:active={ui.view === "editor"}
              aria-selected={ui.view === "editor"}
              onclick={() => (ui.view = "editor")}
            >
              <FileCode size={13} />
              <span>Editor</span>
            </button>
            <span class="grow"></span>
          {:else}
            <FileTabs group={shown[0]} tabs={editor.inGroup(project?.id ?? null, shown[0])} split={false} />
          {/if}
          {#each fixed as f (f.id)}
            <button
              type="button"
              role="tab"
              class="tab fixed"
              class:active={ui.view === f.id}
              aria-selected={ui.view === f.id}
              onclick={() => (ui.view = f.id)}
            >
              <f.icon size={13} />
              <span>{f.label}</span>
            </button>
          {/each}
          {#if ui.view === "diff"}
            <div class="tab file-tab active">
              <button type="button" role="tab" aria-selected="true" title="Git changes">
                <GitBranch size={13} />
                <span class="name">Changes</span>
              </button>
              <button
                type="button"
                class="close"
                aria-label="Close changes"
                title="Close"
                onclick={() => ui.show("editor")}
              >
                <span class="dot"></span>
                <X size={12} />
              </button>
            </div>
          {/if}
          {#if project && ui.view === "editor"}
            {#if split}
              <IconButton
                label={down ? "Put the editor groups side by side" : "Put the editor groups one above the other"}
                onclick={() => editor.setSplit({ direction: down ? "right" : "down" })}
              >
                {#if down}<Columns2 size={14} />{:else}<Rows2 size={14} />{/if}
              </IconButton>
            {:else}
              <IconButton
                label="Split the editor (Ctrl+\)"
                disabled={!editor.activeIn(shown[0])}
                onclick={() => editor.openToSide(editor.activeIn(shown[0])?.key ?? null)}
              >
                {#if down}<Rows2 size={14} />{:else}<Columns2 size={14} />{/if}
              </IconButton>
            {/if}
          {/if}
        </div>

        <div class="content" role="tabpanel">
          {#if !project}
            <div class="placeholder">
              <FileCode size={22} />
              <h3>No project</h3>
              <p>Add a project folder with the + next to Projects in the sidebar.</p>
            </div>
          {:else if ui.view === "editor"}
            <div class="groups" class:down bind:clientWidth={groupsWidth} bind:clientHeight={groupsHeight}>
              {#each shown as g, i (g)}
                {#if i === 1}
                  <Splitter
                    axis={down ? "y" : "x"}
                    size={editor.split.ratio * room}
                    label="Resize the editor groups"
                    onresize={(size) => editor.setSplit({ ratio: size / room })}
                  />
                {/if}
                <div class="slot" style:flex={split && i === 0 ? `0 0 ${editor.split.ratio * room}px` : "1 1 0"}>
                  <EditorGroup group={g} {project} {split} />
                </div>
              {/each}
            </div>
          {:else if ui.view === "preview"}
            <PreviewTab {project} />
          {:else if ui.view === "plan"}
            <PlanView {project} />
          {:else if ui.view === "diff"}
            <SourceControl {project} part="main" refreshToken={agent.turnsEnded} />
          {/if}
        </div>
      </div>
    </div>
  {/if}

  {#if layout.state.terminalVisible}
    {#if !maximized}
      <Splitter
        axis="y"
        direction={-1}
        size={layout.state.terminalHeight}
        label="Resize terminal"
        onresize={(size) => layout.setTerminalHeight(size)}
      />
    {/if}
    <div class="terminal" class:maximized style:height={maximized ? undefined : `${layout.state.terminalHeight}px`}>
      <div class="terminal-bar">
        <div class="terminal-tabs">
          <div class="terminal-tab" class:active={bottom === "problems"}>
            <button type="button" onclick={() => (lsp.bottom = "problems")}>
              <CircleAlert size={13} />
              <span>Problems</span>
              {#if problemCount.errors + problemCount.warnings}
                <span class="count">{problemCount.errors + problemCount.warnings}</span>
              {/if}
            </button>
          </div>
          {#if dap.session || dap.console.length || bottom === "debug"}
            <div class="terminal-tab" class:active={bottom === "debug"}>
              <button type="button" onclick={() => (lsp.bottom = "debug")}>
                <Bug size={13} />
                <span>Debug console</span>
              </button>
            </div>
          {/if}
          {#if lsp.references}
            <div class="terminal-tab" class:active={bottom === "references"}>
              <button type="button" onclick={() => (lsp.bottom = "references")}>
                <ListTree size={13} />
                <span>References</span>
              </button>
              <button
                type="button"
                class="close"
                aria-label="Close references"
                onclick={() => ((lsp.references = null), (lsp.bottom = "terminal"))}
              >
                <X size={11} />
              </button>
            </div>
          {/if}
          {#each termTabs as t (t.key)}
            <div class="terminal-tab" class:active={bottom === "terminal" && t.key === activeTerm}>
              <button
                type="button"
                onclick={() => project && (terminals.select(project.id, t.key), (lsp.bottom = "terminal"))}
              >
                <SquareTerminal size={13} />
                <span>{t.title}</span>
              </button>
              <button type="button" class="close" aria-label="Close terminal" onclick={() => terminals.close(t.key)}>
                <X size={11} />
              </button>
            </div>
          {/each}
        </div>
        <div class="terminal-actions">
          {#if project && terminals.shells.length > 1}
            <select
              title="Shell for new terminals"
              value={settings.value.defaultShell || terminals.shells[0]?.id}
              onchange={(e) => settings.set("defaultShell", (e.target as HTMLSelectElement).value)}
            >
              {#each terminals.shells as s (s.id)}<option value={s.id}>{s.label}</option>{/each}
            </select>
          {/if}
          <IconButton
            label="New terminal"
            disabled={!project}
            onclick={() => project && (terminals.open(project.id), (lsp.bottom = "terminal"))}
          >
            <Plus size={14} />
          </IconButton>
          <IconButton
            label={maximized ? "Restore terminal" : "Maximise terminal"}
            shortcut={key("toggleTerminalMaximized")}
            onclick={() => layout.toggleTerminalMaximized()}
          >
            {#if maximized}<Minimize2 size={14} />{:else}<Maximize2 size={14} />{/if}
          </IconButton>
          <IconButton label="Hide terminal" shortcut={key("toggleTerminal")} onclick={() => layout.toggleTerminal()}>
            <X size={14} />
          </IconButton>
        </div>
      </div>
      <div class="terminal-body">
        {#each terminals.tabs as t (t.key)}
          <TerminalView
            projectId={t.projectId}
            shell={t.shell}
            dir={t.dir}
            visible={bottom === "terminal" && t.projectId === project?.id && t.key === activeTerm}
            ontitle={(title) => (t.title = title.length > 30 ? `${title.slice(0, 30)}…` : title || t.title)}
            onexit={() => (t.ended = true)}
          />
        {/each}
        {#if bottom === "debug"}
          <DebugConsole />
        {:else if bottom !== "terminal"}
          <ProblemsPanel projectId={project?.id ?? null} view={bottom} />
        {:else if termTabs.length === 0}
          <div class="terminal-empty">
            {#if project}
              <button type="button" onclick={() => terminals.open(project!.id)}
                >Open a terminal in {project.name}</button
              >
            {:else}
              <span>Terminals open in a project's folder.</span>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}
  {#if tabDrag.key}
    <div class="drag-ghost" style:left="{tabDrag.x + 14}px" style:top="{tabDrag.y + 10}px">{tabDrag.label}</div>
  {/if}
</section>

<style>
  .work-pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--qx-bg-panel);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    overflow: hidden;
  }

  .top {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  .tabs {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px var(--qx-space-2) 0;
    border-bottom: 1px solid var(--qx-border);
  }

  .file-tab {
    flex: 0 1 auto;
    min-width: 96px;
    max-width: 240px;
  }

  .file-tab > button:first-child {
    flex: 1;
    min-width: 0;
  }

  .file-tab .name {
    overflow: hidden;
    min-width: 0;
    text-overflow: ellipsis;
  }

  .tab {
    flex: none;
    display: flex;
    align-items: center;
    border-bottom: 2px solid transparent;
    color: var(--qx-text-secondary);
  }

  .tab button,
  button.tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 7px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }

  button.tab {
    border-bottom: 2px solid transparent;
  }

  .tab:hover {
    color: var(--qx-text);
  }

  .tab.active {
    color: var(--qx-text);
    border-bottom-color: var(--qx-accent);
  }

  .file-tab .close {
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0;
    justify-content: center;
    border-radius: 4px;
  }

  .file-tab .close :global(svg) {
    display: none;
  }

  .file-tab .close .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: transparent;
  }

  .file-tab:hover .close :global(svg),
  .file-tab.active .close:not(.dirty) :global(svg) {
    display: block;
  }

  .file-tab:hover .close .dot,
  .file-tab.active .close:not(.dirty) .dot {
    display: none;
  }

  .file-tab .close:hover {
    background: var(--qx-bg-raised);
  }

  .grow {
    flex: 1;
  }

  .groups {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .groups.down {
    flex-direction: column;
  }

  .slot {
    min-width: 0;
    min-height: 0;
  }

  .groups:not(.down) .slot ~ .slot {
    border-left: 1px solid var(--qx-border);
  }

  .groups.down .slot ~ .slot {
    border-top: 1px solid var(--qx-border);
  }

  .drag-ghost {
    position: fixed;
    z-index: 100;
    padding: 3px 10px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-raised);
    color: var(--qx-text);
    font-size: var(--qx-text-small);
    pointer-events: none;
    white-space: nowrap;
  }

  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .placeholder {
    margin: auto;
    max-width: 320px;
    text-align: center;
    color: var(--qx-text-muted);
  }

  .placeholder h3 {
    margin: var(--qx-space-2) 0 var(--qx-space-1);
    color: var(--qx-text);
    font-size: 14px;
    font-weight: 600;
  }

  .placeholder p {
    margin: 0;
    color: var(--qx-text-secondary);
  }

  .terminal {
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-top: 1px solid var(--qx-border);
    background: var(--qx-bg-window);
  }

  .terminal.maximized {
    flex: 1;
    border-top: 0;
  }

  .terminal-bar {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--qx-space-2);
    height: 34px;
    padding: 0 var(--qx-space-1) 0 var(--qx-space-2);
  }

  .terminal-tabs {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .terminal-tab {
    flex: none;
    display: flex;
    align-items: center;
    border-radius: 6px;
    color: var(--qx-text-secondary);
  }

  .terminal-tab.active {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .terminal-tab button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 4px 4px 8px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .terminal-tab .count {
    padding: 0 5px;
    border-radius: 8px;
    background: var(--qx-bg-raised);
    font-size: 11px;
  }

  .terminal-tab .close {
    padding: 4px 6px 4px 2px;
    color: var(--qx-text-muted);
  }

  .terminal-tab .close:hover {
    color: var(--qx-text);
  }

  .terminal-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .terminal-actions select {
    height: 24px;
    max-width: 150px;
    margin-right: var(--qx-space-1);
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
  }

  .terminal-body {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  .terminal-empty {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .terminal-empty button {
    padding: 6px 12px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-raised);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }
</style>
