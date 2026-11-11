/**
 * Qanuvi's watch over what the user relies on (15f): at start and every 30 minutes it looks at OmniRoute's
 * connections (when Qanuvi runs OmniRoute for the user) and puts sign-ins to renew and keys that stopped working in
 * the Inbox and the status bar. Solved problems leave the Inbox by themselves.
 */
import { brain } from "./brain.svelte";
import { inbox } from "./inbox.svelte";
import { connectionName, omni } from "./omniroute.svelte";
import { settings } from "./settings.svelte";

const EVERY = 30 * 60 * 1000;
let timer: ReturnType<typeof setInterval> | null = null;

/** Looks once now. */
export async function lookOnce(): Promise<void> {
  const b = settings.value.brain;
  const p = b.providers.find((x) => x.id === "omniroute");
  if (!p || !b.omniAssist) return;
  await brain.loadKeys().catch(() => {});
  if (!brain.keys[p.id]) return;
  await omni.load(p);
  if (!omni.connections) return;
  for (const c of omni.connections) {
    const key = `sign-in:${c.id}`;
    if ((c.state === "needs sign-in" || c.state === "error") && !omni.muted.includes(c.id))
      inbox.add({
        kind: "sign-in",
        key,
        title:
          c.state === "needs sign-in" ? `${connectionName(c)}: sign in again` : `${connectionName(c)}: not working`,
        text:
          (c.problem ? `${c.problem} ` : "") +
          (c.state === "needs sign-in"
            ? "Its free models stop until the sign-in is renewed."
            : "Its models are skipped until it works again."),
        open: { label: "Open OmniRoute", view: "agents" },
      });
    else inbox.resolve(key);
  }
}

/** Starts the watch (once per window). */
export function startWatch(): void {
  if (timer) return;
  lookOnce().catch(() => {});
  timer = setInterval(() => lookOnce().catch(() => {}), EVERY);
}
