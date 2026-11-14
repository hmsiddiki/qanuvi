import { describe, expect, it } from "vitest";
import { DapClient } from "./client";

function pair() {
  const sent: Record<string, unknown>[] = [];
  const c = new DapClient(async (text) => {
    sent.push(JSON.parse(text));
  });
  return { c, sent };
}

describe("DapClient", () => {
  it("matches answers to requests and passes events on", async () => {
    const { c, sent } = pair();
    const events: string[] = [];
    c.onEvent = (e, body) => events.push(`${e}:${body.reason ?? ""}`);
    const a = c.request("threads");
    const b = c.request("launch", { program: "x" });
    expect(sent.map((m) => [m.seq, m.command])).toEqual([
      [1, "threads"],
      [2, "launch"],
    ]);
    c.receive(JSON.stringify({ seq: 9, type: "response", request_seq: 2, success: false, message: "no program" }));
    c.receive(JSON.stringify({ seq: 10, type: "event", event: "stopped", body: { reason: "breakpoint" } }));
    c.receive(JSON.stringify({ seq: 11, type: "response", request_seq: 1, success: true, body: { threads: [] } }));
    await expect(a).resolves.toEqual({ threads: [] });
    await expect(b).rejects.toThrow("no program");
    expect(events).toEqual(["stopped:breakpoint"]);
  });

  it("answers the adapter's own requests", async () => {
    const { c, sent } = pair();
    c.onRequest = async (command) => {
      if (command === "startDebugging") return {};
      throw new Error("not here");
    };
    c.receive(JSON.stringify({ seq: 4, type: "request", command: "startDebugging", arguments: {} }));
    c.receive(JSON.stringify({ seq: 5, type: "request", command: "runInTerminal", arguments: {} }));
    await new Promise((r) => setTimeout(r, 0));
    expect(sent).toEqual([
      { seq: 1, type: "response", request_seq: 4, command: "startDebugging", success: true, body: {} },
      { seq: 2, type: "response", request_seq: 5, command: "runInTerminal", success: false, message: "not here" },
    ]);
  });

  it("fails what is waiting when the connection closes", async () => {
    const { c } = pair();
    const a = c.request("continue");
    c.close();
    await expect(a).rejects.toThrow(/ended/);
    await expect(c.request("next")).rejects.toThrow(/ended/);
  });
});
