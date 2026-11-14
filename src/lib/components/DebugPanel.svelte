<script lang="ts">
  /**
   * The sidebar's "Run and debug" mode (15i part 3): pick a launch.json configuration and start it; while a session
   * runs, the controls, variables, watch expressions, call stack and breakpoints.
   */
  import type { Project } from "$lib/api";
  import { dap, type Variable } from "$lib/stores/dap.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import { startDebugging } from "$lib/dap/run";
  import { contextMenu } from "$lib/stores/contextMenu.svelte";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import StepForward from "@lucide/svelte/icons/redo-dot";
  import ArrowDownToDot from "@lucide/svelte/icons/arrow-down-to-dot";
  import ArrowUpFromDot from "@lucide/svelte/icons/arrow-up-from-dot";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Square from "@lucide/svelte/icons/square";
  import Settings from "@lucide/svelte/icons/settings";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import X from "@lucide/svelte/icons/x";
  import IconButton from "./IconButton.svelte";

  interface Props {
    project: Project;
  }

  let { project }: Props = $props();

  let watchText = $state("");
  let open = $state({ variables: true, watch: true, stack: true, breakpoints: true });

  // Read launch.json when the project changes, the mode opens, or launch.json was saved.
  $effect(() => {
    void project.id;
    void editor.tabs.find((t) => t.projectId === project.id && t.path === ".vscode/launch.json")?.savedText;
    dap.loadConfigs(project.id);
    if (!dap.statuses.length) dap.loadStatuses();
  });

  let mine = $derived(dap.session?.projectId === project.id ? dap.session : null);
  let other = $derived(dap.session && !mine ? dap.session : null);
  let paused = $derived(mine?.status === "stopped");
  let picked = $derived(dap.pickedConfig(project.id));
  let watches = $derived(dap.watchList(project.id));
  let bps = $derived(dap.allBreakpoints.filter((b) => b.projectId === project.id));

  function addWatch(e: SubmitEvent) {
    e.preventDefault();
    dap.addWatch(project.id, watchText);
    watchText = "";
  }

  function bpMenu(e: MouseEvent, path: string, line: number, condition: string) {
    e.preventDefault();
    contextMenu.show(e, [
      [
        {
          label: condition ? "Change the condition…" : "Add a condition…",
          action: () =>
            contextMenu.ask("Stop only when this is true", condition, "Save", (v: string) =>
              dap.setCondition(project.id, path, line, v),
            ),
        },
        { label: "Remove", action: () => dap.toggleBreakpoint(project.id, path, line) },
      ],
    ]);
  }

  const status: Record<string, string> = {
    starting: "Starting…",
    running: "Running",
    stopped: "Paused",
    ending: "Stopping…",
  };
</script>

{#snippet section(id: keyof typeof open, title: string, count?: number)}
  <button type="button" class="head" onclick={() => (open[id] = !open[id])} aria-expanded={open[id]}>
    {#if open[id]}<ChevronDown size={13} />{:else}<ChevronRight size={13} />{/if}
    <span>{title}</span>
    {#if count}<span class="count">{count}</span>{/if}
  </button>
{/snippet}

{#snippet variable(v: Variable, depth: number)}
  <li>
    <button
      type="button"
      class="var"
      style:padding-left="{8 + depth * 12}px"
      disabled={!v.ref}
      onclick={() => dap.toggle(v.ref)}
      title={v.type ? `${v.name}: ${v.type}` : v.name}
    >
      <span class="twist"
        >{#if v.ref}{#if dap.expanded[v.ref]}<ChevronDown size={11} />{:else}<ChevronRight size={11} />{/if}{/if}</span
      >
      {#if v.name}<span class="name">{v.name}</span><span class="sep">:</span>{/if}
      <span class="value">{v.value}</span>
    </button>
    {#if v.ref && dap.expanded[v.ref]}
      <ul>
        {#each dap.vars[v.ref] ?? [] as c, i (`${c.name}#${i}`)}{@render variable(c, depth + 1)}{/each}
      </ul>
    {/if}
  </li>
{/snippet}

<div class="debug">
  <div class="launch">
    <button
      type="button"
      class="start"
      title="Start debugging (F5)"
      disabled={!picked || !!mine}
      onclick={() => startDebugging()}><Play size={14} /></button
    >
    <select
      value={picked?.name ?? ""}
      onchange={(e) => dap.pick(project.id, e.currentTarget.value)}
      disabled={!!mine}
      aria-label="Debug configuration"
    >
      {#each dap.configs as c (c.name)}<option value={c.name}>{c.name}</option>{/each}
    </select>
    <IconButton
      label={dap.fromFile ? "Open launch.json" : "Make a launch.json (VS Code's form)"}
      onclick={() => dap.openLaunchFile(project.id)}><Settings size={14} /></IconButton
    >
  </div>
  {#if !dap.fromFile && !dap.launchError}
    <p class="note">No launch.json: these are Qanuvi's defaults. The gear makes one VS Code also reads.</p>
  {/if}
  {#if dap.launchError}<p class="error">{dap.launchError}</p>{/if}

  {#if dap.offer}
    {@const o = dap.offer}
    <div class="offer">
      {#if o.install}
        <p><strong>{o.label}</strong> is needed. {o.install}</p>
        <div class="row">
          <button
            type="button"
            class="primary"
            disabled={dap.installing}
            onclick={() =>
              dap.install(o.id).then((ok) => {
                if (ok) startDebugging();
              })}>{dap.installing ? "Installing…" : "Install"}</button
          >
          <button type="button" onclick={() => (dap.offer = null)}>Not now</button>
        </div>
      {:else}
        <p>{o.missing}</p>
      {/if}
      {#if dap.installError}<p class="error">{dap.installError}</p>{/if}
    </div>
  {:else if dap.lastError}
    <p class="error">{dap.lastError}</p>
  {/if}

  {#if other}
    <p class="note">Debugging runs in another project ({other.name}).</p>
  {/if}

  {#if mine}
    <div class="controls" role="toolbar" aria-label="Debug controls">
      {#if paused}
        <IconButton label="Continue" shortcut="F5" onclick={() => dap.continue()}><Play size={14} /></IconButton>
      {:else}
        <IconButton label="Pause" shortcut="F6" disabled={mine.status !== "running"} onclick={() => dap.pause()}
          ><Pause size={14} /></IconButton
        >
      {/if}
      <IconButton label="Step over" shortcut="F10" disabled={!paused} onclick={() => dap.stepOver()}
        ><StepForward size={14} /></IconButton
      >
      <IconButton label="Step into" shortcut="F11" disabled={!paused} onclick={() => dap.stepIn()}
        ><ArrowDownToDot size={14} /></IconButton
      >
      <IconButton label="Step out" shortcut="Shift+F11" disabled={!paused} onclick={() => dap.stepOut()}
        ><ArrowUpFromDot size={14} /></IconButton
      >
      <IconButton label="Restart" shortcut="Ctrl+Shift+F5" onclick={() => dap.restart()}
        ><RotateCcw size={14} /></IconButton
      >
      <IconButton label="Stop" shortcut="Shift+F5" onclick={() => dap.stop()}><Square size={14} /></IconButton>
      <span class="state" class:paused>
        {status[mine.status]}{#if paused && dap.stopped}: {dap.stopped.reason}{/if}
      </span>
    </div>
    {#if paused && dap.stopped?.text}<p class="note">{dap.stopped.text}</p>{/if}
  {/if}

  <div class="sections">
    {#if mine}
      <section>
        {@render section("variables", "Variables")}
        {#if open.variables}
          {#if !paused}
            <p class="empty">Shown while paused.</p>
          {:else}
            <ul class="tree">
              {#each dap.scopes as s (s.ref)}
                <li>
                  <button type="button" class="var scope" onclick={() => dap.toggle(s.ref)}>
                    <span class="twist"
                      >{#if dap.expanded[s.ref]}<ChevronDown size={11} />{:else}<ChevronRight size={11} />{/if}</span
                    >
                    <span>{s.name}</span>
                  </button>
                  {#if dap.expanded[s.ref]}
                    <ul>
                      {#each dap.vars[s.ref] ?? [] as v, i (`${v.name}#${i}`)}{@render variable(v, 1)}{/each}
                    </ul>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </section>
    {/if}

    <section>
      {@render section("watch", "Watch", watches.length)}
      {#if open.watch}
        <ul class="tree">
          {#each watches as w (w)}
            {@const r = dap.watchValues[w]}
            <li class="watch">
              <span class="name">{w}</span><span class="sep">:</span>
              <span class="value" class:error={r?.error}>{paused ? (r?.value ?? "…") : "not paused"}</span>
              <button
                type="button"
                class="remove"
                aria-label="Remove {w}"
                onclick={() => dap.removeWatch(project.id, w)}><X size={11} /></button
              >
            </li>
          {/each}
        </ul>
        <form onsubmit={addWatch}>
          <input bind:value={watchText} placeholder="Add an expression to watch" aria-label="Watch expression" />
        </form>
      {/if}
    </section>

    {#if mine}
      <section>
        {@render section("stack", "Call stack")}
        {#if open.stack}
          {#if !paused}
            <p class="empty">Shown while paused.</p>
          {:else}
            <ul class="tree">
              {#each dap.frames as f (f.id)}
                <li>
                  <button
                    type="button"
                    class="frame"
                    class:current={dap.frameId === f.id}
                    class:subtle={f.hint === "subtle" || f.hint === "deemphasize"}
                    onclick={() => dap.selectFrame(f)}
                  >
                    <span class="name">{f.name}</span>
                    <span class="where">{f.path ?? f.sourceName}:{f.line}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </section>
    {/if}

    <section>
      {@render section("breakpoints", "Breakpoints", bps.length)}
      {#if open.breakpoints}
        {#if bps.length === 0}
          <p class="empty">Click left of a line number, or press F9 on a line.</p>
        {:else}
          <ul class="tree">
            {#each bps as b (`${b.path}:${b.bp.line}`)}
              <li class="bp" oncontextmenu={(e) => bpMenu(e, b.path, b.bp.line, b.bp.condition ?? "")}>
                <input
                  type="checkbox"
                  checked={b.bp.enabled}
                  aria-label="Breakpoint on"
                  onchange={(e) => dap.setEnabled(project.id, b.path, b.bp.line, e.currentTarget.checked)}
                />
                <button type="button" class="where" onclick={() => editor.open(project.id, b.path, b.bp.line)}>
                  {b.path.split("/").pop()}:{b.bp.line}
                  {#if b.bp.condition}<span class="cond">when {b.bp.condition}</span>{/if}
                </button>
                <button
                  type="button"
                  class="remove"
                  aria-label="Remove breakpoint"
                  onclick={() => dap.toggleBreakpoint(project.id, b.path, b.bp.line)}><X size={11} /></button
                >
              </li>
            {/each}
          </ul>
          <button type="button" class="link" onclick={() => dap.clearBreakpoints(project.id)}>Remove all</button>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .debug {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: var(--qx-text-small);
    overflow: hidden;
  }

  .launch {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px;
  }

  .launch select {
    flex: 1;
    min-width: 0;
    padding: 3px 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-raised);
    color: inherit;
    font: inherit;
  }

  .start {
    display: grid;
    place-items: center;
    width: 26px;
    height: 24px;
    border: none;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: var(--qx-bg-window, #fff);
    cursor: pointer;
  }

  .start:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .note,
  .error,
  .empty {
    margin: 0 8px 6px;
    color: var(--qx-text-muted);
  }

  .error {
    color: var(--qx-danger);
    white-space: pre-wrap;
  }

  .offer {
    margin: 0 8px 8px;
    padding: 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
  }

  .offer p {
    margin: 0 0 6px;
  }

  .row {
    display: flex;
    gap: 6px;
  }

  .row button {
    padding: 3px 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .row button.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: var(--qx-bg-window, #fff);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 6px 6px;
    flex-wrap: wrap;
  }

  .state {
    margin-left: 6px;
    color: var(--qx-text-secondary);
  }

  .state.paused {
    color: var(--qx-warning);
  }

  .sections {
    flex: 1;
    overflow: auto;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 4px 6px;
    border: none;
    border-top: 1px solid var(--qx-border);
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    cursor: pointer;
  }

  .count {
    margin-left: auto;
    color: var(--qx-text-muted);
    font-weight: normal;
  }

  .tree,
  .tree ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .var,
  .frame,
  .where {
    display: flex;
    align-items: baseline;
    gap: 4px;
    width: 100%;
    padding: 2px 8px;
    border: none;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }

  .var:disabled {
    cursor: default;
  }

  .var:hover:not(:disabled),
  .frame:hover,
  .where:hover {
    background: var(--qx-bg-hover);
  }

  .twist {
    display: inline-flex;
    width: 11px;
    flex: none;
  }

  .name {
    color: var(--qx-accent);
    font-family: var(--qx-font-mono);
    white-space: nowrap;
  }

  .sep {
    color: var(--qx-text-muted);
  }

  .value {
    font-family: var(--qx-font-mono);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .value.error {
    color: var(--qx-danger);
  }

  .frame {
    justify-content: space-between;
  }

  .frame.current {
    background: var(--qx-bg-raised);
  }

  .frame.subtle {
    opacity: 0.6;
  }

  .frame .where {
    width: auto;
    padding: 0;
    color: var(--qx-text-muted);
    white-space: nowrap;
  }

  .watch,
  .bp {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
  }

  .bp {
    padding: 0 8px;
  }

  .bp .where {
    flex: 1;
    padding: 2px 4px;
  }

  .cond {
    color: var(--qx-text-muted);
  }

  .remove {
    margin-left: auto;
    display: grid;
    place-items: center;
    border: none;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  form input {
    width: calc(100% - 16px);
    margin: 4px 8px 8px;
    padding: 3px 6px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-raised);
    color: inherit;
    font: inherit;
  }

  .link {
    margin: 4px 8px 8px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }
</style>
