<script lang="ts" module>
  let made = 0;
</script>

<script lang="ts">
  /**
   * 24rl A2: which relay to use, folded away as one line ("Relay: Qanuvi (default) · Change"). The value is empty for
   * Qanuvi's relay, else the relay's address. F2: "A free relay" lists the signed free relay list, tested from the
   * shown machine, and switches only after the user says yes to what that relay can see.
   */
  import { untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    RELAY_KIT_URL,
    freeRelayAsk,
    freeRelays,
    isDefaultRelay,
    type FreeRelay,
  } from "$lib/stores/remoteAccess.svelte";

  let { value = $bindable(""), disabled = false }: { value?: string; disabled?: boolean } = $props();

  type Kind = "default" | "free" | "own";
  const group = `relay-choice-${++made}`;
  let open = $state(false);
  let kind = $state<Kind>(isDefaultRelay(value) ? "default" : "own");
  /** The typed address is kept while another choice is picked, so switching back does not lose it. */
  let typed = $state(isDefaultRelay(value) ? "" : value);
  /** The free relay waiting for the user's yes. */
  let asking = $state<FreeRelay | null>(null);

  /** What this line last set: only a value set from outside (another machine shown) moves the choice. */
  let mine: string | null = null;

  function put(v: string) {
    mine = v;
    value = v;
  }

  $effect(() => {
    const v = value;
    untrack(() => {
      if (v === mine) return;
      kind = isDefaultRelay(v) ? "default" : freeRelays.find(v) ? "free" : "own";
      typed = kind === "own" ? v : "";
      asking = null;
      open = false;
    });
  });

  /** A saved address that is on the free list reads as that free relay once the list is in. */
  let free = $derived(isDefaultRelay(value) ? undefined : freeRelays.find(value));

  function toggle() {
    open = !open;
    if (open && !freeRelays.list.length && !freeRelays.loading) void freeRelays.load();
  }

  function pickDefault() {
    kind = "default";
    asking = null;
    put("");
  }

  function pickFree() {
    kind = "free";
    void freeRelays.load();
  }

  function pickOwn() {
    kind = "own";
    asking = null;
    put(typed);
  }

  function useFree(r: FreeRelay) {
    asking = null;
    put(r.url);
  }
</script>

<div class="relay">
  <span class="line">
    Relay: {#if isDefaultRelay(value)}Qanuvi (default){:else if free}{free.name} (free){:else}<span class="addr"
        >{value.trim()}</span
      >{/if}
    {#if !disabled}
      · <button type="button" class="link" onclick={toggle}>{open ? "Done" : "Change"}</button>
    {/if}
  </span>
  {#if open && !disabled}
    <div class="choices" role="radiogroup" aria-label="Relay">
      <label
        ><input type="radio" name={group} checked={kind === "default"} onchange={pickDefault} /> Qanuvi relay (default)</label
      >
      <label><input type="radio" name={group} checked={kind === "free"} onchange={pickFree} /> A free relay</label>
      <label><input type="radio" name={group} checked={kind === "own"} onchange={pickOwn} /> Your own relay</label>
      {#if kind === "own"}
        <input
          class="address"
          bind:value={typed}
          oninput={() => put(typed)}
          placeholder="wss://203.0.113.5:47443#pin=… or wss://relay.example.com"
          spellcheck="false"
        />
        <p class="kit">
          Run your own on any Linux server (an IP address is enough, no domain):
          <button type="button" class="link" onclick={() => openUrl(RELAY_KIT_URL)}>download the relay kit</button>,
          run <code>sudo ./setup.sh</code> there, and paste the address it prints.
        </p>
      {/if}
    </div>
    {#if kind === "free"}
      <div class="free">
        {#if freeRelays.loading}
          <p>Looking for free relays and testing them…</p>
        {:else}
          {#each freeRelays.list as r (r.url)}
            <div class="option" class:picked={free?.url === r.url}>
              <span class="name">{r.name}</span>
              <span>{[r.region, r.runBy && `run by ${r.runBy}`].filter(Boolean).join(" · ")}</span>
              {#if r.ms !== null}<span>{r.ms} ms</span>{:else}<span class="bad">{r.error || "No answer"}</span>{/if}
              {#if free?.url === r.url}
                <span>in use</span>
              {:else}
                <button type="button" class="link" disabled={r.ms === null} onclick={() => (asking = r)}>Use</button>
              {/if}
              {#if r.note}<span class="note">{r.note}</span>{/if}
            </div>
          {:else}
            <p>No free relays are listed yet. Qanuvi's relay, or your own, works meanwhile.</p>
          {/each}
          {#if freeRelays.error}<p class="bad">{freeRelays.error}</p>{/if}
          {#if asking}
            <div class="ask">
              <p>{freeRelayAsk(asking)}</p>
              <button type="button" class="btn primary" onclick={() => asking && useFree(asking)}>Yes, use it</button>
              <button type="button" class="btn" onclick={() => (asking = null)}>No</button>
            </div>
          {/if}
        {/if}
      </div>
    {/if}
  {/if}
</div>

<style>
  .relay {
    margin: 4px 0;
    font-size: 12px;
    color: var(--qx-text-secondary);
  }

  .addr {
    font-family: var(--qx-font-mono, monospace);
    color: var(--qx-text);
    user-select: text;
  }

  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  /* fb1: "Use" for a relay that did not answer looked clickable. */
  .link:disabled {
    color: var(--qx-text-secondary);
    cursor: default;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 14px;
    margin-top: 6px;
  }

  .choices label {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--qx-text);
  }

  .address {
    flex: 1;
    min-width: 220px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  .free {
    margin-top: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .kit {
    margin: 4px 0 0;
    font-size: 12px;
    opacity: 0.8;
  }
  .free p {
    margin: 2px 0;
  }

  .option {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
  }

  .option .name {
    color: var(--qx-text);
  }

  .option.picked .name {
    font-weight: 600;
  }

  .note {
    flex-basis: 100%;
  }

  .bad {
    color: var(--qx-status-failed);
  }

  .ask {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
  }

  .ask p {
    flex-basis: 100%;
    color: var(--qx-text);
  }

  .address:focus {
    outline: none;
    border-color: var(--qx-accent);
  }
</style>
