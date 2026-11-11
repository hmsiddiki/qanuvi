<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorText, type Project } from "$lib/api";
  import { editor } from "$lib/stores/editor.svelte";
  import { fuzzyFilter, fuzzyScore } from "$lib/fuzzy";

  interface Props {
    project: Project;
    /** When given, the chosen path is handed here instead of being opened. */
    onpick?: ((path: string) => void) | null;
    /** Text to start with: "#" for symbols in the project, "@" for symbols in the current file. */
    initial?: string;
    onclose: () => void;
  }

  let { project, onpick = null, initial = "", onclose }: Props = $props();

  interface Item {
    key: string;
    title: string;
    detail: string;
    kind?: string;
    path: string;
    line: number | null;
  }

  // Starts with the given text once; after that the box belongs to the user.
  let query = $state(untrack(() => initial));
  let files = $state<string[]>([]);
  let symbols = $state<Item[]>([]);
  let note = $state("Loading files…");
  let selected = $state(0);
  let input: HTMLInputElement;

  /** The file shown in the editor, for "@" (symbols in this file). */
  let activeFile = $derived(
    editor.active && !editor.active.config && editor.active.projectId === project.id ? editor.active.path : null,
  );
  let mode = $derived(onpick ? "files" : query.startsWith("#") ? "project" : query.startsWith("@") ? "file" : "files");

  $effect(() => {
    input?.focus();
    api.files
      .listAll(project.id)
      .then((r) => {
        files = r.files;
        note = r.truncated ? "Showing the first 50,000 files." : "";
      })
      .catch((e) => (note = errorText(e)));
  });

  // Symbols: across the project for "#name", in the current file for "@name".
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const m = mode;
    const q = query.slice(1).trim();
    clearTimeout(timer);
    if (m === "project") {
      if (!q) {
        symbols = [];
        return;
      }
      timer = setTimeout(async () => {
        try {
          const hits = await api.index.find(project.id, q);
          symbols = hits.map((h) => ({
            key: `${h.path}:${h.line}:${h.name}`,
            title: h.name,
            detail: `${h.kind}${h.container ? ` in ${h.container}` : ""} · ${h.path}:${h.line}`,
            kind: h.kind,
            path: h.path,
            line: h.line,
          }));
        } catch (e) {
          note = errorText(e);
        }
      }, 120);
    } else if (m === "file") {
      const path = activeFile;
      if (!path) {
        symbols = [];
        return;
      }
      api.index
        .outline(project.id, path)
        .then((list) => {
          const items = list.map((s) => ({
            key: `${s.line}:${s.name}`,
            title: s.name,
            detail: `${s.kind}${s.container ? ` in ${s.container}` : ""} · line ${s.line}`,
            kind: s.kind,
            path,
            line: s.line,
          }));
          symbols = q ? items.filter((i) => fuzzyScore(q, i.title) !== null) : items;
        })
        .catch((e) => (note = errorText(e)));
    }
  });

  let results = $derived<Item[]>(
    mode === "files"
      ? fuzzyFilter(query, files).map((path) => {
          const slash = path.lastIndexOf("/");
          return {
            key: path,
            title: path.slice(slash + 1),
            detail: slash > 0 ? path.slice(0, slash) : "",
            path,
            line: null,
          };
        })
      : symbols,
  );

  $effect(() => {
    void query;
    selected = 0;
  });

  function choose(item: Item | undefined) {
    if (!item) return;
    if (onpick) onpick(item.path);
    else editor.open(project.id, item.path, item.line);
    onclose();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") onclose();
    else if (event.key === "ArrowDown") selected = Math.min(selected + 1, results.length - 1);
    else if (event.key === "ArrowUp") selected = Math.max(selected - 1, 0);
    else if (event.key === "Enter") choose(results[selected]);
    else return;
    event.preventDefault();
  }

  let placeholder = $derived(
    onpick ? "Mention a file…" : "Go to file…   # symbol in the project   @ symbol in this file",
  );
  let emptyNote = $derived(
    mode === "file" && !activeFile
      ? "Open a file first to see its symbols."
      : mode === "project" && query.length < 2
        ? "Type a name after #."
        : "Nothing found.",
  );
</script>

<div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="palette" role="dialog" aria-label="Go to file or symbol">
    <input bind:this={input} bind:value={query} onkeydown={keydown} {placeholder} spellcheck="false" />
    <ul>
      {#each results as item, i (item.key)}
        <li>
          <button
            type="button"
            class:selected={i === selected}
            onclick={() => choose(item)}
            onmouseenter={() => (selected = i)}
          >
            {#if item.kind}<span class="kind k-{item.kind}">{item.kind.slice(0, 1).toUpperCase()}</span>{/if}
            <span class="name">{item.title}</span>
            <span class="dir">{item.detail}</span>
          </button>
        </li>
      {/each}
    </ul>
    {#if note && mode === "files"}<p class="note">{note}</p>{:else if results.length === 0}<p class="note">
        {emptyNote}
      </p>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    justify-content: center;
    padding-top: 72px;
    background: rgb(0 0 0 / 25%);
  }

  .palette {
    width: min(640px, calc(100% - 32px));
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    box-shadow: 0 18px 48px rgb(0 0 0 / 50%);
    overflow: hidden;
  }

  input {
    margin: var(--qx-space-2);
    height: 34px;
    padding: 0 var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-input);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    outline: none;
    user-select: text;
  }

  input:focus {
    border-color: var(--qx-accent);
  }

  ul {
    margin: 0;
    padding: 0 var(--qx-space-2) var(--qx-space-2);
    list-style: none;
    overflow: auto;
  }

  li button {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: var(--qx-space-2);
    padding: 5px var(--qx-space-2);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  li button.selected {
    background: color-mix(in srgb, var(--qx-accent) 28%, transparent);
  }

  .kind {
    flex: none;
    width: 16px;
    border-radius: 4px;
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
    font-family: var(--qx-font-mono);
    font-size: 10px;
    font-weight: 700;
    text-align: center;
  }

  .k-class,
  .k-struct,
  .k-interface,
  .k-trait,
  .k-enum,
  .k-impl {
    color: #e5c07b;
  }

  .k-function,
  .k-method {
    color: #c79bf2;
  }

  .dir {
    overflow: hidden;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .note {
    margin: 0;
    padding: 0 var(--qx-space-4) var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }
</style>
