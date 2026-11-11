/**
 * Updates (1b): Qanuvi looks for a newer version at its update address (signed; a wrong signature is refused),
 * downloads it in the background with its progress in the status bar, then offers "Restart now". Restarting installs it
 * quietly and opens Qanuvi again. Also: reload the window, restart Qanuvi.
 */
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "$lib/api";
import { agent } from "./agent.svelte";

const EVERY = 4 * 60 * 60 * 1000;

class UpdatesStore {
  state = $state<"idle" | "checking" | "none" | "downloading" | "ready" | "error">("idle");
  current = $state("");
  version = $state("");
  notes = $state("");
  got = $state(0);
  total = $state(0);
  error = $state("");
  checkedAt = $state<number | null>(null);
  /** "Later": the ready notice stays hidden until the next start (the update still installs on Restart). */
  later = $state(false);
  #update: Update | null = null;
  #timer: ReturnType<typeof setInterval> | null = null;

  get percent(): number {
    return this.total ? Math.min(100, Math.round((this.got / this.total) * 100)) : 0;
  }

  /** Starts the regular checks (once per window); development builds only check when asked. */
  start(): void {
    getVersion()
      .then((v) => (this.current = v))
      .catch(() => {});
    if (this.#timer || import.meta.env.DEV) return;
    setTimeout(() => this.check().catch(() => {}), 15_000);
    this.#timer = setInterval(() => this.check().catch(() => {}), EVERY);
  }

  /** Looks for a newer version; one that is found downloads at once. */
  async check(): Promise<void> {
    if (this.state === "checking" || this.state === "downloading" || this.state === "ready") return;
    this.state = "checking";
    this.error = "";
    try {
      const update = await check();
      this.checkedAt = Date.now();
      if (!update) {
        this.state = "none";
        return;
      }
      this.#update = update;
      this.version = update.version;
      this.notes = update.body ?? "";
      this.got = 0;
      this.total = 0;
      this.state = "downloading";
      await update.download((e) => {
        if (e.event === "Started") this.total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") this.got += e.data.chunkLength;
      });
      this.state = "ready";
    } catch (e) {
      this.state = "error";
      this.error = errorText(e);
    }
  }

  /** Installs the downloaded version quietly and opens Qanuvi again. */
  async restartToUpdate(): Promise<void> {
    if (!this.#update || this.state !== "ready") return;
    if (!(await this.#okToStop())) return;
    try {
      await this.#update.install();
      await relaunch();
    } catch (e) {
      this.state = "error";
      this.error = errorText(e);
    }
  }

  /** A full restart of Qanuvi. */
  async restart(): Promise<void> {
    if (!(await this.#okToStop())) return;
    await relaunch();
  }

  /**
   * Reloads the window's interface. Agents keep working in the background (their output is kept and shows when their
   * session is opened); terminals start again in the same folders.
   */
  async reloadWindow(): Promise<void> {
    // The window's shells stop; the list of terminals is kept, so they open again after the reload.
    await api.term.killWindow().catch(() => {});
    location.reload();
  }

  async #okToStop(): Promise<boolean> {
    const busy = agent.sessions.filter((s) => s.status === "running" || s.status === "waiting").length;
    if (!busy) return true;
    return ask(
      `${busy} agent session${busy === 1 ? " is" : "s are"} still working. Restart anyway? They stop and can be continued afterwards.`,
      { title: "Restart Qanuvi", kind: "warning", okLabel: "Restart", cancelLabel: "Wait" },
    );
  }
}

export const updates = new UpdatesStore();
