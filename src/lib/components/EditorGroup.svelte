<script lang="ts">
  /**
   * One editor group (15i part 4): its open file in the editor with the save bars and the status line. When the
   * editor is split, each group has its own tab strip with a button to join the groups again.
   */
  import FileCode from "@lucide/svelte/icons/file-code";
  import X from "@lucide/svelte/icons/x";
  import CodeEditor from "./CodeEditor.svelte";
  import FileTabs from "./FileTabs.svelte";
  import IconButton from "./IconButton.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { tabDrag } from "$lib/stores/tabDrag.svelte";
  import { platform } from "$lib/platform.svelte";
  import { LANGUAGE_LABELS } from "$lib/editor/languages";
  import type { Project } from "$lib/api";

  interface Props {
    group: number;
    project: Project;
    /** Two groups are shown: this group has its own tab strip. */
    split: boolean;
  }

  let { group, project, split }: Props = $props();

  let tabs = $derived(editor.inGroup(project.id, group));
  let active = $derived(editor.activeIn(group));
  let own = $derived(
    active && (active.global || active.projectId === project.id || !settings.value.tabsFollowProject) ? active : null,
  );
  let cursor = $state({ line: 1, column: 1 });
  let editorView: CodeEditor | undefined = $state();

  async function overwrite() {
    const text = editorView?.currentText();
    if (own && text !== null && text !== undefined) await editor.save(own.key, text, true);
  }
</script>

<div class="group" class:focused={split && editor.focused === group}>
  {#if split}
    <div class="strip">
      <FileTabs {group} {tabs} {split} />
      <IconButton label="Close this group (its tabs join the other group)" onclick={() => editor.joinGroups()}>
        <X size={13} />
      </IconButton>
    </div>
  {/if}
  <div class="body" data-drop-group={group}>
    {#if own}
      <div class="editor-area">
        {#if own.conflict}
          <div class="bar warn">
            This file was changed on disk after you opened it.
            <button type="button" onclick={overwrite}>Overwrite with mine</button>
            <button type="button" onclick={() => editor.reload(own!.key)}>Load the disk version</button>
          </div>
        {/if}
        {#if own.saveError}
          <div class="bar warn">
            {own.saveError}
            <button type="button" onclick={() => (own!.saveError = "")}>Dismiss</button>
          </div>
        {/if}
        {#if own.error}
          <div class="placeholder"><p class="err">{own.error}</p></div>
        {:else if own.binary || own.tooLarge}
          <div class="placeholder">
            <p>
              {own.binary
                ? "This is a binary file and is not shown in the editor."
                : "This file is larger than 10 MB and is not opened in the editor."}
            </p>
          </div>
        {:else}
          <div class="editor-host">
            <CodeEditor
              bind:this={editorView}
              tab={own}
              {group}
              oncursor={(line, column) => (cursor = { line, column })}
            />
          </div>
          <div class="status">
            <span class="path">{own.path}</span>
            {#if own.dirty}<span class="unsaved">Unsaved · Ctrl+S</span>{/if}
            <span class="spacer"></span>
            <span>Ln {cursor.line}, Col {cursor.column}</span>
            <span>{own.language ? LANGUAGE_LABELS[own.language] : "Plain text"}</span>
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
    {#if tabDrag.key}
      <!-- While a tab is dragged: the group lights up, and with one group its right side splits the editor. -->
      <div class="drop-shade" class:on={tabDrag.target?.group === group && tabDrag.target.before === null}></div>
      {#if !split}
        <div class="split-zone" class:on={tabDrag.target?.group === "new"} data-drop-group="new">
          <span>Drop to split</span>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .group {
    height: 100%;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .strip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px var(--qx-space-1) 0 var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
    opacity: 0.8;
  }

  .group.focused .strip {
    opacity: 1;
  }

  .body {
    position: relative;
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

  .drop-shade {
    position: absolute;
    inset: 0;
    z-index: 8;
    pointer-events: none;
  }

  .drop-shade.on {
    background: color-mix(in srgb, var(--qx-accent) 12%, transparent);
  }

  .split-zone {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 9;
    width: 35%;
    display: grid;
    place-items: center;
    color: transparent;
    font-size: var(--qx-text-small);
  }

  .split-zone.on {
    background: color-mix(in srgb, var(--qx-accent) 18%, transparent);
    border-left: 2px solid var(--qx-accent);
    color: var(--qx-text);
  }
</style>
