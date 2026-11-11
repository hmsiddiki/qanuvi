<script lang="ts">
  /** A provider's setup, step by step (Milestone 24): pages to open, commands to copy, the key, models to use. */
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { recommend, type Pick } from "$lib/agents/guides";
  import { brain } from "$lib/stores/brain.svelte";
  import { guides } from "$lib/stores/guides.svelte";
  import type { ModelProvider } from "$lib/stores/settings.svelte";

  let { provider }: { provider: ModelProvider } = $props();

  let guide = $derived(guides.of(provider.id));
  let copied = $state("");
  /** The best-ranked models, more than are needed: each is tried before it is recommended. */
  let candidates = $derived<Pick[]>(
    recommend(
      brain.found[provider.id] ?? [],
      Object.fromEntries(
        Object.entries(brain.records).map(([id, r]) => [id, { turns: r.turns, finished: r.finished }]),
      ),
      6,
    ),
  );
  /** Only models that answered a tiny test message (routers list models whose providers are not connected). */
  let picks = $derived(candidates.filter((p) => brain.probes[p.model.id]?.state === "ok").slice(0, 3));
  let testing = $state(false);

  async function testCandidates() {
    if (testing) return;
    testing = true;
    try {
      for (const c of candidates) {
        if (!brain.probes[c.model.id]) await brain.probe(provider, c.model as Parameters<typeof brain.probe>[1]);
      }
    } finally {
      testing = false;
    }
  }

  // Cloud models are tried as soon as they are found (Ollama's are on this computer: nothing to try).
  $effect(() => {
    if (provider.kind !== "ollama" && candidates.some((c) => !brain.probes[c.model.id])) testCandidates();
  });
  let local = $derived(provider.kind === "ollama" ? guides.local : null);
  let have = $derived(new Set((brain.found[provider.id] ?? []).map((m) => m.name)));

  onMount(() => {
    if (provider.kind === "ollama") {
      guides.loadMachine();
      if (!brain.found[provider.id]) brain.find(provider);
    }
  });

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    copied = text;
    setTimeout(() => (copied = ""), 1500);
  }

  function useAll() {
    for (const p of picks) brain.use(p.model as Parameters<typeof brain.use>[0]);
  }

  async function get(name: string) {
    if (await guides.pull(provider, name)) {
      await brain.find(provider);
      const m = brain.found[provider.id]?.find((x) => x.name === name);
      if (m) brain.use(m);
    }
  }

  function gb(n: number) {
    return `${(n / 1e9).toFixed(1)} GB`;
  }
</script>

{#if guide}
  <div class="guide">
    <p class="summary">{guide.summary} <span class="muted">(checked {guide.checkedOn})</span></p>
    <ol>
      {#each guide.steps as step, i (i)}
        <li>
          <strong>{step.title}</strong>
          <span class="text">{step.text}</span>
          <div class="act">
            {#if step.action === "open" && step.url}
              <button type="button" class="btn" onclick={() => openUrl(step.url!)}
                >Open {step.url.replace(/^https?:\/\//, "")}</button
              >
            {:else if step.action === "copy" && step.command}
              <code>{step.command}</code>
              <button type="button" class="btn" onclick={() => copy(step.command!)}
                >{copied === step.command ? "Copied" : "Copy"}</button
              >
            {:else if step.action === "key"}
              <span class="muted"
                >{brain.keys[provider.id] ? "A key is kept for this provider." : "Use the key field above."}</span
              >
            {:else if step.action === "check"}
              <button type="button" class="btn" onclick={() => guides.checkKey(provider)}>Check key</button>
              {#if guides.checking[provider.id]}<span class="muted">{guides.checking[provider.id]}</span>{/if}
            {:else if step.action === "find"}
              <button
                type="button"
                class="btn"
                disabled={brain.searching[provider.id]}
                onclick={() => brain.find(provider)}>{brain.searching[provider.id] ? "Looking…" : "Find models"}</button
              >
              {#if brain.found[provider.id]}<span class="muted">{brain.found[provider.id].length} found</span>{/if}
            {:else if step.action === "recommend"}
              {#if provider.kind === "ollama"}
                {#if guides.machine}
                  <p class="muted machine">
                    This computer: {guides.machine.ramGb} GB memory{guides.machine.gpu
                      ? `, ${guides.machine.gpu.name} (${guides.machine.gpu.vramGb} GB)`
                      : ", no separate graphics card"}.
                  </p>
                  {#each [["Coding", local?.coder], ["Conversation and thinking", local?.chat]] as const as [label, s] (label)}
                    <div class="pick">
                      <span class="role">{label}</span>
                      {#if s}
                        <code>{s.model.name}</code>
                        <span class="muted small"
                          >{s.model.sizeGb} GB · {s.model.note} · {s.fast
                            ? "fits the graphics card: fast"
                            : "runs on the processor: works, slower"}</span
                        >
                        <span class="spacer"></span>
                        {#if brain.uses(`${provider.id}/${s.model.name}`)}
                          <span class="muted small">in use</span>
                        {:else if have.has(s.model.name)}
                          <button
                            type="button"
                            class="btn small"
                            onclick={() => {
                              const m = brain.found[provider.id]?.find((x) => x.name === s.model.name);
                              if (m) brain.use(m);
                            }}>Use</button
                          >
                        {:else if guides.pulling[s.model.name]}
                          {@const p = guides.pulling[s.model.name]!}
                          <span class="muted small">{p.total ? `${gb(p.done)} of ${gb(p.total)}` : p.status}</span>
                          <progress max={p.total || 1} value={p.done}></progress>
                        {:else}
                          <button type="button" class="btn small" onclick={() => get(s.model.name)}
                            >Get it ({s.model.sizeGb} GB)</button
                          >
                        {/if}
                        {#if guides.pullError[s.model.name]}<span class="error">{guides.pullError[s.model.name]}</span
                          >{/if}
                      {:else}
                        <span class="muted small">This computer has too little memory for a useful model.</span>
                      {/if}
                    </div>
                  {/each}
                {:else}
                  <span class="muted">Looking at this computer…</span>
                {/if}
              {:else if candidates.length}
                <ul class="picks">
                  {#each candidates as p (p.model.id)}
                    {@const t = brain.probes[p.model.id]}
                    <li class:failed={t?.state === "failed"}>
                      <code>{p.model.name}</code>
                      <span class="muted small">{p.reasons.join(" · ")}</span>
                      <span class="small" class:ok={t?.state === "ok"} class:error={t?.state === "failed"}
                        >{t ? t.text : "waiting to be tried"}</span
                      >
                    </li>
                  {/each}
                </ul>
                <span class="muted small"
                  >Each is tried with one tiny message; only those that answer are used{testing
                    ? " (trying…)"
                    : ""}.</span
                >
                <button type="button" class="btn primary" disabled={!picks.length} onclick={useAll}
                  >{picks.length ? `Use the ${picks.length} that answered` : "Use these"}</button
                >
              {:else}
                <span class="muted">Find the models first.</span>
              {/if}
            {/if}
          </div>
        </li>
      {/each}
    </ol>
    {#if guide.tips.length}
      <ul class="tips">
        {#each guide.tips as t, i (i)}<li>{t}</li>{/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .guide {
    margin: 6px 0 2px;
    padding: 8px 10px;
    border-radius: 6px;
    background: var(--qx-bg-window);
    font-size: 12px;
  }

  .summary {
    margin: 0 0 6px;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .ok {
    color: var(--qx-status-done);
  }

  .picks li.failed code {
    opacity: 0.55;
    text-decoration: line-through;
  }

  ol {
    margin: 0;
    padding-left: 20px;
  }

  li {
    margin: 6px 0;
  }

  .text {
    display: block;
    color: var(--qx-text-secondary);
  }

  .act {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
  }

  .machine {
    flex-basis: 100%;
    margin: 0;
  }

  .pick {
    display: flex;
    flex-basis: 100%;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .role {
    min-width: 170px;
    font-weight: 600;
  }

  .spacer {
    flex: 1;
  }

  code {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--qx-bg-panel);
    font-family: var(--qx-font-mono, monospace);
    user-select: text;
  }

  .picks,
  .tips {
    flex-basis: 100%;
    margin: 0;
    padding-left: 16px;
  }

  .tips {
    margin-top: 6px;
    color: var(--qx-text-secondary);
  }

  progress {
    width: 120px;
  }

  .btn {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .btn.small {
    height: 22px;
    padding: 0 8px;
  }

  .btn.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
