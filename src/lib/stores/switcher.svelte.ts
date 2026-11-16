/**
 * The machine switcher's state (24z3 Part B): which machine the window shows, kept reactive for the sidebar, and the
 * one the owner picked last, remembered across restarts (the main window only; a project window shows its project's
 * machine).
 */
export const SHOWN_KEY = "qanuvi.shownMachine";

/**
 * A shown master that cannot be reached (24z3 Part E). "lost": its connection ended while it was shown; the screen
 * stays as it was, read-only, under "connection lost, reconnecting" and comes back by itself when it answers.
 * "offline": nothing of it is on screen, only "not connected" and Reconnect (picked while unreachable, or lost for
 * too long). Its work goes on on the machine either way.
 */
export interface Held {
  id: string;
  mode: "lost" | "offline";
  /** Why the last try failed (empty before the first one). */
  error: string;
  /** Tries made since it was lost. */
  tries: number;
  /** When the next try starts (ms; 0: none planned, or one is under way). */
  nextAt: number;
  /** When it was lost (ms). */
  since: number;
}

class SwitcherStore {
  /** The machine on screen: null is This PC. */
  shown = $state<string | null>(null);
  /** A machine being switched to (its screen is being read). */
  switching = $state<string | null>(null);
  /** The shown master when it cannot be reached (Part E), else null. */
  held = $state<Held | null>(null);

  /** The machine the owner picked last (null: This PC). */
  remembered(): string | null {
    try {
      return localStorage.getItem(SHOWN_KEY) || null;
    } catch {
      return null;
    }
  }

  remember(machine: string | null): void {
    try {
      if (machine) localStorage.setItem(SHOWN_KEY, machine);
      else localStorage.removeItem(SHOWN_KEY);
    } catch {
      // No storage: the window opens on This PC next time.
    }
  }
}

export const switcher = new SwitcherStore();
