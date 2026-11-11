<script lang="ts">
  /** Memory disk settings and syncing (Milestone 18), on the Memory page. */
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { memoryDisk, type DiskVersion } from "$lib/stores/memoryDisk.svelte";
  import { settings, type MemoryDiskSettings } from "$lib/stores/settings.svelte";
  import { ago } from "$lib/stores/git.svelte";

  let s = $derived(settings.value.memoryDisk);
  let pass = $state("");
  let pass2 = $state("");
  let passError = $state("");
  let changingPass = $state(false);
  let versions = $state<DiskVersion[] | null>(null);
  let versionError = $state("");

  function set(patch: Partial<MemoryDiskSettings>) {
    settings.set("memoryDisk", { ...settings.value.memoryDisk, ...patch });
    versions = null;
  }

  async function browse() {
    const picked = await open({ directory: true, multiple: false, title: "Folder for the memory disk" });
    if (typeof picked === "string") set({ folder: picked });
  }

  async function savePass(e: SubmitEvent) {
    e.preventDefault();
    if (pass !== pass2) {
      passError = "The two passphrases are not the same.";
      return;
    }
    passError = await memoryDisk.setPassphrase(pass);
    if (!passError) {
      pass = "";
      pass2 = "";
      changingPass = false;
      memoryDisk.auto();
    }
  }

  async function forgetPass() {
    const ok = await ask(
      "Forget the passphrase on this computer? The memory disk stays as it is; this computer stops syncing until you enter the passphrase again.",
      { title: "Forget passphrase", okLabel: "Forget", cancelLabel: "Keep" },
    );
    if (ok) await memoryDisk.forgetPassphrase();
  }

  async function showVersions() {
    versionError = "";
    try {
      versions = await memoryDisk.versions();
    } catch (e) {
      versionError = String(e);
    }
  }

  async function restore(v: DiskVersion) {
    const when = new Date(v.at * 1000).toLocaleString();
    const ok = await ask(
      `Make the memory what it was on ${when}? Entries added since are deleted and changed ones go back, on every computer at their next sync. The memory as it is now stays on the disk as a version.`,
      { title: "Restore memory", kind: "warning", okLabel: "Restore", cancelLabel: "Cancel" },
    );
    if (!ok) return;
    versionError = await memoryDisk.restore(v.name);
    if (!versionError) await showVersions();
  }
</script>

<section class="disk">
  <h3><HardDrive size={14} /> Memory disk</h3>
  <p class="muted">
    An encrypted copy of your memory that your computers share. Each computer writes its own file; syncing takes in what
    the others changed. Without the passphrase the files cannot be read.
  </p>

  <div class="modes" role="radiogroup" aria-label="Where the memory is kept">
    <label
      ><input type="radio" checked={s.mode === "off"} onchange={() => set({ mode: "off" })} /> This computer only</label
    >
    <label><input type="radio" checked={s.mode === "folder"} onchange={() => set({ mode: "folder" })} /> A folder</label
    >
    <label>
      <input type="radio" checked={s.mode === "server"} onchange={() => set({ mode: "server" })} /> A folder on a server
    </label>
  </div>

  {#if s.mode === "folder"}
    <div class="row">
      <input
        value={s.folder}
        onchange={(e) => set({ folder: e.currentTarget.value })}
        placeholder="For example a folder inside Google Drive, Dropbox or OneDrive"
        spellcheck="false"
      />
      <button type="button" class="btn" onclick={browse}>Browse…</button>
    </div>
    <p class="muted">
      A folder that Google Drive, Dropbox or OneDrive keeps in sync shares the memory between your computers.
    </p>
  {:else if s.mode === "server"}
    <div class="row">
      <select value={s.server} onchange={(e) => set({ server: e.currentTarget.value })} aria-label="Server">
        <option value="">Choose a server…</option>
        {#each settings.value.masters as m (m.id)}<option value={m.id}>{m.name} ({m.target})</option>{/each}
      </select>
      <input
        value={s.serverDir}
        onchange={(e) => set({ serverDir: e.currentTarget.value })}
        placeholder="~/.qanuvi-memory"
        spellcheck="false"
        aria-label="Folder on the server"
      />
    </div>
    {#if settings.value.masters.length === 0}<p class="muted">Add your server on the Servers page first.</p>{/if}
  {/if}

  {#if s.mode !== "off"}
    {#if memoryDisk.passphraseSaved && !changingPass}
      <p class="muted">
        Passphrase saved in this computer's credential store.
        <button type="button" class="link" onclick={() => (changingPass = true)}>Change</button> ·
        <button type="button" class="link" onclick={forgetPass}>Forget</button>
      </p>
    {:else}
      <form class="row" onsubmit={savePass}>
        <input
          type="password"
          bind:value={pass}
          placeholder="Passphrase (at least 8 characters)"
          autocomplete="new-password"
        />
        <input type="password" bind:value={pass2} placeholder="Once more" autocomplete="new-password" />
        <button type="submit" class="btn primary">Save</button>
        {#if changingPass}<button type="button" class="btn" onclick={() => (changingPass = false)}>Cancel</button>{/if}
      </form>
      <p class="muted">Use the same passphrase on every computer. It cannot be recovered: keep it somewhere safe.</p>
      {#if passError}<p class="error">{passError}</p>{/if}
    {/if}

    <div class="row">
      <button
        type="button"
        class="btn primary"
        disabled={memoryDisk.busy || !memoryDisk.passphraseSaved || !memoryDisk.disk}
        onclick={() => memoryDisk.sync()}
      >
        <RefreshCw size={13} />
        {memoryDisk.busy ? "Syncing…" : "Sync now"}
      </button>
      <label class="check">
        <input type="checkbox" checked={s.auto} onchange={(e) => set({ auto: e.currentTarget.checked })} />
        Sync by itself (at start-up, after changes, every 10 minutes)
      </label>
    </div>
    {#if memoryDisk.error}<p class="error">{memoryDisk.error}</p>{/if}
    {#if memoryDisk.last}
      {@const l = memoryDisk.last}
      <p class="muted">
        Last synced {ago(l.at)}: {l.written} entries on the disk from this computer{l.devices.length
          ? `; read ${l.devices.join(", ")}`
          : "; no other computer yet"}{l.merged.added || l.merged.changed || l.merged.deleted
          ? ` (${l.merged.added} new, ${l.merged.changed} changed, ${l.merged.deleted} deleted here)`
          : ""}.
      </p>
    {/if}

    <div class="row">
      <button type="button" class="link" disabled={!memoryDisk.disk} onclick={showVersions}>Earlier versions…</button>
    </div>
    {#if versionError}<p class="error">{versionError}</p>{/if}
    {#if versions}
      {#if versions.length === 0}<p class="muted">No earlier versions yet (one is kept at each sync, up to 10).</p>{/if}
      <ul class="versions">
        {#each versions as v (v.name)}
          <li>
            <span>{new Date(v.at * 1000).toLocaleString()}</span>
            <span class="muted">{ago(v.at)}</span>
            <button type="button" class="link" onclick={() => restore(v)}>Restore</button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .disk {
    margin: var(--qx-space-3) 0;
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 4px;
    font-size: 13px;
  }

  .muted {
    margin: 4px 0;
    color: var(--qx-text-secondary);
  }

  .error {
    margin: 4px 0;
    color: var(--qx-status-failed);
  }

  .modes,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin: 6px 0;
  }

  .modes label,
  .check {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--qx-text-secondary);
  }

  .row input:not([type="checkbox"]) {
    flex: 1;
    min-width: 160px;
  }

  input:not([type="checkbox"]):not([type="radio"]),
  select {
    padding: 4px 8px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-window);
    color: var(--qx-text);
    font: inherit;
    user-select: text;
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--qx-accent);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-button);
    background: var(--qx-bg-panel);
    color: var(--qx-text);
    font: inherit;
    cursor: pointer;
  }

  .btn.primary {
    background: var(--qx-accent);
    border-color: var(--qx-accent);
    color: #fff;
  }

  .btn:disabled {
    opacity: 0.5;
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

  .link:disabled {
    color: var(--qx-text-muted);
    cursor: default;
  }

  .versions {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .versions li {
    display: flex;
    gap: 10px;
    padding: 3px 0;
  }
</style>
