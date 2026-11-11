<script lang="ts">
  import SettingsView from "$lib/components/SettingsView.svelte";
  import { startWatch } from "$lib/stores/watch.svelte";
  import { knowledge } from "$lib/stores/knowledge.svelte";
  import { onMount } from "svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import AgentPane from "$lib/components/AgentPane.svelte";
  import WorkPane from "$lib/components/WorkPane.svelte";
  import Splitter from "$lib/components/Splitter.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { sidebarScreenWidth } from "$lib/layout/layout";
  import { platform } from "$lib/platform.svelte";
  import { matchShortcut } from "$lib/shortcuts";
  import QuickOpen from "$lib/components/QuickOpen.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import ProjectSettings from "$lib/components/ProjectSettings.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { masters } from "$lib/stores/masters.svelte";
  import { memory } from "$lib/stores/memory.svelte";
  import { memoryDisk } from "$lib/stores/memoryDisk.svelte";
  import { account } from "$lib/stores/account.svelte";
  import { remoteAccess } from "$lib/stores/remoteAccess.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { untrack } from "svelte";
  import { editor as editorStore, tabKey } from "$lib/stores/editor.svelte";
  import { isProjectView, workspace } from "$lib/stores/workspace.svelte";
  import { projectFromUrl, windows } from "$lib/stores/windows.svelte";

  let ready = $state(false);

  onMount(() => {
    layout.load();
    settings.load();
    ready = true;
    platform.load();
    // A project window opens with its own project and leaves the main window's choice alone.
    projects.startWith = projectFromUrl(location.search);
    windows.init(() => projects.currentId);
    projects.persist = !projects.startWith;
    terminals.remote = (id) => !!projects.list.find((p) => p.id === id)?.master;
    // Servers marked to connect at start-up add their projects once the local ones are listed.
    projects.load().then(() => masters.autoConnect());
    // The memory disk syncs at start-up (in the main window only: one sync per machine is enough).
    if (!projects.startWith) memoryDisk.init();
    account.init();
    if (!projects.startWith) remoteAccess.init();
    terminals.loadShells();
    agent.loadInfo();
    knowledge.check().catch(() => {});
    startWatch();
    const flush = () => {
      layout.saveNow();
      workspace.flush();
    };
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  });

  // A different project: its tabs, sessions and terminals.
  $effect(() => {
    const id = projects.currentId;
    // Only the project is a dependency: what these read inside (settings, the agent's session) must not re-run this.
    untrack(() => {
      editor.projectChanged(id);
      const sessions = agent.loadProject(id);
      windows.announce(id);
      if (id) restore(id, sessions);
    });
  });

  /**
   * Gives back a project's workspace (Milestone 15c): the first time in this run, its files with their cursors, the
   * shown view and its terminals; every time, the agent session it was on.
   */
  async function restore(id: string, sessions: Promise<void>) {
    const w = workspace.read(id);
    if (!workspace.restored.has(id)) {
      for (const t of w.tabs) {
        if (projects.currentId !== id) return;
        if (editorStore.tabs.some((x) => x.key === tabKey(id, t.path))) continue;
        await editorStore.open(id, t.path, t.line, t.column);
        // A file that is gone is not reopened.
        const tab = editorStore.tabs.find((x) => x.key === tabKey(id, t.path));
        if (tab?.error) await editorStore.close(tab.key);
      }
      if (projects.currentId !== id) return;
      if (w.active && editorStore.tabs.some((x) => x.key === tabKey(id, w.active!)))
        editorStore.activate(tabKey(id, w.active));
      if (w.view) ui.show(w.view);
      if (terminals.forProject(id).length === 0) for (const shell of w.terminals) terminals.open(id, shell);
      workspace.restored.add(id);
    }
    await sessions;
    if (projects.currentId === id && w.session && agent.sessions.some((s) => s.id === w.session))
      agent.select(w.session);
  }

  // A memory change is copied to the memory disk a little later.
  $effect(() => {
    void memory.changed;
    untrack(() => memoryDisk.changed());
  });

  // An agent turn may have suggested memory entries; count the ones waiting for the user.
  $effect(() => {
    void agent.turnsEnded;
    untrack(() => memory.refreshWaiting());
  });

  // Keeps the current project's workspace saved while working.
  $effect(() => {
    const id = projects.currentId;
    if (!id || !workspace.restored.has(id)) return;
    const tabs = editorStore.tabs
      .filter((t) => t.projectId === id && !t.config)
      .map((t) => ({ path: t.path, line: t.cursor?.line ?? 1, column: t.cursor?.column ?? 1 }));
    const active = editorStore.tabs.find((t) => t.key === editorStore.activeKey && t.projectId === id && !t.config);
    const view = ui.view;
    const shells = terminals
      .forProject(id)
      .filter((t) => !t.ended)
      .map((t) => t.shell);
    const session = agent.selectedId;
    untrack(() =>
      workspace.saveSoon(id, {
        tabs,
        active: active?.path ?? null,
        terminals: shells,
        ...(isProjectView(view) ? { view } : {}),
        // A blank new session is not worth remembering over the one the project was on.
        ...(session ? { session } : {}),
      }),
    );
  });

  function keydown(event: KeyboardEvent) {
    const action = matchShortcut(event, platform.isMac);
    if (action) {
      event.preventDefault();
      layout[action]();
      return;
    }
    const mod = platform.isMac ? event.metaKey : event.ctrlKey;
    if (!mod || event.altKey) return;
    const k = event.key.toLowerCase();
    if ((k === "p" || k === "t") && !event.shiftKey && projects.current) {
      event.preventDefault();
      ui.quickOpenText = k === "t" ? "#" : "";
      ui.quickOpen = true;
    } else if (k === "o" && event.shiftKey && projects.current) {
      event.preventDefault();
      ui.quickOpenText = "@";
      ui.quickOpen = true;
    } else if (k === "f" && event.shiftKey && projects.current) {
      event.preventDefault();
      if (!layout.state.workVisible) layout.toggleWork();
      ui.show("search");
      ui.focusSearch++;
    } else if (k === "w" && !event.shiftKey && ui.view === "editor" && editor.activeKey) {
      event.preventDefault();
      editor.close(editor.activeKey);
    }
  }

  let title = $derived(agent.selected?.title ?? (projects.current ? projects.current.name : "Qanuvi"));

  let s = $derived(layout.state);
  // With AI features off the agent pane is not shown, and the work pane always is.
  let showAgent = $derived(s.agentVisible && settings.value.ai);
  let showWork = $derived(s.workVisible || !showAgent);
  let workOnly = $derived(!showAgent && showWork);
  let agentOnly = $derived(showAgent && !showWork);
</script>

<svelte:window onkeydown={keydown} />

{#if ready}
  <div class="shell">
    <TitleBar {title} />

    <div class="body">
      {#if s.sidebarVisible}
        <aside class="sidebar" style:width="{sidebarScreenWidth(s)}px">
          <Sidebar />
        </aside>
        {#if !s.sidebarCollapsed}
          <Splitter
            axis="x"
            size={s.sidebarWidth}
            label="Resize sidebar"
            onresize={(size) => layout.setSidebarWidth(size)}
          />
        {/if}
      {/if}

      <div class="panes" class:no-sidebar={!s.sidebarVisible}>
        {#if ui.settings}<SettingsView />{/if}
        {#if showAgent}
          <div class="agent" class:fill={agentOnly} style:width={agentOnly ? undefined : `${s.agentWidth}px`}>
            <AgentPane />
          </div>
        {/if}
        {#if showAgent && showWork}
          <Splitter
            axis="x"
            size={s.agentWidth}
            label="Resize agent pane"
            onresize={(size) => layout.setAgentWidth(size)}
          />
        {/if}
        {#if showWork}
          <div class="work" class:fill={workOnly || showAgent}>
            <WorkPane />
          </div>
        {/if}
      </div>
    </div>
    <StatusBar />
  </div>
{/if}

{#if ui.quickOpen && projects.current}
  <QuickOpen
    project={projects.current}
    onpick={ui.pickFile}
    initial={ui.pickFile ? "" : ui.quickOpenText}
    onclose={() => {
      ui.quickOpen = false;
      ui.pickFile = null;
    }}
  />
{/if}
{#if ui.settingsFor}
  <ProjectSettings projectId={ui.settingsFor} onclose={() => (ui.settingsFor = null)} />
{/if}

<style>
  .shell {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .sidebar {
    flex: none;
    min-width: 0;
  }

  .panes {
    position: relative;
    flex: 1;
    min-width: 0;
    display: flex;
    gap: var(--qx-space-1);
    padding: 0 var(--qx-space-2) var(--qx-space-2) var(--qx-space-1);
  }

  .panes.no-sidebar {
    padding-left: var(--qx-space-2);
  }

  .agent {
    flex: none;
    min-width: 0;
  }

  .work {
    flex: 1;
    min-width: 0;
  }

  .fill {
    flex: 1;
  }
</style>
