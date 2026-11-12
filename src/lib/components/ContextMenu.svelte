<script lang="ts">
  /**
   * The right-click menu and the name prompt (15d), drawn once for the whole window. Arrow keys move, Enter runs, Esc
   * or a click elsewhere closes. Text fields that have no menu of their own get cut / copy / paste / select all; in a
   * release build nothing else shows the browser's menu, and its reload keys do nothing.
   */
  import { tick } from "svelte";
  import { clipboardText, contextMenu, copyText, type MenuItem } from "$lib/stores/contextMenu.svelte";

  let menu: HTMLDivElement | undefined = $state();
  let pos = $state({ x: 0, y: 0 });
  let focused = $state(-1);
  let flat = $derived(contextMenu.open ? contextMenu.open.groups.flat() : []);

  // Kept inside the window.
  $effect(() => {
    const o = contextMenu.open;
    if (!o) return;
    focused = -1;
    pos = { x: o.x, y: o.y };
    tick().then(() => {
      if (!menu) return;
      const r = menu.getBoundingClientRect();
      pos = {
        x: Math.max(4, Math.min(o.x, window.innerWidth - r.width - 4)),
        y: Math.max(4, Math.min(o.y, window.innerHeight - r.height - 4)),
      };
      menu.focus();
    });
  });

  async function run(item: MenuItem) {
    if (item.disabled) return;
    contextMenu.close();
    await item.action();
  }

  function keydown(e: KeyboardEvent) {
    if (!contextMenu.open) return;
    const enabled = flat.map((i, n) => (i.disabled ? -1 : n)).filter((n) => n >= 0);
    if (e.key === "Escape") contextMenu.close();
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!enabled.length) return;
      const at = enabled.indexOf(focused);
      const next = e.key === "ArrowDown" ? (at + 1) % enabled.length : (at - 1 + enabled.length) % enabled.length;
      focused = enabled[at < 0 && e.key === "ArrowUp" ? enabled.length - 1 : next];
    } else if (e.key === "Enter" && focused >= 0) {
      e.preventDefault();
      run(flat[focused]);
    }
  }

  // ---------- Text fields: their own small menu ----------
  function textMenu(e: MouseEvent, el: HTMLInputElement | HTMLTextAreaElement) {
    const start = el.selectionStart ?? 0;
    const end = el.selectionEnd ?? 0;
    const selected = el.value.slice(start, end);
    const writable = !el.readOnly && !el.disabled;
    const put = (text: string) => {
      el.focus();
      el.setRangeText(text, start, end, "end");
      el.dispatchEvent(new Event("input", { bubbles: true }));
    };
    contextMenu.show(e, [
      [
        {
          label: "Cut",
          hint: "Ctrl+X",
          disabled: !selected || !writable,
          action: async () => (await copyText(selected)) && put(""),
        },
        { label: "Copy", hint: "Ctrl+C", disabled: !selected, action: () => copyText(selected) },
        { label: "Paste", hint: "Ctrl+V", disabled: !writable, action: async () => put(await clipboardText()) },
      ],
      [{ label: "Select all", hint: "Ctrl+A", action: () => (el.focus(), el.select()) }],
    ]);
  }

  function windowMenu(e: MouseEvent) {
    if (e.defaultPrevented) return;
    const t = e.target as HTMLElement;
    const field = t.closest("input, textarea") as HTMLInputElement | HTMLTextAreaElement | null;
    if (field && !["checkbox", "radio", "range", "button", "submit", "file", "color"].includes(field.type)) {
      textMenu(e, field);
      return;
    }
    // Nothing of Qanuvi's own here: no browser menu in a release build (kept in development for inspecting).
    if (!import.meta.env.DEV) e.preventDefault();
  }

  function windowKeys(e: KeyboardEvent) {
    keydown(e);
    if (import.meta.env.DEV) return;
    // The browser's reload would restart the whole interface.
    const k = e.key.toLowerCase();
    if (k === "f5" || ((e.ctrlKey || e.metaKey) && k === "r")) e.preventDefault();
  }

  function outside(e: PointerEvent) {
    if (contextMenu.open && menu && !menu.contains(e.target as Node)) contextMenu.close();
  }

  // ---------- The name prompt ----------
  let promptInput: HTMLInputElement | undefined = $state();
  let promptValue = $state("");
  // What had the focus when the prompt opened (the editor after F2); it gets it back when the prompt closes.
  let returnFocus: HTMLElement | null = null;
  $effect(() => {
    const p = contextMenu.prompt;
    if (!p) {
      const el = returnFocus;
      returnFocus = null;
      // Only when nothing else took the focus meanwhile (a new file's tab, for example).
      if (el)
        tick().then(() => {
          const now = document.activeElement;
          if (el.isConnected && (!now || now === document.body)) el.focus();
        });
      return;
    }
    if (!returnFocus && document.activeElement instanceof HTMLElement && document.activeElement !== document.body)
      returnFocus = document.activeElement;
    promptValue = p.value;
    tick().then(() => {
      if (!promptInput) return;
      promptInput.focus();
      const dot = p.value.lastIndexOf(".");
      promptInput.setSelectionRange(0, dot > 0 ? dot : p.value.length);
    });
  });

  async function submitPrompt(e: SubmitEvent) {
    e.preventDefault();
    const p = contextMenu.prompt;
    const v = promptValue.trim();
    if (!p || !v) return;
    contextMenu.prompt = null;
    await p.done(v);
  }
</script>

<svelte:window
  oncontextmenu={windowMenu}
  onkeydown={windowKeys}
  onpointerdown={outside}
  onblur={() => contextMenu.close()}
/>

{#if contextMenu.open}
  <div
    class="cmenu"
    role="menu"
    tabindex="-1"
    bind:this={menu}
    style:left="{pos.x}px"
    style:top="{pos.y}px"
    oncontextmenu={(e) => e.preventDefault()}
  >
    {#each contextMenu.open.groups as group, g (g)}
      {#if g > 0}<div class="sep" role="separator"></div>{/if}
      {#each group as item (item.label)}
        {@const n = flat.indexOf(item)}
        <button
          type="button"
          role="menuitem"
          class:danger={item.danger}
          class:focused={n === focused}
          disabled={item.disabled}
          onpointerenter={() => (focused = item.disabled ? -1 : n)}
          onclick={() => run(item)}
        >
          <span>{item.label}</span>
          {#if item.hint}<span class="hint">{item.hint}</span>{/if}
        </button>
      {/each}
    {/each}
  </div>
{/if}

{#if contextMenu.prompt}
  <div
    class="veil"
    role="presentation"
    onpointerdown={(e) => e.target === e.currentTarget && (contextMenu.prompt = null)}
  >
    <form class="prompt" onsubmit={submitPrompt}>
      <label>
        {contextMenu.prompt.title}
        <input
          bind:this={promptInput}
          bind:value={promptValue}
          onkeydown={(e) => e.key === "Escape" && (contextMenu.prompt = null)}
        />
      </label>
      <div class="row">
        <button type="button" onclick={() => (contextMenu.prompt = null)}>Cancel</button>
        <button type="submit" class="primary">{contextMenu.prompt.ok}</button>
      </div>
    </form>
  </div>
{/if}

<style>
  .cmenu {
    position: fixed;
    z-index: 1000;
    min-width: 210px;
    max-width: 340px;
    padding: 4px;
    border: 1px solid var(--qx-border);
    border-radius: 8px;
    background: var(--qx-bg-raised);
    box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
    outline: none;
    font-size: 12.5px;
  }

  .cmenu button {
    display: flex;
    gap: 16px;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 5px 10px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .cmenu button.focused {
    background: var(--qx-accent);
    color: #fff;
  }

  .cmenu button:disabled {
    color: var(--qx-text-muted);
    cursor: default;
  }

  .cmenu button.danger:not(.focused) {
    color: var(--qx-danger, #e5534b);
  }

  .hint {
    color: var(--qx-text-muted);
    font-size: 11px;
  }

  .cmenu button.focused .hint {
    color: rgb(255 255 255 / 75%);
  }

  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--qx-border);
  }

  .veil {
    position: fixed;
    inset: 0;
    z-index: 1001;
    display: grid;
    place-items: start center;
    padding-top: 18vh;
    background: rgb(0 0 0 / 30%);
  }

  .prompt {
    display: grid;
    gap: 12px;
    width: min(420px, 90vw);
    padding: 14px 16px;
    border: 1px solid var(--qx-border);
    border-radius: 10px;
    background: var(--qx-bg-panel);
    box-shadow: 0 12px 32px rgb(0 0 0 / 50%);
    font-size: 12.5px;
  }

  .prompt label {
    display: grid;
    gap: 6px;
    color: var(--qx-text-secondary);
  }

  .prompt input {
    padding: 6px 8px;
    border: 1px solid var(--qx-accent);
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
  }

  .row {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .row button {
    padding: 5px 14px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .row button.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }
</style>
