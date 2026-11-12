<script lang="ts" module>
  /** Stored images already read, by file name (a conversation shown again does not read them again). */
  const loaded = new Map<string, Promise<string>>();
</script>

<script lang="ts">
  /**
   * An image in the conversation or in the message box: a thumbnail; a click shows it whole over the window (Esc or a
   * click closes it). A stored image (sent earlier) is read from the data folder.
   */
  import { invoke } from "@tauri-apps/api/core";
  import type { ShownImage } from "$lib/agent/events";

  let { image, size = 160 }: { image: ShownImage; size?: number } = $props();

  let src = $state("");
  let missing = $state(false);
  let open = $state(false);

  $effect(() => {
    const { src: inHand, file } = image;
    if (inHand) {
      src = inHand;
      return;
    }
    if (!file) return;
    let p = loaded.get(file);
    if (!p) {
      p = invoke<string>("attachment_image", { file });
      loaded.set(file, p);
      p.catch(() => loaded.delete(file));
    }
    p.then((s) => (src = s)).catch(() => (missing = true));
  });

  function keydown(e: KeyboardEvent) {
    if (open && e.key === "Escape") {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window onkeydown={keydown} />

{#if src}
  <button
    type="button"
    class="thumb"
    style:max-width="{size}px"
    style:max-height="{size}px"
    title="{image.name} (click to view)"
    aria-label="View {image.name}"
    onclick={() => (open = true)}
  >
    <img {src} alt={image.name} style:max-width="{size}px" style:max-height="{size}px" />
  </button>
{:else if missing}
  <span class="gone" title="This image is no longer stored">🖼 {image.name}</span>
{/if}

{#if open}
  <div class="viewer" role="presentation" onclick={() => (open = false)}>
    <img {src} alt={image.name} />
    <span class="caption">{image.name}</span>
  </div>
{/if}

<style>
  .thumb {
    display: inline-block;
    padding: 0;
    overflow: hidden;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-window);
    line-height: 0;
    cursor: zoom-in;
  }

  .thumb img {
    display: block;
    object-fit: contain;
  }

  .gone {
    color: var(--qx-text-secondary);
    font-size: 11.5px;
  }

  .viewer {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
    justify-content: center;
    padding: 32px;
    background: rgb(0 0 0 / 80%);
    cursor: zoom-out;
  }

  .viewer img {
    max-width: 100%;
    max-height: calc(100% - 28px);
    object-fit: contain;
    border-radius: 4px;
    box-shadow: 0 8px 32px rgb(0 0 0 / 50%);
  }

  .caption {
    color: #ddd;
    font-size: 12px;
  }
</style>
