/**
 * The window's own part of Settings, Updates (1b): reload the window, restart Qanuvi, and what the last window says
 * when it closes while work goes on.
 *
 * Migration M8d: the app has no updater of its own any more. This PC's master looks for, downloads and puts in a new
 * version of the whole install (masterUpdate.svelte.ts shows it, as for any shown master); for that it closes the
 * windows itself, as the tray's Quit does.
 */
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import { ask } from "$lib/stores/confirm.svelte";
import { api } from "$lib/api";
import { terminals } from "./terminals.svelte";
import { itOrThem, keepVerb, thisPcWork, workText } from "./thisPcWork";
import { windowGoing } from "./closing";

class UpdatesStore {
  /** The app's own version (the screen's). */
  current = $state("");
  #started = false;

  /** Once per window: its version, and what its closing says. */
  start(): void {
    getVersion()
      .then((v) => (this.current = v))
      .catch(() => {});
    if (this.#started) return;
    this.#started = true;
    // M5a: the window is a viewer. Its closing never stops This PC's work; the last window says what goes on.
    getCurrentWindow()
      .onCloseRequested(async () => {
        // i1: unsaved text is kept as drafts on its machine before anything else.
        await windowGoing();
        const last = (await getAllWebviewWindows().catch(() => [])).length <= 1;
        // A master that does not answer never keeps the window open (Linux: close and tray Quit did nothing).
        const work = await Promise.race([
          thisPcWork().catch(() => null),
          new Promise<null>((done) => setTimeout(() => done(null), 3000)),
        ]);
        const what = work ? workText(work) : null;
        if (last && work && what && !(await invoke<boolean>("app_quitting").catch(() => false))) {
          // Sent before the window goes, but never waited for longer than 2 s.
          const shown = invoke("notify_notice", {
            title: "Qanuvi keeps working",
            text: `${what} ${keepVerb(work)} going on this computer. Quit Qanuvi from the tray icon to stop ${itOrThem(work)}.`,
          }).catch(() => {});
          await Promise.race([shown, new Promise((done) => setTimeout(done, 2000))]);
        }
      })
      .catch(() => {});
  }

  /** A full restart of Qanuvi. */
  async restart(): Promise<void> {
    if (!(await this.#okToStop())) return;
    await windowGoing();
    await invoke("app_restart");
  }

  /**
   * Reloads the window's interface. Agents keep working in the background (their output is kept and shows when their
   * session is opened); terminals start again in the same folders.
   */
  async reloadWindow(): Promise<void> {
    // Unsaved text comes back as drafts when its project opens again (i1).
    await windowGoing();
    // The window's shells stop; the list of terminals is kept, so they open again after the reload.
    await terminals.stopThisPc();
    await api.lsp.stopWindow().catch(() => {});
    await api.dap.stopWindow().catch(() => {});
    location.reload();
  }

  /** M5a: says first what This PC's master stops (agent sessions and plan runs); nothing stops without a yes. */
  async #okToStop(): Promise<boolean> {
    const work = await thisPcWork().catch(() => null);
    const what = work ? workText(work) : null;
    if (!what || !work) return true;
    const they = itOrThem(work) === "it" ? "It" : "They";
    return ask(`Restarting Qanuvi stops ${what} on this computer. ${they} can be resumed afterwards.`, {
      title: "Restart Qanuvi",
      kind: "warning",
      okLabel: "Restart",
      cancelLabel: "Wait",
    });
  }
}

export const updates = new UpdatesStore();
