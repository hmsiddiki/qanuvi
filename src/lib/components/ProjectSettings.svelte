<script lang="ts">
  import { projects, kindStyle } from "$lib/stores/projects.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { terminals } from "$lib/stores/terminals.svelte";
  import { api, errorText } from "$lib/api";

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
  let changesFrom = $state("qanuvi");
  let error = $state("");

  /** Secrets: names shown, values never (they go to the credential store and to the agents' environment). */
  let secretNames = $state<string[]>([]);
  let secretName = $state("");
  let secretValue = $state("");
  let secretError = $state("");
  /** A server keeps them in its environment (QANUVI_SECRET_<NAME>): shown, not added or removed here. */
  let secretStore = $state<"store" | "environment">("store");
  $effect(() => {
    if (!projectId) return;
    secretError = "";
    api.agent
      .secretStore(projectId)
      .then((k) => (secretStore = k))
      .catch(() => (secretStore = "store"));
    api.agent
      .secretNames(projectId)
      .then((n) => (secretNames = n))
      .catch((e) => {
        secretNames = [];
        secretError = errorText(e);
      });
  });
  async function addSecret() {
    secretError = "";
    try {
      await api.agent.setSecret(projectId, secretName.trim(), secretValue);
      secretNames = await api.agent.secretNames(projectId);
      secretName = "";
      secretValue = "";
    } catch (e) {
      secretError = errorText(e);
    }
  }
  async function removeSecret(name: string) {
    secretError = "";
    try {
      await api.agent.removeSecret(projectId, name);
      secretNames = await api.agent.secretNames(projectId);
    } catch (e) {
      secretError = errorText(e);
    }
  }

  $effect(() => {
    if (!project) return;
    name = project.name;
    run = project.settings.run_command;
    test = project.settings.test_command;
    preview = project.settings.preview_url;
    changesFrom = project.settings.changes_from === "git" ? "git" : "qanuvi";
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!project) return;
    error = await projects.update(project.id, name, {
      run_command: run.trim(),
      test_command: test.trim(),
      preview_url: preview.trim(),
      changes_from: changesFrom,
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
      <label
        >Plan step changes from <select bind:value={changesFrom}>
          <option value="qanuvi">Qanuvi's own snapshots (works with or without git)</option>
          <option value="git">Git (each file's repository, against its last commit)</option>
        </select></label
      >
      <p class="note">
        What a plan step's review looks at. With git, files no repository sees (no git, or a folder it ignores) still
        come from Qanuvi's snapshots, and the run tells you which.
      </p>

      <h3>Secrets</h3>
      {#if secretStore === "environment"}
        <p class="note">
          Logins and passwords this project's work needs. This server keeps them in its own environment: set
          <code>QANUVI_SECRET_DB_PASSWORD</code> where qanuvi-master starts (for example in its systemd unit) and
          restart it; every project's agents and test command there get it as <code>DB_PASSWORD</code>. No model sees
          them.
        </p>
      {:else}
        <p class="note">
          Logins and passwords this project's work needs, for example a test database's. They are kept in the system's
          credential store and given to every agent and the test command as environment variables; no model sees them.
        </p>
      {/if}
      {#each secretNames as n (n)}
        <div class="secret">
          <code>{n}</code><span class="dots">••••••</span>
          {#if secretStore === "store"}
            <button type="button" class="link" onclick={() => removeSecret(n)}>Remove</button>
          {/if}
        </div>
      {/each}
      {#if secretStore === "environment" && !secretNames.length && !secretError}
        <p class="note">None set on this server.</p>
      {/if}
      {#if secretStore === "store"}
        <div class="secret add">
          <input bind:value={secretName} placeholder="NAME, for example DB_PASSWORD" spellcheck="false" />
          <input bind:value={secretValue} type="password" placeholder="value" autocomplete="off" />
          <button type="button" disabled={!secretName.trim() || !secretValue} onclick={addSecret}>Add</button>
        </div>
      {/if}
      {#if secretError}<p class="error">{secretError}</p>{/if}

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
    /* Never taller than the window: the form scrolls inside, Cancel and Save stay in view (owner, 2026-11-20). */
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: var(--qx-space-6) var(--qx-space-6) 0;
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    box-shadow: 0 18px 48px rgb(0 0 0 / 50%);
  }

  .note {
    margin: 0 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: 11.5px;
  }

  .secret {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    margin-bottom: 4px;
  }

  .secret .dots {
    flex: 1;
    color: var(--qx-text-muted);
  }

  .secret.add input {
    flex: 1;
    min-width: 0;
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
    position: sticky;
    bottom: 0;
    display: flex;
    justify-content: flex-end;
    gap: var(--qx-space-2);
    margin-top: var(--qx-space-4);
    padding: var(--qx-space-3) 0 var(--qx-space-6);
    border-top: 1px solid var(--qx-border);
    background: var(--qx-bg-raised);
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
