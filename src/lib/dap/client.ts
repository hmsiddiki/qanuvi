/**
 * One debug session's connection (15i part 3): Debug Adapter Protocol requests with answers, events, and the
 * adapter's own requests back (js-debug's `startDebugging`). The transport is given: a function that sends one
 * message's text, and `receive` for each message that comes in.
 */
import { errorOf } from "./protocol";

export interface DapMessage {
  seq: number;
  type: "request" | "response" | "event";
  command?: string;
  event?: string;
  request_seq?: number;
  success?: boolean;
  message?: string;
  arguments?: unknown;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  body?: any;
}

interface Waiting {
  resolve: (body: unknown) => void;
  reject: (e: Error) => void;
  timer?: ReturnType<typeof setTimeout>;
}

export class DapClient {
  #seq = 1;
  #waiting = new Map<number, Waiting>();
  #send: (text: string) => Promise<void>;
  closed = false;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  onEvent: (event: string, body: any) => void = () => {};
  /** The adapter's requests to Qanuvi: answer the response body, or throw to refuse. */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  onRequest: (command: string, args: any) => Promise<unknown> = async (command) => {
    throw new Error(`Qanuvi does not handle "${command}".`);
  };

  constructor(send: (text: string) => Promise<void>) {
    this.#send = send;
  }

  /** Sends a request; resolves with the response body, rejects with the adapter's error. */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  request<T = any>(command: string, args?: unknown, timeoutMs = 0): Promise<T> {
    if (this.closed) return Promise.reject(new Error("The debug session has ended."));
    const seq = this.#seq++;
    return new Promise<T>((resolve, reject) => {
      const w: Waiting = { resolve: resolve as (b: unknown) => void, reject };
      if (timeoutMs > 0) {
        w.timer = setTimeout(() => {
          this.#waiting.delete(seq);
          reject(new Error(`The debugger did not answer "${command}" in time.`));
        }, timeoutMs);
      }
      this.#waiting.set(seq, w);
      const msg = { seq, type: "request", command, ...(args === undefined ? {} : { arguments: args }) };
      this.#send(JSON.stringify(msg)).catch((e) => {
        this.#waiting.delete(seq);
        clearTimeout(w.timer);
        reject(e instanceof Error ? e : new Error(String(e)));
      });
    });
  }

  /** One message from the adapter. */
  receive(text: string): void {
    let msg: DapMessage;
    try {
      msg = JSON.parse(text);
    } catch {
      return;
    }
    if (msg.type === "response" && typeof msg.request_seq === "number") {
      const w = this.#waiting.get(msg.request_seq);
      if (!w) return;
      this.#waiting.delete(msg.request_seq);
      clearTimeout(w.timer);
      if (msg.success) w.resolve(msg.body ?? {});
      else w.reject(new Error(errorOf(msg)));
    } else if (msg.type === "event" && msg.event) {
      this.onEvent(msg.event, msg.body ?? {});
    } else if (msg.type === "request" && msg.command) {
      const reply = (success: boolean, body?: unknown, message?: string) =>
        this.#send(
          JSON.stringify({
            seq: this.#seq++,
            type: "response",
            request_seq: msg.seq,
            command: msg.command,
            success,
            ...(body === undefined ? {} : { body }),
            ...(message ? { message } : {}),
          }),
        ).catch(() => {});
      this.onRequest(msg.command, msg.arguments ?? {}).then(
        (body) => reply(true, body ?? {}),
        (e) => reply(false, undefined, e instanceof Error ? e.message : String(e)),
      );
    }
  }

  /** The connection is gone: everything still waiting fails. */
  close(): void {
    this.closed = true;
    for (const [, w] of this.#waiting) {
      clearTimeout(w.timer);
      w.reject(new Error("The debug session has ended."));
    }
    this.#waiting.clear();
  }
}
