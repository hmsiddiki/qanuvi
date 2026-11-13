<script lang="ts">
  /**
   * One Inbox question with its answer form (24u part 4). Used in the Inbox, and later by the window a desktop
   * notification opens, so a question is answered the same way everywhere. The first answer anywhere counts.
   */
  import { inbox, type AnswerChoice, type InboxItem } from "$lib/stores/inbox.svelte";

  let { item, onopen }: { item: InboxItem; onopen?: (item: InboxItem) => void } = $props();

  let text = $state("");
  let busy = $state(false);
  let late = $state(false);

  async function send(choice: AnswerChoice) {
    if (busy) return;
    busy = true;
    late = !(await inbox.answer(item.id, { choice, text: item.form?.secret ? "" : text.trim() }));
    busy = false;
  }
</script>

<div class="question">
  {#if item.text}<p class="why">{item.text}</p>{/if}
  {#if item.form?.secret}
    <p class="secret">
      This asks for a login or password. Type it in the run panel of the plan, so it is kept as a private detail and
      never shown here.
    </p>
  {:else if item.form?.text}
    <input type="text" placeholder={item.form.text} bind:value={text} disabled={busy} />
  {/if}
  <div class="choices">
    {#each item.form?.choices ?? [] as c, i (c.id)}
      <button type="button" class:primary={i === 0} title={c.hint} disabled={busy} onclick={() => send(c.id)}
        >{c.label}</button
      >
    {/each}
    {#if item.open && onopen}
      <button type="button" class="link" onclick={() => onopen?.(item)}>{item.open.label}</button>
    {/if}
  </div>
  {#if late}<p class="late">Already answered somewhere else.</p>{/if}
</div>

<style>
  .question {
    margin: 4px 0 0 70px;
  }

  .why,
  .secret,
  .late {
    margin: 0 0 6px;
    color: var(--qx-text-secondary);
    white-space: pre-wrap;
  }

  .late {
    color: var(--qx-status-waiting);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 4px 8px;
    margin-bottom: 6px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: inherit;
    font: inherit;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }

  button {
    padding: 3px 10px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  button.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  button.link {
    border: none;
    padding: 0;
    color: var(--qx-accent);
  }

  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
