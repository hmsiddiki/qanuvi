<script lang="ts">
  /** Settings → Appearance (15h): the editor's font size and the size of the whole interface. */
  import { settings } from "$lib/stores/settings.svelte";

  let a = $derived(settings.value.appearance);
  const set = (patch: Partial<typeof a>) => settings.set("appearance", { ...a, ...patch });
</script>

<h2>Appearance</h2>
<p class="intro">How big things are. Changes show at once.</p>

<div class="rows">
  <label>
    <span><strong>Editor font size</strong> <span class="muted">{a.editorFontSize} px</span></span>
    <input
      type="range"
      min="10"
      max="24"
      value={a.editorFontSize}
      oninput={(e) => set({ editorFontSize: Number(e.currentTarget.value) })}
    />
  </label>
  <label>
    <span><strong>Interface size</strong> <span class="muted">{a.uiScale}%</span></span>
    <input
      type="range"
      min="80"
      max="150"
      step="5"
      value={a.uiScale}
      onchange={(e) => set({ uiScale: Number(e.currentTarget.value) })}
    />
  </label>
  <button type="button" class="link" onclick={() => set({ editorFontSize: 13, uiScale: 100 })}
    >Back to the usual sizes</button
  >
</div>

<style>
  h2 {
    margin: 18px 20px 4px;
    font-size: 16px;
  }

  .intro {
    margin: 0 20px 14px;
    color: var(--qx-text-secondary);
    font-size: 12.5px;
  }

  .rows {
    display: grid;
    gap: 18px;
    max-width: 480px;
    margin: 0 20px;
    font-size: 12.5px;
  }

  label {
    display: grid;
    gap: 6px;
  }

  .muted {
    margin-left: 6px;
    color: var(--qx-text-secondary);
  }

  .link {
    justify-self: start;
    border: none;
    background: none;
    padding: 0;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }
</style>
