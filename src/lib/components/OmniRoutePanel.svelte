<script lang="ts">
  /**
   * OmniRoute run by Qanuvi (24c): connections and their health, free providers to add (recommended ones, or anything
   * from OmniRoute's catalog for expert users), sign-in renewal and Qanuvi's fallback combo. Every action is a button
   * the user presses; keys go straight to OmniRoute.
   */
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { recommend, type FreeProvider } from "$lib/agents/guides";
  import { knowledge } from "$lib/stores/knowledge.svelte";
  import { omni } from "$lib/stores/omniroute.svelte";
  import { brain } from "$lib/stores/brain.svelte";
  import type { ModelProvider } from "$lib/stores/settings.svelte";

  let { provider }: { provider: ModelProvider } = $props();

  let keys = $state<Record<string, string>>({});
  let expert = $state(false);
  let search = $state("");
  let custom = $state({ id: "", key: "" });

  onMount(() => omni.load(provider));

  let connected = $derived(new Set((omni.connections ?? []).map((c) => c.provider)));
  let shown = $derived(
    omni.catalog
      .filter((c) => !search.trim() || `${c.id} ${c.name}`.toLowerCase().includes(search.trim().toLowerCase()))
      .slice(0, 40),
  );
  /** The tested OmniRoute models that answered, best first: the combo's order. */
  let answered = $derived(
    Object.entries(brain.probes)
      .filter(([id, t]) => id.startsWith(`${provider.id}/`) && t.state === "ok")
      .map(([id]) => id.slice(provider.id.length + 1))
      .filter((m) => !m.startsWith("auto") && m.includes("/")),
  );

  let testing = $state(false);

  /** Finds OmniRoute's models and tries the best-ranked with one tiny message each (as the guide does). */
  async function testModels() {
    testing = true;
    try {
      await brain.find(provider);
      const picks = recommend(brain.found[provider.id] ?? [], {}, 6);
      for (const p of picks) await brain.probe(provider, p.model as Parameters<typeof brain.probe>[1]);
    } finally {
      testing = false;
    }
  }

  /** Makes the combo, then puts it in Qanuvi core's list (OmniRoute then handles the fallback inside it). */
  async function makeCombo() {
    if (!(await omni.makeCombo(provider, "qanuvi-free", answered))) return;
    await brain.find(provider);
    const m = brain.found[provider.id]?.find((x) => x.name === "qanuvi-free");
    if (m) brain.use(m);
  }

  /** The account a sign-in connection belongs to (OmniRoute names it after the e-mail). */
  function account(providerId: string): string {
    return (omni.connections ?? []).find((c) => c.provider === providerId && c.name.includes("@"))?.name ?? "";
  }

  /**
   * OmniRoute tests key connections itself; it cannot test sign-in ones, so Qanuvi sends one of that provider's models
   * a tiny message instead.
   */
  async function test(c: (typeof omni.connections & object)[number]) {
    if (c.auth !== "oauth") return omni.test(provider, c);
    omni.error = "";
    omni.note = `Testing ${c.provider} with one of its models…`;
    if (!brain.found[provider.id]) await brain.find(provider);
    const m = brain.found[provider.id]?.find((x) => x.name.startsWith(`${c.provider}/`));
    if (!m) {
      omni.note = "";
      omni.error = `OmniRoute lists no model from ${c.provider} right now.`;
      return;
    }
    const ok = await brain.probe(provider, m);
    omni.note = ok ? `${c.provider} answers (${m.name}, ${brain.probes[m.id]?.text}).` : "";
    if (!ok) omni.error = `${c.provider}: ${brain.probes[m.id]?.text}`;
  }

  async function signIn(id: string) {
    if ((await omni.signIn(provider, id)) === "panel") openUrl(`${base()}/dashboard/providers`);
  }

  function base() {
    return provider.url.replace(/\/v1\/?$/, "");
  }

  async function connect(f: FreeProvider) {
    if (await omni.addKey(provider, f.id, keys[f.id] ?? "", undefined)) keys = { ...keys, [f.id]: "" };
  }

  function stateClass(s: string) {
    return s === "working" ? "ok" : s === "resting" ? "warn" : "bad";
  }
</script>

<div class="omni">
  <div class="head">
    <strong>OmniRoute, run by Qanuvi</strong>
    <button type="button" class="link" onclick={() => omni.load(provider)}>Refresh</button>
    <button type="button" class="link" onclick={() => openUrl(base())}>Open OmniRoute's panel</button>
  </div>

  {#if omni.connections}
    <ul class="conns">
      {#each omni.connections as c (c.id)}
        <li>
          <span class="dot {stateClass(c.state)}"></span>
          <span class="name">{c.provider}</span>
          <span class="muted small">{c.name && c.name !== c.provider ? c.name : ""} · {c.state}</span>
          {#if c.problem}<span class="muted small problem" title={c.problem}>{c.problem}</span>{/if}
          <span class="spacer"></span>
          {#if omni.busy[c.id] || omni.busy[c.provider]}
            <span class="muted small">{omni.busy[c.id] ?? omni.busy[c.provider]}</span>
          {:else}
            {#if c.auth === "oauth"}
              {#if c.name.includes("@")}<span class="muted small">sign in as <strong>{c.name}</strong></span>{/if}
              <button
                type="button"
                class="btn small"
                title={c.name.includes("@") ? `Sign in with ${c.name} (the account this connection belongs to)` : ""}
                onclick={() => signIn(c.provider)}>Renew sign-in</button
              >
            {/if}
            <button type="button" class="btn small" onclick={() => test(c)}>Test</button>
          {/if}
        </li>
      {:else}
        <li class="muted">No providers connected yet: add free ones below.</li>
      {/each}
    </ul>
  {:else if omni.busy.load}
    <p class="muted small">{omni.busy.load}</p>
  {/if}

  <p class="sub">
    Free providers worth adding <span class="muted small">(checked {knowledge.freeProvidersCheckedOn})</span>
  </p>
  <ul class="free">
    {#each knowledge.freeProviders as f (f.id)}
      <li>
        <div class="line">
          <strong>{f.name}</strong>
          {#if connected.has(f.id)}<span class="tag">connected</span>{/if}
          <span class="spacer"></span>
          {#if omni.busy[f.id]}
            <span class="muted small">{omni.busy[f.id]}</span>
          {:else if f.how === "sign-in"}
            <button type="button" class="btn small" onclick={() => signIn(f.id)}
              >{connected.has(f.id) ? "Sign in again" : "Sign in"}</button
            >
            {#if account(f.id)}<span class="muted small">as {account(f.id)}</span>{/if}
          {:else if connected.has(f.id)}
            <!-- Connected: nothing to do (Test is in the list above). -->
          {:else if f.reuse && brain.keys[f.reuse]}
            <button
              type="button"
              class="btn small primary"
              title="The key Qanuvi keeps for {f.name} goes to OmniRoute directly"
              onclick={() => omni.addKey(provider, f.id, "", f.reuse)}>Use the key Qanuvi keeps</button
            >
          {:else}
            <button type="button" class="btn small" onclick={() => openUrl(f.url)}>Get a key</button>
            <input
              type="password"
              class="key"
              placeholder="Paste the key"
              bind:value={keys[f.id]}
              aria-label="{f.name} key"
            />
            <button type="button" class="btn small" disabled={!keys[f.id]?.trim()} onclick={() => connect(f)}
              >Connect</button
            >
          {/if}
        </div>
        <span class="muted small">{f.note}</span>
      </li>
    {/each}
  </ul>

  <p class="sub">
    Qanuvi's fallback combo
    <span class="muted small"
      >OmniRoute tries the models in order, each standing in for the one before. Built from the models that answered
      Qanuvi's test.</span
    >
  </p>
  <div class="line">
    {#if answered.length}
      <code class="small">{answered.join(" → ")}</code>
      <span class="spacer"></span>
      <button type="button" class="btn small" disabled={!!omni.busy.combo} onclick={makeCombo}
        >{omni.busy.combo ?? "Make qanuvi-free"}</button
      >
    {:else}
      <span class="muted small">{testing ? "Testing OmniRoute's models…" : "No model tested yet."}</span>
      <span class="spacer"></span>
      <button type="button" class="btn small" disabled={testing} onclick={testModels}>Test the models</button>
    {/if}
  </div>

  <button type="button" class="link expert" onclick={() => ((expert = !expert), expert && omni.loadCatalog(provider))}>
    {expert ? "Hide" : "Choose any provider (expert)"}
  </button>
  {#if expert}
    <div class="catalog">
      <input bind:value={search} placeholder="Search OmniRoute's {omni.catalog.length} providers" aria-label="Search" />
      <ul>
        {#each shown as c (c.id)}
          <li>
            <button type="button" class="pick" onclick={() => (custom = { ...custom, id: c.id })}>
              {c.name} <span class="muted small">{c.id} · {c.category}{c.hasFree ? " · free tier" : ""}</span>
            </button>
            {#if c.website}<button type="button" class="link small" onclick={() => openUrl(c.website)}>site</button
              >{/if}
          </li>
        {/each}
      </ul>
      <div class="line">
        <input bind:value={custom.id} placeholder="provider id" aria-label="Provider id" />
        <input type="password" bind:value={custom.key} placeholder="its key" aria-label="Its key" />
        <button
          type="button"
          class="btn small"
          disabled={!custom.id.trim() || !custom.key.trim()}
          onclick={async () => {
            if (await omni.addKey(provider, custom.id.trim(), custom.key.trim())) custom = { id: "", key: "" };
          }}>Connect</button
        >
        <button type="button" class="btn small" disabled={!custom.id.trim()} onclick={() => signIn(custom.id.trim())}
          >Sign in instead</button
        >
      </div>
    </div>
  {/if}

  {#if omni.note}<p class="ok small">{omni.note}</p>{/if}
  {#if omni.error}<p class="error small">{omni.error}</p>{/if}
</div>

<style>
  .omni {
    margin: 6px 0 2px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--qx-bg-window);
    font-size: 12px;
  }

  .head,
  .line,
  .conns li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .head {
    margin-bottom: 4px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .conns li,
  .free li {
    padding: 3px 0;
  }

  .free li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-top: 1px solid var(--qx-border);
    padding: 5px 0;
  }

  .sub {
    margin: 10px 0 4px;
    font-weight: 600;
  }

  .sub .muted {
    display: block;
    font-weight: normal;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .problem {
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .spacer {
    flex: 1;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.ok {
    background: var(--qx-status-done);
  }

  .dot.warn {
    background: var(--qx-status-waiting);
  }

  .dot.bad {
    background: var(--qx-status-failed);
  }

  .name {
    font-weight: 600;
  }

  .tag {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--qx-status-done) 25%, transparent);
    font-size: 10px;
  }

  .ok {
    color: var(--qx-status-done);
  }

  .error {
    color: var(--qx-status-failed);
  }

  input {
    height: 22px;
    padding: 0 6px;
    border: 1px solid var(--qx-border);
    border-radius: 4px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 11px;
  }

  input.key {
    width: 180px;
  }

  .btn {
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .btn.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .expert {
    margin-top: 10px;
  }

  .catalog {
    margin-top: 6px;
  }

  .catalog > input {
    width: 100%;
    margin-bottom: 4px;
  }

  .catalog ul {
    max-height: 180px;
    overflow-y: auto;
    margin-bottom: 6px;
  }

  .pick {
    padding: 2px 0;
    border: 0;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .pick:hover {
    color: var(--qx-accent);
  }
</style>
