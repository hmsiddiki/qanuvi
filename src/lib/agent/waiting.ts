/**
 * Messages sent while the agent works (24r): Qanuvi keeps each back for a few seconds first, so a message sent too soon
 * or by mistake can still be dropped ("Don't send"). Once the agent has a message it cannot be taken back; when it
 * had already started a turn of its own for it, "Don't send" stops that turn and the next message tells the agent to
 * leave the withdrawn one.
 */

/** How long a message sent while the agent works stays in Qanuvi before it goes to the agent. */
export const HOLD_MS = 10_000;

/** Where the note starts in a sent message (the stored and shown message is the user's words only). */
export const WITHDRAWN_MARK = "\n\n(Qanuvi: the user withdrew ";

/** The note sent with the next message: what the user withdrew after the agent had it. */
export function withdrawnNote(texts: string[]): string {
  const quoted = texts.map((t) => `"${t.length > 300 ? t.slice(0, 300) + "…" : t}"`).join(", ");
  return (
    `${WITHDRAWN_MARK}${texts.length === 1 ? "an earlier message" : "earlier messages"}, ${quoted}, before you ` +
    "answered it. Do not act on it; leave out anything you started for it.)"
  );
}

export function withoutWithdrawn(text: string): string {
  const i = text.indexOf(WITHDRAWN_MARK);
  return i >= 0 ? text.slice(0, i) : text;
}
