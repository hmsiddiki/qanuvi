<script lang="ts">
  /**
   * t1: the tray's Quit question when no Qanuvi window is open. The app starts as this small window alone, in the
   * look of Qanuvi's own dialog (ConfirmDialog): Enter quits, Escape keeps running. It answers This PC's master and
   * ends; closed without an answer, everything keeps running.
   */
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { quitWords } from "$lib/stores/quitQuestion";

  let question = $state<{ id: number; text: string } | null>(null);
  let answered = $state(false);
  let ok = $state<HTMLButtonElement>();

  onMount(() => {
    invoke<{ id: number; text: string } | null>("quit_question_text")
      .then((q) => {
        // Nothing to ask any more (answered elsewhere, or the master went): the window goes.
        if (!q) return void getCurrentWindow().close();
        question = q;
        queueMicrotask(() => ok?.focus());
      })
      .catch(() => void getCurrentWindow().close());
  });

  async function answer(yes: boolean) {
    if (!question || answered) return;
    answered = true;
    await invoke("quit_question_answer", { id: question.id, yes }).catch(() => {});
    void getCurrentWindow().close();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      void answer(false);
    } else if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      void answer(true);
    }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="dialog" role="alertdialog" aria-label={quitWords.title}>
  <div class="head" data-tauri-drag-region>
    <span class="icon"><TriangleAlert size={18} /></span>
    <h2 data-tauri-drag-region>{quitWords.title}</h2>
  </div>
  <p>{question?.text ?? ""}</p>
  <div class="actions">
    <button type="button" disabled={!question || answered} onclick={() => answer(false)}>{quitWords.cancelLabel}</button
    >
    <button type="button" class="primary" bind:this={ok} disabled={!question || answered} onclick={() => answer(true)}
      >{quitWords.okLabel}</button
    >
  </div>
</div>

<style>
  .dialog {
    display: flex;
    flex-direction: column;
    height: 100%;
    padding: 18px 20px 16px;
    border: 1px solid var(--qx-border);
    background: var(--qx-bg-raised);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: default;
  }

  .icon {
    display: flex;
    color: var(--qx-status-waiting);
  }

  h2 {
    margin: 0;
    color: var(--qx-text);
    font-size: 14.5px;
  }

  p {
    flex: 1;
    margin: 10px 0 12px 28px;
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

  button:hover:not(:disabled) {
    border-color: var(--qx-text-muted);
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }

  button.primary {
    border-color: var(--qx-status-waiting);
    background: color-mix(in srgb, var(--qx-status-waiting) 85%, black);
    color: #fff;
  }

  button:focus-visible {
    outline: 2px solid var(--qx-accent);
    outline-offset: 2px;
  }
</style>
