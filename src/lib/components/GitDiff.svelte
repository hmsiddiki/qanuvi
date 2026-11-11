<script lang="ts">
  /**
   * The diff of one file in the Source Control view (Milestone 15b): side by side or inline, unchanged parts folded,
   * and per change: stage, unstage or undo. Merge conflicts get accept current / incoming / both per block.
   */
  import { onDestroy } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { MergeView, unifiedMergeView } from "@codemirror/merge";
  import { ask } from "@tauri-apps/plugin-dialog";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Rows2 from "@lucide/svelte/icons/rows-2";
  import History from "@lucide/svelte/icons/clock";
  import FileCode from "@lucide/svelte/icons/file-code";
  import { api, errorText, type GitChange, type Project } from "$lib/api";
  import { conflicts, giveBack, hunks, resolve, takeHunk, type Conflict, type Resolution } from "$lib/git/hunks";
  import { letterFor } from "$lib/git/decorations";
  import { languageFor, loadLanguage } from "$lib/editor/languages";
  import { qanuviHighlight, qanuviTheme } from "$lib/editor/theme";
  import { editor } from "$lib/stores/editor.svelte";
  import { gitState } from "$lib/stores/git.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
    change: GitChange;
    /** Which side of the file: the staged change (last commit → staged) or the unstaged one (staged → file). */
    side: "staged" | "changes";
    /** Called after an action changed the repository. */
    onchanged: () => void;
    onhistory: (path: string) => void;
  }

  let { project, change, side, onchanged, onhistory }: Props = $props();

  let host = $state<HTMLDivElement>();
  let layout = $state<"split" | "inline">(
    (localStorage.getItem("qanuvi.diff.layout") as "split" | "inline" | null) ?? "split",
  );
  let a = $state(""); // old text
  let b = $state(""); // new text
  let head = $state<string | null>(null);
  let index = $state<string | null>(null);
  let work = $state<string | null>(null);
  let error = $state("");
  let busy = $state(false);
  let current = $state(0);
  let loaded = $state(false);

  let conflicted = $derived(letterFor(change) === "!");
  let blocks = $derived<Conflict[]>(conflicted && work !== null ? conflicts(work) : []);
  let list = $derived(hunks(a, b));
  let views: { destroy(): void; target: EditorView } | null = null;

  async function load() {
    loaded = false;
    error = "";
    try {
      const v = await api.git.versions(project.id, change.path);
      head = v.head;
      index = v.index;
      const file = change.deleted ? null : await api.files.read(project.id, change.path).catch(() => null);
      work = file && !file.binary && !file.too_large ? file.text : null;
      if (file && (file.binary || file.too_large)) error = "This file is binary or too large to compare.";
      if (side === "staged") {
        a = head ?? "";
        b = index ?? "";
      } else {
        a = index ?? head ?? "";
        b = work ?? "";
      }
      current = 0;
      loaded = true;
    } catch (e) {
      error = errorText(e);
    }
  }

  function destroyViews() {
    views?.destroy();
    views = null;
  }

  async function render() {
    destroyViews();
    if (!host || !loaded || conflicted) return;
    const lang = await loadLanguage(languageFor(change.path));
    if (!host) return;
    const base = [
      EditorView.editable.of(false),
      EditorState.readOnly.of(true),
      EditorView.lineWrapping,
      qanuviTheme,
      qanuviHighlight,
      lang,
    ];
    host.replaceChildren();
    const fold = { margin: 3, minSize: 6 };
    if (layout === "split") {
      const mv = new MergeView({
        a: { doc: a, extensions: base },
        b: { doc: b, extensions: base },
        parent: host,
        collapseUnchanged: fold,
        gutter: true,
      });
      views = { destroy: () => mv.destroy(), target: mv.b };
    } else {
      const v = new EditorView({
        doc: b,
        extensions: [...base, unifiedMergeView({ original: a, mergeControls: false, collapseUnchanged: fold })],
        parent: host,
      });
      views = { destroy: () => v.destroy(), target: v };
    }
    reveal(0);
  }

  function reveal(i: number) {
    if (!views || list.length === 0) return;
    current = Math.max(0, Math.min(i, list.length - 1));
    const doc = views.target.state.doc;
    const line = doc.line(Math.min(Math.max(1, list[current].line), doc.lines));
    views.target.dispatch({ effects: EditorView.scrollIntoView(line.from, { y: "center" }) });
  }

  $effect(() => {
    void change.path;
    void change.status;
    void side;
    void gitState.changed;
    load();
  });

  $effect(() => {
    void layout;
    void a;
    void b;
    void loaded;
    void host;
    render();
  });

  onDestroy(destroyViews);

  function setLayout(l: "split" | "inline") {
    layout = l;
    localStorage.setItem("qanuvi.diff.layout", l);
  }

  function openTab() {
    return editor.tabs.find((t) => t.projectId === project.id && t.path === change.path);
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    error = "";
    try {
      await action();
      gitState.touch();
      onchanged();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  /** Writes the file (after undoing a change or resolving a conflict) and reloads its tab. */
  async function writeWork(text: string) {
    const tab = openTab();
    if (tab?.dirty) throw new Error("The file has unsaved changes in the editor; save or close it first.");
    const file = await api.files.read(project.id, change.path);
    await api.files.write(project.id, change.path, text, file.hash);
    if (tab) await editor.reload(tab.key);
  }

  const stageHunk = () =>
    run(async () => {
      const h = list[current];
      await api.git.setIndex(project.id, change.path, takeHunk(a, b, h));
    });

  const unstageHunk = () =>
    run(async () => {
      const h = list[current];
      if (head === null) await api.git.unstage(project.id, change.path);
      else await api.git.setIndex(project.id, change.path, giveBack(a, b, h));
    });

  async function undoHunk() {
    const h = list[current];
    const ok = await ask(
      `Undo this change (line ${h.line}) in ${change.path}? The file goes back to the staged text there.`,
      { title: "Undo change", kind: "warning", okLabel: "Undo change", cancelLabel: "Keep it" },
    );
    if (ok) await run(() => writeWork(giveBack(a, b, h)));
  }

  const accept = (c: Conflict, how: Resolution) => run(() => writeWork(resolve(work ?? "", c, how)));
  const markResolved = () => run(() => api.git.stage(project.id, change.path));

  const preview = (t: string) => (t.length > 400 ? `${t.slice(0, 400)}…` : t) || "(nothing)";
</script>

<div class="gitdiff">
  <header>
    <span class="path" title={change.path}>{change.path}</span>
    <span class="side">{conflicted ? "conflict" : side === "staged" ? "staged" : "not staged"}</span>
    <span class="spacer"></span>
    {#if !conflicted}
      {#if list.length}
        <span class="count">{current + 1} / {list.length}</span>
        <IconButton label="Previous change" onclick={() => reveal(current - 1)}><ChevronUp size={14} /></IconButton>
        <IconButton label="Next change" onclick={() => reveal(current + 1)}><ChevronDown size={14} /></IconButton>
        {#if side === "changes"}
          <button type="button" class="act" disabled={busy} onclick={stageHunk}>Stage change</button>
          {#if index !== null || head !== null}
            <button type="button" class="act" disabled={busy} onclick={undoHunk}>Undo change</button>
          {/if}
        {:else}
          <button type="button" class="act" disabled={busy} onclick={unstageHunk}>Unstage change</button>
        {/if}
      {/if}
      <IconButton label="Side by side" active={layout === "split"} onclick={() => setLayout("split")}>
        <Columns2 size={14} />
      </IconButton>
      <IconButton label="Inline" active={layout === "inline"} onclick={() => setLayout("inline")}>
        <Rows2 size={14} />
      </IconButton>
    {/if}
    <IconButton label="History of this file" onclick={() => onhistory(change.path)}><History size={14} /></IconButton>
    {#if !change.deleted}
      <IconButton label="Open the file" onclick={() => editor.open(project.id, change.path)}>
        <FileCode size={14} />
      </IconButton>
    {/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  {#if conflicted}
    <div class="conflicts">
      {#if blocks.length === 0}
        <p>No conflict markers are left in this file.</p>
        <button type="button" class="act primary" disabled={busy} onclick={markResolved}>Mark as resolved</button>
      {:else}
        <p>{blocks.length} conflict{blocks.length > 1 ? "s" : ""} in this file.</p>
        {#each blocks as c, i (c.from)}
          <section class="block">
            <div class="block-head">
              <span>Line {c.line}</span>
              <span class="spacer"></span>
              <button type="button" class="act" disabled={busy} onclick={() => accept(c, "current")}>
                Accept current
              </button>
              <button type="button" class="act" disabled={busy} onclick={() => accept(c, "incoming")}>
                Accept incoming
              </button>
              <button type="button" class="act" disabled={busy} onclick={() => accept(c, "both")}>Accept both</button>
            </div>
            <div class="sides">
              <div class="current">
                <span class="label">Current{c.currentLabel ? ` (${c.currentLabel})` : ""}</span>
                <pre>{preview(c.current)}</pre>
              </div>
              <div class="incoming">
                <span class="label">Incoming{c.incomingLabel ? ` (${c.incomingLabel})` : ""}</span>
                <pre>{preview(c.incoming)}</pre>
              </div>
            </div>
            {#if i === 0 && blocks.length > 1}<p class="hint">
                After each choice the file is saved and checked again.
              </p>{/if}
          </section>
        {/each}
      {/if}
    </div>
  {:else if loaded && list.length === 0 && !error}
    <p class="empty">No changes in the text (perhaps only the file mode or line endings changed).</p>
  {/if}
  <div class="host" class:hidden={conflicted} bind:this={host}></div>
</div>

<style>
  .gitdiff {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  header {
    flex: none;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    padding: 4px var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
    font-size: var(--qx-text-small);
  }

  .path {
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .side,
  .count {
    color: var(--qx-text-muted);
    white-space: nowrap;
  }

  .spacer {
    flex: 1;
  }

  .act {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    font-size: var(--qx-text-small);
    white-space: nowrap;
    cursor: pointer;
  }

  .act:hover:not(:disabled) {
    border-color: var(--qx-accent);
  }

  .act.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .act:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .error {
    margin: 0;
    padding: 4px var(--qx-space-3);
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }

  .empty,
  .hint {
    padding: var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .host {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .host.hidden {
    display: none;
  }

  .host :global(.cm-mergeView),
  .host :global(.cm-editor) {
    min-height: 100%;
  }

  .host :global(.cm-mergeView .cm-editor) {
    height: auto;
  }

  .conflicts {
    flex: 1;
    overflow: auto;
    padding: var(--qx-space-2) var(--qx-space-3);
    font-size: var(--qx-text-small);
  }

  .block {
    margin-bottom: var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    overflow: hidden;
  }

  .block-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--qx-border);
    background: var(--qx-bg-window);
  }

  .sides {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  }

  .sides > div {
    min-width: 0;
    padding: 6px 8px;
  }

  .current {
    background: rgba(46, 160, 67, 0.1);
  }

  .incoming {
    background: rgba(55, 148, 255, 0.1);
  }

  .label {
    color: var(--qx-text-muted);
  }

  pre {
    margin: 4px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    font-family: var(--qx-font-mono, monospace);
  }
</style>
