<script lang="ts">
  import { shownError } from "$lib/stores/linkState.svelte";
  /**
   * The user's memory (Milestone 17): rules, style, facts, decisions, solutions and approved examples. Every entry is in
   * one of three parts (24z3 D8): About me and Tools and models are account memory (they go to the user's other machines
   * through the memory disk), This project stays on the machine that has the project. Agents get only what fits their
   * request; this view shows, edits and tries that.
   */
  import Brain from "@lucide/svelte/icons/brain";
  import Plus from "@lucide/svelte/icons/plus";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash from "@lucide/svelte/icons/trash";
  import { ask } from "$lib/stores/confirm.svelte";
  import { api, errorText, type MemoryEntry } from "$lib/api";
  import { memory } from "$lib/stores/memory.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { shownMachine } from "$lib/remote";
  import IconButton from "./IconButton.svelte";
  import MemoryDiskPanel from "./MemoryDiskPanel.svelte";

  const KINDS: { id: MemoryEntry["kind"]; label: string; hint: string }[] = [
    { id: "rule", label: "Rule", hint: "Always followed (sent with every request)" },
    { id: "style", label: "Style", hint: "How to write or code (sent with every request)" },
    { id: "fact", label: "Fact", hint: "Something true about a project or you" },
    { id: "decision", label: "Decision", hint: "A choice made, and why" },
    { id: "solution", label: "Solution", hint: "How a problem was solved" },
    { id: "example", label: "Example", hint: "An approved example to follow" },
  ];

  const PARTS: { id: MemoryEntry["part"]; label: string; hint: string }[] = [
    { id: "me", label: "About me", hint: "You and how you work: for every project, on all your computers" },
    { id: "project", label: "This project", hint: "Only this project: stays on the computer that has it" },
    { id: "tools", label: "Tools and models", hint: "Your tools, models and set-up: on all your computers" },
  ];

  let entries = $state<MemoryEntry[]>([]);
  let filter = $state<"" | MemoryEntry["kind"]>("");
  let partFilter = $state<"" | MemoryEntry["part"]>("");
  let query = $state("");
  let error = $state("");
  let editing = $state<Partial<MemoryEntry> | null>(null);
  let tryText = $state("");
  let tried = $state<{ text: string; left_out: number } | null>(null);

  let project = $derived(projects.current);
  let suggested = $derived(entries.filter((e) => e.status === "suggested"));
  let shown = $derived(
    entries
      .filter((e) => e.status === "active")
      .filter((e) => !partFilter || e.part === partFilter)
      .filter((e) => !filter || e.kind === filter)
      .filter((e) => {
        const q = query.trim().toLowerCase();
        return !q || `${e.title} ${e.body} ${e.tags}`.toLowerCase().includes(q);
      }),
  );

  let groups = $derived(
    PARTS.map((p) => ({ ...p, entries: shown.filter((e) => e.part === p.id) })).filter((g) => g.entries.length),
  );

  async function load() {
    const asked = shownMachine();
    try {
      const got = await api.memory.list(project?.id ?? null, true);
      // Entries from the machine shown before a switch are dropped (24z3 D7).
      if (shownMachine() !== asked) return;
      entries = got;
      error = "";
    } catch (e) {
      if (shownMachine() !== asked) return;
      entries = [];
      error = errorText(e);
    }
  }

  $effect(() => {
    void project?.id;
    void memory.changed;
    load();
  });

  function startNew() {
    editing = {
      part: project ? "project" : "me",
      kind: "fact",
      scope: project?.id ?? "",
      title: "",
      body: "",
      tags: "",
    };
  }

  async function save() {
    if (!editing) return;
    try {
      const part = editing.part ?? "me";
      if (part === "project" && !editing.scope) {
        error = "Choose the project this entry is for.";
        return;
      }
      await api.memory.save({
        ...editing,
        part,
        scope: part === "project" ? editing.scope : "",
        kind: editing.kind ?? "fact",
        title: editing.title ?? "",
      });
      editing = null;
      memory.touch();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function approve(e: MemoryEntry) {
    try {
      await api.memory.save({ ...e, status: "active" });
      memory.touch();
    } catch (err) {
      error = errorText(err);
    }
  }

  async function remove(e: MemoryEntry, confirm = true) {
    if (confirm) {
      const ok = await ask(`Delete "${e.title}" from your memory?`, {
        title: "Delete memory entry",
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Keep",
      });
      if (!ok) return;
    }
    try {
      await api.memory.remove(e.id);
      memory.touch();
    } catch (err) {
      error = errorText(err);
    }
  }

  async function tryIt() {
    if (!tryText.trim()) return;
    try {
      const s = await api.memory.slice(project?.id ?? null, tryText, settings.value.memoryBudget);
      tried = { text: s.text, left_out: s.left_out };
    } catch (e) {
      error = errorText(e);
    }
  }

  const projectName = (id: string | undefined) => projects.list.find((p) => p.id === id)?.name ?? "a project";
  const partLabel = (e: Partial<MemoryEntry>) =>
    e.part === "project"
      ? `This project · ${projectName(e.scope)}`
      : (PARTS.find((p) => p.id === e.part)?.label ?? "About me");
  const entryNote = (e: MemoryEntry) =>
    [e.part === "project" ? projectName(e.scope) : "", e.uses ? `given to agents ${e.uses}×` : ""]
      .filter(Boolean)
      .join(" · ");
  const kindLabel = (k: string) => KINDS.find((x) => x.id === k)?.label ?? k;
</script>

<div class="memory">
  <h2><Brain size={16} /> Memory</h2>
  <p class="lead">
    What Qanuvi knows about you, your tools and your projects. About me and Tools and models go with you to your other
    computers (through the memory disk); This project stays on this computer. Agents get only the rules and style plus
    the entries that match their request (at most {settings.value.memoryBudget} characters), and can ask for more or suggest
    new entries for you to approve.
  </p>

  <div class="options">
    <label class="check">
      <input
        type="checkbox"
        checked={settings.value.memoryContext}
        onchange={(e) => settings.set("memoryContext", e.currentTarget.checked)}
      />
      Give agents the relevant part with their work
    </label>
    <label>
      At most
      <input
        type="number"
        min="300"
        max="8000"
        step="100"
        value={settings.value.memoryBudget}
        onchange={(e) => settings.set("memoryBudget", Math.min(8000, Math.max(300, Number(e.currentTarget.value))))}
      />
      characters per request
    </label>
  </div>

  <MemoryDiskPanel />

  {#if shownError(error)}<p class="error">{shownError(error)}</p>{/if}

  {#if suggested.length}
    <h3>Suggested by agents ({suggested.length})</h3>
    {#each suggested as e (e.id)}
      <div class="entry suggested">
        <div class="head">
          <span class="kind k-{e.kind}">{kindLabel(e.kind)}</span>
          <strong>{e.title}</strong>
          <span class="muted">{partLabel(e)}</span>
          <span class="spacer"></span>
          <button type="button" class="btn primary" onclick={() => approve(e)}><Check size={13} /> Keep</button>
          <button type="button" class="btn" onclick={() => (editing = { ...e, status: "active" })}>
            <Pencil size={13} /> Edit
          </button>
          <button type="button" class="btn" onclick={() => remove(e, false)}><X size={13} /> Dismiss</button>
        </div>
        {#if e.body}<p class="body">{e.body}</p>{/if}
      </div>
    {/each}
  {/if}

  <div class="bar">
    <select bind:value={partFilter} aria-label="Part">
      <option value="">All parts</option>
      {#each PARTS as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
    </select>
    <select bind:value={filter} aria-label="Kind">
      <option value="">All kinds</option>
      {#each KINDS as k (k.id)}<option value={k.id}>{k.label}</option>{/each}
    </select>
    <input bind:value={query} placeholder="Filter…" spellcheck="false" />
    <button type="button" class="btn primary" onclick={startNew}><Plus size={13} /> New entry</button>
  </div>

  {#if editing}
    <form
      class="edit"
      onsubmit={(e) => {
        e.preventDefault();
        save();
      }}
    >
      <div class="row parts" role="radiogroup" aria-label="Part">
        {#each PARTS as p (p.id)}
          <label class="part" class:on={editing.part === p.id}>
            <input type="radio" name="part" value={p.id} bind:group={editing.part} />
            {p.label}
          </label>
        {/each}
        {#if editing.part === "project"}
          <select bind:value={editing.scope} aria-label="Project">
            {#if !editing.scope}<option value="">Choose a project…</option>{/if}
            {#each projects.list as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
          </select>
        {/if}
      </div>
      <p class="muted hint">{PARTS.find((p) => p.id === editing?.part)?.hint}</p>
      <div class="row">
        <select bind:value={editing.kind} aria-label="Kind">
          {#each KINDS as k (k.id)}<option value={k.id}>{k.label}</option>{/each}
        </select>
        <span class="muted">{KINDS.find((k) => k.id === editing?.kind)?.hint}</span>
      </div>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={editing.title} placeholder="Title (short)" maxlength="200" autofocus />
      <textarea bind:value={editing.body} rows="4" placeholder="Details (optional)" maxlength="4000"></textarea>
      <input bind:value={editing.tags} placeholder="Tags, for finding it (optional)" />
      <div class="row">
        <button type="submit" class="btn primary">Save</button>
        <button type="button" class="btn" onclick={() => (editing = null)}>Cancel</button>
      </div>
    </form>
  {/if}

  {#each groups as g (g.id)}
    <h3 class="group">{g.label} <span class="muted">({g.entries.length})</span></h3>
    {#each g.entries as e (e.id)}
      <div class="entry">
        <div class="head">
          <span class="kind k-{e.kind}">{kindLabel(e.kind)}</span>
          <strong>{e.title}</strong>
          <span class="muted">{entryNote(e)}</span>
          <span class="spacer"></span>
          <IconButton label="Edit" onclick={() => (editing = { ...e })}><Pencil size={13} /></IconButton>
          <IconButton label="Delete" onclick={() => remove(e)}><Trash size={13} /></IconButton>
        </div>
        {#if e.body}<p class="body">{e.body}</p>{/if}
      </div>
    {/each}
  {:else}
    <p class="muted">
      {entries.some((x) => x.status === "active")
        ? "Nothing matches."
        : project
          ? "No entries yet. Add things about you and how you work, your tools and models, and facts, decisions and solutions for this project."
          : "No entries yet. Add things about you and how you work, and your tools and models. Open a project to add entries for it."}
    </p>
  {/each}

  <h3>Try it</h3>
  <p class="muted">Write a request to see exactly what an agent would get from your memory with it.</p>
  <form
    class="row"
    onsubmit={(e) => {
      e.preventDefault();
      tryIt();
    }}
  >
    <input bind:value={tryText} placeholder="For example: fix the cart total" spellcheck="false" />
    <button type="submit" class="btn">Show</button>
  </form>
  {#if tried}
    {#if tried.text}
      <pre class="tried">{tried.text}</pre>
      <p class="muted">
        {tried.text.length} characters (about {Math.ceil(tried.text.length / 4)} tokens){tried.left_out
          ? `; ${tried.left_out} more matched but did not fit`
          : ""}.
      </p>
    {:else}
      <p class="muted">Nothing applies: the agent gets no memory text, so no tokens are spent on it.</p>
    {/if}
  {/if}
</div>

<style>
  .memory {
    height: 100%;
    overflow: auto;
    padding: var(--qx-space-3) var(--qx-space-4, 20px);
    font-size: var(--qx-text-small);
  }

  h2,
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: var(--qx-space-3) 0 var(--qx-space-2);
  }

  h2 {
    font-size: 15px;
  }

  h3 {
    font-size: 13px;
  }

  .lead,
  .muted {
    color: var(--qx-text-secondary);
  }

  .muted {
    margin: 4px 0;
  }

  .options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--qx-space-3);
    align-items: center;
    color: var(--qx-text-secondary);
  }

  .options input[type="number"] {
    width: 70px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .bar,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin: var(--qx-space-2) 0;
  }

  input:not([type="checkbox"]),
  select,
  textarea {
    min-width: 0;
    padding: 4px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  .bar input,
  .row input {
    flex: 1;
  }

  textarea {
    resize: vertical;
  }

  input:focus,
  select:focus,
  textarea:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .edit {
    display: grid;
    gap: 6px;
    margin-bottom: var(--qx-space-3);
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-accent);
    border-radius: var(--qx-radius-card);
  }

  .parts {
    gap: 4px;
  }

  .part {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 9px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    cursor: pointer;
  }

  .part.on {
    border-color: var(--qx-accent);
    color: var(--qx-accent);
  }

  .part input {
    margin: 0;
  }

  .hint {
    margin: 0;
  }

  h3.group {
    margin-top: var(--qx-space-3);
    color: var(--qx-text-secondary);
  }

  .entry {
    padding: 6px var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
  }

  .entry.suggested {
    border: 1px dashed var(--qx-accent);
    border-radius: var(--qx-radius-card);
    margin-bottom: 6px;
  }

  .head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }

  .body {
    margin: 4px 0 0;
    color: var(--qx-text-secondary);
    white-space: pre-wrap;
  }

  .kind {
    padding: 0 6px;
    border-radius: 4px;
    font-size: 11px;
    background: var(--qx-bg-raised, rgba(127, 127, 127, 0.15));
  }

  .k-rule {
    color: #f85149;
  }

  .k-style {
    color: #d2a8ff;
  }

  .k-fact {
    color: #3794ff;
  }

  .k-decision {
    color: #e2c08d;
  }

  .k-solution {
    color: #73c991;
  }

  .k-example {
    color: #79c0ff;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 9px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .btn.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .tried {
    margin: 6px 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-window);
    white-space: pre-wrap;
    font-family: var(--qx-font-mono, monospace);
    user-select: text;
  }
</style>
