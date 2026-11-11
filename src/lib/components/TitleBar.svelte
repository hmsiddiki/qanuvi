<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Search from "@lucide/svelte/icons/search";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import MessagesSquare from "@lucide/svelte/icons/messages-square";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import X from "@lucide/svelte/icons/x";
  import IconButton from "./IconButton.svelte";
  import LayoutMenu from "./LayoutMenu.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { sidebarScreenWidth } from "$lib/layout/layout";
  import { platform } from "$lib/platform.svelte";
  import { shortcutLabel } from "$lib/shortcuts";
  import { settings } from "$lib/stores/settings.svelte";

  interface Props {
    title: string;
  }

  let { title }: Props = $props();

  let maximized = $state(false);
  const appWindow = getCurrentWindow();

  $effect(() => {
    if (!platform.drawsWindowControls) return;
    let stop: (() => void) | undefined;
    const refresh = async () => (maximized = await appWindow.isMaximized());
    refresh();
    appWindow.onResized(refresh).then((unlisten) => (stop = unlisten));
    return () => stop?.();
  });

  const key = (action: Parameters<typeof shortcutLabel>[0]) => shortcutLabel(action, platform.isMac);
  // Left part lines up with the sidebar; on macOS it leaves room for the system window buttons.
  let leftWidth = $derived(Math.max(sidebarScreenWidth(layout.state), platform.isMac ? 150 : 120));
</script>

<header class="titlebar" class:mac={platform.isMac} data-tauri-drag-region>
  <div class="left" style:width="{leftWidth}px" data-tauri-drag-region>
    {#if !platform.isMac}
      <img class="mark" src="/logo.png" alt="" aria-hidden="true" draggable="false" />
    {/if}
    <IconButton label="Search" disabled><Search size={15} /></IconButton>
    {#if layout.state.sidebarVisible && !layout.state.sidebarCollapsed}
      <span class="spacer" data-tauri-drag-region></span>
      <IconButton label="Back" disabled><ChevronLeft size={16} /></IconButton>
      <IconButton label="Forward" disabled><ChevronRight size={16} /></IconButton>
    {/if}
  </div>

  <div class="title" data-tauri-drag-region>{title}</div>

  <div class="right" data-tauri-drag-region>
    <LayoutMenu />
    <span class="divider"></span>
    <IconButton
      label="Sidebar"
      shortcut={key("toggleSidebar")}
      active={layout.state.sidebarVisible}
      onclick={() => layout.toggleSidebar()}
    >
      <PanelLeft size={15} />
    </IconButton>
    {#if settings.value.ai}
      <IconButton
        label="Agent pane"
        shortcut={key("toggleAgent")}
        active={layout.state.agentVisible}
        onclick={() => layout.toggleAgent()}
      >
        <MessagesSquare size={15} />
      </IconButton>
    {/if}
    <IconButton
      label="Work pane"
      shortcut={key("toggleWork")}
      active={layout.state.workVisible}
      onclick={() => layout.toggleWork()}
    >
      <PanelRight size={15} />
    </IconButton>
    <IconButton
      label="Terminal"
      shortcut={key("toggleTerminal")}
      active={layout.state.workVisible && layout.state.terminalVisible}
      onclick={() => layout.toggleTerminal()}
    >
      <PanelBottom size={15} />
    </IconButton>

    {#if platform.drawsWindowControls}
      <div class="window-controls">
        <button type="button" aria-label="Minimise" title="Minimise" onclick={() => appWindow.minimize()}>
          <Minus size={16} />
        </button>
        <button
          type="button"
          aria-label={maximized ? "Restore" : "Maximise"}
          title={maximized ? "Restore" : "Maximise"}
          onclick={() => appWindow.toggleMaximize()}
        >
          {#if maximized}<Copy size={13} />{:else}<Square size={13} />{/if}
        </button>
        <button type="button" class="close" aria-label="Close" title="Close" onclick={() => appWindow.close()}>
          <X size={16} />
        </button>
      </div>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    height: 40px;
    flex: none;
    display: flex;
    align-items: center;
    background: var(--qx-bg-window);
  }

  .left {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--qx-space-1);
    padding-left: var(--qx-space-3);
    padding-right: var(--qx-space-2);
    transition: width 120ms;
  }

  .titlebar.mac .left {
    padding-left: 78px;
  }

  .mark {
    width: 20px;
    height: 20px;
    margin: 0 var(--qx-space-1) 0 2px;
    object-fit: contain;
    pointer-events: none;
  }

  .spacer {
    flex: 1;
  }

  .title {
    flex: 1;
    min-width: 0;
    padding: 0 var(--qx-space-4);
    overflow: hidden;
    color: var(--qx-text);
    font-weight: 500;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .right {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding-right: var(--qx-space-2);
  }

  .divider {
    width: 1px;
    height: 16px;
    margin: 0 var(--qx-space-1);
    background: var(--qx-border);
  }

  .window-controls {
    display: flex;
    height: 40px;
    margin-left: var(--qx-space-2);
    margin-right: calc(-1 * var(--qx-space-2));
  }

  .window-controls button {
    width: 46px;
    height: 40px;
    display: grid;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .window-controls button:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  .window-controls .close:hover {
    background: #c42b1c;
    color: #fff;
  }
</style>
