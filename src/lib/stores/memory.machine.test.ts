import { beforeEach, describe, expect, it, vi } from "vitest";

// 24z3 D7: memory entries are the shown machine's; an answer from the machine before a switch is dropped.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
const kept = new Map<string, string>();
vi.stubGlobal("localStorage", {
  getItem: (k: string) => kept.get(k) ?? null,
  setItem: (k: string, v: string) => void kept.set(k, v),
  removeItem: (k: string) => void kept.delete(k),
  clear: () => kept.clear(),
});

const { invoke: rawInvoke } = await import("@tauri-apps/api/core");
const invoke = vi.mocked(rawInvoke);
const { thisPcAsPlainCommands } = await import("$lib/testing/thisPc");
thisPcAsPlainCommands(invoke);
const { showMachine } = await import("$lib/remote");
const { memory } = await import("./memory.svelte");

const nameOf = (c: unknown, args: unknown) => (c === "remote_call" ? (args as { cmd: string }).cmd : c);
const suggested = (n: number) => Array.from({ length: n }, (_, i) => ({ id: `s${i}`, status: "suggested" }));

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue([]);
  showMachine(null);
  memory.waiting = 0;
});

describe("memory entries on the shown machine", () => {
  it("asks the shown master for its entries", async () => {
    invoke.mockImplementation((cmd, args) =>
      Promise.resolve(nameOf(cmd, args) === "memory_list" && cmd === "remote_call" ? suggested(2) : []),
    );
    showMachine("vps");
    await memory.refreshWaiting();
    expect(invoke).toHaveBeenCalledWith(
      "remote_call",
      expect.objectContaining({ master: "vps", cmd: "memory_list", args: { project: null, suggested: true } }),
    );
    expect(memory.waiting).toBe(2);
  });

  it("drops a count that comes after a switch", async () => {
    let answer: (v: unknown) => void = () => {};
    invoke.mockImplementation((cmd) =>
      cmd === "remote_call" ? new Promise((r) => (answer = r)) : Promise.resolve(suggested(1)),
    );
    showMachine("vps");
    const late = memory.refreshWaiting();
    showMachine(null);
    await memory.refreshWaiting();
    answer(suggested(5));
    await late;
    expect(memory.waiting).toBe(1);
  });
});
