<script lang="ts">
  /**
   * Settings (15h): one page over the main area, with its parts as tabs on the left and a search box. The agent pane
   * and the work area stay as they were underneath (terminals keep running); × or Esc goes back to them.
   */
  import X from "@lucide/svelte/icons/x";
  import Bot from "@lucide/svelte/icons/bot";
  import Boxes from "@lucide/svelte/icons/boxes";
  import Users from "@lucide/svelte/icons/users";
  import Server from "@lucide/svelte/icons/server";
  import Brain from "@lucide/svelte/icons/brain";
  import UserRound from "@lucide/svelte/icons/user-round";
  import Search from "@lucide/svelte/icons/search";
  import { ui, type SettingsTab } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import AgentsView from "./AgentsView.svelte";
  import OwnModelsPanel from "./OwnModelsPanel.svelte";
  import TeamPanel from "./TeamPanel.svelte";
  import MastersView from "./MastersView.svelte";
  import MemoryView from "./MemoryView.svelte";
  import AccountView from "./AccountView.svelte";
  import LayoutSettings from "./LayoutSettings.svelte";
  import AppearanceSettings from "./AppearanceSettings.svelte";
  import UpdatesSettings from "./UpdatesSettings.svelte";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import LayoutPanelLeft from "@lucide/svelte/icons/layout-panel-left";
  import Palette from "@lucide/svelte/icons/palette";

  const TABS: { id: SettingsTab; label: string; icon: typeof Bot; words: string }[] = [
    {
      id: "agents",
      label: "Agents",
      icon: Bot,
      words: "ai features claude code paid agent model permission mode effort address proxy config files routing",
    },
    {
      id: "models",
      label: "Models and providers",
      icon: Boxes,
      words: "free local ollama openrouter omniroute keys models setup wizard guides knowledge sign-in combo",
    },
    {
      id: "team",
      label: "Team",
      icon: Users,
      words: "decider paid decides free works priority direct save tokens solve",
    },
    {
      id: "servers",
      label: "Servers",
      icon: Server,
      words: "master vps ssh remote access relay devices data folder location",
    },
    {
      id: "memory",
      label: "Memory",
      icon: Brain,
      words: "memory entries preferences facts memory disk sync passphrase",
    },
    { id: "account", label: "Account", icon: UserRound, words: "sign in qanuvi account email" },
    {
      id: "layout",
      label: "Layout",
      icon: LayoutPanelLeft,
      words: "layouts presets vs code coding side screen review sidebar conversation left right save",
    },
    { id: "appearance", label: "Appearance", icon: Palette, words: "font size editor interface zoom scale" },
    {
      id: "updates",
      label: "Updates",
      icon: RefreshCw,
      words: "update version channel check restart reload window new release",
    },
  ];

  let query = $state("");
  let shown = $derived(
    query.trim()
      ? TABS.filter((t) =>
          query
            .toLowerCase()
            .split(/\s+/)
            .every((q) => `${t.label} ${t.words}`.toLowerCase().includes(q)),
        )
      : TABS,
  );
  let tab = $derived(ui.settings ?? "agents");

  function keydown(e: KeyboardEvent) {
    if (e.key !== "Escape" || !ui.settings) return;
    // Esc in the search box first clears it.
    if (e.target instanceof HTMLInputElement && e.target.classList.contains("find") && query) {
      query = "";
      return;
    }
    ui.closeSettings();
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="settings" role="dialog" aria-label="Settings">
  <nav class="side" aria-label="Settings sections">
    <div class="head">
      <strong>Settings</strong>
    </div>
    <label class="search">
      <Search size={13} />
      <input
        class="find"
        type="search"
        placeholder="Search settings"
        bind:value={query}
        onkeydown={(e) => e.key === "Enter" && shown[0] && ui.openSettings(shown[0].id)}
      />
    </label>
    <ul>
      {#each shown as t (t.id)}
        <li>
          <button type="button" class:current={tab === t.id} onclick={() => ui.openSettings(t.id)}>
            <t.icon size={15} />
            <span>{t.label}</span>
          </button>
        </li>
      {:else}
        <li class="none">Nothing matches.</li>
      {/each}
    </ul>
  </nav>

  <section class="content">
    <button
      type="button"
      class="close"
      title="Close settings (Esc)"
      aria-label="Close settings"
      onclick={() => ui.closeSettings()}
    >
      <X size={16} />
    </button>
    <div class="page">
      {#if tab === "agents"}
        <AgentsView team={false} ownModels={false} />
      {:else if tab === "models"}
        <h2>Models and providers</h2>
        <p class="intro">
          The models Qanuvi core uses: free ones online and, if you want, on this computer. Their order, keys, setup
          guides and OmniRoute.
        </p>
        {#if settings.value.ai}<OwnModelsPanel />{:else}<p class="intro">AI features are off (Agents).</p>{/if}
      {:else if tab === "team"}
        <h2>Team</h2>
        <p class="intro">Who decides and who works when a paid agent and free models are both there.</p>
        {#if settings.value.ai}<TeamPanel />{:else}<p class="intro">AI features are off (Agents).</p>{/if}
      {:else if tab === "servers"}
        <MastersView />
      {:else if tab === "memory"}
        <MemoryView />
      {:else if tab === "account"}
        <AccountView />
      {:else if tab === "layout"}
        <LayoutSettings />
      {:else if tab === "appearance"}
        <AppearanceSettings />
      {:else if tab === "updates"}
        <UpdatesSettings />
      {/if}
    </div>
  </section>
</div>

<style>
  .settings {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    background: var(--qx-bg-panel);
    border-left: 1px solid var(--qx-border);
  }

  .side {
    flex: none;
    width: 220px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 10px;
    border-right: 1px solid var(--qx-border);
    background: var(--qx-bg-raised);
  }

  .head {
    padding: 0 6px;
    font-size: 14px;
  }

  .search {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 5px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    color: var(--qx-text-secondary);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    font-size: 12.5px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li button {
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    padding: 7px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }

  li button:hover {
    color: var(--qx-text);
  }

  li button.current {
    background: var(--qx-bg-hover, rgb(255 255 255 / 6%));
    color: var(--qx-text);
  }

  .none {
    padding: 6px 8px;
    color: var(--qx-text-secondary);
    font-size: 12px;
  }

  .content {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow-y: auto;
  }

  .page {
    max-width: 860px;
    padding: 4px 8px 32px;
  }

  .page > h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  .page > .intro {
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .page > :global(.panel),
  .page > :global(section.team) {
    margin: 0 20px;
  }

  .close {
    position: sticky;
    top: 10px;
    float: right;
    z-index: 1;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    margin: 10px 12px 0 0;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-raised);
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .close:hover {
    color: var(--qx-text);
  }
</style>
