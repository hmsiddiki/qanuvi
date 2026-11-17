/**
 * The central Qanuvi account at qanuvi.com (Milestone 19), per machine (migration M2): each machine keeps its own
 * sign-in, and the window shows the shown machine's. A browser sign-in begins on that machine, which makes the sign-in
 * address; this window opens it in this computer's browser, catches the site's answer and hands the one-time code back
 * to the machine, which alone trades it for tokens. A server can also be signed in with an application password
 * (`qanuvi-master login`). Signing in is optional: everything else works without it.
 */
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "$lib/api";
import { callOn } from "$lib/remote";
import { switcher } from "./switcher.svelte";

export interface Account {
  site: string;
  user_id: string;
  name: string;
  email: string;
  /** "browser" (the site's sign-in page) or "password" (an application password, `qanuvi-master login`). */
  via: string;
}

interface Begun {
  url: string;
  state: string;
}

/** This PC is "". */
const key = (machine: string | null) => machine ?? "";

/** Answers that mean "no sign-in kept here", never an error to show. */
const signedOut = (why: string) => why === "Not signed in." || why.startsWith("Signed out");

class AccountStore {
  #accounts = $state<Record<string, Account | null>>({});
  #errors = $state<Record<string, string>>({});
  /** The machine whose browser sign-in is waiting ("" This PC); null when none is (one at a time: one port). */
  waitingFor = $state<string | null>(null);

  /** The shown machine's account. */
  get account(): Account | null {
    return this.#accounts[key(switcher.shown)] ?? null;
  }

  get error(): string {
    return this.#errors[key(switcher.shown)] ?? "";
  }

  get waiting(): boolean {
    return this.waitingFor !== null;
  }

  accountOn(machine: string | null): Account | null {
    return this.#accounts[key(machine)] ?? null;
  }

  #set(machine: string | null, account: Account | null, error = ""): void {
    const k = key(machine);
    this.#accounts = { ...this.#accounts, [k]: account };
    this.#errors = { ...this.#errors, [k]: error };
  }

  /** Who is signed in on a machine, from its kept sign-in (quietly; no sign-in page). */
  async load(machine: string | null = switcher.shown): Promise<void> {
    try {
      this.#set(machine, await callOn<Account>(machine, "account_me", {}));
    } catch (e) {
      const why = errorText(e);
      // Offline is not signed out: the machine keeps its sign-in for next time.
      this.#set(machine, null, signedOut(why) ? "" : why);
    }
  }

  /** Signs a machine in through this computer's browser. */
  async signIn(machine: string | null = switcher.shown): Promise<void> {
    if (this.waiting) return;
    this.waitingFor = key(machine);
    this.#errors = { ...this.#errors, [key(machine)]: "" };
    try {
      const b = await callOn<Begun>(machine, "account_begin", {});
      const code = await invoke<string>("account_wait", { url: b.url, state: b.state });
      this.#set(machine, await callOn<Account>(machine, "account_finish", { state: b.state, code }));
    } catch (e) {
      const why = errorText(e);
      if (why !== "Sign-in cancelled.") this.#errors = { ...this.#errors, [key(machine)]: why };
    } finally {
      this.waitingFor = null;
    }
  }

  cancel(): void {
    invoke("account_cancel").catch(() => {});
  }

  async signOut(machine: string | null = switcher.shown): Promise<void> {
    try {
      await callOn(machine, "account_sign_out", {});
      this.#set(machine, null);
    } catch (e) {
      this.#errors = { ...this.#errors, [key(machine)]: errorText(e) };
    }
  }
}

export const account = new AccountStore();
