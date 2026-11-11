<script lang="ts">
  /**
   * One CodeMirror view for all tabs. Each tab keeps its own editor state (text, cursor, undo history); switching
   * tabs swaps the state, which is cheaper than a view per tab.
   */
  import { onDestroy, onMount } from "svelte";
  import { basicSetup } from "codemirror";
  import { EditorState, Compartment } from "@codemirror/state";
  import { EditorView, keymap } from "@codemirror/view";
  import { indentWithTab } from "@codemirror/commands";
  import { editor, type EditorTab } from "$lib/stores/editor.svelte";
  import { loadLanguage } from "$lib/editor/languages";
  import { qanuviHighlight, qanuviTheme } from "$lib/editor/theme";
  import { api, errorText } from "$lib/api";
  import { gitGutter, setGitBase } from "$lib/git/gutter";
  import { agent } from "$lib/stores/agent.svelte";
  import { ago, gitState } from "$lib/stores/git.svelte";

  interface Props {
    tab: EditorTab;
    oncursor?: (line: number, column: number) => void;
  }

  let { tab, oncursor }: Props = $props();

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let shownKey: string | null = null;
  const language = new Compartment();
  /** Who last changed the cursor's line (Alt+B), shown for a few seconds. */
  let blameText = $state("");
  let blameTimer: ReturnType<typeof setTimeout> | undefined;

  /** Loads the staged version of the shown file for the gutter markers (none for files git does not know). */
  async function loadBase(t: EditorTab) {
    if (t.config || t.binary || t.tooLarge) return;
    const v = await api.git.versions(t.projectId, t.path).catch(() => null);
    const base = v ? (v.index ?? v.head) : null;
    if (view && shownKey === t.key) view.dispatch({ effects: setGitBase.of(base) });
  }

  function blame() {
    const t = editor.tabs.find((x) => x.key === shownKey);
    if (!view || !t || t.config) return true;
    const line = view.state.doc.lineAt(view.state.selection.main.head).number;
    api.git
      .blame(t.projectId, t.path, line)
      .then((b) => {
        blameText = b.uncommitted
          ? `Line ${line}: changed here, not committed yet`
          : `Line ${line}: ${b.author}, ${ago(b.time)} · ${b.summary} (${b.sha.slice(0, 7)})`;
      })
      .catch((e) => (blameText = errorText(e)));
    clearTimeout(blameTimer);
    blameTimer = setTimeout(() => (blameText = ""), 8000);
    return true;
  }

  function save() {
    if (!view || !shownKey) return true;
    editor.save(shownKey, view.state.doc.toString());
    return true;
  }

  function extensions() {
    return [
      basicSetup,
      keymap.of([
        indentWithTab,
        { key: "Mod-s", run: save, preventDefault: true },
        { key: "Alt-b", run: blame, preventDefault: true },
      ]),
      gitGutter(),
      qanuviTheme,
      qanuviHighlight,
      language.of([]),
      EditorState.tabSize.of(4),
      EditorView.updateListener.of((update) => {
        if (update.docChanged && shownKey) {
          const t = editor.tabs.find((x) => x.key === shownKey);
          if (t) {
            // Back to the saved text (for example after undo) is not a change. The full comparison only runs
            // when the length matches, so typing stays cheap in big files.
            const doc = update.state.doc;
            const same = doc.length === t.savedText.length && doc.toString() === t.savedText;
            if (t.dirty === same) t.dirty = !same;
          }
        }
        if (update.selectionSet || update.docChanged) {
          const pos = update.state.selection.main.head;
          const line = update.state.doc.lineAt(pos);
          oncursor?.(line.number, pos - line.from + 1);
          const shown = editor.tabs.find((x) => x.key === shownKey);
          if (shown) shown.cursor = { line: line.number, column: pos - line.from + 1 };
        }
      }),
    ];
  }

  function stateFor(t: EditorTab): EditorState {
    return editor.states.get(t.key) ?? EditorState.create({ doc: t.savedText, extensions: extensions() });
  }

  async function show(t: EditorTab) {
    if (!view) return;
    if (shownKey && shownKey !== t.key) editor.states.set(shownKey, view.state);
    if (shownKey !== t.key || !editor.states.has(t.key)) {
      shownKey = t.key;
      view.setState(stateFor(t));
      // Mark the tab as having a live state, so later updates do not rebuild it from the saved text.
      editor.states.set(t.key, view.state);
      loadBase(t);
      const ext = await loadLanguage(t.language);
      if (shownKey === t.key && view) view.dispatch({ effects: language.reconfigure(ext) });
    }
    if (t.revealLine && view) {
      const n = Math.min(Math.max(1, t.revealLine), view.state.doc.lines);
      const line = view.state.doc.line(n);
      const at = line.from + Math.min(Math.max(0, (t.revealColumn ?? 1) - 1), line.length);
      view.dispatch({
        selection: { anchor: at },
        effects: EditorView.scrollIntoView(line.from, { y: "center" }),
      });
      t.revealLine = null;
      t.revealColumn = null;
    }
    view.focus();
  }

  onMount(() => {
    view = new EditorView({ parent: host });
    show(tab);
  });

  $effect(() => {
    // Re-run when the tab, a reload, or a reveal request changes.
    void tab.key;
    void tab.savedText;
    void tab.revealLine;
    if (view && !editor.states.has(tab.key) && shownKey === tab.key) shownKey = null;
    show(tab);
  });

  // Compare again after a save, an agent turn or a git action (staging changes what the markers compare with).
  $effect(() => {
    void tab.savedText;
    void agent.turnsEnded;
    void gitState.changed;
    if (view && shownKey === tab.key) loadBase(tab);
  });

  /** The current text of the shown tab (for Save from the menu or before closing). */
  export function currentText(): string | null {
    return view ? view.state.doc.toString() : null;
  }

  onDestroy(() => {
    if (view && shownKey) editor.states.set(shownKey, view.state);
    view?.destroy();
    view = null;
  });
</script>

<div class="code-editor" bind:this={host}>
  {#if blameText}<div class="blame" role="status">{blameText}</div>{/if}
</div>

<style>
  .code-editor {
    position: relative;
    height: 100%;
    overflow: hidden;
  }

  .blame {
    position: absolute;
    right: 12px;
    bottom: 10px;
    z-index: 5;
    max-width: 70%;
    padding: 4px 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .code-editor :global(.cm-editor) {
    height: 100%;
  }

  .code-editor :global(.cm-editor.cm-focused) {
    outline: none;
  }
</style>
