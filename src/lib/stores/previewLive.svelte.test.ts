import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LiveEvent } from "$lib/api";
import { BEAT_MS, LiveWatch, REMOTE_GAP_MS, asksToShowLive } from "./previewLive.svelte";

function fakeCalls() {
  let send: (e: LiveEvent) => void = () => {};
  const calls = {
    liveStart: vi.fn(async (_s: string, on: (e: LiveEvent) => void) => {
      send = on;
      return "L1";
    }),
    liveAck: vi.fn(async () => {}),
    liveStop: vi.fn(async () => {}),
  };
  return { calls, send: (e: LiveEvent) => send(e) };
}

const frame = (n: number): LiveEvent => ({ frame: `F${n}`, width: 1280, height: 800 });

describe("a live picture of the agent's browser", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("on This PC acks each picture at once, so the next one may come", async () => {
    const { calls, send } = fakeCalls();
    const w = new LiveWatch("s1", false, calls);
    await w.start();
    expect(w.status).toBe("starting");
    send(frame(1));
    expect(w.status).toBe("live");
    expect(w.frame).toBe("data:image/jpeg;base64,F1");
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.liveAck).toHaveBeenCalledWith("s1", "L1");
  });

  it("through a relay takes at most one picture per gap", async () => {
    const { calls, send } = fakeCalls();
    const w = new LiveWatch("s1", true, calls);
    await w.start();
    send(frame(1));
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.liveAck).toHaveBeenCalledTimes(1);
    send(frame(2));
    await vi.advanceTimersByTimeAsync(REMOTE_GAP_MS - 50);
    expect(calls.liveAck).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(60);
    expect(calls.liveAck).toHaveBeenCalledTimes(2);
    expect(w.frame).toBe("data:image/jpeg;base64,F2");
  });

  it("says it still watches a page that does not change", async () => {
    const { calls } = fakeCalls();
    const w = new LiveWatch("s1", false, calls);
    await w.start();
    await vi.advanceTimersByTimeAsync(BEAT_MS * 2 + 10);
    expect(calls.liveAck).toHaveBeenCalledTimes(2);
    w.stop();
    expect(calls.liveStop).toHaveBeenCalledWith("s1", "L1");
    expect(w.status).toBe("ended");
    expect(w.why).toBe("");
    await vi.advanceTimersByTimeAsync(BEAT_MS * 2);
    expect(calls.liveAck).toHaveBeenCalledTimes(2);
  });

  it("ends with the machine's reason, and a start that fails says why", async () => {
    const { calls, send } = fakeCalls();
    const w = new LiveWatch("s1", false, calls);
    await w.start();
    send({ ended: "The agent's test browser closed." });
    expect(w.status).toBe("ended");
    expect(w.why).toBe("The agent's test browser closed.");

    calls.liveStart.mockRejectedValueOnce("The agent's test browser has not opened a page yet.");
    const v = new LiveWatch("s2", false, calls);
    await v.start();
    expect(v.status).toBe("ended");
    expect(v.why).toContain("has not opened a page yet");
  });

  it("stopped before the start answered, the stream is stopped at once", async () => {
    const { calls } = fakeCalls();
    const w = new LiveWatch("s1", false, calls);
    const started = w.start();
    w.stop();
    await started;
    expect(calls.liveStop).toHaveBeenCalledWith("s1", "L1");
  });
});

describe("the agent asking to show its browser live", () => {
  const call = (name: string) =>
    JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", id: "t", name, input: {} }] } });

  it("is its call of browser_show_live, nothing else", () => {
    expect(asksToShowLive(call("mcp__qanuvi-browser__browser_show_live"))).toBe(true);
    expect(asksToShowLive(call("mcp__qanuvi-browser__browser_open"))).toBe(false);
    const text = JSON.stringify({
      type: "assistant",
      message: { content: [{ type: "text", text: "I could use browser_show_live." }] },
    });
    expect(asksToShowLive(text)).toBe(false);
    expect(asksToShowLive("not json browser_show_live")).toBe(false);
  });
});
