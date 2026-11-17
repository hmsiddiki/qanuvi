import { beforeEach, describe, expect, it, vi } from "vitest";

/** The last stream a master call was given (the Inbox watch). */
let stream: { onmessage?: (m: unknown) => void } | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (m: unknown) => void;
  },
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const { invoke } = await import("@tauri-apps/api/core");
const { thisPcAsPlainCommands } = await import("$lib/testing/thisPc");
thisPcAsPlainCommands(invoke);
const { inbox } = await import("./inbox.svelte");

const item = (id: string, at: number, extra: Record<string, unknown> = {}) => ({
  id,
  at,
  type: "info",
  kind: "run",
  title: id,
  text: "",
  read: false,
  status: "open",
  ...extra,
});

const question = (id: string, at: number) =>
  item(id, at, { type: "answer", form: { choices: [{ id: "go", label: "Go on" }] } });

let local: unknown[] = [];
let onServer: unknown[] = [];
const calls: { where: string; cmd: string; args: Record<string, unknown> }[] = [];

beforeEach(() => {
  local = [item("here-1", 100)];
  onServer = [question("vps-q", 200), item("vps-2", 50)];
  calls.length = 0;
  stream = null;
  vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "remote_call") {
      const a = args as { master: string; cmd: string; args: Record<string, unknown>; onEvent: unknown };
      calls.push({ where: a.master, cmd: a.cmd, args: a.args });
      if (a.cmd === "inbox_watch") stream = a.onEvent as typeof stream;
      if (a.cmd === "inbox_watch" || a.cmd === "inbox_list") return onServer;
      if (a.cmd === "inbox_answer") return { ...question("vps-q", 200), status: "answered" };
      return true;
    }
    calls.push({ where: "here", cmd, args: args ?? {} });
    if (cmd === "inbox_list") return local;
    if (cmd === "inbox_answer") return item(String(args?.id), 1, { status: "answered" });
    return true;
  }) as never);
});

describe("a master's Inbox", () => {
  it("takes a note about the master's own work there, or here when the master has no such command", async () => {
    const note = { kind: "drafts" as const, title: "Unsaved changes kept", text: "a", key: "draft:vps:p:a.md" };
    inbox.add(note, "vps");
    await new Promise((r) => setTimeout(r, 0));
    expect(calls.map((c) => [c.where, c.cmd])).toEqual([["vps", "inbox_add"]]);
    expect(calls[0].args).toEqual({ item: { type: "info", ...note } });

    calls.length = 0;
    vi.mocked(invoke).mockImplementation((async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "remote_call") throw new Error("This master does not know the command inbox_add.");
      calls.push({ where: "here", cmd, args: args ?? {} });
      return true;
    }) as never);
    inbox.add(note, "old");
    await new Promise((r) => setTimeout(r, 0));
    expect(calls.map((c) => [c.where, c.cmd])).toEqual([["here", "inbox_add"]]);

    calls.length = 0;
    inbox.add(note);
    await new Promise((r) => setTimeout(r, 0));
    expect(calls.map((c) => [c.where, c.cmd])).toEqual([["here", "inbox_add"]]);
  });

  it("shows its items with this computer's, newest first, marked with the master", async () => {
    await inbox.refresh();
    await inbox.followMaster("vps");
    expect(inbox.items.map((i) => [i.id, i.master ?? null])).toEqual([
      ["vps-q", "vps"],
      ["here-1", null],
      ["vps-2", "vps"],
    ]);
    expect(inbox.questions.map((i) => i.id)).toEqual(["vps-q"]);
    inbox.forgetMaster("vps");
    expect(inbox.items.map((i) => i.id)).toEqual(["here-1"]);
  });

  it("keeps a dropped master's items as not connected, with nothing sent for them, until it is back", async () => {
    await inbox.refresh();
    await inbox.followMaster("vps");
    inbox.dropMaster("vps");
    expect(inbox.items.map((i) => [i.id, !!i.offline])).toEqual([
      ["vps-q", true],
      ["here-1", false],
      ["vps-2", true],
    ]);
    calls.length = 0;
    expect(await inbox.answer("vps-q", { choice: "go", text: "" })).toBe(false);
    inbox.remove("vps-2");
    inbox.markRead();
    expect(calls.filter((c) => c.where === "vps")).toEqual([]);
    // Back: its own list replaces the copy.
    await inbox.followMaster("vps");
    expect(inbox.items.filter((i) => i.offline)).toEqual([]);
    expect(inbox.items.map((i) => i.id)).toEqual(["vps-q", "here-1", "vps-2"]);
    inbox.forgetMaster("vps");
    expect(inbox.items.map((i) => i.id)).toEqual(["here-1"]);
  });

  it("clears one machine's items only when asked for one", async () => {
    await inbox.refresh();
    await inbox.followMaster("vps");
    calls.length = 0;
    inbox.clear("vps");
    inbox.clear(null);
    expect(calls.map((c) => `${c.where}:${c.cmd}`)).toEqual(["vps:inbox_clear", "here:inbox_clear"]);
    calls.length = 0;
    inbox.clear();
    expect(calls.map((c) => `${c.where}:${c.cmd}`).sort()).toEqual(["here:inbox_clear", "vps:inbox_clear"]);
    inbox.forgetMaster("vps");
  });

  it("sends what is done to an item to the machine that has it", async () => {
    await inbox.refresh();
    await inbox.followMaster("vps");
    calls.length = 0;
    expect(await inbox.answer("vps-q", { choice: "go", text: "" })).toBe(true);
    inbox.remove("vps-2");
    inbox.remove("here-1");
    expect(calls.map((c) => `${c.where}:${c.cmd}:${c.args.id}`)).toEqual([
      "vps:inbox_answer:vps-q",
      "vps:inbox_remove:vps-2",
      "here:inbox_remove:here-1",
    ]);
    // "All read" goes to each machine with unread items (open questions stay unread).
    calls.length = 0;
    inbox.markRead();
    expect(calls.map((c) => `${c.where}:${c.cmd}`).sort()).toEqual(["here:inbox_mark_read", "vps:inbox_mark_read"]);
    inbox.forgetMaster("vps");
  });

  it("follows the master's changes and tells a new question once to the desktop", async () => {
    await inbox.refresh();
    await inbox.followMaster("vps");
    const heard: string[] = [];
    const off = inbox.onAnswer((i) => heard.push(`${i.id}@${i.master}`));
    onServer = [...onServer, question("vps-new", 300)];
    calls.length = 0;
    stream?.onmessage?.({ change: "add", item: question("vps-new", 300), new: true });
    stream?.onmessage?.({ change: "answer", item: { ...question("vps-q", 200), status: "answered" } });
    await vi.waitFor(() => expect(inbox.items[0]?.id).toBe("vps-new"));
    expect(calls.filter((c) => c.cmd === "notify_item").length).toBe(1);
    // The notification names the machine that asked.
    expect((calls.find((c) => c.cmd === "notify_item")?.args.item as { machine?: string }).machine).toBe("vps");
    expect(heard).toEqual(["vps-q@vps"]);
    // A replaced item (same key) is not told again.
    calls.length = 0;
    stream?.onmessage?.({ change: "add", item: question("vps-new", 300), new: false });
    expect(calls.filter((c) => c.cmd === "notify_item").length).toBe(0);
    off();
    inbox.forgetMaster("vps");
  });

  it("leaves an older master without Inbox commands as it was", async () => {
    vi.mocked(invoke).mockImplementation((async (cmd: string) => {
      if (cmd === "remote_call") throw new Error("This master does not know the command inbox_watch.");
      return cmd === "inbox_list" ? local : true;
    }) as never);
    await inbox.refresh();
    await inbox.followMaster("old");
    expect(inbox.items.map((i) => i.id)).toEqual(["here-1"]);
  });
});
