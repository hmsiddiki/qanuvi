import { api, errorText, type LiveEvent } from "$lib/api";
import { shortToolName } from "$lib/agent/events";

/** Through a relay or SSH the window takes at most this many pictures a second (LM, owner: about 5). */
export const REMOTE_GAP_MS = 200;
/** The window says it still watches this often, so the machine never ends a quiet stream (a page that is still). */
export const BEAT_MS = 5000;

type Calls = Pick<typeof api.browser, "liveStart" | "liveAck" | "liveStop">;

/**
 * One live picture of an agent's test browser in Preview (LM2). The machine sends the next picture only after the
 * last one is acked, so acking is how the window sets the pace: at once on This PC, at most every REMOTE_GAP_MS
 * through a relay or SSH.
 */
export class LiveWatch {
  frame = $state("");
  width = $state(0);
  height = $state(0);
  /** "starting" until the first picture, "ended" once the stream ended or could not start. */
  status = $state<"starting" | "live" | "ended">("starting");
  /** Why it ended (empty after the user's own stop). */
  why = $state("");
  #id: string | null = null;
  #stopped = false;
  #lastAck = 0;
  #beat: ReturnType<typeof setInterval> | undefined;
  #ackTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    readonly sessionId: string,
    readonly remote: boolean,
    private calls: Calls = api.browser,
  ) {}

  async start(): Promise<void> {
    try {
      const id = await this.calls.liveStart(this.sessionId, (e) => this.#on(e));
      if (this.#stopped) {
        void this.calls.liveStop(this.sessionId, id).catch(() => {});
        return;
      }
      this.#id = id;
      this.#beat = setInterval(() => this.#ack(), BEAT_MS);
    } catch (e) {
      this.#end(errorText(e));
    }
  }

  /** The user's own stop ("Stop following", leaving Preview). */
  stop(): void {
    if (this.#stopped) return;
    this.#stopped = true;
    if (this.#id) void this.calls.liveStop(this.sessionId, this.#id).catch(() => {});
    this.#end("");
  }

  #on(e: LiveEvent): void {
    if (this.#stopped) return;
    if ("ended" in e) {
      this.#end(e.ended === "stopped" ? "" : e.ended);
      return;
    }
    this.frame = `data:image/jpeg;base64,${e.frame}`;
    this.width = e.width;
    this.height = e.height;
    this.status = "live";
    clearTimeout(this.#ackTimer);
    const wait = this.remote ? Math.max(0, REMOTE_GAP_MS - (Date.now() - this.#lastAck)) : 0;
    this.#ackTimer = setTimeout(() => this.#ack(), wait);
  }

  #ack(): void {
    if (!this.#id || this.#stopped || this.status === "ended") return;
    this.#lastAck = Date.now();
    this.calls.liveAck(this.sessionId, this.#id).catch((e) => {
      if (!this.#stopped) this.#end(errorText(e));
    });
  }

  #end(why: string): void {
    clearInterval(this.#beat);
    clearTimeout(this.#ackTimer);
    this.status = "ended";
    this.why = why;
    this.#id = null;
  }
}

/** The agent asked to show its browser live (LM3): Preview opens on it. */
class PreviewLive {
  ask = $state<{ sessionId: string; at: number } | null>(null);
}

export const previewLive = new PreviewLive();

/** Whether one line of an agent's stream is its call of browser_show_live. */
export function asksToShowLive(line: string): boolean {
  if (!line.includes("browser_show_live")) return false;
  try {
    const msg = JSON.parse(line);
    if (msg?.type !== "assistant" || !Array.isArray(msg.message?.content)) return false;
    return msg.message.content.some(
      (b: { type?: string; name?: unknown }) =>
        b?.type === "tool_use" && typeof b.name === "string" && shortToolName(b.name) === "browser_show_live",
    );
  } catch {
    return false;
  }
}
