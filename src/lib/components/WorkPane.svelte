<script lang="ts">
  import InboxView from "./InboxView.svelte";
  import InboxIcon from "@lucide/svelte/icons/inbox";
  import Globe from "@lucide/svelte/icons/globe";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import Search from "@lucide/svelte/icons/search";
  import FileCode from "@lucide/svelte/icons/file-code";
  import FolderTree from "@lucide/svelte/icons/folder-tree";
  import SquareTerminal from "@lucide/svelte/icons/square-terminal";
  import Plus from "@lucide/svelte/icons/plus";
  import Maximize2 from "@lucide/svelte/icons/maximize-2";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import X from "@lucide/svelte/icons/x";
  import Splitter from "./Splitter.svelte";
  import IconButton from "./IconButton.svelte";
  import Explorer from "./Explorer.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import SearchPanel from "./SearchPanel.svelte";
  import PreviewTab from "./PreviewTab.svelte";
  import PlanView from "./PlanView.svelte";
  import SourceControl from "./SourceControl.svelte";
  import UserRound from "@lucide/svelte/icons/user-round";
  import Brain from "@lucide/svelte/icons/brain";
  import Server from "@lucide/svelte/icons/server";
  import TerminalView from "./TerminalView.svelte";
  import Bot from "@lucide/svelte/icons/bot";
  import { layout } from "$lib/layout/layout.svelte";
  import { platform } from "$lib/platform.svelte";
  import { shortcutLabel } from "$lib/shortcuts";
  import { projects } from "$lib/stores/projects.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui, type WorkView } from "$lib/stores/ui.svelte";
  import { LANGUAGE_LABELS } from "$lib/editor/languages";

  let project = $derived(projects.current);
  let fileTabs = $derived(editor.visible(project?.id ?? null));
  let active = $derived(editor.active);
  let maximized = $derived(layout.state.terminalVisible && layout.state.terminalMaximized);
  let termTabs = $derived(terminals.forProject(project?.id ?? null));
  let activeTerm = $derived(terminals.active(project?.id ?? null));
  let cursor = $state({ line: 1, column: 1 });
  let searchPanel: SearchPanel | undefined = $state();
  let editorView: CodeEditor | undefined = $state();

  const fixed: { id: WorkView; label: string; icon: typeof Globe }[] = [
    { id: "preview", label: "Preview", icon: Globe },
    { id: "plan", label: "Plan", icon: ListChecks },
    { id: "search", label: "Search", icon: Search },
    { id: "diff", label: "Git", icon: GitBranch },
  ];

  const key = (action: Parameters<typeof shortcutLabel>[0]) => shortcutLabel(action, platform.isMac);

  // Opening a file shows the editor.
  $effect(() => {
    if (editor.activeKey) ui.view = "editor";
  });

  $effect(() => {
    void ui.focusSearch;
    if (ui.focusSearch && searchPanel) searchPanel.focus();
  });

  function fileName(path: string) {
    return path.split("/").pop() ?? path;
  }

  async function overwrite() {
    const text = editorView?.currentText();
    if (active && text !== null && text !== undefined) await editor.save(active.key, text, true);
  }

  // The tab strip, and the open tab kept in view (a newly opened one may be past the edge).
  let tabStrip = $state<HTMLDivElement | null>(null);
  $effect(() => {
    const key = editor.activeKey;
    if (!tabStrip || !key) return;
    queueMicrotask(() =>
      tabStrip
        ?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" }),
    );
  });
</script>

<section class="work-pane" aria-label="Work">
  {#if !maximized}
    <div class="top">
      {#if project && settings.value.explorerVisible}
        <div class="explorer" style:width="{settings.value.explorerWidth}px">
          <Explorer {project} />
        </div>
        <Splitter
          axis="x"
          size={settings.value.explorerWidth}
          label="Resize file tree"
          onresize={(w) => settings.set("explorerWidth", w)}
        />
      {/if}

      <div class="main">
        <div class="tabs" role="tablist">
          <IconButton
            label={settings.value.explorerVisible ? "Hide file tree" : "Show file tree"}
            active={settings.value.explorerVisible}
            onclick={() => settings.set("explorerVisible", !settings.value.explorerVisible)}
          >
            <FolderTree size={14} />
          </IconButton>
          <div
            class="file-tabs"
            bind:this={tabStrip}
            onwheel={(e) => {
              if (!tabStrip || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
              tabStrip.scrollLeft += e.deltaY;
            }}
          >
            {#each fileTabs as tab (tab.key)}
              <div
                class="tab file-tab"
                class:active={ui.view === "editor" && editor.activeKey === tab.key}
                data-key={tab.key}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={ui.view === "editor" && editor.activeKey === tab.key}
                  title={tab.path}
                  onclick={() => {
                    editor.activate(tab.key);
                    ui.view = "editor";
                  }}
                  onauxclick={(e) => e.button === 1 && editor.close(tab.key)}
                >
                  <FileCode size={13} />
                  <span class="name">{tab.label ?? fileName(tab.path)}</span>
                </button>
                <button
                  type="button"
                  class="close"
                  class:dirty={tab.dirty}
                  aria-label="Close {fileName(tab.path)}"
                  title={tab.dirty ? "Unsaved changes" : "Close"}
                  onclick={() => editor.close(tab.key)}
                >
                  <span class="dot"></span>
                  <X size={12} />
                </button>
              </div>
            {/each}
          </div>
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
          {#if ui.view === "inbox"}
            <button type="button" role="tab" class="tab fixed active" aria-selected="true">
              <InboxIcon size={13} />
              <span>Inbox</span>
            </button>
          {/if}
        </div>

        <div class="content" role="tabpanel">
          <!-- Pages of the whole app come first: they need no project. -->
          {#if ui.view === "inbox"}
            <InboxView />
          {:else if !project}
            <div class="placeholder">
              <FileCode size={22} />
              <h3>No project</h3>
              <p>Add a project folder with the + next to Projects in the sidebar.</p>
            </div>
          {:else if ui.view === "editor"}
            {#if active && (active.global || active.projectId === project.id || !settings.value.tabsFollowProject)}
              <div class="editor-area">
                {#if active.conflict}
                  <div class="bar warn">
                    This file was changed on disk after you opened it.
                    <button type="button" onclick={overwrite}>Overwrite with mine</button>
                    <button type="button" onclick={() => editor.reload(active!.key)}>Load the disk version</button>
                  </div>
                {/if}
                {#if active.saveError}
                  <div class="bar warn">
                    {active.saveError}
                    <button type="button" onclick={() => (active!.saveError = "")}>Dismiss</button>
                  </div>
                {/if}
                {#if active.error}
                  <div class="placeholder"><p class="err">{active.error}</p></div>
                {:else if active.binary || active.tooLarge}
                  <div class="placeholder">
                    <p>
                      {active.binary
                        ? "This is a binary file and is not shown in the editor."
                        : "This file is larger than 10 MB and is not opened in the editor."}
                    </p>
                  </div>
                {:else}
                  <div class="editor-host">
                    <CodeEditor
                      bind:this={editorView}
                      tab={active}
                      oncursor={(line, column) => (cursor = { line, column })}
                    />
                  </div>
                  <div class="status">
                    <span class="path">{active.path}</span>
                    {#if active.dirty}<span class="unsaved">Unsaved · Ctrl+S</span>{/if}
                    <span class="spacer"></span>
                    <span>Ln {cursor.line}, Col {cursor.column}</span>
                    <span>{active.language ? LANGUAGE_LABELS[active.language] : "Plain text"}</span>
                  </div>
                {/if}
              </div>
            {:else}
              <div class="placeholder">
                <FileCode size={22} />
                <h3>Editor</h3>
                <p>Open a file from the tree, or press {platform.isMac ? "Cmd" : "Ctrl"}+P to go to a file.</p>
              </div>
            {/if}
          {:else if ui.view === "preview"}
            <PreviewTab {project} />
          {:else if ui.view === "plan"}
            <PlanView {project} />
          {:else if ui.view === "search"}
            <SearchPanel bind:this={searchPanel} {project} />
          {:else if ui.view === "diff"}
            <SourceControl {project} refreshToken={agent.turnsEnded} />
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
          {#each termTabs as t (t.key)}
            <div class="terminal-tab" class:active={t.key === activeTerm}>
              <button type="button" onclick={() => project && terminals.select(project.id, t.key)}>
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
          <IconButton label="New terminal" disabled={!project} onclick={() => project && terminals.open(project.id)}>
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
            visible={t.projectId === project?.id && t.key === activeTerm}
            ontitle={(title) => (t.title = title.length > 30 ? `${title.slice(0, 30)}…` : title || t.title)}
            onexit={() => (t.ended = true)}
          />
        {/each}
        {#if termTabs.length === 0}
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

  .explorer {
    flex: none;
    min-width: 0;
    border-right: 1px solid var(--qx-border);
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

  .file-tabs {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .file-tabs:hover {
    scrollbar-width: thin;
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

  .file-tab .close.dirty .dot {
    background: var(--qx-text-secondary);
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

  .content {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .editor-area {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .editor-host {
    flex: 1;
    min-height: 0;
  }

  .status {
    flex: none;
    display: flex;
    gap: var(--qx-space-4);
    padding: 3px var(--qx-space-3);
    border-top: 1px solid var(--qx-border);
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .status .path {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .status .spacer {
    flex: 1;
  }

  .unsaved {
    color: var(--qx-status-waiting);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 6px var(--qx-space-3);
    font-size: var(--qx-text-small);
  }

  .bar.warn {
    background: color-mix(in srgb, var(--qx-status-waiting) 15%, transparent);
    color: var(--qx-text);
  }

  .bar button {
    padding: 2px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-raised);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
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

  .placeholder .err {
    color: var(--qx-status-failed);
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
