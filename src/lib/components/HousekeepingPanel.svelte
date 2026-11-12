<script lang="ts">
  /**
   * Plan housekeeping (24j): move finished sections to the plan's archive, or add another plan's steps to this one.
   * Shows what will change first; nothing is written until the user confirms.
   */
  import X from "@lucide/svelte/icons/x";
  import Archive from "@lucide/svelte/icons/archive";
  import ListPlus from "@lucide/svelte/icons/list-plus";
  import { api, errorText, type Project } from "$lib/api";
  import {
    archiveFinished,
    archivePath,
    mergePlans,
    stepNumbers,
    type ArchiveResult,
    type MergeResult,
  } from "$lib/plan/housekeeping";

  interface Props {
    project: Project;
    file: string;
    mode: "archive" | "merge";
    /** The project's other task lists (sources to add from). */
    candidates: string[];
    onclose: () => void;
    /** Called after a change was written, so the Plan view reloads. */
    ondone: () => void;
  }

  let { project, file, mode, candidates, onclose, ondone }: Props = $props();

  let others = $derived(candidates.filter((c) => c !== file));
  let source = $state("");
  let onlyOpen = $state(true);
  let error = $state("");
  let busy = $state(false);
  let loading = $state(true);
  let archived = $state<ArchiveResult | null>(null);
  let merged = $state<MergeResult | null>(null);
  let archiveExists = $state(false);

  const read = (path: string) => api.files.read(project.id, path).catch(() => null);

  async function preview() {
    loading = true;
    error = "";
    try {
      const plan = await api.files.read(project.id, file);
      if (mode === "archive") {
        const a = await read(archivePath(file));
        archiveExists = !!a;
        archived = archiveFinished(plan.text, a?.text ?? null, file);
      } else if (source) {
        const s = await api.files.read(project.id, source);
        const taken = stepNumbers((await read(archivePath(file)))?.text);
        merged = mergePlans(plan.text, file, s.text, source, { onlyOpen, taken });
      } else merged = null;
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (mode === "merge" && !source && others.length) source = others[0];
  });

  $effect(() => {
    void mode;
    void file;
    void source;
    void onlyOpen;
    void preview();
  });

  /** Writes the change worked out again from the files as they are now (they may have changed since the preview). */
  async function confirm() {
    busy = true;
    error = "";
    try {
      const plan = await api.files.read(project.id, file);
      if (mode === "archive") {
        const a = await read(archivePath(file));
        const r = archiveFinished(plan.text, a?.text ?? null, file);
        if (!r) throw new Error("No section is finished any more.");
        // The archive first: if writing the plan then fails, the steps are in both files, never in neither.
        await api.files.write(project.id, archivePath(file), r.archive, a?.hash ?? null);
        await api.files.write(project.id, file, r.plan, plan.hash);
      } else {
        const s = await api.files.read(project.id, source);
        const taken = stepNumbers((await read(archivePath(file)))?.text);
        const r = mergePlans(plan.text, file, s.text, source, { onlyOpen, taken });
        if (!r) throw new Error("There are no steps to add.");
        await api.files.write(project.id, file, r.text, plan.hash);
      }
      ondone();
      onclose();
    } catch (e) {
      error = errorText(e);
      await preview();
    } finally {
      busy = false;
    }
  }
</script>

<div class="panel">
  <div class="head">
    {#if mode === "archive"}
      <strong><Archive size={14} /> Archive finished sections</strong>
    {:else}
      <strong><ListPlus size={14} /> Add steps from another plan</strong>
    {/if}
    <button type="button" class="x" aria-label="Close" onclick={onclose}><X size={14} /></button>
  </div>

  {#if mode === "merge"}
    {#if others.length === 0}
      <p class="hint">There is no other task list in this project to add from.</p>
    {:else}
      <div class="row">
        <label>
          From
          <select bind:value={source}>
            {#each others as o (o)}<option value={o}>{o}</option>{/each}
          </select>
        </label>
        <label class="check"><input type="checkbox" bind:checked={onlyOpen} /> Only steps not done</label>
      </div>
    {/if}
  {/if}

  {#if loading}
    <p class="hint">Reading…</p>
  {:else if mode === "archive"}
    {#if !archived}
      <p class="hint">No section has all its steps done; nothing to move.</p>
    {:else}
      <p class="hint">
        {archived.sections.length} section{archived.sections.length === 1 ? "" : "s"} move{archived.sections.length ===
        1
          ? "s"
          : ""}
        to {archivePath(file)}{archiveExists ? " (added at its end)" : " (a new file)"}, with the text under each
        heading. Plan runs still count their steps as done.
      </p>
      <ul class="list">
        {#each archived.sections as s, i (i)}
          <li><span class="t">{s.title}</span> <span class="meta">{s.steps} done</span></li>
        {/each}
      </ul>
    {/if}
  {:else if source}
    {#if !merged}
      <p class="hint">{source} has no {onlyOpen ? "open " : ""}steps to add.</p>
    {:else}
      <p class="hint">
        {merged.added} step{merged.added === 1 ? "" : "s"} go to the end of {file} under their own headings{merged.leftOut
          ? `; ${merged.leftOut} done left out`
          : ""}. {source} itself stays as it is.
      </p>
      <ul class="list">
        {#each merged.sections as s, i (i)}
          <li><span class="t">{s.title}</span> <span class="meta">{s.steps} step{s.steps === 1 ? "" : "s"}</span></li>
        {/each}
      </ul>
      {#if merged.renumbered.length}
        <p class="hint">
          Numbers {file} already uses get new ones (their Needs lines follow):
          {merged.renumbered.map(([a, b]) => `${a} → ${b}`).join(", ")}.
        </p>
      {/if}
    {/if}
  {/if}

  <div class="row">
    <span class="spacer"></span>
    <button type="button" onclick={onclose}>Cancel</button>
    <button
      type="button"
      class="primary"
      disabled={busy || loading || (mode === "archive" ? !archived : !merged)}
      onclick={confirm}
    >
      {mode === "archive" ? "Move to the archive" : `Add to ${file}`}
    </button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
</div>

<style>
  .panel {
    margin-bottom: var(--qx-space-3);
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-accent) 40%, var(--qx-border));
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--qx-space-2);
  }

  .head strong {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .x {
    border: 0;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  select {
    height: 28px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
  }

  .row {
    display: flex;
    align-items: flex-end;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-2);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  label.check {
    flex-direction: row;
    align-items: center;
    height: 28px;
  }

  .hint {
    margin: var(--qx-space-2) 0;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .list {
    margin: 0;
    padding-left: var(--qx-space-6);
  }

  .t {
    font-weight: 500;
  }

  .meta {
    margin-left: var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .spacer {
    flex: 1;
  }

  button {
    height: 28px;
    padding: 0 var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  button.primary {
    border-color: transparent;
    background: var(--qx-accent);
    color: #fff;
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .error {
    margin: var(--qx-space-2) 0 0;
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }
</style>
