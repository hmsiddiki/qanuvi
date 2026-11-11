<script lang="ts">
  import { settings } from "$lib/stores/settings.svelte";
  import Slash from "@lucide/svelte/icons/slash";
  import Check from "@lucide/svelte/icons/check";
  import { agent } from "$lib/stores/agent.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { itemsToMarkdown } from "$lib/agent/events";

  interface Props {
    disabled?: boolean;
    /** Lets the message box open the menu when "/" is typed first. */
    open?: boolean;
  }

  let { disabled = false, open = $bindable(false) }: Props = $props();

  let root: HTMLDivElement;
  let note = $state("");

  let provider = $derived(agent.provider);
  let effortIndex = $derived(Math.max(0, provider.efforts.indexOf(agent.effort)));

  function attach() {
    close();
    ui.pickFile = (path) => {
      const mention = `@${path} `;
      agent.draft = agent.draft && !agent.draft.endsWith(" ") ? `${agent.draft} ${mention}` : agent.draft + mention;
      agent.input?.focus();
    };
    ui.quickOpen = true;
  }

  /** Puts the menu away; a "/" typed only to open it goes too. */
  function close() {
    open = false;
    if (agent.draft === "/") agent.draft = "";
    agent.input?.focus();
  }

  function clear() {
    agent.newSession();
    open = false;
    agent.input?.focus();
  }

  async function exportConversation() {
    const text = itemsToMarkdown(agent.selected?.title ?? "Conversation", agent.items);
    try {
      await navigator.clipboard.writeText(text);
      note = "Copied as Markdown.";
    } catch {
      note = "Could not copy to the clipboard.";
    }
  }
</script>

<svelte:window
  onpointerdown={(e) => open && root && !root.contains(e.target as Node) && (open = false)}
  onkeydown={(e) => open && e.key === "Escape" && ((open = false), agent.input?.focus())}
/>

<div class="command-menu" bind:this={root}>
  <button
    type="button"
    class="trigger"
    class:on={open}
    title="Commands (type / in an empty message)"
    aria-label="Commands"
    aria-haspopup="menu"
    aria-expanded={open}
    {disabled}
    onclick={() => ((open = !open), (note = ""))}
  >
    <Slash size={14} />
  </button>

  {#if open}
    <div class="menu" role="menu">
      <div class="section">Context</div>
      <button type="button" role="menuitem" onclick={attach}>Mention a file from this project…</button>
      <button type="button" role="menuitem" onclick={clear}>Clear conversation (new session)</button>
      <button type="button" role="menuitem" disabled={agent.items.length === 0} onclick={exportConversation}>
        Export conversation
      </button>
      {#if note}<p class="note">{note}</p>{/if}

      <div class="section">Model · {provider.name}</div>
      {#if provider.id !== "qanuvi" && settings.value.team.mode === "free-decides" && settings.value.brain.models.length}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={agent.model === "auto"}
          onclick={() => {
            agent.model = "auto";
            close();
          }}
        >
          <span>Auto (a free model picks)</span>
          {#if agent.model === "auto"}<Check size={14} />{/if}
        </button>
      {/if}
      {#each provider.models as m (m.id)}
        <button
          type="button"
          role="menuitemradio"
          aria-checked={agent.model === m.id}
          onclick={() => {
            agent.pickModel(m.id);
            close();
          }}
        >
          <span>{m.label}</span>
          {#if agent.model === m.id}<Check size={14} />{/if}
        </button>
      {/each}
      {#if provider.efforts.length}
        <div class="effort">
          <span>Effort <em>({agent.effort})</em></span>
          <input
            type="range"
            min="0"
            max={provider.efforts.length - 1}
            step="1"
            value={effortIndex}
            aria-label="Effort"
            oninput={(e) => (agent.effort = provider.efforts[Number(e.currentTarget.value)])}
          />
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .command-menu {
    position: relative;
  }

  .trigger {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 1px solid var(--qx-border);
    border-radius: 7px;
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .trigger:hover:not(:disabled),
  .trigger.on {
    background: var(--qx-bg-panel);
    color: var(--qx-text);
  }

  .menu {
    position: absolute;
    left: -34px;
    bottom: calc(100% + 8px);
    width: min(320px, 90vw);
    max-height: 420px;
    overflow: auto;
    padding: var(--qx-space-1);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    box-shadow: 0 16px 40px rgb(0 0 0 / 50%);
    z-index: 60;
  }

  .section {
    padding: var(--qx-space-2) var(--qx-space-2) var(--qx-space-1);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .menu button {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px var(--qx-space-2);
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .menu button:hover:not(:disabled) {
    background: var(--qx-bg-panel);
  }

  .menu button:disabled {
    color: var(--qx-text-muted);
    cursor: default;
  }

  .note {
    margin: 0;
    padding: 2px var(--qx-space-2) var(--qx-space-1);
    color: var(--qx-status-done);
    font-size: var(--qx-text-small);
  }

  .effort {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--qx-space-3);
    padding: 6px var(--qx-space-2) var(--qx-space-2);
  }

  .effort em {
    color: var(--qx-text-muted);
    font-style: normal;
  }

  .effort input {
    width: 120px;
    accent-color: var(--qx-accent);
  }
</style>
