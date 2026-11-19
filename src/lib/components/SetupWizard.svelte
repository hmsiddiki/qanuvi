<script lang="ts">
  /**
   * First-time setup (24c): one guided path so the user configures nothing by hand. It measures the computer and says
   * honestly what local models can do here, then sets up what the user ticks: a local model through Ollama (installed
   * if missing), OpenRouter (the user signs in and pastes the key; Qanuvi does the rest) and OmniRoute (installed,
   * started and connected; sign-ins open in the browser). Every install, page and sign-in waits for a click.
   * It is the shown machine's (24z3 D2): an install is asked first with its exact command, then runs in a terminal
   * on that machine, shown here (a password it asks for is typed there), and its end is said.
   */
  import { onMount } from "svelte";
  import { routed as invoke, shownMachine, shownPlace, shownPlaceTitle } from "$lib/remote";
  import { api } from "$lib/api";
  import { ask } from "$lib/stores/confirm.svelte";
  import TerminalView from "./TerminalView.svelte";
  import ClaudeCodeSetup from "./ClaudeCodeSetup.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { errorText } from "$lib/api";
  import { localVerdict, recommend } from "$lib/agents/guides";
  import { brain } from "$lib/stores/brain.svelte";
  import { guides } from "$lib/stores/guides.svelte";
  import { omni } from "$lib/stores/omniroute.svelte";
  import { settings, type ModelProvider } from "$lib/stores/settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  interface Tools {
    ollama: boolean;
    node: boolean;
    npm: boolean;
    omniroute: boolean;
    winget: boolean;
    os: string;
  }

  let tools = $state<Tools | null>(null);
  let room = $state<[number, number] | null>(null);
  // c1: Claude Code is suggested, never required (unticked).
  let want = $state({ local: false, openrouter: true, omniroute: true, claude: false });
  let step = $state<"pc" | "local" | "openrouter" | "omniroute" | "claude" | "done">("pc");
  let busy = $state("");
  let error = $state("");
  /** Where to get a tool by hand when there is no install command for it on the machine. */
  let link = $state<{ tool: string; url: string } | null>(null);
  /** The install running in the terminal below, and what it ended with. */
  let installing = $state<{ tool: string; line: string; ended: boolean } | null>(null);
  let outcome = $state("");
  const NAMES: Record<string, string> = { ollama: "Ollama", node: "Node.js", omniroute: "OmniRoute" };

  interface InstallPlan {
    tool: string;
    line: string;
    admin: boolean;
  }
  let orKey = $state("");

  const provider = (id: string) => settings.value.brain.providers.find((p) => p.id === id) as ModelProvider;
  let place = $state(shownPlace());
  let title = $state(shownPlaceTitle());
  let verdict = $derived(guides.machine ? localVerdict(guides.machine, title) : null);

  onMount(async () => {
    await guides.loadMachine();
    tools = await invoke<Tools>("setup_tools").catch(() => null);
    room = await invoke<[number, number] | null>("machine_room").catch(() => null);
    // Local is ticked only when it is useful here.
    want.local = verdict?.level !== "none" && settings.value.brain.local;
  });

  const order = ["pc", "local", "openrouter", "omniroute", "claude", "done"] as const;
  function next() {
    // An earlier step's install result stays only while its terminal is still shown.
    if (!installing) outcome = "";
    const i = order.indexOf(step);
    for (const s of order.slice(i + 1)) {
      if (s === "done" || want[s as keyof typeof want]) {
        step = s;
        if (s === "done") finish();
        return;
      }
    }
  }

  async function run(label: string, f: () => Promise<unknown>) {
    busy = label;
    error = "";
    try {
      await f();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  async function install(tool: string) {
    error = "";
    link = null;
    outcome = "";
    const name = NAMES[tool] ?? tool;
    let plan: InstallPlan;
    try {
      plan = await invoke<InstallPlan>("setup_install_plan", { tool });
    } catch (e) {
      const text = errorText(e);
      if (text.startsWith("link:")) link = { tool, url: text.slice("link:".length) };
      else error = text;
      return;
    }
    const where = shownPlace();
    const lines = [`Install ${name} on ${where}? It runs:`, plan.line];
    if (plan.admin) lines.push(`Windows asks for permission on ${where}'s own screen: someone there has to click Yes.`);
    lines.push("You see it run here; if it asks for a password, type it in the terminal.");
    if (!(await ask(lines.join("\n\n"), { title: `Install ${name}`, okLabel: "Install" }))) return;
    installing = { tool, line: plan.line, ended: false };
  }

  /** The install ended: the tools are looked for again, and the wizard says whether it is there now. */
  async function installEnded(code: number | null) {
    const job = installing;
    if (!job) return;
    installing = { ...job, ended: true };
    tools = await invoke<Tools>("setup_tools").catch(() => tools);
    const name = NAMES[job.tool] ?? job.tool;
    const there = !!tools?.[job.tool as keyof Tools];
    outcome =
      there || code === 0
        ? `${name} is installed on ${place}.`
        : `${name} was not installed${code !== null ? ` (exit code ${code})` : ""}. What it said is above.`;
    if (code === 0 && !there)
      outcome += ` Qanuvi does not find it yet: restart Qanuvi on ${place} so it sees the new program.`;
  }

  /** The terminal goes and what the install ended with goes with it. */
  function closeInstall() {
    installing = null;
    outcome = "";
  }

  async function lookAgain() {
    link = null;
    tools = await invoke<Tools>("setup_tools").catch(() => tools);
  }

  // Local: get the suggested coder and chat models, then use them.
  async function getLocal() {
    const p = provider("ollama");
    const s = guides.local;
    await run("Starting Ollama…", async () => {
      await brain.find(p);
      if (brain.errors[p.id]) throw new Error(brain.errors[p.id]);
    });
    if (error) return;
    for (const m of [s?.coder, s?.chat]) {
      if (!m) continue;
      if (!brain.found[p.id]?.some((x) => x.name === m.model.name)) {
        busy = `Downloading ${m.model.name} (${m.model.sizeGb} GB)…`;
        const ok = await guides.pull(p, m.model.name);
        busy = "";
        if (!ok) {
          error = guides.pullError[m.model.name] ?? `${m.model.name} could not be downloaded.`;
          return;
        }
        await brain.find(p);
      }
      const found = brain.found[p.id]?.find((x) => x.name === m.model.name);
      if (found) brain.use(found);
    }
    settings.set("brain", { ...settings.value.brain, local: true });
    next();
  }

  // OpenRouter: the user pastes the key; Qanuvi keeps it, checks it, finds and tests the free models, uses the best.
  async function setUpOpenRouter() {
    const p = provider("openrouter");
    if (orKey.trim()) {
      const e = await brain.setKey(p.id, orKey.trim());
      if (e) return (error = e);
      orKey = "";
    }
    await run("Checking the key and testing the free models…", async () => {
      await guides.checkKey(p);
      await brain.find(p);
      if (brain.errors[p.id]) throw new Error(brain.errors[p.id]);
      const picks = recommend(brain.found[p.id] ?? [], {}, 5);
      for (const c of picks)
        if (await brain.probe(p, c.model as Parameters<typeof brain.probe>[1]))
          brain.use(c.model as Parameters<typeof brain.use>[0]);
    });
    if (!error) next();
  }

  // OmniRoute: installed, started and connected; OpenRouter's key reused; models tested; combo made.
  async function setUpOmniRoute() {
    const p = provider("omniroute");
    await run("Starting OmniRoute and connecting Qanuvi…", async () => {
      if (!brain.keys[p.id]) await brain.connect(p);
      else await brain.find(p);
      if (brain.errors[p.id] && !brain.keys[p.id]) throw new Error(brain.errors[p.id]);
      await omni.load(p);
      if (brain.keys.openrouter && !(omni.connections ?? []).some((c) => c.provider === "openrouter"))
        await omni.addKey(p, "openrouter", "", "openrouter");
    });
  }

  async function finishOmniRoute() {
    const p = provider("omniroute");
    await run("Testing OmniRoute's models and making Qanuvi's combo…", async () => {
      await brain.find(p);
      const picks = recommend(brain.found[p.id] ?? [], {}, 6);
      const ok: string[] = [];
      for (const c of picks)
        if (await brain.probe(p, c.model as Parameters<typeof brain.probe>[1])) {
          brain.use(c.model as Parameters<typeof brain.use>[0]);
          if (c.model.name.includes("/") && !c.model.name.startsWith("auto")) ok.push(c.model.name);
        }
      if (ok.length) await omni.makeCombo(p, "qanuvi-free", ok);
    });
    if (!error) next();
  }

  function finish() {
    settings.set("agent", "qanuvi");
    // Online models first for changes; questions still go to a local model when there is one.
    const b = settings.value.brain;
    const rank = (id: string) => (id.startsWith("omniroute/") ? 0 : id.startsWith("openrouter/") ? 1 : 2);
    settings.set("brain", { ...b, models: [...b.models].sort((x, y) => rank(x.id) - rank(y.id)) });
  }
</script>

<div class="wizard">
  <div class="top">
    <strong>Set up free AI</strong>
    <span class="muted small">Every install, page and sign-in waits for your click.</span>
    <span class="spacer"></span>
    <button type="button" class="link" onclick={onclose}>Close</button>
  </div>

  {#if step === "pc"}
    <h4>1. {title}</h4>
    {#if guides.machine}
      <p>
        {guides.machine.ramGb} GB memory{guides.machine.gpu
          ? `, ${guides.machine.gpu.name} (${guides.machine.gpu.vramGb} GB)`
          : ", no separate graphics card"}{room
          ? ` · free now: ${room[0].toFixed(1)} GB memory, ${room[1].toFixed(1)} GB on the card`
          : ""}.
      </p>
      {#if verdict}<p class="verdict {verdict.level}">{verdict.text}</p>{/if}
      <p class="muted small">Set up:</p>
      <label class="check"
        ><input type="checkbox" bind:checked={want.local} disabled={verdict?.level === "none"} /> A model on {place}
        (Ollama: free, works offline)</label
      >
      <label class="check"
        ><input type="checkbox" bind:checked={want.openrouter} /> OpenRouter (free online models; you sign in and copy one
        key)</label
      >
      <label class="check"
        ><input type="checkbox" bind:checked={want.omniroute} /> OmniRoute (free Claude and Gemini through your Google account;
        Qanuvi installs and runs it)</label
      >
      <label class="check"
        ><input type="checkbox" bind:checked={want.claude} /> Claude Code (paid: a Claude subscription or an Anthropic
        Console account; Qanuvi installs it on {place} and you sign in from here)</label
      >
      <button type="button" class="btn primary" onclick={next}>Next</button>
    {:else}
      <p class="muted">Looking at {place}…</p>
    {/if}
  {:else if step === "local"}
    <h4>2. A model on {place}</h4>
    {#if tools && !tools.ollama}
      <p>Ollama runs the models. It is not installed yet.</p>
      <button type="button" class="btn primary" disabled={!!busy || !!installing} onclick={() => install("ollama")}
        >Install Ollama</button
      >
    {:else}
      <p>
        Qanuvi downloads {[guides.local?.coder?.model.name, guides.local?.chat?.model.name]
          .filter(Boolean)
          .join(" and ")}
        ({[guides.local?.coder?.model.sizeGb, guides.local?.chat?.model.sizeGb]
          .filter(Boolean)
          .reduce((a, b) => (a ?? 0) + (b ?? 0), 0)} GB) and sets them up.
      </p>
      <button type="button" class="btn primary" disabled={!!busy} onclick={getLocal}>Get them</button>
    {/if}
    <button type="button" class="link" onclick={next}>Skip</button>
  {:else if step === "openrouter"}
    <h4>3. OpenRouter</h4>
    {#if brain.keys.openrouter}
      <p>A key is already kept. Qanuvi checks it and picks the best free models.</p>
    {:else}
      <ol class="small">
        <li>
          Sign in or sign up (Google, GitHub or e-mail):
          <button type="button" class="link" onclick={() => openUrl("https://openrouter.ai")}>open openrouter.ai</button
          >
        </li>
        <li>
          Make a key named Qanuvi and copy it:
          <button type="button" class="link" onclick={() => openUrl("https://openrouter.ai/keys")}
            >open the keys page</button
          >
        </li>
        <li>Paste it here; it goes to your system's credential store.</li>
      </ol>
      <input type="password" bind:value={orKey} placeholder="OpenRouter key" aria-label="OpenRouter key" />
    {/if}
    <button
      type="button"
      class="btn primary"
      disabled={!!busy || (!brain.keys.openrouter && !orKey.trim())}
      onclick={setUpOpenRouter}>Set it up</button
    >
    <button type="button" class="link" onclick={next}>Skip</button>
  {:else if step === "omniroute"}
    <h4>4. OmniRoute</h4>
    {#if tools && !tools.node}
      <p>OmniRoute needs Node.js.</p>
      <button type="button" class="btn primary" disabled={!!busy || !!installing} onclick={() => install("node")}
        >Install Node.js</button
      >
    {:else if tools && !tools.omniroute}
      <p>OmniRoute is not installed yet (MIT licence, from npm).</p>
      <button type="button" class="btn primary" disabled={!!busy || !!installing} onclick={() => install("omniroute")}
        >Install OmniRoute</button
      >
    {:else if !brain.keys.omniroute}
      <button type="button" class="btn primary" disabled={!!busy} onclick={setUpOmniRoute}>Start and connect it</button>
    {:else}
      <p>
        Connected. For the free Claude and Gemini models, sign in with your Google account (the page opens in your
        browser; come back when it says you are done).
      </p>
      <button
        type="button"
        class="btn"
        disabled={!!busy || !!omni.busy.antigravity}
        onclick={() => omni.signIn(provider("omniroute"), "antigravity")}
        >{omni.busy.antigravity ?? "Sign in with Google"}</button
      >
      <button type="button" class="btn primary" disabled={!!busy} onclick={finishOmniRoute}
        >Test the models and finish</button
      >
    {/if}
    <button type="button" class="link" onclick={next}>Skip</button>
  {:else if step === "claude"}
    <h4>5. Claude Code</h4>
    <ClaudeCodeSetup />
    <button type="button" class="btn primary" onclick={next}>Next</button>
  {:else}
    <h4>Done</h4>
    <p>
      Qanuvi agent is set for new sessions with {settings.value.brain.models.length} models: online ones first for changes,
      a model on {place} for questions when there is one. The list is below; you can reorder it any time.
    </p>
    <button type="button" class="btn primary" onclick={onclose}>Close</button>
  {/if}

  {#if busy}<p class="muted small">{busy}</p>{/if}
  {#if installing}
    <p class="small">On {place}: <code>{installing.line}</code></p>
    <div class="term">
      <TerminalView
        projectId=""
        shell=""
        visible={true}
        start={(cols, rows, onEvent) => api.term.installStart(installing?.tool ?? "", cols, rows, onEvent)}
        endText="The install has ended."
        onexit={installEnded}
      />
    </div>
    {#if installing.ended}
      <button type="button" class="link" onclick={closeInstall}>Close the terminal</button>
    {:else}
      <p class="muted small">Installing… type in the terminal if it asks for something.</p>
    {/if}
  {/if}
  {#if outcome}<p class="small">{outcome}</p>{/if}
  {#if link}
    <p class="small">
      There is no install command for {NAMES[link.tool] ?? link.tool} on {place}. Install it there from
      {#if shownMachine() === null}
        <button type="button" class="link" onclick={() => link && openUrl(link.url)}>{link.url}</button>,
      {:else}
        <code>{link.url}</code>,
      {/if}
      then <button type="button" class="link" onclick={lookAgain}>look again</button>.
    </p>
  {/if}
  {#if error}<p class="error small">{error}</p>{/if}
  {#if omni.error}<p class="error small">{omni.error}</p>{/if}
</div>

<style>
  .wizard {
    margin: 8px 0;
    padding: 10px 12px;
    border: 1px solid var(--qx-accent);
    border-radius: 8px;
    background: var(--qx-bg-window);
    font-size: 12px;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h4 {
    margin: 10px 0 6px;
    font-size: 13px;
  }

  p {
    margin: 4px 0;
  }

  .verdict {
    padding: 6px 8px;
    border-left: 3px solid var(--qx-status-waiting);
    background: var(--qx-bg-panel);
  }

  .verdict.coding {
    border-color: var(--qx-status-done);
  }

  .verdict.none {
    border-color: var(--qx-status-failed);
  }

  .check {
    display: flex;
    gap: 6px;
    align-items: center;
    margin: 3px 0;
  }

  .muted {
    color: var(--qx-text-secondary);
  }

  .small {
    font-size: 11px;
  }

  .spacer {
    flex: 1;
  }

  .error {
    color: var(--qx-status-failed);
  }

  ol {
    margin: 4px 0;
    padding-left: 18px;
  }

  input[type="password"] {
    width: 260px;
    height: 24px;
    margin: 4px 8px 4px 0;
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
    height: 200px;
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
  }
</style>
