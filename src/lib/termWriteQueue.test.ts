import { describe, expect, it } from "vitest";
import { termWriteQueue } from "./termWriteQueue";

// A send whose calls finish only when the test says so, in any order it likes.
function slowSend() {
  const calls: Array<{ id: string; data: string; finish: () => void; fail: (e: unknown) => void }> = [];
  const send = (id: string, data: string) =>
    new Promise<void>((finish, fail) => calls.push({ id, data, finish: () => finish(), fail }));
  return { calls, send };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("terminal write queue", () => {
  it("sends a lone keystroke at once", () => {
    const { calls, send } = slowSend();
    const write = termWriteQueue(send);
    write("t1", "a");
    expect(calls.map((c) => c.data)).toEqual(["a"]);
  });

  it("gathers what is typed while a write is on its way and sends it after, in order", async () => {
    const { calls, send } = slowSend();
    const write = termWriteQueue(send);
    const typed = "hostname\r".split("").map((ch) => write("t1", ch));
    expect(calls.map((c) => c.data)).toEqual(["h"]);
    calls[0].finish();
    await tick();
    expect(calls.map((c) => c.data)).toEqual(["h", "ostname\r"]);
    calls[1].finish();
    await Promise.all(typed);
  });

  it("keeps terminals apart", () => {
    const { calls, send } = slowSend();
    const write = termWriteQueue(send);
    write("t1", "a");
    write("t2", "b");
    write("t1", "c");
    expect(calls.map((c) => `${c.id}:${c.data}`)).toEqual(["t1:a", "t2:b"]);
  });

  it("tells every writer of a failed batch, and goes on with the next", async () => {
    const { calls, send } = slowSend();
    const write = termWriteQueue(send);
    const first = write("t1", "a");
    const second = write("t1", "b");
    const third = write("t1", "c");
    calls[0].finish();
    await first;
    await tick();
    calls[1].fail(new Error("link ended"));
    await expect(second).rejects.toThrow("link ended");
    await expect(third).rejects.toThrow("link ended");
    write("t1", "d");
    expect(calls.at(-1)?.data).toBe("d");
  });
});
