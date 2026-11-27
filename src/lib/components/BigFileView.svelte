<script lang="ts">
  /**
   * s2 (ST2): a file over 10 MB opens read-only, a part (about 1 MB) at a time, with colours and code help off, like
   * VS Code's large-file mode. Parts keep each answer small, also over the relay.
   */
  import { api, errorText, type FilePart } from "$lib/api";

  let { projectId, path }: { projectId: string; path: string } = $props();

  let text = $state("");
  let next = $state(0);
  let size = $state(0);
  let loading = $state(false);
  let error = $state("");

  const mb = (n: number) => `${(n / (1024 * 1024)).toFixed(1)} MB`;

  async function load(from: number, replace: boolean) {
    loading = true;
    error = "";
    try {
      const p: FilePart = await api.files.readPart(projectId, path, from);
      text = replace ? p.text : text + p.text;
      next = p.next;
      size = p.size;
    } catch (e) {
      error = errorText(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void projectId;
    void path;
    text = "";
    void load(0, true);
  });
</script>

<div class="big">
  <div class="bar">
    <span
      >Large file ({mb(size)}): opened read-only, without colours or code help. Shown: {mb(next)} of {mb(size)}.</span
    >
    {#if next < size}
      <button type="button" disabled={loading} onclick={() => void load(next, false)}>Load more</button>
    {/if}
  </div>
  {#if error}<p class="err">{error}</p>{/if}
  <pre>{text}</pre>
</div>

<style>
  .big {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .bar {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 4px 8px;
    font-size: var(--qx-text-small, 12px);
    color: var(--qx-text-secondary);
    border-bottom: 1px solid var(--qx-border);
  }

  .bar button {
    margin-left: auto;
  }

  pre {
    flex: 1;
    margin: 0;
    padding: 6px 8px;
    overflow: auto;
    font-family: var(--qx-font-mono, monospace);
    font-size: 13px;
    user-select: text;
  }

  .err {
    color: var(--qx-danger, #c33);
    padding: 4px 8px;
  }
</style>
