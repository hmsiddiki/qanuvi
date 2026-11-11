<script lang="ts">
  /** Qanuvi's own agent (Milestone 24): the models its brain may use, and where they come from. */
  import { knowledge } from "$lib/stores/knowledge.svelte";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import X from "@lucide/svelte/icons/x";
  import { onMount } from "svelte";
  import { brain } from "$lib/stores/brain.svelte";
  import { settings, type ModelProvider } from "$lib/stores/settings.svelte";
  import { ask } from "$lib/stores/confirm.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { guides } from "$lib/stores/guides.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import SetupGuide from "./SetupGuide.svelte";
  import OmniRoutePanel from "./OmniRoutePanel.svelte";
  import SetupWizard from "./SetupWizard.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";

  /** Which providers show their step-by-step guide; open by default while nothing is set up. */
  let open = $state<Record<string, boolean>>({});
  let paidAgent = $derived(agent.info?.available ? "claude-code" : null);

  /** The guide opened for a new user: Ollama, or OpenRouter when they chose online models only. */
  let firstGuide = $derived(settings.value.brain.local ? "ollama" : "openrouter");

  function isOpen(id: string): boolean {
    return open[id] ?? (brain.missing[id] || (settings.value.brain.models.length === 0 && id === firstGuide));
  }

  async function updateGuides() {
    if (!paidAgent) return;
    if (!projects.current) {
      guides.updateError = "Open a project first: the agent works in one (it changes nothing there).";
      return;
    }
    const ok = await ask(
      "Qanuvi will ask your paid agent (Claude Code) to check OpenRouter's, OmniRoute's and Ollama's current pages on the web and bring these guides up to date. It uses your plan, may ask you to allow web pages, and changes no files. You see what changed before anything is kept.",
      { title: "Update the setup guides", okLabel: "Update", cancelLabel: "Not now" },
    );
    if (ok) await guides.update(projects.current, paidAgent);
  }

  let keyDraft = $state<Record<string, string>>({});
  let keyNote = $state<Record<string, string>>({});
  let paid = $state<Record<string, boolean>>({});
  let newProvider = $state({ id: "", name: "", url: "http://127.0.0.1:8080/v1" });
  let addError = $state("");

  let b = $derived(settings.value.brain);

  onMount(() => {
    brain.loadKeys();
    brain.loadRecords();
  });

  function size(bytes: number): string {
    return bytes ? `${(bytes / 1e9).toFixed(1)} GB` : "";
  }

  function record(id: string): string {
    const r = brain.records[id];
    if (!r || !r.turns) return "not used yet";
    const parts = [`${r.finished}/${r.turns} turns finished`];
    if (r.calls) parts.push(`${r.brokenCalls} of ${r.calls} tool calls broken`);
    const rest = r.restUntil - Date.now() / 1000;
    if (rest > 0) parts.push(`resting ${Math.ceil(rest / 60)} min: ${r.lastProblem}`);
    return parts.join(" · ");
  }

  async function saveKey(p: ModelProvider) {
    const err = await brain.setKey(p.id, keyDraft[p.id] ?? "");
    keyNote = {
      ...keyNote,
      [p.id]: err || (keyDraft[p.id]?.trim() ? "Key kept in the credential store." : "Key forgotten."),
    };
    keyDraft = { ...keyDraft, [p.id]: "" };
  }

  function setUrl(p: ModelProvider, url: string) {
    brain.saveProvider({ ...p, url: url.trim() });
  }

  /** Servers on this computer that Qanuvi cannot start by itself take the user's start command. */
  function takesStart(p: ModelProvider) {
    return (
      p.kind === "openai" && p.id !== "omniroute" && /^https?:\/\/(localhost|127\.0\.0\.1|\[::1\])[:/]/.test(p.url)
    );
  }

  function setStart(p: ModelProvider, start: string) {
    const { start: _old, ...rest } = p;
    void _old;
    brain.saveProvider(start.trim() ? { ...rest, start: start.trim() } : rest);
  }

  function addProvider(e: SubmitEvent) {
    e.preventDefault();
    const id = newProvider.id.trim().toLowerCase();
    if (!/^[a-z0-9_-]{1,32}$/.test(id))
      return (addError = "The id is letters, digits or dashes, for example llamacpp.");
    if (b.providers.some((p) => p.id === id)) return (addError = "That id is taken.");
    if (!/^https?:\/\/\S+$/.test(newProvider.url.trim()))
      return (addError = "The address starts with http:// or https://.");
    brain.saveProvider({ id, name: newProvider.name.trim() || id, kind: "openai", url: newProvider.url.trim() });
    newProvider = { id: "", name: "", url: "http://127.0.0.1:8080/v1" };
    addError = "";
  }
</script>

<div class="own">
  {#if wizard.open}
    <SetupWizard onclose={() => (wizard.open = false)} />
  {:else}
    <button type="button" class="btn primary wizard-open" onclick={() => (wizard.open = true)}
      >Set up free AI step by step</button
    >
  {/if}
  <label class="minutes">
    Approval cards offer "Allow for
    <input
      type="number"
      min="1"
      max="240"
      value={settings.value.approveMinutes}
      aria-label="Minutes"
      onchange={(e) =>
        settings.set("approveMinutes", Math.min(240, Math.max(1, Math.round(Number(e.currentTarget.value) || 10))))}
    />
    min": changes of that kind need no approval for that long (risky commands always ask).
  </label>
  <h3>
    Models Qanuvi core may use{#if b.models.length > 1}<button type="button" class="link" onclick={() => brain.clear()}
        >Remove all</button
      >{/if}
  </h3>
  <p class="note">
    Best first. With "Auto", questions go to a model on this computer first, changes to the first model in this list; a
    model that is rate limited, down or keeps getting tool calls wrong hands the turn to the next one.
  </p>
  {#each b.models as m, i (m.id)}
    <div class="model">
      <span class="name">{m.label}</span>
      {#if m.local}<span class="tag local">this computer</span>{:else if m.free}<span class="tag free">free</span>{/if}
      <span class="muted small">{m.id.split("/")[0]} · {record(m.id)}</span>
      <span class="spacer"></span>
      <button type="button" class="icon" title="Move up" disabled={i === 0} onclick={() => brain.move(m.id, -1)}
        ><ArrowUp size={13} /></button
      >
      <button
        type="button"
        class="icon"
        title="Move down"
        disabled={i === b.models.length - 1}
        onclick={() => brain.move(m.id, 1)}><ArrowDown size={13} /></button
      >
      <button type="button" class="icon" title="Remove" onclick={() => brain.drop(m.id)}><X size={13} /></button>
    </div>
  {:else}
    <p class="empty">None yet. Find models below and press Use.</p>
  {/each}

  <h3>Where models come from</h3>
  <p class="note">
    No paid plan needed: a model on this computer through Ollama, free models through OpenRouter, or OmniRoute's free
    providers. Each has a step-by-step guide. Guides checked on {guides.all.map((g) => g.checkedOn).sort()[0]}.
    {#if paidAgent}
      <button type="button" class="link" disabled={guides.updating} onclick={updateGuides}
        >{guides.updating ? "Updating…" : "Update them with Claude Code"}</button
      >
    {/if}
    {#if guides.kept}<button type="button" class="link" onclick={() => guides.reset()}>Use the built-in ones</button
      >{/if}
  </p>
  {#if guides.updateError}<p class="error">{guides.updateError}</p>{/if}
  {#if knowledge.offer}
    <div class="proposal">
      <strong
        >Newer knowledge from Qanuvi (version {knowledge.offer.pack.version}, {knowledge.offer.pack.published})</strong
      >
      {#if knowledge.offer.pack.note}<div class="muted small">{knowledge.offer.pack.note}</div>{/if}
      {#each knowledge.offer.summary as line (line)}<div class="muted small">{line}</div>{/each}
      <div class="row">
        <button type="button" class="btn" onclick={() => knowledge.keep()}>Use it</button>
        <button type="button" class="btn" onclick={() => knowledge.later()}>Not now</button>
      </div>
    </div>
  {/if}
  <p class="muted small">
    Qanuvi's own knowledge (guides, free providers):
    {knowledge.kept ? `version ${knowledge.kept.version} from ${knowledge.kept.published}` : "as shipped"}.
    <button type="button" class="link" disabled={knowledge.checking} onclick={() => knowledge.check(true)}
      >{knowledge.checking ? "Checking…" : "Check for newer"}</button
    >
    {#if knowledge.kept}<button type="button" class="link" onclick={() => knowledge.reset()}
        >Use the built-in ones</button
      >{/if}
    {#if knowledge.error}<span class="error">{knowledge.error}</span>{/if}
  </p>
  {#if guides.proposal}
    <div class="proposal">
      <strong>Updated guides are ready</strong>
      {#each Object.entries(guides.proposal.summary) as [id, what] (id)}
        <div class="muted small">{id}: {what}</div>
      {/each}
      <div class="row">
        <button type="button" class="btn" onclick={() => guides.accept()}>Keep them</button>
        <button type="button" class="btn" onclick={() => guides.discard()}>Discard</button>
        {#if guides.proposal.costUsd}<span class="muted small">cost ${guides.proposal.costUsd.toFixed(3)}</span>{/if}
      </div>
    </div>
  {/if}
  <label class="local-choice">
    <input
      type="checkbox"
      checked={b.local}
      onchange={(e) => settings.set("brain", { ...b, local: e.currentTarget.checked })}
    />
    <span>
      <strong>Use models on this computer (Ollama)</strong>
      <span class="muted small"
        >Off: only free models online (OpenRouter, OmniRoute). Nothing is installed, downloaded or started on this
        computer, and models already in the list from it are skipped.</span
      >
    </span>
  </label>
  {#each b.providers.filter((p) => b.local || p.kind !== "ollama") as p (p.id)}
    <div class="provider">
      <div class="row">
        <strong>{p.name}</strong>
        <input
          value={p.url}
          spellcheck="false"
          aria-label="{p.name} address"
          onchange={(e) => setUrl(p, e.currentTarget.value)}
        />
        <button type="button" class="btn" disabled={brain.searching[p.id]} onclick={() => brain.find(p, paid[p.id])}>
          {brain.searching[p.id] ? "Looking…" : "Find models"}
        </button>
        {#if guides.of(p.id)}
          <button type="button" class="btn" onclick={() => (open = { ...open, [p.id]: !isOpen(p.id) })}>
            {isOpen(p.id) ? "Hide steps" : "Step by step"}
          </button>
        {/if}
      </div>
      {#if p.kind !== "ollama"}
        <div class="row">
          <input
            type="password"
            class="key"
            placeholder={brain.keys[p.id] ? "Key kept (type a new one to replace it)" : "API key (if it needs one)"}
            bind:value={keyDraft[p.id]}
            aria-label="{p.name} key"
          />
          <button type="button" class="btn" onclick={() => saveKey(p)}>
            {keyDraft[p.id]?.trim() ? "Save key" : brain.keys[p.id] ? "Forget key" : "Save key"}
          </button>
          {#if p.id === "omniroute" && !brain.keys[p.id]}
            <button
              type="button"
              class="btn primary"
              disabled={brain.searching[p.id]}
              title="OmniRoute makes a key named Qanuvi; Qanuvi keeps it in your system's credential store"
              onclick={() => brain.connect(p)}>Connect Qanuvi</button
            >
          {/if}
          {#if p.kind === "openrouter"}
            <label class="check"><input type="checkbox" bind:checked={paid[p.id]} /> paid models too</label>
          {/if}
          {#if keyNote[p.id]}<span class="muted small">{keyNote[p.id]}</span>{/if}
        </div>
      {/if}
      {#if takesStart(p)}
        <div class="row">
          <input
            class="key"
            value={p.start ?? ""}
            spellcheck="false"
            placeholder={'Start command, e.g. llama-server -m "C:\\models\\qwen2.5-coder-7b.gguf" --port 8080'}
            aria-label="{p.name} start command"
            title="Qanuvi runs this when the server does not answer (in the background, no window)."
            onchange={(e) => setStart(p, e.currentTarget.value)}
          />
        </div>
      {/if}
      {#if p.id === "omniroute" && brain.keys[p.id]}
        <label class="check assist">
          <input
            type="checkbox"
            checked={b.omniAssist}
            onchange={(e) => settings.set("brain", { ...b, omniAssist: e.currentTarget.checked })}
          />
          Let Qanuvi run OmniRoute for me (connections, sign-ins, fallback combo)
        </label>
        {#if b.omniAssist}<OmniRoutePanel provider={p} />{/if}
      {/if}
      {#if brain.notes[p.id]}<p class="muted small">{brain.notes[p.id]}</p>{/if}
      {#if brain.errors[p.id]}<p class="error">{brain.errors[p.id]}</p>{/if}
      {#if isOpen(p.id)}<SetupGuide provider={p} />{/if}
      {#if brain.found[p.id]?.length}
        <ul class="found">
          {#each brain.found[p.id] as f (f.id)}
            <li>
              <span class="name">{f.name}</span>
              <span class="muted small">
                {[
                  f.local ? "this computer" : f.free ? "free" : "paid",
                  f.context ? `${Math.round(f.context / 1024)}k context` : "",
                  size(f.size),
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
              <span class="spacer"></span>
              {#if !f.tools}
                <span class="muted small">cannot use tools</span>
              {:else if brain.uses(f.id)}
                <button
                  type="button"
                  class="btn small"
                  title="Take it out of Qanuvi core's list"
                  onclick={() => brain.drop(f.id)}>Stop using</button
                >
              {:else}
                <button type="button" class="btn small" onclick={() => brain.use(f)}>Use</button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/each}

  <form class="add" onsubmit={addProvider}>
    <span class="muted">Another OpenAI-compatible server (llama.cpp, LM Studio, vLLM…):</span>
    <input bind:value={newProvider.id} placeholder="id, e.g. llamacpp" spellcheck="false" aria-label="Provider id" />
    <input bind:value={newProvider.name} placeholder="Name" aria-label="Provider name" />
    <input bind:value={newProvider.url} spellcheck="false" aria-label="Provider address" />
    <button type="submit" class="btn">Add</button>
    {#if addError}<span class="error">{addError}</span>{/if}
  </form>
</div>

<style>
  .own {
    margin-top: 8px;
  }

  .wizard-open {
    margin: 4px 0 8px;
  }

  .minutes {
    display: block;
    margin: 4px 0 8px;
    color: var(--qx-text-secondary);
    font-size: 12px;
  }

  .minutes input {
    flex: none;
    min-width: 0;
    width: 52px;
    height: 22px;
    margin: 0 2px;
    padding: 0 4px;
    border: 1px solid var(--qx-border);
    border-radius: 4px;
    background: var(--qx-bg-input, var(--qx-bg-panel));
    color: var(--qx-text);
    font: inherit;
  }

  h3 {
    margin: 12px 0 4px;
    font-size: 12px;
  }

  .note,
  .empty {
    margin: 2px 0 6px;
    color: var(--qx-text-secondary);
    font-size: 12px;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .error {
    margin: 4px 0;
    color: var(--qx-status-failed);
    font-size: 12px;
  }

  .model,
  .found li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 6px;
  }

  .model:hover,
  .found li:hover {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.08));
  }

  .name {
    font-family: var(--qx-font-mono, monospace);
    font-size: 12px;
    white-space: nowrap;
  }

  .tag {
    padding: 0 6px;
    border-radius: 8px;
    font-size: 10px;
  }

  .tag.local {
    background: rgba(14, 159, 181, 0.2);
    color: #38bdd8;
  }

  .tag.free {
    background: rgba(46, 160, 67, 0.2);
    color: #3fb950;
  }

  .spacer {
    flex: 1;
  }

  .provider {
    margin: 6px 0;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
  }

  .row,
  .add {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 4px 0;
  }

  .row strong {
    min-width: 150px;
    font-size: 12px;
  }

  input {
    flex: 1;
    min-width: 160px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    font-size: 12px;
    user-select: text;
  }

  input[type="checkbox"] {
    flex: none;
    min-width: 0;
    height: auto;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
  }

  .found {
    max-height: 260px;
    overflow: auto;
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
  }

  .btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .btn.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .btn.small {
    height: 22px;
    padding: 0 8px;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  h3 .link {
    margin-left: 10px;
    font-weight: normal;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .proposal {
    margin: 6px 0;
    padding: 6px 8px;
    border: 1px solid var(--qx-accent);
    border-radius: 6px;
    font-size: 12px;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .icon:hover:not(:disabled) {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
    color: var(--qx-text);
  }

  .icon:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .local-choice {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 8px 0 4px;
    font-size: 12px;
  }

  .local-choice > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>
