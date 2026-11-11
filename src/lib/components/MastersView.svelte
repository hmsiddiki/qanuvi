<script lang="ts">
  /**
   * Servers (Milestone 16): Qanuvi without a window on a Linux server or VPS, reached over SSH. Add a server,
   * connect, and add its folders as projects; they then work like this PC's projects.
   */
  import Server from "@lucide/svelte/icons/server";
  import Plug from "@lucide/svelte/icons/plug";
  import Unplug from "@lucide/svelte/icons/unplug";
  import Trash2 from "@lucide/svelte/icons/trash";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import { ask } from "$lib/stores/confirm.svelte";
  import { masters } from "$lib/stores/masters.svelte";
  import { projects } from "$lib/stores/projects.svelte";
  import { settings, type MasterSettings } from "$lib/stores/settings.svelte";
  import IconButton from "./IconButton.svelte";
  import RemoteAccessPanel from "./RemoteAccessPanel.svelte";
  import DataFolderPanel from "./DataFolderPanel.svelte";

  let name = $state("");
  let target = $state("");
  let program = $state("qanuvi-master");
  let identity = $state("");
  let auto = $state(true);
  let formError = $state("");
  let pairRelay = $state(settings.value.remoteAccess.relay);
  let pairCode = $state("");
  let pairName = $state("");
  let pairError = $state("");
  let pairing = $state(false);

  async function pairNow(e: SubmitEvent) {
    e.preventDefault();
    pairing = true;
    pairError = await masters.pair(pairRelay, pairCode, pairName);
    pairing = false;
    if (!pairError) pairCode = "";
  }
  let folder = $state<Record<string, string>>({});
  let folderError = $state<Record<string, string>>({});

  function add(e: SubmitEvent) {
    e.preventDefault();
    const t = target.trim();
    if (!t) {
      formError = "Give the server's SSH address, for example deploy@203.0.113.5 or an alias from your SSH config.";
      return;
    }
    const m: MasterSettings = {
      id: crypto.randomUUID(),
      name: name.trim() || (t === "local" ? "This PC (test)" : t),
      target: t,
      program: program.trim() || "qanuvi-master",
      identity: identity.trim(),
      autoConnect: auto,
    };
    masters.save(m);
    name = "";
    target = "";
    formError = "";
    masters.connect(m);
  }

  async function remove(m: MasterSettings) {
    const ok = await ask(
      `Remove ${m.name} from Qanuvi? Nothing on the server is changed; its projects just leave the sidebar.`,
      { title: "Remove server", okLabel: "Remove", cancelLabel: "Keep" },
    );
    if (ok) await masters.remove(m.id);
  }

  async function addFolder(m: MasterSettings) {
    const path = (folder[m.id] ?? "").trim();
    if (!path) return;
    const err = await masters.addFolder(m.id, path);
    folderError = { ...folderError, [m.id]: err };
    if (!err) folder = { ...folder, [m.id]: "" };
  }

  const count = (id: string) => projects.list.filter((p) => p.master === id).length;
</script>

<div class="servers">
  <h2><Server size={16} /> Servers</h2>
  <p class="lead">
    Qanuvi without a window on a Linux server or VPS. Its projects show in the sidebar and work like this PC's: files,
    git, search, terminals and agents run on the server, and agent sessions keep going when this PC is off.
  </p>

  <DataFolderPanel />
  <RemoteAccessPanel />

  {#each masters.list as m (m.id)}
    {@const st = masters.statusOf(m.id)}
    <section class="card">
      <header>
        <span class="dot {st.state}"></span>
        <strong>{m.name}</strong>
        <span class="muted">{m.relay ? "through the relay" : m.target === "local" ? "this PC" : m.target}</span>
        <span class="spacer"></span>
        {#if st.state === "on"}
          <IconButton label="Disconnect" onclick={() => masters.disconnect(m.id)}><Unplug size={14} /></IconButton>
        {:else}
          <IconButton label="Connect" disabled={st.state === "connecting"} onclick={() => masters.connect(m)}>
            <Plug size={14} />
          </IconButton>
        {/if}
        <IconButton label="Remove" onclick={() => remove(m)}><Trash2 size={14} /></IconButton>
      </header>
      {#if st.state === "connecting"}<p class="muted">Connecting…</p>{/if}
      {#if st.state === "error"}<p class="error">{st.error}</p>{/if}
      {#if st.state === "on" && st.info}
        <p class="muted">
          {st.info.host || "server"} · {st.info.os}/{st.info.arch} · master {st.info.version} · {count(m.id)} project{count(
            m.id,
          ) === 1
            ? ""
            : "s"}
        </p>
        <form
          class="row"
          onsubmit={(e) => {
            e.preventDefault();
            addFolder(m);
          }}
        >
          <input
            value={folder[m.id] ?? ""}
            oninput={(e) => (folder = { ...folder, [m.id]: e.currentTarget.value })}
            placeholder="Folder on the server, for example /home/deploy/sites/shop"
            spellcheck="false"
          />
          <button type="submit" class="btn"><FolderPlus size={14} /> Add as project</button>
        </form>
        {#if folderError[m.id]}<p class="error">{folderError[m.id]}</p>{/if}
      {/if}
      <label class="check">
        <input
          type="checkbox"
          checked={m.autoConnect}
          onchange={(e) => masters.save({ ...m, autoConnect: e.currentTarget.checked })}
        />
        Connect when Qanuvi starts
      </label>
    </section>
  {:else}
    <p class="muted">No servers yet.</p>
  {/each}

  <h3>Pair with a computer through the relay</h3>
  <p class="muted">
    On the other computer, turn on remote access and press "Pair a device"; type the code it shows here.
  </p>
  <form class="add" onsubmit={pairNow}>
    <label>
      Relay address
      <input bind:value={pairRelay} placeholder="wss://relay.qanuvi.com" spellcheck="false" />
    </label>
    <label>
      Pairing code
      <input bind:value={pairCode} placeholder="K7QF-29MX-T4HD" spellcheck="false" />
    </label>
    <label>
      This device's name (shown on the other computer)
      <input bind:value={pairName} placeholder="My laptop" />
    </label>
    {#if pairError}<p class="error">{pairError}</p>{/if}
    <button type="submit" class="btn primary" disabled={pairing || !pairRelay.trim() || !pairCode.trim()}>
      {pairing ? "Pairing…" : "Pair and connect"}
    </button>
  </form>

  <h3>Add a server</h3>
  <form class="add" onsubmit={add}>
    <label>
      Name
      <input bind:value={name} placeholder="My VPS" />
    </label>
    <label>
      SSH address
      <input bind:value={target} placeholder="deploy@203.0.113.5 or an alias from ~/.ssh/config" spellcheck="false" />
    </label>
    <label>
      Program on the server
      <input bind:value={program} spellcheck="false" />
    </label>
    <label>
      SSH key file (optional)
      <input bind:value={identity} placeholder="~/.ssh/id_ed25519 · empty: your SSH defaults" spellcheck="false" />
    </label>
    <label class="check"><input type="checkbox" bind:checked={auto} /> Connect when Qanuvi starts</label>
    {#if formError}<p class="error">{formError}</p>{/if}
    <button type="submit" class="btn primary">Add and connect</button>
  </form>

  <h3>Setting up a server</h3>
  <ol class="help">
    <li>
      Copy the <code>qanuvi-master</code> program to the server (for example to <code>~/.local/bin</code>) and make it
      runnable: <code>chmod +x ~/.local/bin/qanuvi-master</code>. Install <code>git</code>, and the agent (<code
        >claude</code
      >) if agents should run there.
    </li>
    <li>
      Sign-in uses your SSH keys, never a password typed into Qanuvi: <code>ssh-copy-id deploy@your-server</code>, then
      check that <code>ssh deploy@your-server</code> works without a password. For another port, add an alias in
      <code>~/.ssh/config</code> and use the alias here.
    </li>
    <li>
      That is all: Qanuvi starts the master's background service on first connect. To keep it running after the server
      restarts, run it as a user service: <code>systemctl --user enable --now qanuvi-master</code> with a unit whose
      <code>ExecStart</code> is <code>%h/.local/bin/qanuvi-master serve</code> (and
      <code>loginctl enable-linger</code>).
    </li>
    <li>
      To try it without a server, add a server with the SSH address <code>local</code>: the master program next to
      Qanuvi runs on this PC with its own data.
    </li>
  </ol>
</div>

<style>
  .servers {
    height: 100%;
    overflow: auto;
    padding: var(--qx-space-3) var(--qx-space-4, 20px);
    font-size: var(--qx-text-small);
  }

  h2,
  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: var(--qx-space-3) 0 var(--qx-space-2);
  }

  h2 {
    font-size: 15px;
  }

  h3 {
    font-size: 13px;
  }

  .lead,
  .muted {
    color: var(--qx-text-secondary);
  }

  .muted {
    margin: 4px 0;
  }

  .card {
    margin-bottom: var(--qx-space-2);
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
  }

  .card header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .spacer {
    flex: 1;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--qx-text-muted);
  }

  .dot.on {
    background: #2ea043;
  }

  .dot.connecting {
    background: #d29922;
  }

  .dot.error {
    background: #f85149;
  }

  .error {
    margin: 4px 0;
    color: var(--qx-status-failed);
  }

  .row {
    display: flex;
    gap: 6px;
    margin: 6px 0;
  }

  .add {
    display: grid;
    gap: 8px;
    max-width: 520px;
  }

  .add label:not(.check) {
    display: grid;
    gap: 3px;
    color: var(--qx-text-secondary);
  }

  input:not([type]) {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  input:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    color: var(--qx-text-secondary);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }

  .btn.primary {
    justify-self: start;
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .help {
    max-width: 720px;
    padding-left: 18px;
    color: var(--qx-text-secondary);
    line-height: 1.6;
  }

  code {
    padding: 0 4px;
    border-radius: 3px;
    background: var(--qx-bg-window);
    font-family: var(--qx-font-mono, monospace);
    user-select: text;
  }
</style>
