<script lang="ts">
  /**
   * The machine switcher (24z3 Part B), at the top of the sidebar: This PC and every paired master, with its colour,
   * its connection and its Inbox questions not read yet. Picking one shows that machine: the whole screen becomes its
   * own. This PC is always in the list. A master's SSH address shows on hover only, so the list stays small.
   */
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Check from "@lucide/svelte/icons/check";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Server from "@lucide/svelte/icons/server";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { masters, displayName } from "$lib/stores/masters.svelte";
  import { inbox } from "$lib/stores/inbox.svelte";
  import { pick } from "$lib/stores/machines";
  import { switcher } from "$lib/stores/switcher.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import type { MasterSettings } from "$lib/stores/settings.svelte";

  let { collapsed = false, onmanage }: { collapsed?: boolean; onmanage: () => void } = $props();

  let open = $state(false);

  let shownMaster = $derived(masters.list.find((m) => m.id === switcher.shown) ?? null);
  let shownName = $derived(shownMaster ? displayName(shownMaster) : "This PC");

  /** Inbox items not read yet, per machine ("" is This PC). */
  let unread = $derived.by(() => {
    const n: Record<string, number> = {};
    for (const i of inbox.items) if (!i.read) n[i.master ?? ""] = (n[i.master ?? ""] ?? 0) + 1;
    return n;
  });
  /** Questions waiting on the machines not shown: a badge on the closed switcher. */
  let elsewhere = $derived(
    Object.entries(unread)
      .filter(([m]) => m !== (switcher.shown ?? ""))
      .reduce((a, [, c]) => a + c, 0),
  );

  /** On hover: how the machine is reached. */
  const reach = (m: MasterSettings) =>
    m.relay ? "Through the relay" : m.target === "local" ? "On this PC (test master)" : `SSH: ${m.target}`;

  const stateText = (m: MasterSettings) => {
    const s = masters.statusOf(m.id);
    if (switcher.held?.id === m.id && switcher.held.mode === "lost" && s.state !== "on") return "reconnecting…";
    return s.state === "on"
      ? "connected"
      : s.state === "connecting"
        ? "connecting…"
        : s.state === "error"
          ? "not connected"
          : "off";
  };

  async function choose(id: string | null) {
    if (id === switcher.shown) {
      open = false;
      // The machine shown cannot be reached (24z3 Part E): picking it again tries it now.
      if (switcher.held) void masters.retryNow();
      return;
    }
    if (id) {
      const m = masters.list.find((x) => x.id === id);
      if (!m) return;
      if (masters.statusOf(id).state !== "on") {
        // Not connected: connect first; the screen stays on the machine shown until it answers.
        await masters.connect(m);
        if (masters.statusOf(id).state !== "on") {
          // No answer: its own "not connected" screen with Reconnect (Part E), never the screen before taken for it.
          open = false;
          if (projects.persist) switcher.remember(id);
          await masters.showUnreachable(id);
          return;
        }
      }
    }
    open = false;
    await pick(id, projects.persist);
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (open && !(e.target as HTMLElement).closest(".switcher")) open = false;
  }}
  onkeydown={(e) => {
    if (open && e.key === "Escape") open = false;
  }}
/>

<div class="switcher">
  <button
    type="button"
    class="current"
    class:busy={switcher.switching !== null}
    title={shownMaster ? `${shownName}: ${reach(shownMaster)}` : "This PC"}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label="Machine: {shownName}"
    onclick={() => {
      open = !open;
    }}
  >
    <span class="avatar" style:background={shownMaster?.color || undefined} aria-hidden="true">
      {#if shownMaster}<Server size={13} />{:else}<Monitor size={13} />{/if}
    </span>
    {#if !collapsed}
      <span class="name">{shownName}</span>
      {#if elsewhere > 0}<span class="pill" title="{elsewhere} not read on other machines">{elsewhere}</span>{/if}
      <ChevronDown size={14} />
    {:else if elsewhere > 0}
      <span class="dot-badge" title="{elsewhere} not read on other machines"></span>
    {/if}
  </button>

  {#if open}
    <div class="menu" role="listbox" aria-label="Machines">
      <button
        type="button"
        class="machine"
        role="option"
        aria-selected={switcher.shown === null}
        title="This PC"
        onclick={() => choose(null)}
      >
        <span class="swatch pc"><Monitor size={12} /></span>
        <span class="label">This PC</span>
        {#if unread[""]}<span class="pill" title="{unread['']} not read">{unread[""]}</span>{/if}
        <span class="tick"
          >{#if switcher.shown === null}<Check size={13} />{/if}</span
        >
      </button>
      {#each masters.list as m (m.id)}
        {@const st = masters.statusOf(m.id)}
        <button
          type="button"
          class="machine"
          role="option"
          aria-selected={switcher.shown === m.id}
          title="{displayName(m)}: {reach(m)} · {stateText(m)}"
          disabled={st.state === "connecting"}
          onclick={() => choose(m.id)}
        >
          <span class="swatch" style:background={m.color || undefined}><Server size={12} /></span>
          <span class="label">{displayName(m)}</span>
          <span class="state {st.state}" aria-label={stateText(m)}></span>
          {#if unread[m.id]}<span class="pill" title="{unread[m.id]} not read">{unread[m.id]}</span>{/if}
          <span class="tick"
            >{#if switcher.shown === m.id}<Check size={13} />{/if}</span
          >
        </button>
      {/each}
      <button
        type="button"
        class="manage"
        onclick={() => {
          open = false;
          onmanage();
        }}><Settings2 size={13} /> Servers…</button
      >
    </div>
  {/if}
</div>

<style>
  .switcher {
    position: relative;
    margin-bottom: var(--qx-space-3);
  }

  button {
    font: inherit;
  }

  .current {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    width: 100%;
    padding: 6px var(--qx-space-2);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .current:hover {
    background: var(--qx-bg-raised);
  }

  .current.busy {
    opacity: 0.7;
    cursor: progress;
  }

  .avatar {
    position: relative;
    width: 22px;
    height: 22px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 6px;
    background: var(--qx-text);
    color: var(--qx-bg-window);
  }

  .name {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .pill {
    min-width: 16px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--qx-accent);
    color: #fff;
    font-size: 10px;
    font-weight: 600;
    text-align: center;
  }

  .dot-badge {
    width: 7px;
    height: 7px;
    margin-left: -10px;
    margin-top: -14px;
    border-radius: 50%;
    background: var(--qx-accent);
  }

  .menu {
    position: absolute;
    z-index: 30;
    top: calc(100% + 4px);
    left: 0;
    min-width: 220px;
    max-width: 300px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }

  .machine,
  .manage {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text);
    text-align: left;
    cursor: pointer;
  }

  .machine:hover:not(:disabled),
  .manage:hover {
    background: var(--qx-bg-raised);
  }

  .machine:disabled {
    cursor: progress;
    opacity: 0.7;
  }

  .swatch {
    width: 18px;
    height: 18px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 5px;
    background: var(--qx-text-secondary);
    color: var(--qx-bg-window);
  }

  .swatch.pc {
    background: var(--qx-text);
  }

  .label {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .state {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--qx-text-secondary);
  }

  .state.on {
    background: #3fb950;
  }

  .state.connecting {
    background: #d29922;
  }

  .state.error {
    background: #f85149;
  }

  .tick {
    width: 14px;
    flex: none;
    display: grid;
    place-items: center;
    color: var(--qx-accent);
  }

  .manage {
    margin-top: 4px;
    border-top: 1px solid var(--qx-border);
    border-radius: 0;
    color: var(--qx-text-secondary);
  }
</style>
