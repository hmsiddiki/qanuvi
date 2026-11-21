<script lang="ts" module>
  let made = 0;
</script>

<script lang="ts">
  /**
   * 24rl A2: which relay to use, folded away as one line ("Relay: Qanuvi (default) · Change"). The value is empty for
   * Qanuvi's relay; an address only for the user's own relay (the only address box in the app).
   */
  import { untrack } from "svelte";
  import { isDefaultRelay } from "$lib/stores/remoteAccess.svelte";

  let { value = $bindable(""), disabled = false }: { value?: string; disabled?: boolean } = $props();

  const group = `relay-choice-${++made}`;
  let open = $state(false);
  let own = $state(!isDefaultRelay(value));
  /** The typed address is kept while "Qanuvi relay" is picked, so switching back does not lose it. */
  let typed = $state(isDefaultRelay(value) ? "" : value);

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
      own = !isDefaultRelay(v);
      typed = own ? v : "";
      open = false;
    });
  });

  function pickDefault() {
    own = false;
    put("");
  }

  function pickOwn() {
    own = true;
    put(typed);
  }
</script>

<div class="relay">
  <span class="line">
    Relay: {#if isDefaultRelay(value)}Qanuvi (default){:else}<span class="addr">{value.trim()}</span>{/if}
    {#if !disabled}
      · <button type="button" class="link" onclick={() => (open = !open)}>{open ? "Done" : "Change"}</button>
    {/if}
  </span>
  {#if open && !disabled}
    <div class="choices" role="radiogroup" aria-label="Relay">
      <label><input type="radio" name={group} checked={!own} onchange={pickDefault} /> Qanuvi relay (default)</label>
      <label><input type="radio" name={group} checked={own} onchange={pickOwn} /> Your own relay</label>
      {#if own}
        <input
          class="address"
          bind:value={typed}
          oninput={() => put(typed)}
          placeholder="wss://relay.example.com"
          spellcheck="false"
        />
      {/if}
    </div>
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

  .address:focus {
    outline: none;
    border-color: var(--qx-accent);
  }
</style>
