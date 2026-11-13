<script lang="ts">
  /** The Inbox (24f, 15f): sessions waiting for the user, plan run reports, sign-ins to renew, newer knowledge. */
  import { onMount } from "svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { inbox, isOpenQuestion, type InboxItem } from "$lib/stores/inbox.svelte";
  import InboxQuestion from "./InboxQuestion.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { ago } from "$lib/stores/git.svelte";

  let waiting = $derived(agent.sessions.filter((s) => s.status === "waiting"));
  let questions = $derived(inbox.items.filter(isOpenQuestion));
  let happened = $derived(inbox.items.filter((i) => !isOpenQuestion(i)));

  /** How an answered or closed question ended, in a few words. */
  function outcome(item: InboxItem): string {
    if (item.status === "closed") return "No longer asked.";
    const label = item.form?.choices.find((c) => c.id === item.answer?.choice)?.label ?? item.answer?.choice ?? "";
    const said = item.answer?.text?.trim();
    const where = item.answeredBy && item.answeredBy !== "main" ? ` (in window ${item.answeredBy})` : "";
    return `You answered: ${label}${said ? ` · "${said}"` : ""}${where}`;
  }

  const KIND: Record<InboxItem["kind"], string> = {
    run: "Plan run",
    "sign-in": "Sign-in",
    knowledge: "Knowledge",
    chats: "Chats",
  };

  function open(item: InboxItem) {
    inbox.markRead(item.id);
    ui.inboxOpen = false;
    if (!item.open) return;
    if (item.open.projectId && projects.current?.id !== item.open.projectId) projects.select(item.open.projectId);
    if (!layout.state.workVisible) layout.toggleWork();
    ui.show(item.open.view);
  }

  function openSession(id: string) {
    ui.inboxOpen = false;
    if (!layout.state.agentVisible) layout.toggleAgent();
    agent.select(id);
  }

  // A clicked desktop notification opened the Inbox at its item: brought into view and marked for a moment.
  let root: HTMLDivElement | undefined = $state();
  $effect(() => {
    const id = ui.inboxFocus;
    if (!id || !root) return;
    void inbox.items.length;
    const el = root.querySelector(`[data-id="${CSS.escape(id)}"]`);
    el?.scrollIntoView({ block: "nearest" });
    const t = setTimeout(() => {
      if (ui.inboxFocus === id) ui.inboxFocus = null;
    }, 4000);
    return () => clearTimeout(t);
  });

  // What is on screen counts as seen once the user has looked for a moment.
  onMount(() => {
    const t = setTimeout(() => inbox.markRead(), 4000);
    return () => clearTimeout(t);
  });
</script>

<div class="inbox" bind:this={root}>
  <header>
    <h2>Inbox</h2>
    {#if happened.length}
      <button type="button" class="link" onclick={() => inbox.clear()}>Clear all</button>
    {/if}
  </header>

  {#if questions.length}
    <h3>Needs your answer</h3>
    {#each questions as item (item.id)}
      <div class="item unread" data-id={item.id} class:focus={ui.inboxFocus === item.id}>
        <div class="line">
          <span class="kind">{KIND[item.kind]}</span>
          <strong class="title">{item.title}</strong>
          <span class="muted small when">{ago(Math.floor(item.at / 1000))}</span>
        </div>
        <InboxQuestion {item} onopen={open} />
      </div>
    {/each}
  {/if}

  {#if waiting.length}
    <h3>Waiting for you</h3>
    {#each waiting as s (s.id)}
      <button type="button" class="row" onclick={() => openSession(s.id)}>
        <span class="kind">Session</span>
        <span class="title">{s.title}</span>
        <span class="muted small">needs your approval</span>
      </button>
    {/each}
  {/if}

  {#if happened.length}
    <h3>What happened</h3>
    {#each happened as item (item.id)}
      <div class="item" class:unread={!item.read} data-id={item.id} class:focus={ui.inboxFocus === item.id}>
        <div class="line">
          <span class="kind">{KIND[item.kind]}</span>
          <strong class="title">{item.title}</strong>
          <span class="muted small when">{ago(Math.floor(item.at / 1000))}</span>
        </div>
        {#if item.text}<p class="text">{item.text}</p>{/if}
        {#if item.type === "answer"}<p class="text">{outcome(item)}</p>{/if}
        <div class="actions">
          {#if item.open}<button type="button" class="btn" onclick={() => open(item)}>{item.open.label}</button>{/if}
          <button type="button" class="link" onclick={() => inbox.remove(item.id)}>Remove</button>
        </div>
      </div>
    {/each}
  {:else if !waiting.length && !questions.length}
    <p class="muted">
      Nothing here. Questions a plan run waits on, run reports, sign-ins that need renewing and newer knowledge show up
      here.
    </p>
  {/if}
</div>

<style>
  .inbox {
    padding: 12px 14px;
    font-size: 12.5px;
  }

  header {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }

  h3 {
    margin: 14px 0 6px;
    font-size: 12px;
    color: var(--qx-text-secondary);
    font-weight: 600;
  }

  .row,
  .item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 8px 10px;
    margin-bottom: 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card, 8px);
    background: var(--qx-bg-raised);
    color: inherit;
    font: inherit;
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: baseline;
    cursor: pointer;
  }

  .item.unread {
    border-color: var(--qx-accent);
  }

  .item.focus {
    border-color: var(--qx-accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--qx-accent) 45%, transparent);
  }

  .line {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .kind {
    font-size: 11px;
    color: var(--qx-text-secondary);
    min-width: 62px;
  }

  .when {
    margin-left: auto;
  }

  .text {
    margin: 4px 0 0 70px;
    color: var(--qx-text-secondary);
    white-space: pre-wrap;
  }

  .actions {
    display: flex;
    gap: 10px;
    margin: 6px 0 0 70px;
    align-items: center;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .btn {
    padding: 3px 10px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }
</style>
