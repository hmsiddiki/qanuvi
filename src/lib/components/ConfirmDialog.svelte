<script lang="ts">
  /** Qanuvi's own confirm dialog (stores/confirm.svelte.ts): Enter confirms, Escape or a click outside cancels. */
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Info from "@lucide/svelte/icons/info";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import { confirmDialog } from "$lib/stores/confirm.svelte";

  let c = $derived(confirmDialog.current);
  let ok = $state<HTMLButtonElement>();

  $effect(() => {
    if (c) queueMicrotask(() => ok?.focus());
  });

  function keydown(e: KeyboardEvent) {
    if (!c) return;
    if (e.key === "Escape") {
      e.preventDefault();
      confirmDialog.answer(false);
    } else if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      confirmDialog.answer(true);
    }
  }
</script>

<svelte:window onkeydown={keydown} />

{#if c}
  <div
    class="backdrop"
    role="presentation"
    onpointerdown={(e) => e.target === e.currentTarget && confirmDialog.answer(false)}
  >
    <div class="dialog {c.kind ?? 'info'}" role="alertdialog" aria-modal="true" aria-label={c.title ?? "Confirm"}>
      <div class="head">
        <span class="icon">
          {#if c.kind === "warning"}<TriangleAlert size={18} />{:else if c.kind === "error"}<CircleX
              size={18}
            />{:else}<Info size={18} />{/if}
        </span>
        <h2>{c.title ?? "Qanuvi"}</h2>
      </div>
      <p>{c.message}</p>
      <div class="actions">
        <button type="button" onclick={() => confirmDialog.answer(false)}>{c.cancelLabel ?? "Cancel"}</button>
        <button type="button" class="primary" bind:this={ok} onclick={() => confirmDialog.answer(true)}
          >{c.okLabel ?? "OK"}</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 300;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 40%);
  }

  .dialog {
    width: min(440px, calc(100% - 32px));
    padding: 18px 20px 16px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    background: var(--qx-bg-raised);
    box-shadow: 0 18px 48px rgb(0 0 0 / 50%);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .icon {
    display: flex;
    color: var(--qx-accent);
  }

  .warning .icon {
    color: var(--qx-status-waiting);
  }

  .error .icon {
    color: var(--qx-status-failed);
  }

  h2 {
    margin: 0;
    color: var(--qx-text);
    font-size: 14.5px;
  }

  p {
    margin: 10px 0 16px 28px;
    color: var(--qx-text-secondary);
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-line;
    user-select: text;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  button {
    height: 30px;
    padding: 0 14px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }

  button:hover {
    border-color: var(--qx-text-muted);
  }

  button.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .warning button.primary {
    border-color: var(--qx-status-waiting);
    background: color-mix(in srgb, var(--qx-status-waiting) 85%, black);
  }

  button:focus-visible {
    outline: 2px solid var(--qx-accent);
    outline-offset: 2px;
  }
</style>
