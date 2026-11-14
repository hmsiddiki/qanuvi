<script lang="ts">
  /** Settings → Appearance (15h): the editor's font size, the size of the whole interface, the minimap (15i). */
  import { settings } from "$lib/stores/settings.svelte";

  let a = $derived(settings.value.appearance);
  const set = (patch: Partial<typeof a>) => settings.set("appearance", { ...a, ...patch });
</script>

<h2>Appearance</h2>
<p class="intro">How big things are and what the editor shows. Changes show at once.</p>

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
  <label class="check">
    <input type="checkbox" checked={a.minimap} onchange={(e) => set({ minimap: e.currentTarget.checked })} />
    <span
      ><strong>Minimap</strong>
      <span class="muted"
        >A small picture of the file beside the editor, with errors, git changes and search hits. Click or drag it to
        scroll.</span
      ></span
    >
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

  label.check {
    grid-template-columns: auto 1fr;
    align-items: start;
    gap: 8px;
  }

  label.check .muted {
    display: block;
    margin: 2px 0 0;
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
