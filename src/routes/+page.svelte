<script lang="ts">
  import { updates } from "$lib/stores/updates.svelte";
  import { startup } from "$lib/stores/startup.svelte";
  import { claudeUpdate } from "$lib/stores/claudeUpdate.svelte";
  import { usage } from "$lib/stores/usage.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import { startWatch } from "$lib/stores/watch.svelte";
  import { knowledge } from "$lib/stores/knowledge.svelte";
  import { onMount } from "svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import AgentPane from "$lib/components/AgentPane.svelte";
  import WorkPane from "$lib/components/WorkPane.svelte";
  import MachineHeld from "$lib/components/MachineHeld.svelte";
  import { switcher } from "$lib/stores/switcher.svelte";
  import Splitter from "$lib/components/Splitter.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { sidebarScreenWidth } from "$lib/layout/layout";
  import { platform } from "$lib/platform.svelte";
  import { matchShortcut } from "$lib/shortcuts";
  import { debugKey } from "$lib/dap/run";
  import QuickOpen from "$lib/components/QuickOpen.svelte";
  import StatusBar from "$lib/components/StatusBar.svelte";
  import ProjectSettings from "$lib/components/ProjectSettings.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { displayName, masters } from "$lib/stores/masters.svelte";
  import { pick, restoreShown } from "$lib/stores/machines";
  import { beforeWindowGoes } from "$lib/stores/closing";
  import { keepUnsavedOnClose } from "$lib/stores/keepUnsaved";
  import { working } from "$lib/stores/working.svelte";
  import { memory } from "$lib/stores/memory.svelte";
  import { memoryDisk } from "$lib/stores/memoryDisk.svelte";
  import { account } from "$lib/stores/account.svelte";
  import { remoteAccess } from "$lib/stores/remoteAccess.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { inbox } from "$lib/stores/inbox.svelte";
  import { planData } from "$lib/plan/planData.svelte";
  import { startNotifications } from "$lib/stores/notify.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { untrack } from "svelte";
  import { editor as editorStore, tabKey } from "$lib/stores/editor.svelte";
  import { isProjectView, workspace, type SavedTab } from "$lib/stores/workspace.svelte";
  import { machineFromUrl, projectFromUrl, windows } from "$lib/stores/windows.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import FolderPicker from "$lib/components/FolderPicker.svelte";
  import { draftPreview, drafts } from "$lib/stores/drafts";
  import { machineName, shownMachine } from "$lib/remote";

  let ready = $state(false);

  onMount(() => {
    layout.load();
    settings.load();
    // Then This PC's own settings from its database (the window's copy is moved in the first time).
    void settings.loadMachine();
    // First: what plans keep (picks, plan state, notes, run settings) reads at once from its copy.
    planData.start().catch(() => {});
    ready = true;
    platform.load();
    // A project window opens with its own project and leaves the main window's choice alone; so does a machine window
    // (24z3 step 4), which opens on its own machine.
    projects.startWith = projectFromUrl(location.search);
    const machineStart = machineFromUrl(location.search);
    const extra = !!projects.startWith || machineStart !== undefined;
    windows.init(
      () => projects.currentId,
      () => switcher.shown,
    );
    projects.persist = !extra;
    terminals.remote = (id) => !!projects.list.find((p) => p.id === id)?.master;
    // Servers marked to connect at start-up connect once This PC's projects are listed (a project window for a
    // server's project shows that server). The main window then shows the machine picked last, when it is connected.
    projects
      .load()
      .then(() => masters.load())
      .then(() => masters.autoConnect())
      .then(async () => {
        if (machineStart !== undefined) return openOn(machineStart);
        if (extra) return;
        await restoreShown((id) => masters.statusOf(id).state === "on");
        // The machine windows open when the app last closed come back, each on its machine.
        for (const m of windows.takeMachineWindows()) {
          const known = m === "" || masters.find(m) !== undefined || m.includes("/");
          if (known && m !== (switcher.shown ?? "")) {
            const master = masters.find(m);
            void windows
              .openMachine(m || null, master ? displayName(master) : m ? masters.route(m) : "This PC")
              .catch(() => {});
          }
        }
      });
    working.start();
    // The memory disk syncs at start-up (in the main window only: one sync per machine is enough).
    if (!extra) memoryDisk.init();
    if (!extra) remoteAccess.init();
    terminals.loadShells();
    agent.loadInfo();
    // Chat clean-up (24t), once per start in the main window; said once, nothing when none went.
    if (!extra)
      agent
        .cleanUpChats(settings.value.chatCleanupDays)
        .then((n) => {
          if (n > 0)
            inbox.add({
              kind: "chats",
              title: `Removed ${n} chat${n === 1 ? "" : "s"} not opened for ${settings.value.chatCleanupDays} days`,
              text: "Starred chats and plan steps are never removed. Settings, Agents, Chat clean-up changes this.",
              open: { label: "Settings", view: "agents" },
            });
        })
        .catch(() => {});
    knowledge.check().catch(() => {});
    startWatch();
    inbox.start().catch(() => {});
    startNotifications().catch(() => {});
    updates.start();
    startup.start().catch(() => {});
    claudeUpdate.start();
    usage.start();
    // i1: a window that goes keeps its unsaved text as drafts on the shown machine.
    beforeWindowGoes(keepUnsavedOnClose);
    const flush = () => {
      layout.saveNow();
      workspace.flush();
    };
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  });

  /**
   * A machine window (24z3 step 4) opens on its machine: a master not connected yet is connected first (the app's
   * connection, shared by its windows); one that does not answer shows its own "not connected" screen (Part E).
   */
  async function openOn(machine: string | null) {
    if (!machine) return;
    // A machine reached through others: the machines on its path are connected first (migration M3c).
    const m = await masters.reach(machine);
    if (!m) return;
    if (masters.statusOf(machine).state !== "on") await masters.connect(m);
    if (masters.statusOf(machine).state === "on") await pick(machine, false);
    else await masters.showUnreachable(machine);
  }

  // The shown machine's Qanuvi account (migration M2: each machine keeps its own).
  $effect(() => {
    const shown = switcher.shown;
    untrack(() => void account.load(shown));
  });

  // The other windows know which machine this one shows (picking it there brings this window forward).
  $effect(() => {
    const shown = switcher.shown;
    untrack(() => windows.announceMachine(shown));
  });

  // A machine window's title names the machine it shows.
  $effect(() => {
    const name = switcher.shown ? machineName(switcher.shown) : "This PC";
    untrack(() => windows.titleFor(name));
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
        await editorStore.open(id, t.path, t.line, t.column, 0);
        // A file that is gone is not reopened.
        const tab = editorStore.tabs.find((x) => x.key === tabKey(id, t.path));
        if (tab?.error) await editorStore.close(tab.key);
      }
      if (projects.currentId !== id) return;
      if (w.active && editorStore.tabs.some((x) => x.key === tabKey(id, w.active!)))
        editorStore.activate(tabKey(id, w.active), 0);
      // The split editor as it was: which files the second group shows (15i part 4).
      const second = w.tabs.filter((t) => t.groups?.includes(1));
      if (second.length)
        editorStore.restoreGroups(
          second.map((t) => tabKey(id, t.path)),
          second.filter((t) => !t.groups?.includes(0)).map((t) => tabKey(id, t.path)),
          w.active2 ? tabKey(id, w.active2) : null,
          w.focused,
        );
      if (w.view) ui.show(w.view);
      if (terminals.forProject(id).length === 0) for (const shell of w.terminals) terminals.open(id, shell);
      await giveBackDrafts(id);
      if (projects.currentId !== id) return;
      workspace.restored.add(id);
    }
    await sessions;
    if (projects.currentId === id && w.session && agent.sessions.some((s) => s.id === w.session))
      agent.select(w.session);
  }

  /**
   * Unsaved text the project's machine kept while it was hidden (24z3 Part B2): back in its files, as typed, with
   * no question. A project left before they are all back keeps the rest for next time.
   */
  async function giveBackDrafts(id: string) {
    const machine = projects.machine;
    const list = await drafts.take(machine, id);
    for (let i = 0; i < list.length; i++) {
      const d = list[i];
      if (projects.currentId !== id) {
        await drafts.keep(machine, list.slice(i), shownMachine() === machine);
        return;
      }
      const key = tabKey(id, d.path);
      if (!editorStore.tabs.some((x) => x.key === key)) await editorStore.open(id, d.path, null, null, 0);
      if (!editorStore.applyDraft(key, d.text, d.hash)) {
        // The file cannot be opened any more (gone, or now too large): its text stays kept rather than lost, and the
        // owner is told once (the same key replaces the note on each return) with the text to copy from.
        await drafts.keep(machine, [d], shownMachine() === machine);
        const tab = editorStore.tabs.find((x) => x.key === key);
        const why = tab?.error
          ? tab.error
          : tab?.tooLarge
            ? "It is now too large to open."
            : tab?.binary
              ? "It is no longer a text file."
              : "It could not be opened.";
        if (tab?.error) await editorStore.close(key);
        inbox.add(
          {
            kind: "drafts",
            key: `draft:${machine ?? "this-pc"}:${id}:${d.path}`,
            title: `Unsaved changes to ${d.path} on ${machine ? machineName(machine) : "This PC"} are kept`,
            text: `${why} The unsaved text stays kept and comes back once the file opens again.\n\n${draftPreview(d.text)}`,
          },
          machine,
        );
      }
    }
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
    const groups = editorStore.groups;
    const tabs = editorStore.tabs
      .filter((t) => t.projectId === id && !t.config)
      .map((t) => {
        const saved: SavedTab = { path: t.path, line: t.cursor?.line ?? 1, column: t.cursor?.column ?? 1 };
        const g = [0, 1].filter((n) => groups[n]?.keys.includes(t.key));
        if (g.includes(1)) saved.groups = g;
        return saved;
      });
    const own = (key: string | null | undefined) =>
      editorStore.tabs.find((t) => t.key === key && t.projectId === id && !t.config);
    const active = own(groups[0]?.active);
    const active2 = own(groups[1]?.active);
    const focused = editorStore.focused;
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
        active2: active2?.path ?? null,
        focused,
        terminals: shells,
        ...(isProjectView(view) ? { view } : {}),
        // A blank new session is not worth remembering over the one the project was on.
        ...(session ? { session } : {}),
      }),
    );
  });

  function keydown(event: KeyboardEvent) {
    if (debugKey(event)) {
      event.preventDefault();
      return;
    }
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
    } else if (k === "r" && event.shiftKey) {
      event.preventDefault();
      updates.reloadWindow();
    } else if (k === "f" && event.shiftKey && projects.current) {
      event.preventDefault();
      if (!layout.state.workVisible) layout.toggleWork();
      ui.show("search");
    } else if (k === "w" && !event.shiftKey && ui.view === "editor" && editor.activeKey) {
      event.preventDefault();
      editor.close(editor.activeKey, editor.focused);
    } else if (event.key === "\\" && !event.shiftKey && ui.view === "editor" && editor.activeKey) {
      // Ctrl+\: the open file in the other editor group (splitting the editor when there is one group).
      event.preventDefault();
      editor.openToSide();
    } else if ((k === "1" || k === "2") && !event.shiftKey && ui.view === "editor") {
      // Ctrl+1 / Ctrl+2: the keyboard to the first or second editor group.
      const g = Number(k) - 1;
      if (editor.shownGroups(projects.currentId).includes(g)) {
        event.preventDefault();
        editor.focus(g, true);
      }
    }
  }

  // Appearance: the editor's font size and the size of the whole interface (the window's zoom).
  $effect(() => {
    const a = settings.value.appearance;
    document.documentElement.style.setProperty("--qx-editor-font-size", `${a.editorFontSize}px`);
    getCurrentWebview()
      .setZoom(a.uiScale / 100)
      .catch(() => {});
  });

  let title = $derived(agent.selected?.title ?? (projects.current ? projects.current.name : "Qanuvi"));

  let s = $derived(layout.state);
  // With AI features off the agent pane is not shown, and the work pane always is.
  let showAgent = $derived(s.agentVisible && settings.value.ai);
  let showWork = $derived(s.workVisible || !showAgent);
  let workOnly = $derived(!showAgent && showWork);
  let agentOnly = $derived(showAgent && !showWork);
  // The shown machine cannot be reached (24z3 Part E): its panes take no input under the cover.
  let held = $derived(switcher.held !== null);
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

      <div class="panes" class:no-sidebar={!s.sidebarVisible} class:agent-right={s.agentSide === "right"}>
        {#if ui.settings}<SettingsView />{/if}
        {#if showAgent}
          <div
            class="agent"
            inert={held}
            class:fill={agentOnly}
            style:width={agentOnly ? undefined : `${s.agentWidth}px`}
          >
            <AgentPane />
          </div>
        {/if}
        {#if showAgent && showWork}
          <Splitter
            axis="x"
            size={s.agentWidth}
            direction={s.agentSide === "right" ? -1 : 1}
            label="Resize agent pane"
            onresize={(size) => layout.setAgentWidth(size)}
          />
        {/if}
        {#if showWork}
          <div class="work" inert={held} class:fill={workOnly || showAgent}>
            <WorkPane />
          </div>
        {/if}
        <MachineHeld />
      </div>
    </div>
    <StatusBar />
  </div>
{/if}

<ContextMenu />
<ConfirmDialog />
<FolderPicker />

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

  /* The conversation on the editor's right (the VS Code-like layout). */
  .panes.agent-right {
    flex-direction: row-reverse;
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
