<script lang="ts">
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Tablet from "@lucide/svelte/icons/tablet";
  import Smartphone from "@lucide/svelte/icons/smartphone";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import Expand from "@lucide/svelte/icons/expand";
  import Fullscreen from "@lucide/svelte/icons/fullscreen";
  import Minimize2 from "@lucide/svelte/icons/minimize-2";
  import Eye from "@lucide/svelte/icons/eye";
  import FileInput from "@lucide/svelte/icons/file-input";
  import MousePointerClick from "@lucide/svelte/icons/mouse-pointer-click";
  import X from "@lucide/svelte/icons/x";
  import { onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, errorText, type InspectorShot, type Project } from "$lib/api";
  import { agent } from "$lib/stores/agent.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { workspace } from "$lib/stores/workspace.svelte";
  import { normalizeUrl } from "$lib/url";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
  }

  let { project }: Props = $props();

  const DEVICES = {
    desktop: { label: "Desktop", width: 0 },
    tablet: { label: "Tablet", width: 820 },
    phone: { label: "Phone", width: 390 },
  } as const;
  type Device = keyof typeof DEVICES;

  let device = $state<Device>("desktop");
  let history = $state<string[]>([]);
  let position = $state(-1);
  let address = $state("");
  let reloadKey = $state(0);
  let error = $state("");
  /** "window": the preview covers the whole Qanuvi window; "screen": the window is also full screen. */
  let expanded = $state<"" | "window" | "screen">("");

  let current = $derived(history[position] ?? "");

  // Milestone 15: follow the page the agent's test browser is on, and hand the agent a page or an element.
  let follow = $state(false);
  let agentPage = $state("");
  let busy = $state("");
  let stageWidth = $state(0);
  let stageHeight = $state(0);
  /** Inspector: a picture of the page from the test browser, to click on an element. */
  let shot = $state<InspectorShot | null>(null);
  let lastSeen = 0;
  let timer: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    clearInterval(timer);
    if (!follow) return;
    const check = async () => {
      const id = agent.selectedId;
      if (!id) return;
      const view = await api.browser.agentView(id).catch(() => null);
      if (!view || view.updated === lastSeen) return;
      lastSeen = view.updated;
      agentPage = view.title || view.url;
      if (view.url && view.url !== current && /^(https?|file):/.test(view.url)) go(view.url);
    };
    check();
    timer = setInterval(check, 1500);
    return () => clearInterval(timer);
  });

  onDestroy(() => {
    clearInterval(timer);
    if (shot) api.browser.close().catch(() => {});
  });

  async function sendPage() {
    if (!current) return;
    busy = "Reading the page…";
    error = "";
    try {
      agent.attach(await api.browser.pageText(current));
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  function viewSize(): [number, number] {
    const w = DEVICES[device].width || Math.max(320, Math.round(stageWidth));
    return [w, Math.max(240, Math.round(stageHeight - (DEVICES[device].width ? 24 : 0)))];
  }

  async function inspect() {
    if (!current) return;
    busy = "Opening the page in the test browser…";
    error = "";
    try {
      const [w, h] = viewSize();
      shot = await api.browser.show(current, w, h);
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  async function pick(event: MouseEvent) {
    if (!shot) return;
    const img = event.currentTarget as HTMLImageElement;
    const r = img.getBoundingClientRect();
    const x = ((event.clientX - r.left) / r.width) * shot.width;
    const y = ((event.clientY - r.top) / r.height) * shot.height;
    busy = "Finding the element…";
    try {
      const el = await api.browser.pick(x, y);
      agent.attach(el.note);
      shot = null;
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  let scrolling = false;
  async function wheel(event: WheelEvent) {
    event.preventDefault();
    if (scrolling || !shot) return;
    scrolling = true;
    try {
      shot = await api.browser.scroll(Math.sign(event.deltaY) * Math.round(shot.height * 0.6));
    } catch (e) {
      error = errorText(e);
    } finally {
      scrolling = false;
    }
  }

  $effect(() => {
    void project.id;
    history = [];
    position = -1;
    // The page this project was on last time, else its preview address.
    const start = workspace.read(project.id).preview ?? project.settings.preview_url;
    address = start;
    if (start) go(start);
  });

  function go(raw: string) {
    const url = normalizeUrl(raw);
    if (!url) {
      error = "Type an address such as http://localhost/my-site/.";
      return;
    }
    error = "";
    history = [...history.slice(0, position + 1), url];
    position = history.length - 1;
    address = url;
    workspace.saveSoon(project.id, { preview: url });
  }

  async function expand(mode: "window" | "screen") {
    expanded = mode;
    if (mode === "screen") await getCurrentWindow().setFullscreen(true);
  }

  async function restore() {
    if (expanded === "screen") await getCurrentWindow().setFullscreen(false);
    expanded = "";
  }

  function keydown(event: KeyboardEvent) {
    if (shot && event.key === "Escape") {
      event.preventDefault();
      shot = null;
      return;
    }
    // Esc works while the focus is in Qanuvi; inside the page itself the page keeps its keys, so use Exit there.
    if (expanded && event.key === "Escape") {
      event.preventDefault();
      restore();
    }
  }

  function step(delta: number) {
    const next = position + delta;
    if (next < 0 || next >= history.length) return;
    position = next;
    address = history[position];
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="preview" class:expanded={expanded !== ""}>
  <form
    class="bar"
    onsubmit={(e) => {
      e.preventDefault();
      go(address);
    }}
  >
    <IconButton label="Back" disabled={position <= 0} onclick={() => step(-1)}><ChevronLeft size={15} /></IconButton>
    <IconButton label="Forward" disabled={position >= history.length - 1} onclick={() => step(1)}>
      <ChevronRight size={15} />
    </IconButton>
    <IconButton label="Reload" disabled={!current} onclick={() => reloadKey++}><RotateCw size={14} /></IconButton>
    <input bind:value={address} placeholder="http://localhost/…" spellcheck="false" aria-label="Address" />
    {#each Object.entries(DEVICES) as [id, d] (id)}
      <IconButton label={d.label} active={device === id} onclick={() => (device = id as Device)}>
        {#if id === "desktop"}<Monitor size={14} />{:else if id === "tablet"}<Tablet size={14} />{:else}<Smartphone
            size={14}
          />{/if}
      </IconButton>
    {/each}
    {#if settings.value.ai}
      <IconButton
        label={follow ? "Stop following the agent" : "Follow the agent: show the page its test browser is on"}
        active={follow}
        onclick={() => (follow = !follow)}
      >
        <Eye size={14} />
      </IconButton>
      <IconButton label="Send this page to the agent (as text)" disabled={!current || !!busy} onclick={sendPage}>
        <FileInput size={14} />
      </IconButton>
      <IconButton
        label="Point at an element to send it to the agent"
        active={!!shot}
        disabled={!current || !!busy}
        onclick={() => (shot ? (shot = null) : inspect())}
      >
        <MousePointerClick size={14} />
      </IconButton>
    {/if}
    <IconButton label="Open in the browser" disabled={!current} onclick={() => openUrl(current)}>
      <ExternalLink size={14} />
    </IconButton>
    {#if expanded}
      <button type="button" class="exit" title="Back to the normal view (Esc)" onclick={restore}>
        <Minimize2 size={13} /> Exit
      </button>
    {:else}
      <IconButton label="Fill the Qanuvi window" onclick={() => expand("window")}><Expand size={14} /></IconButton>
      <IconButton label="Full screen" onclick={() => expand("screen")}><Fullscreen size={14} /></IconButton>
    {/if}
  </form>
  {#if error}<p class="error">{error}</p>{/if}
  {#if busy}<p class="note">{busy}</p>{:else if shot}<p class="note">
      Click an element to send it to the agent; scroll to move the page; Esc to stop.
      <button type="button" class="link" onclick={() => (shot = null)}><X size={12} /> Stop</button>
    </p>{:else if follow}<p class="note">
      Following the agent{agentPage ? `: ${agentPage}` : " (its test browser has not opened a page yet)"}
    </p>{/if}
  <div class="stage" bind:clientWidth={stageWidth} bind:clientHeight={stageHeight}>
    {#if shot}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <img
        class="inspect"
        class:framed={DEVICES[device].width > 0}
        src={shot.image}
        alt="The page in the test browser"
        style:width={`${shot.width}px`}
        onclick={pick}
        onwheel={wheel}
      />
    {:else}
      {#if current}
        {#key `${current}#${reloadKey}`}
          <iframe
            title="Preview"
            src={current}
            style:width={DEVICES[device].width ? `${DEVICES[device].width}px` : "100%"}
            class:framed={DEVICES[device].width > 0}
          ></iframe>
        {/key}
      {:else}
        <div class="empty">
          <p>Type the address of the project's site above, or set it in the project's settings.</p>
          <p class="hint">Pages that refuse to be shown inside another page open in the browser instead.</p>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .preview {
    height: 100%;
    display: flex;
    flex-direction: column;
  }

  .preview.expanded {
    position: fixed;
    inset: 0;
    z-index: 90;
    height: auto;
    background: var(--qx-bg-panel);
  }

  .exit {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    margin-left: var(--qx-space-1);
    padding: 0 var(--qx-space-3);
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px var(--qx-space-2);
    border-bottom: 1px solid var(--qx-border);
  }

  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    margin: 0 var(--qx-space-2);
    padding: 0 var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: 999px;
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    font-size: var(--qx-text-small);
    outline: none;
    user-select: text;
  }

  input:focus {
    border-color: var(--qx-accent);
  }

  .error {
    margin: 0;
    padding: var(--qx-space-1) var(--qx-space-3);
    color: var(--qx-status-failed);
    font-size: var(--qx-text-small);
  }

  .note {
    margin: 0;
    padding: var(--qx-space-1) var(--qx-space-3);
    color: var(--qx-text-secondary);
    font-size: var(--qx-text-small);
    border-bottom: 1px solid var(--qx-border);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-left: var(--qx-space-2);
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .inspect {
    align-self: flex-start;
    max-width: 100%;
    cursor: crosshair;
  }

  .inspect.framed {
    margin: var(--qx-space-3) 0;
    border-radius: var(--qx-radius-card);
    box-shadow: 0 0 0 1px var(--qx-border);
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    justify-content: center;
    overflow: auto;
    background: var(--qx-bg-window);
  }

  iframe {
    height: 100%;
    border: 0;
    background: #fff;
  }

  iframe.framed {
    margin: var(--qx-space-3) 0;
    height: calc(100% - 24px);
    border-radius: var(--qx-radius-card);
    box-shadow: 0 0 0 1px var(--qx-border);
  }

  .empty {
    margin: auto;
    max-width: 340px;
    text-align: center;
    color: var(--qx-text-secondary);
  }

  .hint {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }
</style>
