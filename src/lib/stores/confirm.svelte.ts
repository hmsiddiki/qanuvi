/**
 * Qanuvi's own confirm dialog: the same call as the system one (plugin-dialog's ask), drawn in Qanuvi's look, with
 * Enter to confirm and Escape to cancel. Answers true for the OK button.
 */

export interface AskOptions {
  title?: string;
  kind?: "info" | "warning" | "error";
  okLabel?: string;
  cancelLabel?: string;
}

interface Pending extends AskOptions {
  message: string;
  resolve: (yes: boolean) => void;
}

class ConfirmStore {
  current = $state<Pending | null>(null);
  #queue: Pending[] = [];

  ask(message: string, options: AskOptions | string = {}): Promise<boolean> {
    const o = typeof options === "string" ? { title: options } : options;
    return new Promise((resolve) => {
      const p: Pending = { ...o, message, resolve };
      if (this.current) this.#queue.push(p);
      else this.current = p;
    });
  }

  answer(yes: boolean): void {
    const c = this.current;
    if (!c) return;
    this.current = this.#queue.shift() ?? null;
    c.resolve(yes);
  }
}

export const confirmDialog = new ConfirmStore();

/** Asks the user to confirm, in Qanuvi's own dialog (the same call as the system dialog's ask). */
export function ask(message: string, options?: AskOptions | string): Promise<boolean> {
  return confirmDialog.ask(message, options);
}
