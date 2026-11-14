<script lang="ts">
  /**
   * The file tabs of one editor group (15i part 4). Tabs can be dragged to another place, to the other group, or to
   * the editor's edge to split it; the right-click menu opens a file to the side.
   */
  import { contextMenu } from "$lib/stores/contextMenu.svelte";
  import * as files from "$lib/fileActions";
  import FileCode from "@lucide/svelte/icons/file-code";
  import X from "@lucide/svelte/icons/x";
  import { editor, type EditorTab } from "$lib/stores/editor.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { tabDrag } from "$lib/stores/tabDrag.svelte";
  import { ui } from "$lib/stores/ui.svelte";

  interface Props {
    group: number;
    tabs: EditorTab[];
    /** Two groups are shown (the menu offers moving to the other). */
    split: boolean;
  }

  let { group, tabs, split }: Props = $props();

  let project = $derived(projects.current);
  let activeKey = $derived(editor.groups[group]?.active ?? null);
  let strip = $state<HTMLDivElement | null>(null);

  const fileName = (path: string) => path.split("/").pop() ?? path;

  /** Right-click on a tab. */
  function tabMenu(e: MouseEvent, tab: EditorTab) {
    const at = tabs.findIndex((t) => t.key === tab.key);
    const closeAll = async (list: EditorTab[]) => {
      for (const t of list) await editor.close(t.key, group);
    };
    const own = project && !tab.global && tab.projectId === project.id ? project : null;
    contextMenu.show(e, [
      [
        { label: "Close", hint: "Ctrl+W", action: () => editor.close(tab.key, group) },
        {
          label: "Close others",
          disabled: tabs.length < 2,
          action: () => closeAll(tabs.filter((t) => t.key !== tab.key)),
        },
        { label: "Close to the right", disabled: at === tabs.length - 1, action: () => closeAll(tabs.slice(at + 1)) },
        { label: "Close saved", action: () => closeAll(tabs.filter((t) => !t.dirty)) },
      ],
      split
        ? [
            { label: "Open in the other group", hint: "Ctrl+\\", action: () => editor.openToSide(tab.key) },
            { label: "Move to the other group", action: () => editor.moveTab(tab.key, group, group === 0 ? 1 : 0) },
          ]
        : [
            { label: "Open to the side", hint: "Ctrl+\\", action: () => editor.openToSide(tab.key, "right") },
            { label: "Open below", action: () => editor.openToSide(tab.key, "down") },
          ],
      own && [
        { label: "Copy path", action: () => files.copyPath(own, tab.path) },
        { label: "Copy relative path", action: () => files.copyRelativePath(tab.path) },
        { label: "Reveal in File Explorer", action: () => files.reveal(own, tab.path) },
      ],
      own && [{ label: "Add to the message", action: () => files.addToMessage(tab.path) }],
    ]);
  }

  // Dragging: a press becomes a drag after the pointer moves a few pixels; a plain click still opens the tab.
  let press: { key: string; label: string; x: number; y: number } | null = null;
  let dragged = false;

  function down(e: PointerEvent, tab: EditorTab) {
    if (e.button !== 0) return;
    press = { key: tab.key, label: tab.label ?? fileName(tab.path), x: e.clientX, y: e.clientY };
    dragged = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function move(e: PointerEvent) {
    if (!press) return;
    if (!tabDrag.key) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 6) return;
      tabDrag.start(press.key, group, press.label);
    }
    tabDrag.moveTo(e.clientX, e.clientY);
  }

  function up(e: PointerEvent) {
    if (!press) return;
    press = null;
    if (!tabDrag.key) return;
    dragged = true;
    tabDrag.drop(e.clientX, e.clientY);
  }

  function cancel() {
    press = null;
    if (tabDrag.key) tabDrag.cancel();
  }

  function open(tab: EditorTab) {
    if (dragged) {
      dragged = false;
      return;
    }
    editor.activate(tab.key, group);
    editor.focusTick++;
    ui.view = "editor";
  }

  // The open tab kept in view (a newly opened one may be past the edge).
  $effect(() => {
    const key = activeKey;
    if (!strip || !key) return;
    queueMicrotask(() =>
      strip
        ?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)
        ?.scrollIntoView({ block: "nearest", inline: "nearest" }),
    );
  });
</script>

<div
  class="file-tabs"
  class:drop={tabDrag.key !== null && tabDrag.target?.group === group}
  data-drop-group={group}
  bind:this={strip}
  role="tablist"
  tabindex="-1"
  onwheel={(e) => {
    if (!strip || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    strip.scrollLeft += e.deltaY;
  }}
>
  {#each tabs as tab (tab.key)}
    {@const shown = ui.view === "editor" && activeKey === tab.key}
    <div
      class="tab file-tab"
      class:active={shown}
      class:focused={shown && (!split || editor.focused === group)}
      class:drop-before={tabDrag.key !== null &&
        tabDrag.target?.group === group &&
        tabDrag.target.before === tab.key &&
        tab.key !== tabDrag.key}
      class:dragging={tabDrag.key === tab.key && tabDrag.from === group}
      data-key={tab.key}
    >
      <button
        type="button"
        role="tab"
        aria-selected={shown}
        title={tab.path}
        onpointerdown={(e) => down(e, tab)}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={cancel}
        onlostpointercapture={() => press && cancel()}
        onclick={() => open(tab)}
        onauxclick={(e) => e.button === 1 && editor.close(tab.key, group)}
        oncontextmenu={(e) => tabMenu(e, tab)}
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
        onclick={() => editor.close(tab.key, group)}
      >
        <span class="dot"></span>
        <X size={12} />
      </button>
    </div>
  {/each}
</div>

<style>
  .file-tabs {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
    outline: none;
  }

  .file-tabs:hover {
    scrollbar-width: thin;
  }

  .file-tabs.drop {
    background: color-mix(in srgb, var(--qx-accent) 8%, transparent);
  }

  .tab {
    flex: 0 1 auto;
    min-width: 96px;
    max-width: 240px;
    display: flex;
    align-items: center;
    border-bottom: 2px solid transparent;
    color: var(--qx-text-secondary);
  }

  .tab.drop-before {
    box-shadow: inset 2px 0 0 var(--qx-accent);
  }

  .tab.dragging {
    opacity: 0.5;
  }

  .tab button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 7px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
    white-space: nowrap;
  }

  .tab > button:first-child {
    flex: 1;
    min-width: 0;
  }

  .name {
    overflow: hidden;
    min-width: 0;
    text-overflow: ellipsis;
  }

  .tab:hover {
    color: var(--qx-text);
  }

  .tab.active {
    color: var(--qx-text);
    border-bottom-color: color-mix(in srgb, var(--qx-accent) 45%, transparent);
  }

  .tab.active.focused {
    border-bottom-color: var(--qx-accent);
  }

  .close {
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0 !important;
    justify-content: center;
    border-radius: 4px;
  }

  .close :global(svg) {
    display: none;
  }

  .close .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: transparent;
  }

  .close.dirty .dot {
    background: var(--qx-text-secondary);
  }

  .tab:hover .close :global(svg),
  .tab.active .close:not(.dirty) :global(svg) {
    display: block;
  }

  .tab:hover .close .dot,
  .tab.active .close:not(.dirty) .dot {
    display: none;
  }

  .close:hover {
    background: var(--qx-bg-raised);
  }
</style>
