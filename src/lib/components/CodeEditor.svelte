<script lang="ts">
  /**
   * One CodeMirror view for all tabs. Each tab keeps its own editor state (text, cursor, undo history); switching
   * tabs swaps the state, which is cheaper than a view per tab.
   */
  import { clipboardText, contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import { addToMessage } from "$lib/fileActions";
  import { ui } from "$lib/stores/ui.svelte";
  import { onDestroy, onMount, untrack } from "svelte";
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
  import { lintGutter, setDiagnostics } from "@codemirror/lint";
  import { lsp, type Doc, type Problem } from "$lib/stores/lsp.svelte";
  import { lspExtensions, toDiagnostics } from "$lib/lsp/codemirror";
  import {
    findReferences,
    fixWithAgent,
    formatDocument,
    goToDefinition as lspDefinition,
    problemsAt,
    realProblems,
    quickFixMenu,
    renameBlocked,
    renameSymbol,
    setShownView,
  } from "$lib/lsp/actions";
  import { serverFor } from "$lib/lsp/protocol";
  import { settings } from "$lib/stores/settings.svelte";
  import { dap } from "$lib/stores/dap.svelte";
  import { debugExtensions, setBreakpointMarks, setPausedLine } from "$lib/dap/codemirror";

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

  /** Right-click in the editor. */
  function menu(e: MouseEvent) {
    const v = view;
    const t = tab;
    if (!v || !t) return;
    // A right-click outside the selection moves the cursor there first, as editors do.
    const at = v.posAtCoords({ x: e.clientX, y: e.clientY });
    const sel = v.state.selection.main;
    if (at !== null && (at < sel.from || at > sel.to)) v.dispatch({ selection: { anchor: at } });
    const s = v.state.selection.main;
    const text = v.state.sliceDoc(s.from, s.to);
    const word = v.state.wordAt(s.head);
    const name = word ? v.state.sliceDoc(word.from, word.to) : "";
    const project = !t.global && !t.config;
    const server = serverFor(t.path);
    const lspFile = project && !!server && lsp.enabled(server);
    const lines = { from: v.state.doc.lineAt(s.from).number, to: v.state.doc.lineAt(s.to).number, text };
    const d = lspFile ? currentDoc() : null;
    const here = d ? realProblems(problemsAt(v, d)) : [];
    const insert = (value: string) => {
      v.dispatch(v.state.replaceSelection(value));
      v.focus();
    };
    contextMenu.show(e, [
      project &&
        name && [
          { label: `Go to definition of ${name}`, hint: "F12", action: () => goToDefinition(name) },
          lspFile && { label: `Find references to ${name}`, hint: "Shift+F12", action: () => references() },
          lspFile && { label: `Rename ${name}`, hint: "F2", action: () => rename() },
        ],
      lspFile && [
        { label: "Quick fix…", hint: "Ctrl+.", action: () => quickFix() },
        here.length > 0 && {
          label: here.length === 1 ? "Fix with agent" : `Fix these ${here.length} problems with agent`,
          action: () => fixWithAgent(here),
        },
        { label: "Format document", hint: "Shift+Alt+F", action: () => format(false) },
        text && { label: "Format selection", hint: "Ctrl+K Ctrl+F", action: () => format(true) },
      ],
      [
        {
          label: "Cut",
          hint: "Ctrl+X",
          disabled: !text,
          action: async () => (await copyText(text)) && insert(""),
        },
        { label: "Copy", hint: "Ctrl+C", disabled: !text, action: () => copyText(text) },
        { label: "Paste", hint: "Ctrl+V", action: async () => insert(await clipboardText()) },
        {
          label: "Select all",
          hint: "Ctrl+A",
          action: () => (v.dispatch({ selection: { anchor: 0, head: v.state.doc.length } }), v.focus()),
        },
      ],
      project && [
        {
          label: text ? "Add the selection to the message" : "Add this line to the message",
          action: () => addToMessage(t.path, text ? lines : { from: lines.from, to: lines.from, text: "" }),
        },
        { label: "Git: who changed this line", hint: "Alt+B", action: () => blame() },
      ],
    ]);
  }

  /** The shown file as the language servers know it; null for agent config files and files without text. */
  function currentDoc(): Doc | null {
    const t = editor.tabs.find((x) => x.key === shownKey);
    if (!t || t.config || t.global || t.binary || t.tooLarge || t.error) return null;
    return { key: t.key, projectId: t.projectId, path: t.path };
  }

  function note(text: string) {
    blameText = text;
    clearTimeout(blameTimer);
    blameTimer = setTimeout(() => (blameText = ""), 8000);
  }

  /** The word at the cursor (for menus and keys). */
  function wordAtCursor(): string {
    if (!view) return "";
    const w = view.state.wordAt(view.state.selection.main.head);
    return w ? view.state.sliceDoc(w.from, w.to) : "";
  }

  function definitionKey() {
    const name = wordAtCursor();
    if (name) goToDefinition(name);
    return true;
  }

  function references() {
    const d = currentDoc();
    const name = wordAtCursor();
    if (view && d && name) findReferences(view, d, name).then((r) => r && note(r));
    return true;
  }

  function rename() {
    const v = view;
    const d = currentDoc();
    if (!v || !d) return true;
    const w = v.state.wordAt(v.state.selection.main.head);
    if (!w) return true;
    const pos = w.from;
    const old = v.state.sliceDoc(w.from, w.to);
    // Ask the server first: no point typing a new name it cannot apply.
    renameBlocked(d).then((why) => {
      if (why) return note(why);
      contextMenu.ask(`Rename ${old} to`, old, "Rename", async (value: string) => {
        const name = value.trim();
        if (name && name !== old) note(await renameSymbol(v, d, pos, name));
      });
    });
    return true;
  }

  /** Ctrl+. : the language server's fixes at the cursor, and "Fix with agent" for the problems there. */
  function quickFix() {
    const d = currentDoc();
    if (view && d) quickFixMenu(view, d, note);
    return true;
  }

  function format(onlySelection: boolean) {
    const d = currentDoc();
    const v = view;
    if (v && d) formatDocument(v, d, onlySelection).then((r) => note(r || "Already formatted."));
    return true;
  }

  /** Go to definition: the language server first (exact), else the code index: one match opens, several "Go to". */
  async function goToDefinition(name: string) {
    const t = tab;
    if (!t) return;
    const d = currentDoc();
    if (view && d) {
      const r = await lspDefinition(view, d, name);
      if (r === true) return;
      if (r) note(r);
    }
    try {
      const found = (await api.index.find(t.projectId, name)).filter((d) => d.name === name);
      if (found.length === 1) await editor.open(t.projectId, found[0].path, found[0].line);
      else {
        ui.quickOpenText = `#${name}`;
        ui.quickOpen = true;
      }
    } catch (err) {
      blameText = errorText(err);
    }
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
    const v = view;
    const key = shownKey;
    if (!v || !key) return true;
    const d = currentDoc();
    const s = d ? serverFor(d.path) : null;
    // Format on save (Settings, Languages): only when the file's server already runs, and at most 3 s, so a slow
    // or missing server never holds the save back.
    if (d && s && settings.value.languages.formatOnSave && lsp.state[`${d.projectId}::${s}`] === "ready") {
      formatDocument(v, d, false, 3000, true).then((r) => {
        if (r && r !== "Formatted.") note(`Saved without formatting: ${r}`);
        if (view === v && shownKey === key) editor.save(key, v.state.doc.toString());
      });
      return true;
    }
    editor.save(key, v.state.doc.toString());
    return true;
  }

  /** F9: a breakpoint on the cursor's line, or none. */
  function toggleBreakpoint(line?: number) {
    const d = currentDoc();
    if (!view || !d) return true;
    dap.toggleBreakpoint(d.projectId, d.path, line ?? view.state.doc.lineAt(view.state.selection.main.head).number);
    return true;
  }

  /** The shown file's breakpoints and paused line, from the debug store. */
  function showDebug() {
    const d = currentDoc();
    if (!view || !d) return;
    const here = dap.current && dap.current.projectId === d.projectId && dap.current.path === d.path;
    view.dispatch({
      effects: [
        setBreakpointMarks.of(dap.shownBreakpoints(d.projectId, d.path)),
        setPausedLine.of(here ? dap.current!.line : null),
      ],
    });
  }

  function extensions() {
    return [
      // Before basicSetup, so the breakpoint gutter sits left of the line numbers.
      debugExtensions(
        (line) => toggleBreakpoint(line),
        (lines) => {
          const d = currentDoc();
          if (d) dap.moveBreakpoints(d.projectId, d.path, lines);
        },
      ),
      basicSetup,
      keymap.of([
        indentWithTab,
        { key: "Mod-s", run: save, preventDefault: true },
        { key: "Alt-b", run: blame, preventDefault: true },
        { key: "F12", run: definitionKey, preventDefault: true },
        { key: "Shift-F12", run: references, preventDefault: true },
        { key: "F2", run: rename, preventDefault: true },
        { key: "Mod-.", run: quickFix, preventDefault: true },
        { key: "Shift-Alt-f", run: () => format(false), preventDefault: true },
        { key: "Mod-k Mod-f", run: () => format(true), preventDefault: true },
        { key: "F9", run: () => toggleBreakpoint(), preventDefault: true },
      ]),
      gitGutter(),
      lintGutter(),
      lspExtensions(currentDoc),
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
            const d = currentDoc();
            const key = shownKey;
            const text = () => (shownKey === key && view ? view.state : editor.states.get(key))?.doc.toString() ?? "";
            if (d) lsp.changed(d, text);
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
      // setState runs no update listener: give the status bar this tab's cursor, not the last tab's.
      const head = view.state.selection.main.head;
      const at = view.state.doc.lineAt(head);
      oncursor?.(at.number, head - at.from + 1);
      loadBase(t);
      const ext = await loadLanguage(t.language);
      if (shownKey === t.key && view) view.dispatch({ effects: language.reconfigure(ext) });
      reattach();
      showDebug();
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
    setShownView(() => (view && shownKey ? { view, key: shownKey } : null));
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

  /** Opens the shown file in its language server (started if needed), then shows its problems. */
  function reattach() {
    const d = currentDoc();
    if (!d) return;
    const fallback = editor.tabs.find((x) => x.key === d.key)?.savedText ?? "";
    lsp.attach(d, () => (view && shownKey === d.key ? view.state.doc.toString() : fallback)).then(showProblems);
    showProblems();
  }

  /** The language server's problems for the shown file, as underlines and gutter marks. */
  function showProblems() {
    const d = currentDoc();
    if (!view || !d) return;
    // A hint (rustc's "remove the whole `use` item") is help for a problem, not one of its own: no buttons.
    const buttons = (p: Problem) =>
      p.severity === "hint"
        ? []
        : [
            {
              name: "Quick fix",
              apply: (v: EditorView, from: number) => {
                v.dispatch({ selection: { anchor: from } });
                quickFix();
              },
            },
            { name: "Fix with agent", apply: () => fixWithAgent([p]) },
          ];
    const list = toDiagnostics(view.state, lsp.problemsFor(d.projectId, d.path), buttons);
    view.dispatch(setDiagnostics(view.state, list));
  }

  $effect(() => {
    void lsp.problemsChanged;
    void tab.key;
    untrack(showProblems);
  });

  $effect(() => {
    void dap.breakpointsChanged;
    void dap.current;
    void tab.key;
    untrack(showDebug);
  });

  // Documents of closed tabs are closed in their servers too.
  $effect(() => {
    const keys = editor.tabs.map((t) => t.key);
    untrack(() => lsp.sync(keys));
  });

  let offer = $derived(lsp.offer && !tab.config && serverFor(tab.path) === lsp.offer.server ? lsp.offer : null);

  /** The current text of the shown tab (for Save from the menu or before closing). */
  export function currentText(): string | null {
    return view ? view.state.doc.toString() : null;
  }

  onDestroy(() => {
    setShownView(() => null);
    if (view && shownKey) editor.states.set(shownKey, view.state);
    view?.destroy();
    view = null;
  });
</script>

<div class="code-editor" bind:this={host} oncontextmenu={menu} role="presentation">
  {#if blameText}<div class="blame" role="status">{blameText}</div>{/if}
  {#if offer}
    {@const o = offer}
    <div class="lsp-offer" role="status">
      {#if o.status.install}
        <span
          ><strong>{o.status.serves}:</strong> errors, completion and exact go to definition need {o.status.label}.
          {o.status.install}</span
        >
        <button
          type="button"
          class="primary"
          disabled={lsp.installing !== null}
          onclick={() => lsp.install(o.server).then((ok) => ok && reattach())}
          >{lsp.installing === o.server ? "Installing…" : "Install"}</button
        >
      {:else}
        <span><strong>{o.status.serves}:</strong> {o.status.missing}</span>
      {/if}
      <button type="button" onclick={() => lsp.notNow(o.server)}>Not now</button>
      <button type="button" onclick={() => lsp.never(o.server)}>Don't ask again</button>
      {#if lsp.installError}<span class="error">{lsp.installError}</span>{/if}
    </div>
  {/if}
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

  .lsp-offer {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 10px;
    z-index: 6;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    font-size: var(--qx-text-small);
  }

  .lsp-offer span {
    flex: 1 1 260px;
  }

  .lsp-offer button {
    padding: 3px 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .lsp-offer button.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: var(--qx-bg-app, #fff);
  }

  .lsp-offer .error {
    flex-basis: 100%;
    color: var(--qx-danger, #e5534b);
    white-space: pre-wrap;
  }

  .code-editor :global(.qx-lsp-hover) {
    max-width: 560px;
    max-height: 320px;
    overflow: auto;
    padding: 6px 10px;
    font-size: var(--qx-text-small);
  }

  .code-editor :global(.qx-lsp-hover p) {
    margin: 4px 0;
  }

  .code-editor :global(.qx-lsp-hover pre) {
    margin: 4px 0;
    white-space: pre-wrap;
  }

  .code-editor :global(.cm-editor) {
    height: 100%;
  }

  .code-editor :global(.cm-editor.cm-focused) {
    outline: none;
  }
</style>
