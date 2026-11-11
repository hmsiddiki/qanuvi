<script lang="ts">
  import { projects, kindStyle } from "$lib/stores/projects.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";

  interface Props {
    projectId: string;
    onclose: () => void;
  }

  let { projectId, onclose }: Props = $props();

  let project = $derived(projects.list.find((p) => p.id === projectId) ?? null);
  let name = $state("");
  let run = $state("");
  let test = $state("");
  let preview = $state("");
  let error = $state("");

  $effect(() => {
    if (!project) return;
    name = project.name;
    run = project.settings.run_command;
    test = project.settings.test_command;
    preview = project.settings.preview_url;
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!project) return;
    error = await projects.update(project.id, name, {
      run_command: run.trim(),
      test_command: test.trim(),
      preview_url: preview.trim(),
    });
    if (!error) onclose();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

{#if project}
  <div class="backdrop" role="presentation" onpointerdown={(e) => e.target === e.currentTarget && onclose()}>
    <form class="dialog" aria-label="Project settings" onsubmit={save}>
      <h2>Project settings</h2>
      <p class="path">{project.path}</p>
      <p class="kind">
        Recognised as <strong>{kindStyle(project.kind).label}</strong>{#if project.tags.length > 1}
          ({project.tags.join(", ")}){/if}.
        <button type="button" class="link" onclick={() => projects.redetect(project!.id)}>Look again</button>
      </p>

      <label>Name <input bind:value={name} maxlength="80" /></label>
      <label>Run command <input bind:value={run} placeholder="for example npm run dev" spellcheck="false" /></label>
      <label
        >Test command <input bind:value={test} placeholder="for example php tests/run.php" spellcheck="false" /></label
      >
      <label
        >Preview address <input
          bind:value={preview}
          placeholder="http://localhost/my-site/"
          spellcheck="false"
        /></label
      >

      <h3>For all projects</h3>
      <label class="check">
        <input
          type="checkbox"
          checked={settings.value.tabsFollowProject}
          onchange={(e) => settings.set("tabsFollowProject", (e.target as HTMLInputElement).checked)}
        />
        Editor tabs follow the project (switching project shows that project's files)
      </label>
      <label>
        Terminal shell
        <select
          value={settings.value.defaultShell}
          onchange={(e) => settings.set("defaultShell", (e.target as HTMLSelectElement).value)}
        >
          <option value="">First available</option>
          {#each terminals.shells as s (s.id)}<option value={s.id}>{s.label}</option>{/each}
        </select>
      </label>

      {#if error}<p class="error">{error}</p>{/if}
      <div class="actions">
        <button type="button" onclick={onclose}>Cancel</button>
        <button type="submit" class="primary">Save</button>
      </div>
    </form>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 35%);
  }

  .dialog {
    width: min(480px, calc(100% - 32px));
    padding: var(--qx-space-6);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    box-shadow: 0 18px 48px rgb(0 0 0 / 50%);
  }

  h2 {
    margin: 0;
    font-size: 15px;
  }

  h3 {
    margin: var(--qx-space-4) 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .path {
    margin: var(--qx-space-1) 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .kind {
    margin: 0 0 var(--qx-space-4);
    color: var(--qx-text-secondary);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: var(--qx-space-3);
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: var(--qx-space-2);
  }

  input:not([type="checkbox"]),
  select {
    height: 30px;
    padding: 0 var(--qx-space-2);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: var(--qx-text-base);
    outline: none;
    user-select: text;
  }

  input:focus,
  select:focus {
    border-color: var(--qx-accent);
  }

  .link {
    border: 0;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-4);
  }

  .actions button {
    height: 30px;
    padding: 0 var(--qx-space-4);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .actions .primary {
    border-color: transparent;
    background: var(--qx-accent);
    color: #fff;
  }
</style>
