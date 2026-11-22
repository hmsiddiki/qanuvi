<script lang="ts">
  /**
   * The machine switcher (24z3 Part B), at the top of the sidebar: This PC and every paired master, with its colour,
   * its connection and its Inbox questions not read yet. Picking one shows that machine: the whole screen becomes its
   * own. This PC is always in the list. A master's SSH address shows on hover only, so the list stays small.
   * Step 4: a working icon per machine (how many agents work there, on hover), and a machine's right-click menu opens
   * it in a window of its own; picking a machine another window shows brings that window forward.
   * Migration M3c: the machines a master connects to itself show under it, indented on a guide line (at most three
   * levels), and behave as This PC's servers do: their state, picking one connects it first, a right-click opens it
   * in a window of its own. The button shows the way to a nested machine ("VPS › build-box").
   */
  import AppWindow from "@lucide/svelte/icons/app-window";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Check from "@lucide/svelte/icons/check";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Server from "@lucide/svelte/icons/server";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { masters, displayName, parentOf } from "$lib/stores/masters.svelte";
  import { inbox } from "$lib/stores/inbox.svelte";
  import { pick } from "$lib/stores/machines";
  import { switcher } from "$lib/stores/switcher.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import type { MasterSettings } from "$lib/stores/settings.svelte";
  import { working, workingText } from "$lib/stores/working.svelte";
  import { windows } from "$lib/stores/windows.svelte";

  let { collapsed = false, onmanage }: { collapsed?: boolean; onmanage: () => void } = $props();

  let open = $state(false);
  /** The right-click menu of a machine ("" is This PC), with where it opened. */
  let menuFor = $state<{ id: string; x: number; y: number } | null>(null);

  /** Agents working on the machines not shown (the closed switcher's icon). */
  let workingElsewhere = $derived(
    Object.entries(working.counts)
      .filter(([m]) => m !== (switcher.shown ?? ""))
      .reduce((a, [, c]) => a + c, 0),
  );

  const nameOf = (id: string) => {
    if (!id) return "This PC";
    const m = masters.find(id);
    return m ? (parentOf(id) ? masters.route(id) : displayName(m)) : id;
  };

  async function openWindow(id: string) {
    menuFor = null;
    open = false;
    await windows.openMachine(id || null, nameOf(id)).catch(() => {});
  }

  let shownMaster = $derived(switcher.shown ? (masters.find(switcher.shown) ?? null) : null);
  let shownName = $derived(shownMaster ? nameOf(shownMaster.id) : "This PC");
  /** A nested machine on the button (finding n2): the way to it shortens first, its own name stays readable. */
  let shownVia = $derived.by(() => {
    const parent = shownMaster ? parentOf(shownMaster.id) : null;
    return parent ? `${masters.route(parent)} ›` : "";
  });
  let shownOwn = $derived(shownMaster ? displayName(shownMaster) : "This PC");

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
  const reach = (m: MasterSettings) => {
    const how = m.relay
      ? masters.statusOf(m.id).direct
        ? "directly on the home network (the relay if that drops)"
        : "through the relay"
      : m.target === "local"
        ? "on this PC (test master)"
        : `SSH: ${m.target}`;
    const parent = parentOf(m.id);
    // A nested machine is reached by its master, the way that master reaches it.
    if (parent) return `Through ${masters.route(parent)}, ${how}`;
    return how.charAt(0).toUpperCase() + how.slice(1);
  };

  const stateText = (m: MasterSettings) => {
    const s = masters.statusOf(m.id);
    if (s.twin) return `the same computer as ${nameOf(s.twin)}`;
    if (switcher.held?.id === m.id && switcher.held.mode === "lost" && s.state !== "on") return "reconnecting…";
    return s.state === "on"
      ? s.direct
        ? "connected · direct"
        : "connected"
      : s.state === "connecting"
        ? "connecting…"
        : s.state === "error"
          ? "not connected"
          : "off";
  };

  async function choose(id: string | null) {
    // Another window shows it already: that window comes forward, this one stays as it is.
    const other = id === switcher.shown ? null : windows.windowOfMachine(id);
    if (other) {
      open = false;
      await windows.focus(other).catch(() => {});
      return;
    }
    if (id === switcher.shown) {
      open = false;
      // The machine shown cannot be reached (24z3 Part E): picking it again tries it now.
      if (switcher.held) void masters.retryNow();
      return;
    }
    if (id) {
      // The same computer reached another way: that one is shown.
      const twin = masters.statusOf(id).twin;
      if (twin && masters.statusOf(twin).state === "on") return choose(twin);
      const m = masters.find(id);
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
    const t = e.target as HTMLElement;
    if (menuFor && !t.closest(".context")) menuFor = null;
    if (open && !t.closest(".switcher")) open = false;
  }}
  onkeydown={(e) => {
    if (e.key !== "Escape") return;
    if (menuFor) menuFor = null;
    else if (open) open = false;
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
      if (open) void working.refresh();
    }}
  >
    <span class="avatar" style:background={shownMaster?.color || undefined} aria-hidden="true">
      {#if shownMaster}<Server size={13} />{:else}<Monitor size={13} />{/if}
    </span>
    {#if !collapsed}
      {#if shownVia}<span class="via">{shownVia}</span>{/if}
      <span class="name">{shownOwn}</span>
      {#if workingElsewhere > 0}<span class="working" title="{workingText(workingElsewhere)} on other machines"
          ><LoaderCircle size={13} /></span
        >{/if}
      {#if elsewhere > 0}<span class="pill" title="{elsewhere} not read on other machines">{elsewhere}</span>{/if}
      <ChevronDown size={14} />
    {:else if elsewhere > 0}
      <span class="dot-badge" title="{elsewhere} not read on other machines"></span>
    {/if}
  </button>

  {#if open}
    <div class="menu" class:narrow={collapsed} role="listbox" aria-label="Machines">
      <button
        type="button"
        class="machine"
        role="option"
        aria-selected={switcher.shown === null}
        title={["This PC", workingText(working.of(null)), windows.windowOfMachine(null) ? "open in another window" : ""]
          .filter(Boolean)
          .join(" · ")}
        onclick={() => choose(null)}
        oncontextmenu={(e) => {
          e.preventDefault();
          menuFor = { id: "", x: e.clientX, y: e.clientY };
        }}
      >
        <span class="swatch pc"><Monitor size={12} /></span>
        <span class="label">This PC</span>
        {#if windows.windowOfMachine(null)}<span class="elsewhere" aria-label="Open in another window"
            ><AppWindow size={12} /></span
          >{/if}
        {#if working.of(null)}<span class="working" aria-label={workingText(working.of(null))}
            ><LoaderCircle size={13} /></span
          >{/if}
        {#if unread[""]}<span class="pill" title="{unread['']} not read">{unread[""]}</span>{/if}
        <span class="tick"
          >{#if switcher.shown === null}<Check size={13} />{/if}</span
        >
      </button>
      {#each masters.tree as { m, depth } (m.id)}
        {@const st = masters.statusOf(m.id)}
        <button
          type="button"
          class="machine"
          class:nested={depth > 1}
          style:--depth={depth - 1}
          role="option"
          aria-selected={switcher.shown === m.id}
          title={[
            `${displayName(m)}: ${reach(m)}`,
            stateText(m),
            workingText(working.of(m.id)),
            windows.windowOfMachine(m.id) ? "open in another window" : "",
          ]
            .filter(Boolean)
            .join(" · ")}
          disabled={st.state === "connecting"}
          onclick={() => choose(m.id)}
          oncontextmenu={(e) => {
            e.preventDefault();
            menuFor = { id: m.id, x: e.clientX, y: e.clientY };
          }}
        >
          <span class="swatch" style:background={m.color || undefined}><Server size={12} /></span>
          <span class="label">{displayName(m)}</span>
          {#if windows.windowOfMachine(m.id)}<span class="elsewhere" aria-label="Open in another window"
              ><AppWindow size={12} /></span
            >{/if}
          {#if working.of(m.id)}<span class="working" aria-label={workingText(working.of(m.id))}
              ><LoaderCircle size={13} /></span
            >{/if}
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

  {#if menuFor}
    {@const id = menuFor.id}
    <div class="context" role="menu" style:left="{menuFor.x}px" style:top="{menuFor.y}px">
      {#if id === (switcher.shown ?? "")}
        <span class="note">Shown in this window</span>
      {:else}
        <button type="button" role="menuitem" onclick={() => openWindow(id)}>
          <AppWindow size={13} />
          {windows.windowOfMachine(id || null) ? "Go to its window" : "Open in a new window"}
        </button>
      {/if}
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

  .via {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--qx-text-secondary);
    font-weight: 400;
  }

  .via + .name {
    flex: none;
    max-width: 70%;
    margin-right: auto;
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
    /* As wide as the sidebar: a long name shortens instead of pushing its dot, icons and tick out of sight. */
    left: 0;
    right: 0;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }

  /* A collapsed sidebar is only an icon wide: the list keeps a readable width there. */
  .menu.narrow {
    right: auto;
    min-width: 220px;
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

  /* A machine reached through the one above: indented on a guide line, one step per level. */
  .machine.nested {
    position: relative;
    padding-left: calc(8px + var(--depth) * 14px);
  }

  .machine.nested::before {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: calc(16px + (var(--depth) - 1) * 14px);
    border-left: 1px solid var(--qx-border);
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

  .working {
    flex: none;
    display: grid;
    place-items: center;
    color: var(--qx-accent);
  }

  .working :global(svg) {
    animation: qx-working-spin 1.2s linear infinite;
  }

  @keyframes qx-working-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .working :global(svg) {
      animation: none;
    }
  }

  .elsewhere {
    flex: none;
    display: grid;
    place-items: center;
    color: var(--qx-text-secondary);
  }

  .context {
    position: fixed;
    z-index: 40;
    min-width: 190px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
  }

  .context button {
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

  .context button:hover {
    background: var(--qx-bg-raised);
  }

  .context .note {
    display: block;
    padding: 5px 8px;
    color: var(--qx-text-secondary);
  }

  .manage {
    margin-top: 4px;
    border-top: 1px solid var(--qx-border);
    border-radius: 0;
    color: var(--qx-text-secondary);
  }
</style>
