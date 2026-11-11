<script lang="ts">
  /**
   * Source Control, as in VS Code (Milestone 15b): commit message and commit, staged and unstaged changes, merge
   * conflicts, branches, pull / push / fetch, stash and history. Everything runs through the installed git.
   */
  import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import CloudDownload from "@lucide/svelte/icons/cloud-download";
  import Plus from "@lucide/svelte/icons/plus";
  import Minus from "@lucide/svelte/icons/minus";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Archive from "@lucide/svelte/icons/archive";
  import History from "@lucide/svelte/icons/clock";
  import X from "@lucide/svelte/icons/x";
  import { ask } from "@tauri-apps/plugin-dialog";
  import {
    api,
    errorText,
    type GitBranch as Branch,
    type GitChange,
    type GitCommit,
    type GitStash,
    type GitStatus,
    type Project,
  } from "$lib/api";
  import { letterFor } from "$lib/git/decorations";
  import { editor } from "$lib/stores/editor.svelte";
  import { gitState } from "$lib/stores/git.svelte";
  import GitDiff from "./GitDiff.svelte";
  import { scmView } from "$lib/stores/scmView.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
    /** Changes when something may have changed files (an agent turn ended), to refresh the lists. */
    refreshToken?: number;
    /** "list": the lists only (the project panel); "main": the diff and history only (the work area). */
    part?: "both" | "list" | "main";
  }

  let { project, refreshToken = 0, part = "both" }: Props = $props();

  let status = $state<GitStatus | null>(null);
  let message = $state("");
  let amend = $state(false);
  let busy = $state("");
  let error = $state("");
  let notice = $state("");
  let menu = $state<"" | "branches" | "stash">("");
  let branches = $state<Branch[]>([]);
  let newBranch = $state("");
  let stashes = $state<GitStash[]>([]);
  let stashMessage = $state("");
  let commits = $state<GitCommit[]>([]);
  let commitText = $state("");
  let commitSha = $state("");

  const conflicted = (c: GitChange) => letterFor(c) === "!";
  let changes = $derived(status?.changes ?? []);
  let merge = $derived(changes.filter(conflicted));
  let staged = $derived(changes.filter((c) => c.staged && !conflicted(c)));
  let unstaged = $derived(changes.filter((c) => !conflicted(c) && (c.untracked || c.status[1] !== " ")));
  let selectedChange = $derived(scmView.selected ? changes.find((c) => c.path === scmView.selected!.path) : undefined);

  async function refresh() {
    try {
      status = await api.git.status(project.id);
      error = "";
      const s = scmView.selected;
      if (s) {
        const c = status.changes.find((x) => x.path === s.path);
        const still = c && (conflicted(c) || (s.side === "staged" ? c.staged : c.untracked || c.status[1] !== " "));
        if (!still) {
          // The other side of the same file, if it has one (after staging all of it, show the staged change).
          if (c) scmView.selected = { path: s.path, side: s.side === "staged" ? "changes" : "staged" };
          else scmView.selected = null;
        }
      }
    } catch (e) {
      error = errorText(e);
    }
  }

  $effect(() => {
    void project.id;
    scmView.selected = null;
    scmView.historyFor = null;
    menu = "";
  });

  $effect(() => {
    void project.id;
    void refreshToken;
    void gitState.changed;
    refresh();
  });

  async function run(label: string, action: () => Promise<string | void>) {
    busy = label;
    error = "";
    notice = "";
    try {
      const said = await action();
      if (said) notice = said;
      gitState.touch();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  const stage = (c: GitChange) => run("Staging…", () => api.git.stage(project.id, c.path));
  const unstage = (c: GitChange) => run("Unstaging…", () => api.git.unstage(project.id, c.path));
  const stageAll = () => run("Staging…", () => api.git.stageAll(project.id));
  const unstageAll = () => run("Unstaging…", () => api.git.unstage(project.id, "."));

  async function revert(c: GitChange) {
    if (c.untracked) {
      error = "This file is new and not in git yet; delete it yourself if you do not want it.";
      return;
    }
    const ok = await ask(`Undo all changes to ${c.path} and go back to the last commit? This cannot be undone.`, {
      title: "Undo changes",
      kind: "warning",
      okLabel: "Undo changes",
      cancelLabel: "Keep them",
    });
    if (!ok) return;
    await run("Undoing…", async () => {
      await api.git.revert(project.id, c.path);
      const tab = editor.tabs.find((t) => t.projectId === project.id && t.path === c.path);
      if (tab && !tab.dirty) await editor.reload(tab.key);
    });
  }

  async function commit() {
    if (!message.trim() && !amend) {
      error = "Write a commit message first.";
      return;
    }
    if (staged.length === 0 && !amend) {
      if (unstaged.length === 0) {
        error = "There is nothing to commit.";
        return;
      }
      const ok = await ask("Nothing is staged. Stage all changes and commit them?", {
        title: "Commit",
        okLabel: "Stage all and commit",
        cancelLabel: "Cancel",
      });
      if (!ok) return;
      await api.git.stageAll(project.id).catch((e) => (error = errorText(e)));
      if (error) return;
    }
    await run("Committing…", async () => {
      const said = await api.git.commit(project.id, message, amend, false);
      message = "";
      amend = false;
      return said;
    });
  }

  const sync = (action: "fetch" | "pull" | "push") =>
    run(action === "fetch" ? "Fetching…" : action === "pull" ? "Pulling…" : "Pushing…", () =>
      api.git.sync(project.id, action),
    );

  async function openMenu(which: "branches" | "stash") {
    if (menu === which) {
      menu = "";
      return;
    }
    menu = which;
    try {
      if (which === "branches") branches = await api.git.branches(project.id);
      else stashes = await api.git.stashes(project.id);
    } catch (e) {
      error = errorText(e);
    }
  }

  async function switchTo(name: string, create = false) {
    await run(create ? "Creating the branch…" : "Switching…", async () => {
      await api.git.switch(project.id, name, create);
      menu = "";
      newBranch = "";
      // Open files may differ on the other branch.
      for (const t of editor.tabs.filter((t) => t.projectId === project.id && !t.dirty && !t.config))
        await editor.reload(t.key).catch(() => {});
      return `On ${name} now.`;
    });
  }

  async function stashAction(action: "push" | "pop" | "apply" | "drop", s?: GitStash) {
    if (action === "drop") {
      const ok = await ask(`Delete the stash "${s?.message}"? This cannot be undone.`, {
        title: "Delete stash",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Keep it",
      });
      if (!ok) return;
    }
    await run("Working…", async () => {
      await api.git.stash(project.id, action, s?.index ?? 0, stashMessage);
      stashMessage = "";
      stashes = await api.git.stashes(project.id);
      if (action !== "drop")
        for (const t of editor.tabs.filter((t) => t.projectId === project.id && !t.dirty && !t.config))
          await editor.reload(t.key).catch(() => {});
    });
  }

  function showHistory(path: string | null) {
    scmView.selected = null;
    scmView.historyFor = path ?? "";
    if (part === "list") ui.show("diff");
  }

  // The part that shows the history loads it.
  $effect(() => {
    const h = scmView.historyFor;
    if (part === "list" || h === null) return;
    commitText = "";
    commitSha = "";
    api.git
      .log(project.id, h || null, 100)
      .then((list) => (commits = list))
      .catch((e) => (error = errorText(e)));
  });

  async function showCommit(c: GitCommit) {
    commitSha = c.sha;
    try {
      commitText = await api.git.show(project.id, c.sha, scmView.historyFor || null);
    } catch (e) {
      commitText = errorText(e);
    }
  }

  function lineClass(line: string) {
    if (line.startsWith("+++") || line.startsWith("---")) return "meta";
    if (line.startsWith("@@")) return "hunk";
    if (line.startsWith("+")) return "add";
    if (line.startsWith("-")) return "del";
    if (line.startsWith("diff ") || line.startsWith("index ")) return "meta";
    return "";
  }

  const name = (p: string) => p.split("/").pop();
  const dir = (p: string) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");
  const letter = (c: GitChange, side: "staged" | "changes") =>
    side === "changes" && c.untracked ? "U" : letterFor(c) === "U" ? "A" : letterFor(c);
</script>

{#snippet row(c: GitChange, side: "staged" | "changes" | "merge")}
  {@const s = side === "merge" ? "changes" : side}
  {@const l = side === "merge" ? "!" : letter(c, s)}
  <li class:selected={scmView.selected?.path === c.path && scmView.selected?.side === s}>
    <button
      type="button"
      class="file"
      title={c.path}
      onclick={() => {
        scmView.historyFor = null;
        scmView.selected = { path: c.path, side: s };
        if (part === "list") ui.show("diff");
      }}
      oncontextmenu={(e) =>
        contextMenu.show(e, [
          [
            {
              label: "Open changes",
              action: () => {
                scmView.historyFor = null;
                scmView.selected = { path: c.path, side: s };
                ui.show("diff");
              },
            },
            { label: "Open file", disabled: c.deleted, action: () => editor.open(project.id, c.path) },
          ],
          [
            s === "staged"
              ? { label: "Unstage", action: () => unstage(c) }
              : { label: "Stage", action: () => stage(c) },
            s !== "staged" && { label: "Discard changes", danger: true, action: () => revert(c) },
          ],
          [
            { label: "History of this file", action: () => showHistory(c.path) },
            { label: "Copy relative path", action: () => copyText(c.path) },
          ],
        ])}
    >
      <span class="badge g{l === '!' ? 'C' : l}">{l}</span>
      <span class="name">{name(c.path)}</span>
      <span class="dir">{dir(c.path)}</span>
    </button>
    <span class="actions">
      {#if side === "staged"}
        <IconButton label="Unstage" disabled={!!busy} onclick={() => unstage(c)}><Minus size={13} /></IconButton>
      {:else if side === "changes"}
        {#if !c.untracked}
          <IconButton label="Undo changes" disabled={!!busy} onclick={() => revert(c)}><Undo2 size={13} /></IconButton>
        {/if}
        <IconButton label="Stage" disabled={!!busy} onclick={() => stage(c)}><Plus size={13} /></IconButton>
      {/if}
    </span>
  </li>
{/snippet}

<div class="scm-wrap">
  <div class="scm" class:single={part !== "both"}>
    {#if part !== "main"}
      <aside>
        <header>
          {#if status?.is_repo}
            <button type="button" class="branch" title="Branches" onclick={() => openMenu("branches")}>
              <GitBranch size={14} />
              <span>{status.branch || "detached"}</span>
            </button>
            {#if status.upstream}
              <span class="sync" title="Behind / ahead of {status.upstream}">
                <ArrowDown size={12} />{status.behind}
                <ArrowUp size={12} />{status.ahead}
              </span>
            {/if}
          {:else}
            <span class="muted">{status ? "Not a git repository" : "…"}</span>
          {/if}
          <span class="spacer"></span>
          {#if status?.is_repo}
            <IconButton label="Pull" disabled={!!busy} onclick={() => sync("pull")}><ArrowDown size={13} /></IconButton>
            <IconButton label="Push" disabled={!!busy} onclick={() => sync("push")}><ArrowUp size={13} /></IconButton>
            <IconButton label="Fetch" disabled={!!busy} onclick={() => sync("fetch")}>
              <CloudDownload size={13} />
            </IconButton>
            <IconButton label="Stash" active={menu === "stash"} onclick={() => openMenu("stash")}>
              <Archive size={13} />
            </IconButton>
            <IconButton label="History of the project" onclick={() => showHistory(null)}
              ><History size={13} /></IconButton
            >
          {/if}
          <IconButton label="Refresh" onclick={refresh}><RefreshCw size={13} /></IconButton>
        </header>

        {#if menu === "branches"}
          <div class="menu">
            <form
              onsubmit={(e) => {
                e.preventDefault();
                if (newBranch.trim()) switchTo(newBranch.trim(), true);
              }}
            >
              <input bind:value={newBranch} placeholder="New branch name" spellcheck="false" />
              <button type="submit" class="small" disabled={!newBranch.trim() || !!busy}>Create</button>
            </form>
            <ul>
              {#each branches as br (br.name)}
                <li>
                  <button type="button" disabled={br.current || !!busy} onclick={() => switchTo(br.name)}>
                    <span class:current={br.current}>{br.name}</span>
                    {#if br.remote}<span class="muted">remote</span>{/if}
                    {#if br.current}<span class="muted">current</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        {:else if menu === "stash"}
          <div class="menu">
            <form
              onsubmit={(e) => {
                e.preventDefault();
                stashAction("push");
              }}
            >
              <input bind:value={stashMessage} placeholder="Stash message (optional)" spellcheck="false" />
              <button type="submit" class="small" disabled={changes.length === 0 || !!busy}>Stash changes</button>
            </form>
            <ul>
              {#each stashes as s (s.index)}
                <li class="stash">
                  <span class="stash-msg" title={s.message}>{s.message}</span>
                  <button type="button" class="small" disabled={!!busy} onclick={() => stashAction("pop", s)}
                    >Pop</button
                  >
                  <button type="button" class="small" disabled={!!busy} onclick={() => stashAction("apply", s)}
                    >Apply</button
                  >
                  <button type="button" class="small" disabled={!!busy} onclick={() => stashAction("drop", s)}
                    >Delete</button
                  >
                </li>
              {:else}
                <li class="muted pad">No stashes.</li>
              {/each}
            </ul>
          </div>
        {/if}

        {#if status?.is_repo}
          <div class="commit">
            <textarea
              bind:value={message}
              placeholder="Message (Ctrl+Enter to commit)"
              rows="3"
              spellcheck="true"
              onkeydown={(e) => {
                if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                  e.preventDefault();
                  commit();
                }
              }}></textarea>
            <div class="commit-row">
              <label><input type="checkbox" bind:checked={amend} /> Amend last commit</label>
              <button type="button" class="primary" disabled={!!busy} onclick={commit}>
                {amend ? "Amend" : staged.length ? "Commit" : "Commit all"}
              </button>
            </div>
          </div>
        {/if}
        {#if busy}<p class="note">{busy}</p>{/if}
        {#if error}<p class="error">{error}</p>{/if}
        {#if notice}<p class="note">{notice}</p>{/if}

        <div class="lists">
          {#if merge.length}
            <h4>Merge changes <span class="n">{merge.length}</span></h4>
            <ul>
              {#each merge as c (c.path)}{@render row(c, "merge")}{/each}
            </ul>
          {/if}
          {#if staged.length}
            <h4>
              Staged changes <span class="n">{staged.length}</span>
              <span class="spacer"></span>
              <IconButton label="Unstage all" disabled={!!busy} onclick={unstageAll}><Minus size={13} /></IconButton>
            </h4>
            <ul>
              {#each staged as c (c.path)}{@render row(c, "staged")}{/each}
            </ul>
          {/if}
          {#if status?.is_repo}
            <h4>
              Changes <span class="n">{unstaged.length}</span>
              <span class="spacer"></span>
              {#if unstaged.length}
                <IconButton label="Stage all" disabled={!!busy} onclick={stageAll}><Plus size={13} /></IconButton>
              {/if}
            </h4>
            <ul>
              {#each unstaged as c (c.path)}{@render row(c, "changes")}{:else}
                <li class="muted pad">No changes.</li>
              {/each}
            </ul>
          {/if}
        </div>
      </aside>
    {/if}

    {#if part !== "list"}
      <section class="main">
        {#if scmView.historyFor !== null}
          <div class="history">
            <header>
              <History size={14} />
              <span class="title">History of {scmView.historyFor || "the project"}</span>
              <span class="spacer"></span>
              <IconButton label="Close" onclick={() => (scmView.historyFor = null)}><X size={13} /></IconButton>
            </header>
            <div class="history-body">
              <ul class="commits">
                {#each commits as c (c.sha)}
                  <li class:selected={commitSha === c.sha}>
                    <button type="button" onclick={() => showCommit(c)}>
                      <span class="subject">{c.subject}</span>
                      <span class="muted">{c.short} · {c.author} · {c.date}</span>
                    </button>
                  </li>
                {:else}
                  <li class="muted pad">No commits yet.</li>
                {/each}
              </ul>
              <pre class="patch">{#each commitText.split(/\r?\n/) as line, i (i)}<span class={lineClass(line)}
                    >{line || " "}</span
                  >
                {/each}</pre>
            </div>
          </div>
        {:else if scmView.selected && selectedChange}
          <GitDiff
            {project}
            change={selectedChange}
            side={scmView.selected.side}
            onchanged={refresh}
            onhistory={(p) => showHistory(p)}
          />
        {:else}
          <p class="empty">
            {status?.is_repo
              ? changes.length
                ? "Choose a file to see its changes."
                : "No changes since the last commit."
              : "This project is not a git repository."}
          </p>
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  .scm {
    height: 100%;
    display: grid;
    grid-template-columns: minmax(220px, 300px) 1fr;
    min-height: 0;
  }

  .scm.single {
    grid-template-columns: 1fr;
    grid-template-rows: 1fr;
  }

  .scm.single aside {
    border-right: 0;
  }

  .scm-wrap {
    height: 100%;
    container-type: inline-size;
  }

  /* A narrow pane: the lists above, the diff below. */
  @container (max-width: 640px) {
    .scm {
      grid-template-columns: 1fr;
      grid-template-rows: minmax(0, 45%) minmax(0, 1fr);
    }

    aside {
      border-right: 0;
      border-bottom: 1px solid var(--qx-border);
    }
  }

  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--qx-border);
    font-size: var(--qx-text-small);
  }

  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
    font-size: var(--qx-text-small);
  }

  .branch {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    padding: 2px 6px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: none;
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .branch span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .branch:hover {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
  }

  .sync {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    color: var(--qx-text-muted);
  }

  .spacer {
    flex: 1;
  }

  .muted {
    color: var(--qx-text-muted);
  }

  .pad {
    padding: 4px var(--qx-space-3);
  }

  .menu {
    flex: none;
    max-height: 240px;
    overflow: auto;
    border-bottom: 1px solid var(--qx-border);
    background: var(--qx-bg-window);
  }

  .menu form {
    display: flex;
    gap: 4px;
    padding: 6px var(--qx-space-2);
  }

  input:not([type]),
  textarea {
    flex: 1;
    min-width: 0;
    padding: 4px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .menu ul,
  .lists ul,
  .commits {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .menu li button {
    display: flex;
    gap: 8px;
    width: 100%;
    padding: 4px var(--qx-space-3);
    border: 0;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .menu li button:hover:not(:disabled) {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
  }

  .menu .current {
    font-weight: 600;
  }

  .stash {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px var(--qx-space-2);
  }

  .stash-msg {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .small,
  .primary {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .small:disabled,
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .commit {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
  }

  textarea {
    resize: vertical;
  }

  .commit-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    color: var(--qx-text-secondary);
  }

  .note,
  .error {
    margin: 0;
    padding: 4px var(--qx-space-3);
  }

  .note {
    color: var(--qx-text-secondary);
  }

  .error {
    color: var(--qx-status-failed);
  }

  .lists {
    flex: 1;
    overflow: auto;
    padding-bottom: var(--qx-space-2);
  }

  h4 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    padding: 6px var(--qx-space-2) 2px;
    font-size: var(--qx-text-small);
    font-weight: 600;
    color: var(--qx-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .n {
    color: var(--qx-text-muted);
    font-weight: 400;
  }

  .lists li {
    display: flex;
    align-items: center;
  }

  .lists li.selected {
    background: var(--qx-bg-selected, rgba(55, 148, 255, 0.15));
  }

  .file {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
    padding: 3px var(--qx-space-2);
    border: 0;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .name {
    white-space: nowrap;
  }

  .dir {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--qx-text-muted);
  }

  .actions {
    display: none;
    padding-right: 4px;
  }

  .lists li:hover .actions,
  .lists li.selected .actions {
    display: flex;
  }

  .badge {
    flex: none;
    width: 12px;
    font-weight: 600;
    text-align: center;
  }

  .gM {
    color: #e2c08d;
  }

  .gA,
  .gU {
    color: #73c991;
  }

  .gD {
    color: #f85149;
  }

  .gR {
    color: #73c991;
  }

  .gC {
    color: #e5534b;
  }

  .main {
    min-width: 0;
    min-height: 0;
  }

  .empty {
    margin: 0;
    padding: var(--qx-space-4, 24px);
    color: var(--qx-text-muted);
    text-align: center;
  }

  .history {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .history .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .history-body {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(200px, 320px) 1fr;
  }

  .commits {
    overflow: auto;
    border-right: 1px solid var(--qx-border);
    font-size: var(--qx-text-small);
  }

  .commits button {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 5px var(--qx-space-3);
    border: 0;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .commits li.selected button,
  .commits button:hover {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
  }

  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .patch {
    margin: 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    overflow: auto;
    font-family: var(--qx-font-mono, monospace);
    font-size: var(--qx-text-small);
    user-select: text;
  }

  .patch span {
    display: block;
    white-space: pre;
  }

  .patch .add {
    color: #73c991;
    background: rgba(46, 160, 67, 0.12);
  }

  .patch .del {
    color: #f85149;
    background: rgba(248, 81, 73, 0.12);
  }

  .patch .hunk {
    color: #3794ff;
  }

  .patch .meta {
    color: var(--qx-text-muted);
  }
</style>
