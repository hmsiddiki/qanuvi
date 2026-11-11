<script lang="ts">
  import { contextMenu, copyText } from "$lib/stores/contextMenu.svelte";
  import AttachMenu from "./AttachMenu.svelte";
  import { overruleIn } from "$lib/core/decider";
  import { reviewFreeTurn } from "$lib/stores/chatReview";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Square from "@lucide/svelte/icons/square";
  import Bot from "@lucide/svelte/icons/bot";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import ShieldQuestion from "@lucide/svelte/icons/shield-question";
  import { tick, untrack } from "svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { editor } from "$lib/stores/editor.svelte";
  import Settings2 from "@lucide/svelte/icons/settings-2";
  import { currentModel, formatCost, formatTokens, looksLikeCall } from "$lib/agent/events";
  import { renderMarkdown } from "$lib/agent/markdown";
  import { openUrl } from "@tauri-apps/plugin-opener";

  /** Links in an answer open in the user's browser, not inside Qanuvi. */
  function openLink(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest("a");
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (/^https?:\/\//.test(href)) openUrl(href);
  }
  import { modelLabel, paidAgent } from "$lib/agents/providers";
  import { layout } from "$lib/layout/layout.svelte";
  import { ui } from "$lib/stores/ui.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { wizard } from "$lib/stores/wizard.svelte";

  /** No paid agent and no free models yet: Qanuvi's own agent can be set up for free. */
  function setUpFreeAi() {
    settings.set("agent", "qanuvi");
    agent.newSession();
    wizard.open = true;
    ui.show("agents");
  }
  import ModePicker from "./ModePicker.svelte";
  import { ask } from "$lib/stores/confirm.svelte";
  import { guidanceNote, isStopOrder, looksSecret } from "$lib/plan/guidance";
  import { isLimit } from "$lib/plan/limits";
  import { runner } from "$lib/stores/runner.svelte";
  import CommandMenu from "./CommandMenu.svelte";

  let commandsOpen = $state(false);

  // Paid decides, free works (24b): when the free models fail a request (with Auto, not a model the user picked), the
  // paid agent takes it over by itself. Once per session.
  const handedOver = new Set<string>();
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    const id = agent.selectedId;
    if (!last || last.kind !== "result" || last.ok || !id || handedOver.has(id)) return;
    const team = settings.value.team;
    if (
      agent.provider.id === "qanuvi" &&
      agent.model === "auto" &&
      agent.info?.available &&
      (team.mode === "paid-decides" || team.mode === "priority") &&
      projects.current
    ) {
      handedOver.add(id);
      // Untracked: what the hand-over changes must never re-run this.
      const pid = projects.current.id;
      untrack(() => agent.handToPaid(pid));
    }
  });

  // Paid agent on top (24e): a free chat turn that ended well and changed files is reviewed by the paid agent.
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    if (!last || last.kind !== "result" || !agent.freeTurn || !projects.current) return;
    const p = projects.current;
    untrack(() => {
      if (last.ok) reviewFreeTurn(p);
      else agent.freeTurn = null;
    });
  });

  // Free decides, paid works, with the paid agent on top (24e): its answer can overrule the free pick.
  $effect(() => {
    const last = agent.items[agent.items.length - 1];
    if (!last || last.kind !== "result" || !agent.freePicked || !projects.current) return;
    const answer = [...agent.items].reverse().find((i) => i.kind === "text");
    const why = answer && answer.kind === "text" ? overruleIn(answer.text) : null;
    const pid = projects.current.id;
    untrack(() => {
      if (why) agent.overrule(pid, why);
      else agent.freePicked = null;
    });
  });

  function openAgentSettings() {
    if (!layout.state.workVisible) layout.toggleWork();
    ui.show("agents");
  }

  let scroller: HTMLDivElement;

  let project = $derived(projects.current);
  let session = $derived(agent.selected);
  let running = $derived(agent.busy || session?.status === "running" || session?.status === "waiting");
  /** The model at work, as the agent named it (Qanuvi's brain names each pick and handover). */
  let working = $derived(currentModel(agent.items));

  $effect(() => {
    void agent.items.length;
    tick().then(() => scroller && (scroller.scrollTop = scroller.scrollHeight));
  });

  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    // While the agent works the message joins the task: the agent reads it before its next step.
    if (!project || !agent.draft.trim()) return;
    const text = agent.draft;
    agent.draft = "";
    // The step a plan run is working on now: the user is watching and guides it.
    const step =
      runner.sessionId && runner.sessionId === session?.id
        ? runner.run?.steps.find((s) => s.status === "running")
        : null;
    // A step the last run stopped at: writing in its session resumes the run from it (not a plain chat message).
    if (!step && session && !running && /^Step [^:\s]+:/.test(session.title) && !isStopOrder(text)) {
      if (looksSecret(text) && runner.run) await runner.addPrivate(project.id, runner.run.file, text);
      else runner.addNote(text);
      if (await runner.resumeFrom(project, session.id, text)) {
        agent.notice("The plan run goes on from this step, in this session, with your message first.");
        return;
      }
    }
    if (step && isStopOrder(text)) {
      runner.stop();
      agent.notice("You said stop: the plan run stops here. Run it again to go on.");
      return;
    }
    if (step && looksSecret(text)) {
      const asSecret = await ask(
        "This looks like a login or password. It will be kept for this plan, so every later step and run of it has it (in Windows' credential store, never in files). Better still: a project secret reaches every step and the tests as an environment variable, without any model seeing it.",
        {
          title: "A password in the chat",
          kind: "warning",
          okLabel: "Open project secrets",
          cancelLabel: "Use it for this plan",
        },
      );
      if (asSecret) {
        agent.draft = text;
        ui.settingsFor = project.id;
        return;
      }
      if (runner.run) await runner.addPrivate(project.id, runner.run.file, text);
    } else if (step) runner.addNote(text);
    const note = step && runner.run ? guidanceNote(step.number ?? step.title, runner.run.file) : "";
    if (!(await agent.send(project.id, text, note))) agent.draft = text;
  }

  function keydown(event: KeyboardEvent) {
    // Enter with the menu open puts it away; it never sends a lone "/".
    if (commandsOpen && event.key === "Enter") {
      event.preventDefault();
      commandsOpen = false;
      return;
    }
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      send();
    } else if (event.key === "Tab" && event.shiftKey && !running) {
      event.preventDefault();
      agent.cycleMode();
    } else if (commandsOpen && event.key !== "/") {
      // The menu came from typing "/": any other key (space, arrows, Escape, more text) puts it away again.
      commandsOpen = false;
    }
  }

  /** "/" as the whole message opens the menu (read from the text, so every keyboard layout works). */
  function input() {
    if (agent.draft === "/") commandsOpen = true;
  }

  /** Opens a file an agent step worked on, when it is inside the project. */
  /** Right-click on a message: copy it, or quote it in the reply. */
  function messageMenu(e: MouseEvent, text: string) {
    const picked = window.getSelection()?.toString() ?? "";
    contextMenu.show(e, [
      [
        picked && { label: "Copy the selection", hint: "Ctrl+C", action: () => copyText(picked) },
        { label: "Copy the message", action: () => copyText(text) },
      ],
      [
        {
          label: "Quote in reply",
          action: () => {
            const quote = (picked || text)
              .trim()
              .split("\n")
              .map((l) => `> ${l}`)
              .join("\n");
            agent.draft = `${quote}\n\n${agent.draft}`;
            agent.input?.focus();
          },
        },
      ],
    ]);
  }

  function openFile(file: string | null) {
    if (!file || !project) return;
    const root = project.path.replace(/\\/g, "/").replace(/\/$/, "").toLowerCase();
    const path = file.replace(/\\/g, "/");
    if (path.toLowerCase().startsWith(root + "/")) editor.open(project.id, path.slice(root.length + 1));
  }

  /** What a "without asking" approval covers, in words: all file changes together, otherwise the tool. */
  function kindOf(tool: string): string {
    if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(tool)) return "file changes";
    if (tool === "Bash" || tool === "PowerShell") return "commands (risky ones still ask)";
    return tool;
  }
</script>

<section class="agent-pane" aria-label="Agent">
  <div class="conversation" bind:this={scroller}>
    {#if !project}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>Add a project first</h2>
        <p>Use the + next to Projects in the sidebar to add a folder. Then ask Qanuvi to work on it.</p>
      </div>
    {:else if (agent.provider.id === "claude-code" && agent.info && !agent.info.available) || (agent.provider.id === "qanuvi" && !settings.value.brain.models.length)}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>Get AI help for free</h2>
        <p>
          Qanuvi's own agent works without a paid plan: with a model on this computer (Qanuvi suggests one that fits) or
          free models from OpenRouter. It takes a few steps.
        </p>
        <button type="button" class="setup" onclick={setUpFreeAi}>Set up free AI</button>
        {#if agent.provider.id === "claude-code"}
          <p class="small">
            Have Claude Code? Install it and make sure the <code>claude</code> command works in a terminal.
          </p>
        {/if}
      </div>
    {:else if agent.items.length === 0}
      <div class="empty">
        <div class="badge" aria-hidden="true"><Bot size={18} /></div>
        <h2>{session ? session.title : "New session"}</h2>
        <p>
          Ask Qanuvi to work on {project.name}. Steps and results show here; changes wait for your approval unless you
          allow them.
        </p>
      </div>
    {:else}
      {#each agent.items as item (item.id)}
        {#if item.kind === "user"}
          <div class="user" role="presentation" oncontextmenu={(e) => messageMenu(e, item.text)}>
            {#if item.during}<span
                class="during"
                title="Sent while the agent was working; it reads it before its next step">Added while it worked</span
              >{/if}
            {item.text}
            {#if item.attachments?.length}
              <div class="chips">
                {#each item.attachments as name (name)}<span class="chip" title={name}>📎 {name}</span>{/each}
              </div>
            {/if}
          </div>
        {:else if item.kind === "text"}
          {#if item.live}
            {#if !looksLikeCall(item.text)}<div class="text live">{item.text}</div>{/if}
          {:else}
            <!-- Escaped first, then formatted (agent/markdown.ts): the text cannot add HTML of its own. -->
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            <div
              class="text md"
              onclick={openLink}
              oncontextmenu={(e) => messageMenu(e, item.text)}
              role="presentation"
            >
              {@html renderMarkdown(item.text)}
            </div>
          {/if}
        {:else if item.kind === "tool"}
          <button
            type="button"
            class="step {item.state}"
            disabled={!item.file}
            onclick={() => openFile(item.file)}
            oncontextmenu={(e) =>
              contextMenu.show(e, [
                [
                  { label: "Open the file", disabled: !item.file, action: () => openFile(item.file) },
                  { label: "Copy the file's path", disabled: !item.file, action: () => copyText(item.file ?? "") },
                ],
                [{ label: "Copy the result", disabled: !item.result, action: () => copyText(item.result) }],
              ])}
          >
            <span class="step-icon">
              {#if item.state === "running"}<LoaderCircle
                  size={13}
                  class="spin"
                />{:else if item.state === "done"}<CircleCheck size={13} />{:else}<CircleX size={13} />{/if}
            </span>
            <span class="step-body">
              <span class="step-title">{item.name} · {item.summary}</span>
              {#if item.result}<span class="step-result">{item.result}</span>{/if}
            </span>
          </button>
          {#if item.image}
            <a class="shot" href={item.image} target="_blank" rel="noreferrer" title="Screenshot (open full size)">
              <img src={item.image} alt="Screenshot from the test browser" loading="lazy" />
            </a>
          {/if}
        {:else if item.kind === "approval"}
          <div class="approval {item.state}">
            <div class="approval-head">
              <ShieldQuestion size={15} />
              <span><strong>{item.tool}</strong> · {item.summary}</span>
            </div>
            <pre>{item.detail}</pre>
            {#if item.state === "pending"}
              <div class="approval-actions">
                <button type="button" class="allow" onclick={() => agent.respond(item.id, true)}>Approve</button>
                {#if session?.agent === "qanuvi"}
                  <!-- Free and local models: a while at a time. -->
                  <button
                    type="button"
                    title="Also allow {kindOf(item.tool)} without asking for {settings.value.approveMinutes} minutes"
                    onclick={() => agent.respond(item.id, true, `minutes:${settings.value.approveMinutes}`)}
                    >Allow for {settings.value.approveMinutes} min</button
                  >
                {:else}
                  <!-- A paid agent: for the session. -->
                  <button
                    type="button"
                    title="Also allow {kindOf(item.tool)} without asking until this session ends"
                    onclick={() => agent.respond(item.id, true, "session")}>Allow for this session</button
                  >
                {/if}
                <button type="button" class="decline" onclick={() => agent.respond(item.id, false)}>Decline</button>
              </div>
            {:else}
              <div class="decision">
                {item.state === "allowed" ? (item.auto ? "Approved (allowed earlier)" : "Approved") : "Declined"}
              </div>
            {/if}
          </div>
        {:else if item.kind === "result"}
          <div class="result" class:failed={!item.ok}>
            {item.ok ? "Finished" : "Stopped with an error"} · {item.seconds.toFixed(1)} s{item.text
              ? ` · ${item.text}`
              : ""}
            {#if !item.ok && agent.provider.id === "qanuvi" && agent.model === "auto" && agent.info?.available && project}
              <button type="button" class="to-auto" onclick={() => project && agent.handToPaid(project.id)}
                >Hand this to {paidAgent()?.name ?? "the paid agent"}</button
              >
            {/if}
            {#if !item.ok && isLimit(item.text) && agent.provider.id !== "qanuvi" && settings.value.brain.models.length && project}
              <!-- The paid agent hit its limit: the free models can go on now. -->
              <button type="button" class="to-auto" onclick={() => project && agent.handToFree(project.id)}
                >Continue with free models</button
              >
            {/if}
            {#if !item.ok && item.text.includes("Switch to Auto") && agent.model !== "auto"}
              <!-- The chosen model is the only one used; when it fails, Qanuvi core offers to choose again. -->
              <button type="button" class="to-auto" onclick={() => (agent.model = "auto")}>Switch to Auto</button>
            {/if}
          </div>
        {:else if item.kind === "notice"}
          <div class="notice {item.tone}">{item.text}</div>
        {/if}
      {/each}
      {#if running}<div class="notice info working">
          <LoaderCircle size={12} class="spin" /> Working…{working ? ` · ${working}` : ""}
        </div>{/if}
    {/if}
    {#each agent.errors.slice(-3) as e, i (i)}<div class="notice error">{e}</div>{/each}
    {#if agent.error}<div class="notice error">{agent.error}</div>{/if}
  </div>

  {#if session && agent.grants.length}
    <div class="grants">
      Allowed without asking: {agent.grants
        .map(
          (g) =>
            `${g.group === "edits" ? "file changes" : g.group}${g.secondsLeft === null ? "" : ` (${Math.ceil(g.secondsLeft / 60)} min)`}`,
        )
        .join(", ")}
      <button type="button" class="link" onclick={() => agent.forgetGrants()}>Ask again</button>
    </div>
  {/if}
  {#if session}
    <div class="usage" title="Tokens sent (including cache writes), tokens read from cache, tokens received, and cost">
      in {formatTokens(session.input_tokens)} · cache {formatTokens(session.cache_read_tokens)} · out {formatTokens(
        session.output_tokens,
      )}
      · {formatCost(session.cost_usd)} · {(session.duration_ms / 1000).toFixed(0)} s{working ? ` · ${working}` : ""}
    </div>
  {/if}

  <form class="composer" onsubmit={send}>
    {#if agent.attachments.length}
      <div class="chips pending">
        {#each agent.attachments as f, i (i)}
          <span class="chip" title={f.name}
            >{f.kind === "image" ? "🖼" : f.kind === "page" ? "🌐" : "📄"}
            {f.name}<button
              type="button"
              aria-label="Remove {f.name}"
              onclick={() => (agent.attachments = agent.attachments.filter((_, j) => j !== i))}>×</button
            ></span
          >
        {/each}
      </div>
    {/if}
    <textarea
      bind:value={agent.draft}
      bind:this={agent.input}
      rows="3"
      placeholder={!project
        ? "Add a project first"
        : running
          ? "Add to the task while it works…  (Enter to send; the agent reads it before its next step)"
          : "Ask Qanuvi…  (Enter to send, Shift+Enter for a new line)"}
      aria-label="Message to the agent"
      disabled={!project}
      onkeydown={keydown}
      oninput={input}></textarea>
    <div class="tools">
      <AttachMenu disabled={!project} />
      <CommandMenu bind:open={commandsOpen} disabled={!project} />
      <span class="spacer"></span>
      <ModePicker disabled={running && !agent.onPlanStep} />
      {#if running}
        <button type="button" class="stop" title="Stop" aria-label="Stop" onclick={() => agent.stop()}>
          <Square size={12} />
        </button>
      {/if}
      <button
        type="submit"
        class="send"
        title={running ? "Send: joins the task, the agent reads it before its next step" : "Send"}
        aria-label="Send"
        disabled={!agent.draft.trim() || !project}
      >
        <ArrowUp size={15} />
      </button>
    </div>
  </form>
  <div class="agent-line">
    <button
      type="button"
      class="agent-chip"
      title="Choose the model ({agent.provider.name})"
      onpointerdown={(e) => e.stopPropagation()}
      onclick={() => (commandsOpen = !commandsOpen)}
    >
      <span class="badge" style:background={agent.provider.color}>{agent.provider.badge}</span>
      <span>{agent.provider.name}</span>
      <span class="dim"
        >· {modelLabel(agent.provider, agent.model)}{agent.provider.efforts.length ? ` · ${agent.effort}` : ""}</span
      >
    </button>
    <button
      type="button"
      class="gear"
      title="Agent settings and configuration files"
      aria-label="Agent settings"
      onclick={openAgentSettings}
    >
      <Settings2 size={13} />
    </button>
  </div>
</section>

<style>
  .agent-pane {
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--qx-bg-panel);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-panel);
    /* Visible, so the mode and command menus can open past the pane's edges. */
    overflow: visible;
    position: relative;
    z-index: 2;
  }

  .conversation {
    border-radius: var(--qx-radius-panel) var(--qx-radius-panel) 0 0;
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: var(--qx-space-4);
    display: flex;
    flex-direction: column;
    gap: var(--qx-space-3);
    user-select: text;
  }

  .empty {
    margin: auto;
    max-width: 300px;
    text-align: center;
  }

  .badge {
    width: 36px;
    height: 36px;
    margin: 0 auto var(--qx-space-3);
    display: grid;
    place-items: center;
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }

  h2 {
    margin: 0 0 var(--qx-space-1);
    font-size: 14px;
    font-weight: 600;
  }

  .empty p {
    margin: 0;
    color: var(--qx-text-secondary);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 6px;
  }

  .chips.pending {
    margin: 0 0 6px;
  }

  .chip {
    display: inline-flex;
    gap: 4px;
    align-items: center;
    overflow: hidden;
    max-width: 260px;
    padding: 2px 8px;
    border: 1px solid var(--qx-border);
    border-radius: 10px;
    color: var(--qx-text-secondary);
    font-size: 11px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .chip button {
    padding: 0 0 0 2px;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .user {
    align-self: flex-end;
    max-width: 85%;
    padding: var(--qx-space-2) var(--qx-space-3);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-raised);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.55;
  }

  .text.md {
    white-space: normal;
  }

  .text.md :global(p),
  .text.md :global(ul),
  .text.md :global(ol),
  .text.md :global(pre),
  .text.md :global(blockquote) {
    margin: 0 0 6px;
  }

  .text.md :global(ul),
  .text.md :global(ol) {
    padding-left: 20px;
  }

  .text.md :global(h3),
  .text.md :global(h4),
  .text.md :global(h5),
  .text.md :global(h6) {
    margin: 8px 0 4px;
    font-size: 13px;
  }

  .text.md :global(code) {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--qx-bg-raised);
    font-family: var(--qx-font-mono, monospace);
    font-size: 12px;
  }

  .text.md :global(pre) {
    padding: 6px 8px;
    border-radius: 6px;
    background: var(--qx-bg-raised);
    white-space: pre-wrap;
  }

  .text.md :global(pre code) {
    padding: 0;
    background: none;
  }

  .text.md :global(blockquote) {
    padding-left: 8px;
    border-left: 2px solid var(--qx-border);
    color: var(--qx-text-secondary);
  }

  .text.md :global(a) {
    color: var(--qx-accent);
  }

  .step {
    display: flex;
    gap: var(--qx-space-2);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--qx-text-secondary);
    font: inherit;
    text-align: left;
  }

  .step:not(:disabled) {
    cursor: pointer;
  }

  .step:not(:disabled):hover .step-title {
    color: var(--qx-text);
    text-decoration: underline;
  }

  .step-icon {
    flex: none;
    padding-top: 2px;
    color: var(--qx-text-muted);
  }

  .step.done .step-icon {
    color: var(--qx-status-done);
  }

  .step.failed .step-icon {
    color: var(--qx-status-failed);
  }

  .step-body {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .step-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .step-result {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .approval {
    padding: var(--qx-space-3);
    border: 1px solid color-mix(in srgb, var(--qx-status-waiting) 45%, var(--qx-border));
    border-radius: var(--qx-radius-card);
    background: color-mix(in srgb, var(--qx-status-waiting) 7%, var(--qx-bg-panel));
  }

  .approval.allowed,
  .approval.refused {
    border-color: var(--qx-border);
    background: var(--qx-bg-panel);
  }

  .approval-head {
    display: flex;
    align-items: center;
    gap: var(--qx-space-2);
    color: var(--qx-status-waiting);
  }

  .approval-head span {
    color: var(--qx-text);
  }

  .approval pre {
    max-height: 180px;
    margin: var(--qx-space-2) 0;
    padding: var(--qx-space-2);
    overflow: auto;
    border-radius: 6px;
    background: var(--qx-bg-window);
    color: var(--qx-text-secondary);
    font-size: 12px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .approval-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .approval-actions button {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  .approval-actions button:hover {
    background: var(--qx-bg-raised);
  }

  .approval-actions .allow {
    border-color: transparent;
    background: var(--qx-accent);
    color: #fff;
  }

  .approval-actions .allow:hover {
    background: var(--qx-accent);
    filter: brightness(1.1);
  }

  /* Declining is the quiet choice: text only, pushed to the end. */
  .approval-actions .decline {
    margin-left: auto;
    border-color: transparent;
    color: var(--qx-text-secondary);
  }

  .decision {
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .result {
    color: var(--qx-status-done);
    font-size: var(--qx-text-small);
  }

  .result.failed {
    color: var(--qx-status-failed);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .notice.error {
    color: var(--qx-status-failed);
    overflow-wrap: anywhere;
  }

  .usage {
    padding: 0 var(--qx-space-3) var(--qx-space-1);
    color: var(--qx-text-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .composer {
    margin: 0 var(--qx-space-2) var(--qx-space-2);
    padding: var(--qx-space-2) var(--qx-space-2) var(--qx-space-1);
    background: var(--qx-bg-raised);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-input);
  }

  .composer:focus-within {
    border-color: color-mix(in srgb, var(--qx-accent) 55%, var(--qx-border));
  }

  textarea {
    width: 100%;
    padding: var(--qx-space-1);
    border: 0;
    background: transparent;
    color: var(--qx-text);
    font: inherit;
    resize: none;
    outline: none;
    user-select: text;
  }

  .user .during {
    display: block;
    margin-bottom: 3px;
    color: var(--qx-text-muted);
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  textarea::placeholder {
    color: var(--qx-text-muted);
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .spacer {
    flex: 1;
  }

  .tools button {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: transparent;
    color: var(--qx-text-secondary);
    cursor: pointer;
  }

  .tools button:hover:not(:disabled) {
    background: var(--qx-bg-panel);
    color: var(--qx-text);
  }

  .tools .send {
    background: var(--qx-text);
    color: var(--qx-bg-window);
    border-radius: 50%;
  }

  .tools .send:disabled {
    background: var(--qx-border);
    color: var(--qx-text-muted);
    cursor: default;
  }

  .tools .stop {
    background: var(--qx-status-failed);
    color: #fff;
    border-radius: 50%;
  }

  .agent-line {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 var(--qx-space-2) var(--qx-space-2);
  }

  .agent-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 var(--qx-space-2) 0 4px;
    border: 1px solid var(--qx-border);
    border-radius: 999px;
    background: var(--qx-bg-raised);
    color: var(--qx-text-secondary);
    font: inherit;
    font-size: var(--qx-text-small);
    cursor: pointer;
  }

  .agent-chip:hover {
    color: var(--qx-text);
  }

  .agent-chip .badge {
    width: 16px;
    height: 16px;
    margin: 0;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 50%;
    color: #fff;
    font-size: 9px;
    font-weight: 800;
  }

  .agent-chip .dim {
    color: var(--qx-text-muted);
  }

  .gear {
    width: 24px;
    height: 24px;
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--qx-text-muted);
    cursor: pointer;
  }

  .gear:hover {
    background: var(--qx-bg-raised);
    color: var(--qx-text);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .shot {
    display: block;
    margin: 2px 0 var(--qx-space-2) 22px;
  }

  .shot img {
    display: block;
    max-width: 100%;
    max-height: 220px;
    border-radius: var(--qx-radius-card);
    box-shadow: 0 0 0 1px var(--qx-border);
  }

  .setup {
    margin-top: 6px;
    padding: 6px 14px;
    border: 0;
    border-radius: var(--qx-radius-button);
    background: var(--qx-accent);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }

  .small {
    font-size: 11px;
  }

  .grants .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .grants {
    padding: 0 var(--qx-space-3);
    color: var(--qx-text-muted);
    font-size: var(--qx-text-small);
  }

  .to-auto {
    margin-left: 8px;
    padding: 0 8px;
    height: 20px;
    border: 1px solid var(--qx-accent);
    border-radius: 6px;
    background: transparent;
    color: var(--qx-accent);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
</style>
