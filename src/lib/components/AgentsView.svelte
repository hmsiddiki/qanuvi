<script lang="ts">
  import FileCog from "@lucide/svelte/icons/file-cog";
  import CirclePlus from "@lucide/svelte/icons/circle-plus";
  import Plug from "@lucide/svelte/icons/plug";
  import { api, errorText, type AgentConfigFile } from "$lib/api";
  import { COMING, PROVIDERS, type AgentProvider } from "$lib/agents/providers";
  import { agent } from "$lib/stores/agent.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import RoutingSettings from "./RoutingSettings.svelte";
  import OwnModelsPanel from "./OwnModelsPanel.svelte";
  import TeamPanel from "./TeamPanel.svelte";

  let files = $state<Record<string, AgentConfigFile[]>>({});
  let error = $state("");

  $effect(() => {
    const projectId = projects.currentId;
    for (const p of PROVIDERS) {
      api.agentConfig
        .list(p.id, projectId)
        .then((list) => (files = { ...files, [p.id]: list }))
        .catch((e) => (error = errorText(e)));
    }
  });

  function openConfig(p: AgentProvider, f: AgentConfigFile) {
    editor.openConfig(p.id, f.id, `${f.label} · ${p.name}`, f.scope === "user" ? null : projects.currentId);
    ui.view = "editor";
  }

  const available = (p: AgentProvider) =>
    p.id === "claude-code" ? !!agent.info?.available : settings.value.brain.models.length > 0;
  const statusText = (p: AgentProvider) =>
    p.id === "claude-code"
      ? `Connected${agent.info?.version ? ` · ${agent.info.version}` : ""}`
      : `${settings.value.brain.models.length} model${settings.value.brain.models.length === 1 ? "" : "s"}`;
</script>

<div class="agents">
  <h2>Agents</h2>
  <p class="intro">
    The agents Qanuvi can run. Each brings its own models, permission modes and settings; more arrive with extensions.
  </p>
  <label class="ai-switch">
    <input type="checkbox" checked={settings.value.ai} onchange={(e) => settings.set("ai", e.currentTarget.checked)} />
    <span>
      <strong>AI features</strong>
      <span
        >Agent pane, sessions and plan runs. Off: Qanuvi works as a plain code editor; nothing is sent anywhere.</span
      >
    </span>
  </label>
  {#if error}<p class="error">{error}</p>{/if}

  {#if settings.value.ai}<TeamPanel />{/if}
  {#each PROVIDERS as p (p.id)}
    {@const a = settings.agent(p.id)}
    <section class="card">
      <header>
        <span class="badge" style:background={p.color}>{p.badge}</span>
        <div class="title">
          <strong>{p.name}</strong>
          <span>{p.kind}</span>
        </div>
        {#if available(p)}
          <span class="status on"><Plug size={12} /> {statusText(p)}</span>
        {:else}
          <span class="status off">{p.id === "claude-code" ? "Not found" : "No models yet"}</span>
        {/if}
      </header>
      {#if !available(p)}<p class="hint">{p.installHint}</p>{/if}
      {#if p.id === "claude-code" && agent.info?.path}<p class="path">{agent.info.path}</p>{/if}

      <label class="check">
        <input
          type="radio"
          name="default-agent"
          checked={settings.value.agent === p.id}
          onchange={() => settings.set("agent", p.id)}
        />
        Use for new sessions
      </label>

      <div class="grid">
        <label>
          Model
          <select
            value={a.model}
            onchange={(e) => {
              settings.setAgent(p.id, { model: e.currentTarget.value });
              // The current session too, when it uses this agent: the choice applies from its next message.
              if (agent.providerId === p.id) agent.model = e.currentTarget.value;
            }}
          >
            {#each p.models as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
          </select>
        </label>
        <label>
          Permission mode
          <select
            value={a.mode}
            onchange={(e) => {
              settings.setAgent(p.id, { mode: e.currentTarget.value });
              if (agent.providerId === p.id) agent.mode = e.currentTarget.value;
            }}
          >
            {#each p.modes as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
          </select>
        </label>
        {#if p.efforts.length}
          <label>
            Effort
            <select value={a.effort} onchange={(e) => settings.setAgent(p.id, { effort: e.currentTarget.value })}>
              {#each p.efforts as level (level)}<option value={level}>{level}</option>{/each}
            </select>
          </label>
        {/if}
        {#if p.supportsBaseUrl}
          <label class="wide">
            Address
            <input
              value={a.baseUrl}
              placeholder="Empty: the agent's own setting"
              spellcheck="false"
              onchange={(e) => settings.setAgent(p.id, { baseUrl: e.currentTarget.value })}
            />
            <span class="note">{p.baseUrlHint}</span>
          </label>
        {/if}
      </div>

      {#if p.id === "qanuvi"}
        <OwnModelsPanel />
      {:else}
        <label class="check">
          <input
            type="checkbox"
            checked={a.indexTools}
            onchange={(e) => settings.setAgent(p.id, { indexTools: e.currentTarget.checked })}
          />
          Give it Qanuvi&apos;s code index tools (find a definition, outline a file, read some lines; fewer tokens when it
          has to find its way around the code)
        </label>
        <label class="check">
          <input
            type="checkbox"
            checked={a.browserTools}
            onchange={(e) => settings.setAgent(p.id, { browserTools: e.currentTarget.checked })}
          />
          Give it Qanuvi&apos;s test browser (it checks pages as short text instead of starting Playwright or its own browser;
          screenshots only when it asks; a headless browser with an empty profile, started only when needed)
        </label>
      {/if}

      <h3>Configuration files</h3>
      <ul class="files">
        {#each files[p.id] ?? [] as f (f.id)}
          <li>
            <button type="button" onclick={() => openConfig(p, f)}>
              <FileCog size={14} />
              <span class="label">{f.label}</span>
              <span class="display">{f.display}</span>
              {#if !f.exists}<span class="new">new</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
      {#if !projects.current}<p class="note">Choose a project to see its project files too.</p>{/if}
    </section>
  {/each}

  {#if settings.value.ai}
    <RoutingSettings
      provider={PROVIDERS.find((p) => p.id === settings.value.agent) ?? PROVIDERS[0]}
      project={projects.current}
    />
  {/if}

  <section class="card coming">
    <header>
      <span class="badge muted"><CirclePlus size={14} /></span>
      <div class="title">
        <strong>Add an agent</strong>
        <span>Coming next</span>
      </div>
    </header>
    <ul class="list">
      {#each COMING as c (c.name)}<li><strong>{c.name}</strong> <span>{c.note}</span></li>{/each}
    </ul>
  </section>
</div>

<style>
  .agents {
    height: 100%;
    overflow: auto;
    padding: var(--qx-space-4) var(--qx-space-6) var(--qx-space-8);
  }

  h2 {
    margin: 0;
    font-size: 16px;
  }

  .intro {
    margin: var(--qx-space-1) 0 var(--qx-space-4);
    color: var(--qx-text-secondary);
  }

  .ai-switch {
    max-width: 760px;
    display: flex;
    align-items: flex-start;
    gap: var(--qx-space-3);
    margin-bottom: var(--qx-space-4);
    padding: var(--qx-space-3) var(--qx-space-4);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
  }

  .ai-switch > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .ai-switch span span {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .card {
    max-width: 760px;
    margin-bottom: var(--qx-space-4);
    padding: var(--qx-space-4);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--qx-space-3);
  }

  .badge {
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: 8px;
    color: #fff;
    font-weight: 700;
  }

  .badge.muted {
    background: var(--qx-bg-panel);
    color: var(--qx-text-secondary);
  }

  .title {
    flex: 1;
    display: flex;
    flex-direction: column;
  }

  .title span {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: var(--qx-text-small);
  }

  .status.on {
    background: color-mix(in srgb, var(--qx-status-done) 18%, transparent);
    color: var(--qx-status-done);
  }

  .status.off {
    background: color-mix(in srgb, var(--qx-status-failed) 18%, transparent);
    color: var(--qx-status-failed);
  }

  .path {
    margin: var(--qx-space-2) 0 0 42px;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 11px;
    overflow-wrap: anywhere;
  }

  .hint,
  .note {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    margin: var(--qx-space-3) 0;
    color: var(--qx-text-secondary);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: var(--qx-space-3);
  }

  .grid label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
  }

  .grid .wide {
    grid-column: 1 / -1;
  }

  select,
  input:not([type]) {
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

  select:focus,
  input:focus {
    border-color: var(--qx-accent);
  }

  h3 {
    margin: var(--qx-space-4) 0 var(--qx-space-2);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-section);
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .files {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .files button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    padding: 6px var(--qx-space-2);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .files button:hover {
    background: var(--qx-bg-panel);
  }

  .files .display {
    margin-left: auto;
    color: var(--qx-text-muted);
    font-family: var(--qx-font-mono);
    font-size: 11px;
  }

  .new {
    padding: 0 6px;
    border-radius: 4px;
    background: var(--qx-bg-panel);
    color: var(--qx-text-muted);
    font-size: 10px;
  }

  .list {
    margin: var(--qx-space-3) 0 0;
    padding-left: 42px;
    list-style: none;
    color: var(--qx-text-secondary);
  }

  .list li {
    padding: 3px 0;
  }

  .list span {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .error {
    color: var(--qx-status-failed);
  }
</style>
