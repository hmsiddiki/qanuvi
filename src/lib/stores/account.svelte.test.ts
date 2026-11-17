import { describe, expect, it, vi, beforeEach } from "vitest";

// Migration M2: each machine keeps its own account; a browser sign-in begins and finishes on the machine, and only
// the browser part (account_wait) runs in this window.
const calls: string[] = [];
const answers: Record<string, unknown> = {};
const callOn = vi.fn(async (machine: string | null, cmd: string, args: Record<string, unknown>) => {
  calls.push(`${machine ?? "pc"} ${cmd}${args.code ? ` ${args.state}/${args.code}` : ""}`);
  const a = answers[`${machine ?? "pc"} ${cmd}`];
  if (a instanceof Error) throw a.message;
  return a;
});
const invoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
  calls.push(`window ${cmd}${args?.url ? ` ${args.url}` : ""}`);
  return "the-code";
});
vi.mock("$lib/remote", () => ({ callOn }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("$lib/api", () => ({ errorText: String }));
const switcher = { shown: "vps" as string | null };
vi.mock("./switcher.svelte", () => ({ switcher }));

const { account } = await import("./account.svelte");
const owner = { site: "https://qanuvi.com", user_id: "7", name: "Test Owner", email: "", via: "browser" };

beforeEach(() => {
  calls.length = 0;
  for (const k of Object.keys(answers)) delete answers[k];
});

describe("account per machine", () => {
  it("signs the shown machine in: begin and finish there, the browser here", async () => {
    answers["vps account_begin"] = { url: "https://qanuvi.com/auth?x", state: "st" };
    answers["vps account_finish"] = owner;
    await account.signIn();
    expect(calls).toEqual([
      "vps account_begin",
      "window account_wait https://qanuvi.com/auth?x",
      "vps account_finish st/the-code",
    ]);
    expect(account.account?.name).toBe("Test Owner");
    expect(account.waiting).toBe(false);
    switcher.shown = null;
    expect(account.account, "This PC has its own (none yet)").toBeNull();
    switcher.shown = "vps";
  });

  it("keeps a machine without a sign-in quiet, and shows other errors", async () => {
    answers["pc account_me"] = new Error("Not signed in.");
    await account.load(null);
    switcher.shown = null;
    expect(account.error).toBe("");
    answers["pc account_me"] = new Error("The site could not be reached: offline");
    await account.load(null);
    expect(account.error).toMatch(/could not be reached/);
    switcher.shown = "vps";
    expect(account.error).toBe("");
  });

  it("signs out on the machine it belongs to", async () => {
    await account.signOut("vps");
    expect(calls).toEqual(["vps account_sign_out"]);
    expect(account.accountOn("vps")).toBeNull();
  });
});
