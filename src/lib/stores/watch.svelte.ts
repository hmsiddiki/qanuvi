/**
 * Qanuvi's watch over what the user relies on (15f). Since 24z3 D5 each machine's core watches its own OmniRoute (a
 * minute after it starts, then every 30 minutes) and keeps the Inbox items itself, so a machine not shown is watched
 * too. The window only asks the shown machine to look, at start and every 30 minutes, for the status bar.
 */
import { moveOldMuted, omni } from "./omniroute.svelte";

const EVERY = 30 * 60 * 1000;
let timer: ReturnType<typeof setInterval> | null = null;

/** Asks the shown machine to look now. */
export async function lookOnce(): Promise<void> {
  await omni.look();
}

/** Starts the watch (once per window). */
export function startWatch(): void {
  if (timer) return;
  moveOldMuted();
  lookOnce().catch(() => {});
  timer = setInterval(() => lookOnce().catch(() => {}), EVERY);
}
