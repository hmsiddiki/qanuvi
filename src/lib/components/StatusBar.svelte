<script lang="ts">
  /**
   * The thin bar along the bottom of the window, as in VS Code: the account (with its popup), the current project's
   * git branch, servers, the memory disk, agents at work and this month's agent cost. Each item opens its page.
   */
  import { connectionName, omni } from "$lib/stores/omniroute.svelte";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import UserRound from "@lucide/svelte/icons/user-round";
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Server from "@lucide/svelte/icons/server";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Bot from "@lucide/svelte/icons/bot";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { api, type GitStatus } from "$lib/api";
  import { account } from "$lib/stores/account.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { gitState, ago } from "$lib/stores/git.svelte";
  import { masters } from "$lib/stores/masters.svelte";
  import { memoryDisk } from "$lib/stores/memoryDisk.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { ui, type WorkView } from "$lib/stores/ui.svelte";
  import { layout } from "$lib/layout/layout.svelte";
  import { formatCost } from "$lib/agent/events";
  import { monthStart } from "$lib/plan/routing";
  import { remoteAccess } from "$lib/stores/remoteAccess.svelte";
  import MonitorSmartphone from "@lucide/svelte/icons/monitor-smartphone";

  let menu = $state(false);
  let git = $state<GitStatus | null>(null);
  let monthCost = $state<number | null>(null);

  function show(view: WorkView) {
    menu = false;
    if (!layout.state.workVisible) layout.toggleWork();
    ui.show(view);
  }

  // The branch: when the project or the repository changes, after agent turns, and every 15 seconds.
  $effect(() => {
    const p = projects.current;
    void gitState.changed;
    void agent.turnsEnded;
    if (!p) {
      git = null;
      return;
    }
    const load = () =>
      api.git
        .status(p.id)
        .then((s) => (git = s))
        .catch(() => (git = null));
    load();
    const timer = setInterval(() => document.visibilityState === "visible" && load(), 15000);
    return () => clearInterval(timer);
  });

  // This month's agent cost, after each agent turn.
  $effect(() => {
    void agent.turnsEnded;
    if (!settings.value.ai) return;
    api.usage
      .since(monthStart())
      .then((u) => (monthCost = u.cost_usd))
      .catch(() => (monthCost = null));
  });

  let running = $derived(agent.sessions.filter((s) => s.status === "running").length);
  let waiting = $derived(agent.sessions.filter((s) => s.status === "waiting").length);
  let connected = $derived(masters.list.filter((m) => masters.statusOf(m.id).state === "on").length);
  let serverProblem = $derived(masters.list.some((m) => masters.statusOf(m.id).state === "error"));
  let diskOn = $derived(settings.value.memoryDisk.mode !== "off");
  let initial = $derived((account.account?.name || account.account?.email || "").slice(0, 1).toUpperCase());

  function outside(e: MouseEvent) {
    if (menu && !(e.target as HTMLElement).closest(".account-area")) menu = false;
  }
</script>

<svelte:window onclick={outside} />

<footer class="status">
  <div class="account-area">
    <button
      type="button"
      class="item account"
      title={account.account ? `${account.account.name} (Qanuvi account)` : "Qanuvi account: sign in"}
      onclick={() => (menu = !menu)}
    >
      {#if account.account}<span class="avatar">{initial || "Q"}</span>{:else}<UserRound size={13} />{/if}
    </button>
    {#if menu}
      <div class="menu" role="menu">
        {#if account.account}
          <div class="who">
            <strong>{account.account.name || "Signed in"}</strong>
            {#if account.account.email}<span>{account.account.email}</span>{/if}
          </div>
          <button type="button" role="menuitem" onclick={() => show("account")}>Account</button>
          <button
            type="button"
            role="menuitem"
            onclick={() => {
              menu = false;
              account.signOut();
            }}>Sign out</button
          >
        {:else}
          <button
            type="button"
            role="menuitem"
            disabled={account.waiting}
            onclick={() => {
              menu = false;
              show("account");
              account.signIn();
            }}>Sign in to Qanuvi…</button
          >
          <button type="button" role="menuitem" onclick={() => show("account")}>About the Qanuvi account</button>
        {/if}
      </div>
    {/if}
  </div>

  {#if git?.is_repo}
    <button type="button" class="item" title="Git: {git.branch || 'detached'}" onclick={() => show("diff")}>
      <GitBranch size={12} />
      <span>{git.branch || "detached"}</span>
      {#if git.upstream && (git.behind || git.ahead)}
        <span class="sync"
          >{#if git.behind}<ArrowDown size={11} />{git.behind}{/if}{#if git.ahead}<ArrowUp
              size={11}
            />{git.ahead}{/if}</span
        >
      {/if}
      {#if git.changes.length}<span class="dim">· {git.changes.length}</span>{/if}
    </button>
  {/if}

  {#if masters.list.length}
    <button
      type="button"
      class="item"
      class:warn={serverProblem}
      title="Servers: {connected} of {masters.list.length} connected"
      onclick={() => show("servers")}
    >
      <Server size={12} />
      <span>{connected}/{masters.list.length}</span>
    </button>
  {/if}

  {#if diskOn}
    <button
      type="button"
      class="item"
      class:warn={!!memoryDisk.error}
      title={memoryDisk.error ||
        (memoryDisk.last ? `Memory disk: last synced ${ago(memoryDisk.last.at)}` : "Memory disk: not synced yet")}
      onclick={() => show("memory")}
    >
      {#if memoryDisk.busy}<LoaderCircle size={12} class="spin" />{:else}<HardDrive size={12} />{/if}
      <span>{memoryDisk.error ? "Sync problem" : memoryDisk.last ? ago(memoryDisk.last.at) : "not synced"}</span>
    </button>
  {/if}

  {#if remoteAccess.status?.sessions.length}
    {@const who = [...new Set(remoteAccess.status.sessions.map((s) => s.device))].join(", ")}
    <span class="item static controlled" title="Being reached from {who} through the relay">
      <MonitorSmartphone size={12} />
      <span>Used from {who}</span>
    </span>
    <button type="button" class="item stop" title="End every remote connection now" onclick={() => remoteAccess.stop()}>
      Stop
    </button>
  {/if}

  {#if omni.problems.length}
    {@const first = omni.problems[0]}
    <button
      type="button"
      class="item warn"
      title={omni.problems
        .map((c) => `${connectionName(c)}: ${c.state}${c.problem ? ` (${c.problem})` : ""}`)
        .join("\n")}
      onclick={() => show("agents")}
    >
      <KeyRound size={12} />
      <span
        >{first.state === "needs sign-in"
          ? `${connectionName(first)}: sign in again`
          : `${connectionName(first)}: not working`}{omni.problems.length > 1
          ? ` (+${omni.problems.length - 1})`
          : ""}</span
      >
    </button>
  {/if}

  <span class="spacer"></span>

  {#if settings.value.ai}
    {#if running || waiting}
      <span class="item static" title="{running} working, {waiting} waiting for you">
        <LoaderCircle size={12} class="spin" />
        <span>{running} working{waiting ? `, ${waiting} waiting` : ""}</span>
      </span>
    {/if}
    {#if monthCost !== null}
      <button
        type="button"
        class="item"
        title="Agent cost this month (all sessions on this computer)"
        onclick={() => show("agents")}
      >
        <Bot size={12} />
        <span>{formatCost(monthCost)} this month</span>
      </button>
    {/if}
  {/if}
</footer>

<style>
  .status {
    flex: none;
    height: 24px;
    display: flex;
    align-items: stretch;
    gap: 2px;
    padding: 0 6px;
    border-top: 1px solid var(--qx-border);
    background: var(--qx-bg-window);
    color: var(--qx-text-secondary);
    font-size: 11.5px;
    user-select: none;
  }

  .item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 7px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .item:hover:not(.static) {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
    color: var(--qx-text);
  }

  .item.static {
    cursor: default;
  }

  .item.warn {
    color: #e5534b;
  }

  .controlled {
    color: #fff;
    background: #c2410c;
  }

  .item.stop {
    color: #fff;
    background: #9a3412;
    font-weight: 600;
  }

  .dim {
    color: var(--qx-text-muted);
  }

  .sync {
    display: inline-flex;
    align-items: center;
    gap: 1px;
  }

  .spacer {
    flex: 1;
  }

  .account-area {
    position: relative;
    display: flex;
  }

  .avatar {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--qx-accent);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
  }

  .menu {
    position: absolute;
    left: 0;
    bottom: 28px;
    z-index: 50;
    min-width: 220px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
  }

  .menu .who {
    display: grid;
    gap: 2px;
    padding: 6px 8px 8px;
    border-bottom: 1px solid var(--qx-border);
    margin-bottom: 4px;
    color: var(--qx-text);
  }

  .menu .who span {
    color: var(--qx-text-muted);
  }

  .menu button {
    display: block;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .menu button:hover:not(:disabled) {
    background: var(--qx-bg-hover, rgba(127, 127, 127, 0.15));
  }

  .status :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
