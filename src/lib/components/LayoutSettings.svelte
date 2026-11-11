<script lang="ts">
  /** Settings → Layout (15h): the built-in layouts and the user's own, where the conversation sits, the sidebar's mode. */
  import Check from "@lucide/svelte/icons/check";
  import { layout } from "$lib/layout/layout.svelte";
  import { PRESET_NOTES, type SidebarMode } from "$lib/layout/layout";
  import { panel } from "$lib/stores/panel.svelte";

  const MODES: { id: SidebarMode; label: string }[] = [
    { id: "workspace", label: "Workspace (projects, sessions)" },
    { id: "explorer", label: "Explorer (the project's files)" },
    { id: "search", label: "Search" },
    { id: "git", label: "Git" },
  ];
  let newName = $state("");
  let message = $state("");

  function save(event: SubmitEvent) {
    event.preventDefault();
    const name = layout.saveAs(newName);
    message = name ? `Saved as "${name}".` : "Type a name (up to 40 characters, 20 layouts at most).";
    if (name) newName = "";
  }
</script>

<h2>Layout</h2>
<p class="intro">
  How the window is arranged. Pick one; drag the dividers and use the buttons in the title bar to change it, then save
  it under a name of your own.
</p>

<div class="list">
  {#each Object.keys(layout.named) as name (name)}
    <div class="item" class:on={name === layout.active}>
      <button type="button" class="pick" onclick={() => layout.apply(name)}>
        <span class="tick"
          >{#if name === layout.active}<Check size={14} />{/if}</span
        >
        <span class="text">
          <strong>{name}</strong>
          <span class="muted">{PRESET_NOTES[name] ?? "Your own layout."}</span>
        </span>
      </button>
      {#if !layout.isPreset(name)}
        <button type="button" class="link" onclick={() => layout.remove(name)}>Remove</button>
      {/if}
    </div>
  {/each}
</div>

<h3>The current layout</h3>
<div class="grid">
  <label>
    Conversation
    <select
      value={layout.state.agentSide}
      onchange={(e) => layout.setAgentSide(e.currentTarget.value as "left" | "right")}
    >
      <option value="left">Left of the editor</option>
      <option value="right">Right of the editor</option>
    </select>
  </label>
  <label>
    Sidebar shows
    <select value={panel.view} onchange={(e) => layout.setSidebarMode(e.currentTarget.value as SidebarMode)}>
      {#each MODES as m (m.id)}<option value={m.id}>{m.label}</option>{/each}
    </select>
  </label>
</div>
<form class="save" onsubmit={save}>
  <input placeholder="Save the current layout as…" bind:value={newName} />
  <button type="submit" class="btn">Save</button>
  {#if message}<span class="muted">{message}</span>{/if}
</form>

<style>
  h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  h3 {
    margin: 22px 20px 8px;
    font-size: 13px;
  }

  .intro {
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .list {
    display: grid;
    gap: 6px;
    margin: 0 20px;
  }

  .item {
    display: flex;
    align-items: center;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
  }

  .item.on {
    border-color: var(--qx-accent);
  }

  .pick {
    flex: 1;
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 9px 12px;
    border: none;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .tick {
    width: 14px;
    color: var(--qx-accent);
  }

  .text {
    display: grid;
    gap: 2px;
    font-size: 12.5px;
  }

  .muted {
    color: var(--qx-text-secondary);
    font-size: 12px;
  }

  .link {
    margin-right: 12px;
    border: none;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin: 0 20px;
    font-size: 12.5px;
  }

  label {
    display: grid;
    gap: 4px;
    color: var(--qx-text-secondary);
  }

  select,
  input {
    padding: 6px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
  }

  .save {
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 14px 20px;
    font-size: 12.5px;
  }

  .save input {
    flex: 0 1 280px;
  }

  .btn {
    padding: 6px 14px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-raised);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }
</style>
