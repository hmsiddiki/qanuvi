/**
 * The machine switcher's state (24z3 Part B): which machine the window shows, kept reactive for the sidebar, and the
 * one the owner picked last, remembered across restarts (the main window only; a project window shows its project's
 * machine).
 */
export const SHOWN_KEY = "qanuvi.shownMachine";

class SwitcherStore {
  /** The machine on screen: null is This PC. */
  shown = $state<string | null>(null);
  /** A machine being switched to (its screen is being read). */
  switching = $state<string | null>(null);

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
