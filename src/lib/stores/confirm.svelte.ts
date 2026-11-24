/**
 * Qanuvi's own confirm dialog: the same call as the system one (plugin-dialog's ask), drawn in Qanuvi's look, with
 * Enter to confirm and Escape to cancel. Answers true for the OK button.
 */

export interface AskOptions {
  title?: string;
  kind?: "info" | "warning" | "error";
  okLabel?: string;
  cancelLabel?: string;
  /** s10: a third button beside OK (between Cancel and OK); its answer is "other". */
  otherLabel?: string;
}

interface Pending extends AskOptions {
  message: string;
  resolve: (answer: Answer) => void;
}

/** OK (true), Cancel (false), or the third button ("other"). */
export type Answer = boolean | "other";

class ConfirmStore {
  current = $state<Pending | null>(null);
  #queue: Pending[] = [];

  ask(message: string, options: AskOptions | string = {}): Promise<Answer> {
    const o = typeof options === "string" ? { title: options } : options;
    return new Promise((resolve) => {
      const p: Pending = { ...o, message, resolve };
      if (this.current) this.#queue.push(p);
      else this.current = p;
    });
  }

  answer(a: Answer): void {
    const c = this.current;
    if (!c) return;
    this.current = this.#queue.shift() ?? null;
    c.resolve(a === "other" && !c.otherLabel ? false : a);
  }
}

export const confirmDialog = new ConfirmStore();

/** Asks the user to confirm, in Qanuvi's own dialog (the same call as the system dialog's ask). */
export function ask(message: string, options?: AskOptions | string): Promise<boolean> {
  const o = typeof options === "string" ? { title: options } : (options ?? {});
  return confirmDialog.ask(message, { ...o, otherLabel: undefined }) as Promise<boolean>;
}

/** s10: asks with three buttons: OK (true), Cancel (false), or the third (`otherLabel`, "other"). */
export function choose(message: string, options: AskOptions & { otherLabel: string }): Promise<Answer> {
  return confirmDialog.ask(message, options);
}
