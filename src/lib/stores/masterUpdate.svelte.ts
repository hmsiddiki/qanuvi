/**
 * Migration M8c: the update of the shown master, on screen. The master does all the work (M8a looks on the channel,
 * M8b puts the update in); the window only asks it and shows where it is, with the same look as This PC's own update:
 * a slim "Update to x" button in the status bar, its progress, then "Restart to update". The restart waits for running
 * work on that master and says so; it never cuts a run off. "Download updates by themselves" is that master's own
 * setting (Settings, Updates, while it is shown), off unless switched on.
 *
 * M8d: This PC too. Its master updates the whole install: the build is the app's signed installer, and "Restart to
 * update" closes the windows (unsaved text kept as drafts) once nothing works there; the installer starts Qanuvi again.
 */
import { callMaster, shownMachine, THIS_PC } from "$lib/remote";
import { errorText } from "$lib/api";
import { masters } from "./masters.svelte";

/** What a master's `update_status` answers (master_update.rs UpdateState). */
export interface MasterUpdateState {
  current: string;
  checking: boolean;
  checkedAt: number | null;
  available: { version: string; notes: string; url: string } | null;
  error: string;
  /** s12: "dev", or a test channel's address (older masters do not say). */
  channel?: string;
  install: {
    stage: "" | "downloading" | "checking" | "ready" | "waiting" | "restarting" | "failed";
    version: string;
    done: number;
    total: number;
    waitingFor: string;
    error: string;
    program: string;
  };
}

export type ShownUpdateState =
  | "idle"
  | "checking"
  | "none"
  | "available"
  | "downloading"
  | "verifying"
  | "ready"
  | "waiting"
  | "restarting"
  | "error";

/** The window's view of a master's answer. */
export function viewOf(s: MasterUpdateState | null): {
  state: ShownUpdateState;
  version: string;
  notes: string;
  error: string;
  noBuild: string;
} {
  const found = s?.available ?? null;
  const out = {
    state: "idle" as ShownUpdateState,
    version: found?.version ?? "",
    notes: found?.notes ?? "",
    error: "",
    noBuild: "",
  };
  if (!s) return out;
  const i = s.install;
  if (i.version) out.version = i.version;
  switch (i.stage) {
    case "downloading":
      return { ...out, state: "downloading" };
    case "checking":
      return { ...out, state: "verifying" };
    case "ready":
      return { ...out, state: "ready" };
    case "waiting":
      return { ...out, state: "waiting" };
    case "restarting":
      return { ...out, state: "restarting" };
  }
  // A failed update changed nothing: the button is offered again, the reason beside it.
  const failed = i.stage === "failed" ? `The update failed: ${i.error}` : "";
  if (found?.url) return { ...out, state: "available", error: failed || s.error };
  if (s.checking) return { ...out, state: "checking" };
  if (found)
    return { ...out, state: "none", noBuild: `${found.version} is out, but has no build for this machine yet.` };
  if (s.error) return { ...out, state: "error", error: s.error };
  return { ...out, state: s.checkedAt ? "none" : "idle" };
}

/** Nothing being put in. */
const NO_INSTALL: MasterUpdateState["install"] = {
  stage: "",
  version: "",
  done: 0,
  total: 0,
  waitingFor: "",
  error: "",
  program: "",
};

/** The machine on screen as a link: This PC's own master, or a paired one. */
const here = (): string => shownMachine() ?? THIS_PC;

/** How often the shown master is asked while its update goes on, and otherwise (it looks on its own every hour). */
const WHILE_UNDER_WAY = 1000;
const OTHERWISE = 5 * 60 * 1000;

class MasterUpdateStore {
  /** The machine these facts are about (null: none read yet). */
  machine = $state<string | null>(null);
  raw = $state<MasterUpdateState | null>(null);
  /** A call failed (the button's own error, shown in Settings). */
  note = $state("");
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** The version a restart was asked for on that machine (its connection ends on the restart). */
  #restartFor: { machine: string; version: string } | null = null;

  #view = $derived(viewOf(this.raw));

  get state(): ShownUpdateState {
    return this.#view.state;
  }
  get version(): string {
    return this.#view.version;
  }
  get notes(): string {
    return this.#view.notes;
  }
  get error(): string {
    return this.note || this.#view.error;
  }
  get noBuild(): string {
    return this.#view.noBuild;
  }
  get current(): string {
    return this.raw?.current ?? "";
  }
  /** Where updates come from: "dev", or a test channel's address. */
  get channel(): string {
    return this.raw?.channel || "dev";
  }
  get checkedAt(): number | null {
    return this.raw?.checkedAt ?? null;
  }
  get waitingFor(): string {
    return this.raw?.install.waitingFor ?? "";
  }
  /** This PC is shown: its update is the whole install's. */
  get thisPc(): boolean {
    return this.machine === THIS_PC;
  }
  get percent(): number {
    const i = this.raw?.install;
    return i?.total ? Math.min(100, Math.round((i.done / i.total) * 100)) : 0;
  }

  /** Another machine is shown: its own facts are read (none of the machine before's show meanwhile). */
  switched(): void {
    const machine = here();
    if (machine === this.machine) return;
    this.machine = machine;
    this.raw = null;
    this.note = "";
    void this.read();
  }

  /** Reads the shown master's update state, and asks again soon while an update goes on. */
  async read(): Promise<void> {
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = null;
    const machine = here();
    let got: MasterUpdateState | null = null;
    try {
      got = await callMaster<MasterUpdateState>(machine, "update_status", {});
    } catch {
      // Its connection ended while it restarts for the update: the window connects again.
      if (this.#restartFor?.machine === machine) return this.#backAfterRestart(machine);
    }
    if (machine !== here()) return;
    // A master from before M8b answers without its install part (and a wrong answer has none either).
    if (got && typeof got === "object") got = { ...got, install: { ...NO_INSTALL, ...got.install } };
    if (got) {
      this.raw = got;
      if (this.#restartFor?.machine === machine && got.current === this.#restartFor.version) {
        this.#restartFor = null;
        this.note = "";
      } else if (got.install.stage === "restarting" && machine !== THIS_PC) {
        // (This PC's windows close for its update; the installer opens Qanuvi again.)
        this.#restartFor = { machine, version: got.install.version };
      }
    }
    const busy = ["downloading", "verifying", "waiting", "restarting"].includes(this.state);
    this.#timer = setTimeout(() => void this.read(), busy || this.#restartFor ? WHILE_UNDER_WAY : OTHERWISE);
  }

  /** "Check now" in Settings: the master looks on the channel at once. */
  async check(): Promise<void> {
    const machine = here();
    this.note = "";
    try {
      const got = await callMaster<MasterUpdateState>(machine, "update_check", {});
      if (machine === here()) this.raw = got;
    } catch (e) {
      if (machine === here()) this.note = errorText(e);
    }
  }

  /** "Update to x": the master downloads and checks the build, then holds it for "Restart to update". */
  download(): Promise<void> {
    return this.#install(true);
  }

  /** "Restart to update": the master puts the checked build in once nothing works there, and restarts from it. */
  restartToUpdate(): Promise<void> {
    return this.#install(false);
  }

  async #install(hold: boolean): Promise<void> {
    const machine = here();
    this.note = "";
    try {
      const got = await callMaster<MasterUpdateState>(machine, "update_install", { hold });
      if (!hold && machine !== THIS_PC) this.#restartFor = { machine, version: got.install.version };
      if (machine === here()) this.raw = got;
    } catch (e) {
      if (machine === here()) this.note = errorText(e);
      return;
    }
    void this.read();
  }

  /** The master ended for its update: connect again (the new one may take a while to start), then read it. */
  async #backAfterRestart(machine: string): Promise<void> {
    const failed = await masters.backAfterRestart(machine, here() === machine, 10);
    if (machine !== here()) return;
    if (failed) {
      this.#restartFor = null;
      this.note = failed;
      return;
    }
    void this.read();
  }
}

export const masterUpdate = new MasterUpdateStore();
