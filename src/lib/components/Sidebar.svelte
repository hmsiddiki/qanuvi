<script lang="ts">
  import { inbox } from "$lib/stores/inbox.svelte";
  import House from "@lucide/svelte/icons/house";
  import Inbox from "@lucide/svelte/icons/inbox";
  import FolderKanban from "@lucide/svelte/icons/folder-kanban";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import Bot from "@lucide/svelte/icons/bot";
  import Server from "@lucide/svelte/icons/server";
  import Brain from "@lucide/svelte/icons/brain";
  import { memory } from "$lib/stores/memory.svelte";
  import AppWindow from "@lucide/svelte/icons/app-window";
  import { windows } from "$lib/stores/windows.svelte";
  import { workspace } from "$lib/stores/workspace.svelte";
  import { masters } from "$lib/stores/masters.svelte";
  import Bell from "@lucide/svelte/icons/bell";
  import Plus from "@lucide/svelte/icons/plus";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import X from "@lucide/svelte/icons/x";
  import { ask } from "@tauri-apps/plugin-dialog";
  import { layout } from "$lib/layout/layout.svelte";
  import { projects, kindStyle } from "$lib/stores/projects.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { settings } from "$lib/stores/settings.svelte";

  const items = [
    { id: "home", label: "Home", icon: House },
    { id: "inbox", label: "Inbox", icon: Inbox },
    { id: "projects", label: "Projects", icon: FolderKanban },
    { id: "plans", label: "Plans", icon: ListChecks },
    { id: "agents", label: "Agents", icon: Bot },
    { id: "memory", label: "Memory", icon: Brain },
    { id: "servers", label: "Servers", icon: Server },
    { id: "reminders", label: "Reminders", icon: Bell },
  ];

  let collapsed = $derived(layout.state.sidebarCollapsed);
  let waiting = $derived(agent.sessions.filter((s) => s.status === "waiting").length + inbox.unread);

  function navigate(id: string) {
    ui.section = id;
    if (id === "plans") ui.show("plan");
    if (id === "agents" || id === "memory" || id === "servers") ui.openSettings(id);
    if (id === "inbox") {
      if (!layout.state.workVisible) layout.toggleWork();
      ui.show("inbox");
    }
    if (id === "projects") projects.addWithPicker();
  }

  async function removeProject(id: string, name: string) {
    const ok = await ask(`Remove ${name} from Qanuvi? Its files stay where they are.`, {
      title: "Remove project",
      okLabel: "Remove",
      cancelLabel: "Keep",
    });
    if (!ok) return;
    editor.forgetProject(id);
    terminals.forgetProject(id);
    workspace.forget(id);
    await projects.remove(id);
  }

  const dot = (status: string) =>
    status === "running"
      ? "running"
      : status === "waiting"
        ? "waiting"
        : status === "failed"
          ? "failed"
          : status === "idle"
            ? "done"
            : "muted";
</script>

<nav class="sidebar" class:collapsed aria-label="Main">
  <button type="button" class="workspace" title="Workspace">
    <span class="avatar" aria-hidden="true">Q</span>
    {#if !collapsed}
      <span class="workspace-name">My workspace</span>
      <ChevronDown size={14} />
    {/if}
  </button>

  <ul class="items">
    {#each items as item (item.id)}
      <li>
        <button
          type="button"
          class="item"
          class:current={ui.section === item.id}
          aria-current={ui.section === item.id ? "page" : undefined}
          title={collapsed ? item.label : undefined}
          onclick={() => navigate(item.id)}
        >
          <item.icon size={16} />
          {#if !collapsed}<span>{item.label}</span>{/if}
          {#if item.id === "memory" && memory.waiting > 0}<span
              class="pill"
              title="{memory.waiting} suggested by agents">{memory.waiting}</span
            >{/if}
          {#if item.id === "inbox" && waiting > 0}<span class="pill" title="{waiting} waiting for you or not read yet"
              >{waiting}</span
            >{/if}
        </button>
      </li>
    {/each}
  </ul>

  {#if !collapsed}
    <section class="group">
      <header>
        <span>Projects</span>
        <button
          type="button"
          class="add"
          title="Add a project"
          aria-label="Add a project"
          onclick={() => projects.addWithPicker()}
        >
          <Plus size={14} />
        </button>
      </header>
      {#if projects.error}<p class="error">{projects.error}</p>{/if}
      {#each projects.list as p (p.id)}
        {@const style = kindStyle(p.kind)}
        <div class="project" class:current={p.id === projects.currentId}>
          <button
            type="button"
            class="project-main"
            title="{p.master
              ? `On ${masters.list.find((m) => m.id === p.master)?.name ?? 'a server'}: `
              : ''}{p.path}  ·  {style.label}"
            onclick={() => {
              // A project open in another window: bring that window forward instead of editing it twice.
              const other = windows.windowOf(p.id);
              if (other) windows.focus(other);
              else projects.select(p.id);
            }}
          >
            <span class="kind" style:background={style.color}>{style.label.slice(0, 1)}</span>
            <span class="project-name">{p.name}</span>
            {#if p.master}<span class="remote" title="On a server"><Server size={11} /></span>{/if}
            {#if windows.windowOf(p.id)}<span class="elsewhere" title="Open in another window (click to go there)"
                ><AppWindow size={11} /></span
              >{/if}
          </button>
          <button
            type="button"
            class="mini"
            title="Open in a new window"
            aria-label="Open {p.name} in a new window"
            onclick={() => windows.openProject(p.id, p.name)}
          >
            <AppWindow size={13} />
          </button>
          <button
            type="button"
            class="mini"
            title="Project settings"
            aria-label="Settings for {p.name}"
            onclick={() => (ui.settingsFor = p.id)}
          >
            <Settings2 size={13} />
          </button>
          <button
            type="button"
            class="mini"
            title="Remove from Qanuvi"
            aria-label="Remove {p.name}"
            onclick={() => removeProject(p.id, p.name)}
          >
            <X size={13} />
          </button>
        </div>
      {:else}
        <p class="empty">No projects yet. Use + to add a folder.</p>
      {/each}
    </section>

    {#if settings.value.ai}
      <section class="group sessions">
        <header>
          <span>Sessions</span>
          {#if projects.current}
            <button
              type="button"
              class="add"
              title="New session"
              aria-label="New session"
              onclick={() => agent.newSession()}
            >
              <Plus size={14} />
            </button>
          {/if}
        </header>
        {#each agent.sessions as s (s.id)}
          <div class="session" class:current={s.id === agent.selectedId}>
            <button type="button" class="session-main" title="{s.title} · {s.model}" onclick={() => agent.select(s.id)}>
              <span class="session-title">{s.title}</span>
              <span class="dot {dot(s.status)}" title={s.status}></span>
            </button>
            <button
              type="button"
              class="mini"
              title="Delete session"
              aria-label="Delete session"
              onclick={() => agent.remove(s.id)}
            >
              <X size={12} />
            </button>
          </div>
        {:else}
          <p class="empty">{projects.current ? "Agent sessions appear here." : "Add a project to start."}</p>
        {/each}
      </section>
    {:else}
      <div class="sessions"></div>
    {/if}
  {/if}

  <div class="bottom">
    <button
      type="button"
      class="item"
      class:current={!!ui.settings}
      title={collapsed ? "Settings" : undefined}
      onclick={() => (ui.settings ? ui.closeSettings() : ui.openSettings())}
    >
      <Settings2 size={16} />
      {#if !collapsed}<span>Settings</span>{/if}
    </button>
    <button
      type="button"
      class="item"
      title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      onclick={() => layout.toggleSidebarCollapsed()}
    >
      <PanelLeft size={16} />
      {#if !collapsed}<span>Collapse</span>{/if}
    </button>
  </div>
</nav>

<style>
  .sidebar {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--qx-space-2) var(--qx-space-2) var(--qx-space-2) var(--qx-space-3);
    overflow: hidden;
  }

  .sidebar.collapsed {
    padding: var(--qx-space-2);
    align-items: center;
  }

  button {
    font: inherit;
  }

  .workspace {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    width: 100%;
    margin-bottom: var(--qx-space-3);
    padding: 6px var(--qx-space-2);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .collapsed .workspace {
    width: auto;
    padding: 6px;
  }

  .workspace:hover {
    background: var(--qx-bg-raised);
  }

  .avatar {
    width: 22px;
    height: 22px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 6px;
    background: var(--qx-text);
    color: var(--qx-bg-window);
    font-size: 12px;
    font-weight: 700;
  }

  .workspace-name {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .items {
    margin: 0 0 var(--qx-space-4);
    padding: 0;
    list-style: none;
  }

  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px var(--qx-space-2);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    text-align: left;
    cursor: pointer;
  }

  .collapsed .item {
    width: 34px;
    height: 34px;
    justify-content: center;
    padding: 0;
  }

  .item:hover,
  .item.current {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .pill {
    margin-left: auto;
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--qx-status-waiting);
    color: #1c1c1f;
    font-size: 11px;
    font-weight: 700;
    text-align: center;
  }

  .group {
    margin-bottom: var(--qx-space-4);
    min-height: 0;
  }

  .sessions {
    flex: 1;
    overflow: auto;
  }

  .group header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 var(--qx-space-2) var(--qx-space-1);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .add {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .add:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .project,
  .session {
    display: flex;
    align-items: center;
    border-radius: var(--qx-radius-button);
  }

  .project:hover,
  .project.current,
  .session:hover,
  .session.current {
    background: var(--qx-bg-raised);
  }

  .project-main,
  .session-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 5px var(--qx-space-2);
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    text-align: left;
    cursor: pointer;
  }

  .project.current .project-main,
  .session.current .session-main {
    color: var(--qx-text);
  }

  .kind {
    flex: none;
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    border-radius: 5px;
    color: #141416;
    font-size: 10px;
    font-weight: 800;
  }

  .project-name,
  .session-title {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .mini {
    flex: none;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
    visibility: hidden;
  }

  .project:hover .mini,
  .session:hover .mini {
    visibility: visible;
  }

  .mini:hover {
    color: var(--qx-text);
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--qx-text-muted);
  }

  .dot.running {
    background: var(--qx-status-running);
  }

  .dot.waiting {
    background: var(--qx-status-waiting);
  }

  .dot.done {
    background: var(--qx-status-done);
  }

  .dot.failed {
    background: var(--qx-status-failed);
  }

  .dot.muted {
    background: var(--qx-border);
  }

  .empty,
  .error {
    margin: 0;
    padding: 4px var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .bottom {
    margin-top: auto;
    padding-top: var(--qx-space-2);
    width: 100%;
  }

  .collapsed .bottom {
    display: flex;
    justify-content: center;
  }

  .remote {
    display: inline-flex;
    margin-left: 4px;
    color: var(--qx-text-muted);
  }

  .elsewhere {
    flex: none;
    display: inline-flex;
    margin-left: 4px;
    color: var(--qx-accent);
  }
</style>
