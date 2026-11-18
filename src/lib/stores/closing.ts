/**
 * What a window does before it goes (migration M5c, i1): closed by hand or by tray Quit, or gone with a restart or a
 * reload. Each step runs in turn, and one that fails never keeps the window open. The window's one close handler
 * (updates.svelte.ts) runs them first: two close handlers would race, and the first done closes the window.
 */
const steps: (() => Promise<void>)[] = [];

/** Adds a step (once per window load). */
export function beforeWindowGoes(step: () => Promise<void>): void {
  steps.push(step);
}

/** Runs the steps; never longer than `ms` in all, so a machine that does not answer never holds the window. */
export async function windowGoing(ms = 6000): Promise<void> {
  const all = (async () => {
    for (const step of steps) await step().catch(() => {});
  })();
  await Promise.race([all, new Promise<void>((done) => setTimeout(done, ms))]);
}
