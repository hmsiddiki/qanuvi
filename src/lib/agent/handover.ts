/**
 * Chat handover (24s): a chat whose context has grown past the limit goes on in a new session. The agent at work
 * finishes the edit in hand and answers with a handover note; the new session starts from the note and the last
 * request, and the chat shows both sessions as one timeline.
 */
import type { Item } from "./events";

/** How the request for a note starts (the conversation shows it as a line, not as the user's words). */
export const CHAT_HANDOVER_MARK = "(Qanuvi: chat handover.";
/** How the new session's first message starts (shown as the "New session" line with the note). */
export const CHAT_RESUME_MARK = "(Qanuvi: this chat goes on from an earlier session.";
/** The first line of the agent's note. */
export const NOTE_HEAD = "Handover note";
/** "Not now" holds the bar back until the context has grown by this much again. */
export const OFFER_AGAIN_AFTER = 50_000;

export const tokensK = (n: number) => `${Math.round(n / 1000)}k`;

/** Whether the chat should offer (or start) a handover now. */
export function chatHandoverDue(
  context: number,
  setting: { when: "ask" | "auto" | "off"; limit: number },
  chatOff: boolean,
  notNowAt: number,
): boolean {
  if (setting.when === "off" || chatOff || setting.limit <= 0 || context < setting.limit) return false;
  return !notNowAt || context >= notNowAt + OFFER_AGAIN_AFTER;
}

/** What the agent is told: finish the edit in hand, then answer with a note a new session can work from. */
export function chatHandoverRequest(context: number): string {
  return (
    `${CHAT_HANDOVER_MARK} This session's context is now about ${tokensK(context)} tokens; a new session takes the ` +
    `chat over so the work stays focused and the cost stays down. Finish the edit you are making so no file is left ` +
    `half-written or broken; do not start anything new. Then answer with a handover note only, starting with the ` +
    `line "${NOTE_HEAD}", that a new session can work from without guessing: the goal; what is done and checked; ` +
    `what is left, in order; the files you touched and why; anything half-way; decisions and their reasons; facts ` +
    `about the code you learned (names, places, traps). Nothing else after the note.)`
  );
}

/** The new session's first message: the note, the last request, and how to go on. */
export function chatResumePrompt(note: string, lastRequest: string, was: number): string {
  return (
    `${CHAT_RESUME_MARK} It handed over at about ${tokensK(was)} tokens of context.)\n\n` +
    `${note.trim()}\n\n` +
    (lastRequest.trim() ? `The user's last request in that session:\n\n${lastRequest.trim()}\n\n` : "") +
    `Read the files the note names and check that what it calls done really is (fix it if not). Then go on with ` +
    `what is left; do not redo what is done and correct. If nothing is left, say so briefly and wait for the user.`
  );
}

/** Reads a resume message back: the note and the size it handed over at (null: not a resume message). */
export function parseResume(text: string): { note: string; was: number } | null {
  if (!text.startsWith(CHAT_RESUME_MARK)) return null;
  const was = Number(/about (\d+)k tokens/.exec(text)?.[1] ?? 0) * 1000;
  const body = text.slice(text.indexOf(")") + 1).trim();
  const end = body.indexOf("\n\nThe user's last request in that session:");
  const end2 = body.indexOf("\n\nRead the files the note names");
  const cut = end >= 0 ? end : end2 >= 0 ? end2 : body.length;
  return { note: body.slice(0, cut).trim(), was };
}

/**
 * The note the agent wrote: its text after the request (from the "Handover note" line when there is one). Without
 * one (the agent did not answer, or was stopped), the conversation's end stands in for it.
 */
export function noteAfter(items: Item[], requestIndex: number, fallback: () => string): string {
  const texts = items
    .slice(requestIndex + 1)
    .filter((i): i is Extract<Item, { kind: "text" }> => i.kind === "text" && !i.live)
    .map((i) => i.text);
  const all = texts.join("\n\n");
  const at = all.lastIndexOf(NOTE_HEAD);
  const note = (at >= 0 ? all.slice(at) : all).trim();
  return note.length >= 40 ? note : fallback();
}

/** The user's last request before index `before` (their own words, not Qanuvi's lines). */
export function lastRequest(items: Item[], before: number): string {
  for (let i = Math.min(before, items.length) - 1; i >= 0; i--) {
    const x = items[i];
    if (x.kind === "user") return x.text;
  }
  return "";
}

/** The chain of a chat, oldest first: each session and the ones it goes on from (stops at a loop or a gap). */
export function chainOf<T extends { id: string; continues_from?: string | null }>(sessions: T[], id: string): T[] {
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const out: T[] = [];
  const seen = new Set<string>();
  let s = byId.get(id);
  while (s && !seen.has(s.id)) {
    seen.add(s.id);
    out.unshift(s);
    s = s.continues_from ? byId.get(s.continues_from) : undefined;
  }
  return out;
}

/** The chats to list: a session another one goes on from is shown through that one. */
export function chatsOnly<T extends { id: string; continues_from?: string | null }>(sessions: T[]): T[] {
  const continued = new Set(sessions.map((s) => s.continues_from).filter((x): x is string => !!x));
  return sessions.filter((s) => !continued.has(s.id));
}
