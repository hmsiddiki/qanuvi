<script lang="ts">
  import { shownError } from "$lib/stores/linkState.svelte";
  /**
   * The Servers list of the master on screen (migration M3b): the servers that machine connects to itself, kept on it
   * with their pairing keys. Adding, pairing, connecting and removing here change that machine's list, never This
   * PC's. Its servers are not shown from the switcher yet (nested machines, M3c).
   */
  import Plug from "@lucide/svelte/icons/plug";
  import Unplug from "@lucide/svelte/icons/unplug";
  import Trash2 from "@lucide/svelte/icons/trash";
  import Pencil from "@lucide/svelte/icons/pencil";
  import { ask } from "$lib/stores/confirm.svelte";
  import { displayName, masters } from "$lib/stores/masters.svelte";
  import { machineServers as ms } from "$lib/stores/machineServers.svelte";
  import { machineName } from "$lib/remote";
  import { COLORS, NAME_MAX, nameProblem } from "$lib/machineNames";
  import { settings, type MasterSettings } from "$lib/stores/settings.svelte";
  import IconButton from "./IconButton.svelte";

  let { machine }: { machine: string } = $props();

  const here = $derived(machineName(machine));
  const connection = $derived(masters.statusOf(machine).info);

  $effect(() => {
    // Read again for another machine, or a new connection to it.
    if (connection) void ms.load(machine, connection);
  });

  let name = $state("");
  let target = $state("");
  let program = $state("qanuvi-master");
  let identity = $state("");
  let auto = $state(true);
  let formError = $state("");
  let adding = $state(false);

  let pairRelay = $state(settings.value.remoteAccess.relay);
  let pairCode = $state("");
  let pairName = $state("");
  let pairError = $state("");
  let pairing = $state(false);

  let renaming = $state<string | null>(null);
  let newName = $state("");
  let renameError = $state("");

  async function add(e: SubmitEvent) {
    e.preventDefault();
    const t = target.trim();
    if (!t) {
      formError = "Give the server's SSH address, for example deploy@203.0.113.5 or an alias from its SSH config.";
      return;
    }
    adding = true;
    formError = await ms.add({
      id: crypto.randomUUID(),
      name: name.trim(),
      target: t,
      program: program.trim() || "qanuvi-master",
      identity: identity.trim(),
      autoConnect: auto,
    });
    adding = false;
    if (!formError) {
      name = "";
      target = "";
    }
  }

  async function pairNow(e: SubmitEvent) {
    e.preventDefault();
    pairing = true;
    pairError = await ms.pair(pairRelay, pairCode, pairName);
    pairing = false;
    if (!pairError) pairCode = "";
  }

  async function saveName(e: SubmitEvent, id: string) {
    e.preventDefault();
    renameError = await ms.rename(id, newName);
    if (!renameError) renaming = null;
  }

  async function remove(m: MasterSettings) {
    const ok = await ask(
      `Remove ${displayName(m)} from the servers of ${here}? Its connection there ends at once; nothing on ${displayName(m)} is changed.`,
      { title: "Remove server", okLabel: "Remove", cancelLabel: "Keep" },
    );
    if (ok) await ms.remove(m);
  }

  const reachedBy = (m: MasterSettings) =>
    m.relay ? "through the relay" : displayName(m) === m.target ? "" : m.target;
  const nameIssue = (m: MasterSettings) => (m.name ? nameProblem(m.name, ms.namesBesides(m.id)) : "");
</script>

<p class="lead">
  The servers that {here} connects to itself. {here} keeps this list and their pairing keys, connects to them when it starts,
  and tries an away one again every minute. Their work runs on them, whether this window is open or not.
</p>
{#if !connection}
  <p class="muted">Not connected to {here}.</p>
{:else if ms.machine === machine}
  {#if shownError(ms.error)}<p class="error">{shownError(ms.error)}</p>{/if}

  {#each ms.list as m (m.id)}
    {@const st = ms.statusOf(m.id)}
    <section class="card">
      <header>
        <span class="dot {st.state}"></span>
        {#if m.color}<span class="swatch" style:background={m.color} title="Its colour"></span>{/if}
        <strong>{displayName(m)}</strong>
        {#if reachedBy(m)}<span class="muted">{reachedBy(m)}</span>{/if}
        <span class="spacer"></span>
        {#if st.state === "on"}
          <IconButton label="Disconnect" onclick={() => ms.disconnect(m.id)}><Unplug size={14} /></IconButton>
        {:else}
          <IconButton label="Connect" disabled={st.state === "connecting"} onclick={() => ms.connect(m)}>
            <Plug size={14} />
          </IconButton>
        {/if}
        <IconButton
          label="Rename"
          onclick={() => {
            renaming = m.id;
            newName = m.name;
            renameError = "";
          }}><Pencil size={14} /></IconButton
        >
        <IconButton label="Remove" onclick={() => remove(m)}><Trash2 size={14} /></IconButton>
      </header>
      {#if renaming === m.id}
        <form class="row" onsubmit={(e) => saveName(e, m.id)}>
          <input bind:value={newName} maxlength={NAME_MAX + 10} placeholder="Empty: its own computer name" />
          <button type="submit" class="btn primary">Save</button>
          <button type="button" class="btn" onclick={() => (renaming = null)}>Cancel</button>
        </form>
        {#if shownError(renameError)}<p class="error">{shownError(renameError)}</p>{/if}
      {:else if nameIssue(m)}
        <p class="error">{nameIssue(m)} Change the name so the two are never mixed up.</p>
      {/if}
      {#if st.state === "connecting"}<p class="muted">Connecting…</p>{/if}
      {#if st.state === "error"}<p class="error">{st.error}</p>{/if}
      {#if st.state === "on" && st.info}
        <p class="muted">{st.info.host || "server"} · {st.info.os}/{st.info.arch} · master {st.info.version}</p>
      {/if}
      <label class="check">
        <input
          type="checkbox"
          checked={m.autoConnect}
          onchange={(e) => ms.setAutoConnect(m.id, e.currentTarget.checked)}
        />
        Connect when {here} starts
      </label>
      <div class="colors" role="group" aria-label="Its colour">
        <span class="muted">Colour</span>
        {#each COLORS as c (c)}
          <button
            type="button"
            class="color"
            class:picked={m.color === c}
            style:background={c}
            aria-label="Colour {c}"
            aria-pressed={m.color === c}
            onclick={() => ms.setColor(m.id, c)}
          ></button>
        {/each}
      </div>
    </section>
  {:else}
    <p class="muted">{here} has no servers yet.</p>
  {/each}

  <h3>Pair {here} with a computer through the relay</h3>
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
      Name of {here} on the other computer (empty: its computer name)
      <input bind:value={pairName} maxlength={NAME_MAX + 10} />
    </label>
    {#if shownError(pairError)}<p class="error">{shownError(pairError)}</p>{/if}
    <button type="submit" class="btn primary" disabled={pairing || !pairRelay.trim() || !pairCode.trim()}>
      {pairing ? "Pairing…" : "Pair and connect"}
    </button>
  </form>

  <h3>Add a server to {here}</h3>
  <form class="add" onsubmit={add}>
    <label>
      Name (empty: the server's own computer name)
      <input bind:value={name} placeholder="My VPS" maxlength={NAME_MAX + 10} />
    </label>
    <label>
      SSH address, as {here} reaches it
      <input
        bind:value={target}
        placeholder="deploy@203.0.113.5 or an alias from its ~/.ssh/config"
        spellcheck="false"
      />
    </label>
    <label>
      Program on the server
      <input bind:value={program} spellcheck="false" />
    </label>
    <label>
      SSH key file on {here} (optional)
      <input bind:value={identity} placeholder="~/.ssh/id_ed25519 · empty: its SSH defaults" spellcheck="false" />
    </label>
    <label class="check"><input type="checkbox" bind:checked={auto} /> Connect when {here} starts</label>
    {#if shownError(formError)}<p class="error">{shownError(formError)}</p>{/if}
    <button type="submit" class="btn primary" disabled={adding}>{adding ? "Connecting…" : "Add and connect"}</button>
  </form>
  <p class="muted">
    {here} signs in with its own SSH keys. In its terminal the same list works too:
    <code>qanuvi-master servers list</code>, <code>servers add &lt;ssh address&gt;</code>,
    <code>servers remove &lt;name&gt;</code>, <code>pair --relay &lt;address&gt; --code &lt;code&gt;</code>.
  </p>
{:else}
  <p class="muted">Reading the servers of {here}…</p>
{/if}
