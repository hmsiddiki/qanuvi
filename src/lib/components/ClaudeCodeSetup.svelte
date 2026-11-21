<script lang="ts">
  import { shownError } from "$lib/stores/linkState.svelte";
  /**
   * c1: Claude Code on the shown machine, from the window. Suggested, never required: Qanuvi works without it. The
   * install is Claude Code's own native installer, its command shown first; the sign-in runs `claude auth login` in a
   * terminal on that machine, shown here. Its sign-in page opens in this computer's browser from a button (a shown
   * VPS has no browser of its own), and the code the page gives goes in the box below, typed into that terminal.
   */
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { routed as invoke, shownPlace } from "$lib/remote";
  import { api, errorText, type TermEvent } from "$lib/api";
  import { ask } from "$lib/stores/confirm.svelte";
  import { agent } from "$lib/stores/agent.svelte";
  import TerminalView from "./TerminalView.svelte";

  interface Status {
    installed: boolean;
    version: string;
    signedIn: boolean | null;
    method: string;
  }
  interface InstallPlan {
    line: string;
  }

  let status = $state<Status | null>(null);
  let error = $state("");
  let link = $state("");
  /** What runs in the terminal below: the install or the sign-in, and whether it ended. */
  let job = $state<{ tool: "claude" | "claude-sign-in"; line: string; ended: boolean } | null>(null);
  let outcome = $state("");
  let signInUrl = $state("");
  let code = $state("");
  let termId = $state("");
  let seen = "";
  const place = shownPlace();
  /** In the Agents card: nothing more is said once it is installed and signed in (the card says so). */
  let { compact = false }: { compact?: boolean } = $props();
  /** A new terminal for each install or sign-in. */
  let round = $state(0);

  onMount(() => void look());

  async function look() {
    status = await invoke<Status>("setup_claude").catch((e) => {
      error = errorText(e);
      return null;
    });
  }

  async function install() {
    error = "";
    link = "";
    outcome = "";
    let plan: InstallPlan;
    try {
      plan = await invoke<InstallPlan>("setup_install_plan", { tool: "claude" });
    } catch (e) {
      const text = errorText(e);
      if (text.startsWith("link:")) link = text.slice("link:".length);
      else error = text;
      return;
    }
    const lines = [
      `Install Claude Code on ${place}? It runs Claude Code's own installer:`,
      plan.line,
      "You see it run here. Using Claude Code needs a Claude subscription or an Anthropic Console account; Qanuvi works without it.",
    ];
    if (!(await ask(lines.join("\n\n"), { title: "Install Claude Code", okLabel: "Install" }))) return;
    begin("claude", plan.line);
  }

  function signIn() {
    error = "";
    outcome = "";
    begin("claude-sign-in", "claude auth login");
  }

  function begin(tool: "claude" | "claude-sign-in", line: string) {
    signInUrl = "";
    code = "";
    seen = "";
    termId = "";
    round++;
    job = { tool, line, ended: false };
  }

  /** The sign-in page's address in what the terminal shows: its hyperlink first (one piece), else the plain text. */
  function heard(e: TermEvent) {
    if (e.kind !== "data" || job?.tool !== "claude-sign-in" || signInUrl) return;
    seen = (seen + e.data).slice(-16000);
    const osc = /\x1b\]8;[^;]*;(https:\/\/[^\x07\x1b]+)(?:\x07|\x1b\\)/g;
    const plain = seen.replace(/\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*(?:\x07|\x1b\\)/g, "");
    const urls = [...seen.matchAll(osc)].map((m) => m[1]);
    urls.push(...(plain.match(/https:\/\/[^\s"'<>]+/g) ?? []));
    const page = urls.find((u) => {
      try {
        const h = new URL(u).hostname;
        return /(^|\.)(claude\.ai|claude\.com|anthropic\.com)$/.test(h) && /oauth|login|authorize/i.test(u);
      } catch {
        return false;
      }
    });
    if (page) signInUrl = page;
  }

  function start(cols: number, rows: number, onEvent: (e: TermEvent) => void) {
    const tool = job?.tool ?? "claude";
    return api.term
      .installStart(tool, cols, rows, (e) => {
        heard(e);
        onEvent(e);
      })
      .then((id) => (termId = id));
  }

  async function sendCode() {
    const c = code.trim();
    if (!c || !termId) return;
    code = "";
    await api.term.write(termId, c + "\r");
  }

  async function ended(exitCode: number | null) {
    const j = job;
    if (!j) return;
    job = { ...j, ended: true };
    await look();
    void agent.loadInfo();
    if (j.tool === "claude") {
      outcome = status?.installed
        ? `Claude Code is installed on ${place}. Sign in to use it.`
        : `Claude Code was not installed${exitCode !== null ? ` (exit code ${exitCode})` : ""}. What it said is above.`;
    } else {
      outcome = status?.signedIn
        ? `Claude Code is signed in on ${place}.`
        : `Claude Code is not signed in yet${exitCode !== null ? ` (exit code ${exitCode})` : ""}. What it said is above.`;
    }
  }

  function closeJob() {
    job = null;
    outcome = "";
  }
</script>

<div class="cc">
  {#if !status}
    <p class="muted">{error ? "" : `Looking for Claude Code on ${place}…`}</p>
  {:else if !status.installed}
    <p>
      Claude Code is not on {place}. Qanuvi can install it with Claude Code's own installer. It is optional: Qanuvi
      works without it, and using it needs a Claude subscription or an Anthropic Console account.
    </p>
    <button type="button" class="btn primary" disabled={!!job && !job.ended} onclick={install}
      >Install Claude Code</button
    >
  {:else if status.signedIn === false}
    <p>Claude Code {status.version} is on {place}, not signed in yet.</p>
    <button type="button" class="btn primary" disabled={!!job && !job.ended} onclick={signIn}
      >Sign in to Claude Code</button
    >
  {:else if !(compact && status.signedIn)}
    <p>
      Claude Code {status.version} is on {place}{status.signedIn
        ? `, signed in${status.method ? ` (${status.method})` : ""}`
        : ""}.
    </p>
    {#if status.signedIn === null}
      <button type="button" class="btn" disabled={!!job && !job.ended} onclick={signIn}>Sign in to Claude Code</button>
    {/if}
  {/if}

  {#if job}
    <p class="small">On {place}: <code>{job.line}</code></p>
    {#if job.tool === "claude-sign-in" && !job.ended}
      <div class="signin">
        {#if signInUrl}
          <button type="button" class="btn primary" onclick={() => openUrl(signInUrl)}>Open the sign-in page</button>
          <span class="muted small">in your browser here; sign in, then copy the code the page shows.</span>
        {:else}
          <span class="muted small">Waiting for Claude Code's sign-in page…</span>
        {/if}
      </div>
      <form
        class="code"
        onsubmit={(e) => {
          e.preventDefault();
          void sendCode();
        }}
      >
        <input bind:value={code} placeholder="Code from the sign-in page" aria-label="Code from the sign-in page" />
        <button type="submit" class="btn" disabled={!code.trim() || !termId}>Send the code</button>
      </form>
    {/if}
    <div class="term">
      {#key round}
        <TerminalView
          projectId=""
          shell=""
          visible={true}
          {start}
          endText={job.tool === "claude" ? "The install has ended." : "The sign-in has ended."}
          onexit={ended}
        />
      {/key}
    </div>
    {#if job.ended}
      <button type="button" class="link" onclick={closeJob}>Close the terminal</button>
    {:else}
      <p class="muted small">Type in the terminal if it asks for something.</p>
    {/if}
  {/if}
  {#if outcome}<p class="small">{outcome}</p>{/if}
  {#if link}
    <p class="small">
      There is no install command for Claude Code on {place}. Install it there from <code>{link}</code>, then
      <button type="button" class="link" onclick={look}>look again</button>.
    </p>
  {/if}
  {#if shownError(error)}<p class="error small">{shownError(error)}</p>{/if}
</div>

<style>
  .cc p {
    margin: 4px 0;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .signin,
  .code {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 6px 0;
  }

  .code input {
    width: 280px;
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--qx-border);
    border-radius: 4px;
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
  }

  .btn {
    height: 24px;
    margin: 6px 8px 0 0;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .signin .btn,
  .code .btn {
    margin: 0;
  }

  .btn.primary {
    border-color: var(--qx-accent);
    background: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--qx-accent);
    font: inherit;
    cursor: pointer;
  }

  .term {
    position: relative;
    height: 220px;
    margin: 6px 0;
    border-radius: 6px;
    background: #141416;
    overflow: hidden;
  }

  code {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--qx-bg-panel);
    user-select: text;
    word-break: break-all;
  }
</style>
