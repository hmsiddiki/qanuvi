/**
 * The central Qanuvi account at qanuvi.com (Milestone 19). Signing in opens qanuvi.com's sign-in page in the browser
 * (Google, Microsoft or GitHub there); Qanuvi keeps only a refresh token in the system credential store. Signing in is
 * optional: everything else works without it.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { settings } from "./settings.svelte";

export interface Account {
  site: string;
  user_id: string;
  name: string;
  email: string;
}

class AccountStore {
  account = $state<Account | null>(null);
  /** Waiting for the sign-in to be finished in the browser. */
  waiting = $state(false);
  error = $state("");

  /** At start-up: who is signed in, from the kept sign-in (quietly; no sign-in page). */
  async init(): Promise<void> {
    if (!settings.value.account.signedIn) return;
    try {
      this.account = await invoke<Account>("account_me");
    } catch (e) {
      this.account = null;
      const why = errorText(e);
      // Offline is not signed out: keep the sign-in for next time.
      if (why.startsWith("Signed out") || why === "Not signed in.") this.#mark(false);
      else this.error = why;
    }
  }

  #mark(signedIn: boolean) {
    settings.set("account", { signedIn });
  }

  async signIn(): Promise<void> {
    this.waiting = true;
    this.error = "";
    try {
      this.account = await invoke<Account>("account_sign_in");
      this.#mark(true);
    } catch (e) {
      const why = errorText(e);
      if (why !== "Sign-in cancelled.") this.error = why;
    } finally {
      this.waiting = false;
    }
  }

  cancel(): void {
    invoke("account_cancel").catch(() => {});
  }

  async signOut(): Promise<void> {
    try {
      await invoke("account_sign_out");
    } catch (e) {
      this.error = errorText(e);
    } finally {
      this.account = null;
      this.#mark(false);
    }
  }
}

export const account = new AccountStore();
