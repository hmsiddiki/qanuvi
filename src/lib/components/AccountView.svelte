<script lang="ts">
  /** The central Qanuvi account (Milestone 19). Optional; Qanuvi works fully without it. */
  import UserRound from "@lucide/svelte/icons/user-round";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { account } from "$lib/stores/account.svelte";
</script>

<div class="account">
  <h2><UserRound size={16} /> Qanuvi account</h2>

  {#if account.account}
    <div class="card">
      <p class="who">
        <strong>{account.account.name || "Signed in"}</strong>
        {#if account.account.email}<span class="muted">{account.account.email}</span>{/if}
      </p>
      <button type="button" class="btn" onclick={() => account.signOut()}>Sign out</button>
      <p class="muted small">Signing out also ends this computer's sign-in at qanuvi.com.</p>
    </div>
  {:else}
    <p class="lead">
      Sign in with your Qanuvi account to reach your computers from anywhere, use the phone app and keep your settings
      with you. It is optional: the editor, agents, servers and memory all work without an account.
    </p>
    <button type="button" class="btn primary" disabled={account.waiting} onclick={() => account.signIn()}>
      Sign in
    </button>
    <p class="muted small">
      qanuvi.com opens in your browser, where you sign in with Google, Microsoft or GitHub. Qanuvi never sees your
      password.
    </p>
  {/if}
  {#if account.error}<p class="error">{account.error}</p>{/if}
</div>

{#if account.waiting}
  <div class="backdrop">
    <div class="popup" role="dialog" aria-modal="true" aria-label="Signing in">
      <LoaderCircle size={22} class="spin" />
      <h3>Finish signing in in your browser</h3>
      <p class="muted">
        qanuvi.com opened in your browser. Choose Google, Microsoft or GitHub there; this closes by itself when you are
        signed in.
      </p>
      <button type="button" class="btn" onclick={() => account.cancel()}>Cancel</button>
    </div>
  </div>
{/if}

<style>
  .account {
    height: 100%;
    overflow: auto;
    padding: var(--qx-space-3) var(--qx-space-4, 20px);
    font-size: var(--qx-text-small);
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: var(--qx-space-3) 0 var(--qx-space-2);
    font-size: 15px;
  }

  .lead,
  .muted {
    color: var(--qx-text-secondary);
  }

  .lead {
    max-width: 560px;
  }

  .small {
    font-size: 11px;
  }

  .card {
    padding: var(--qx-space-2) var(--qx-space-3);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    max-width: 520px;
  }

  .who {
    display: flex;
    gap: 10px;
    align-items: baseline;
    margin: 0;
  }

  .btn {
    height: 30px;
    margin: 6px 0;
    padding: 0 16px;
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
    opacity: 0.6;
    cursor: default;
  }

  .error {
    color: var(--qx-status-failed);
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .popup {
    display: grid;
    justify-items: center;
    gap: 6px;
    width: min(380px, 90vw);
    padding: var(--qx-space-4, 20px);
    border: 1px solid var(--qx-border);
    border-radius: var(--qx-radius-card);
    background: var(--qx-bg-panel);
    text-align: center;
    font-size: var(--qx-text-small);
  }

  .popup h3 {
    margin: 4px 0 0;
    font-size: 14px;
  }

  .popup :global(.spin) {
    animation: spin 1s linear infinite;
    color: var(--qx-accent);
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
