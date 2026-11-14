import { describe, expect, it } from "vitest";
import {
  REOPEN_FRESH_FROM,
  forgetNote,
  keepNote,
  keptNote,
  reopenFresh,
  CHAT_HANDOVER_MARK,
  chainOf,
  chatHandoverDue,
  chatHandoverRequest,
  chatResumePrompt,
  chatGroups,
  chatsOnly,
  lastRequest,
  noteAfter,
  parseResume,
} from "./handover";
import { reduceAll, type Item } from "./events";

const ask = { when: "ask" as const, limit: 150_000 };

describe("chatHandoverDue", () => {
  it("is due past the limit unless off, for the chat or everywhere", () => {
    expect(chatHandoverDue(149_000, ask, false, 0)).toBe(false);
    expect(chatHandoverDue(152_000, ask, false, 0)).toBe(true);
    expect(chatHandoverDue(152_000, { ...ask, when: "off" }, false, 0)).toBe(false);
    expect(chatHandoverDue(152_000, { ...ask, limit: 0 }, false, 0)).toBe(false);
    expect(chatHandoverDue(152_000, ask, true, 0)).toBe(false);
  });

  it("holds back after Not now until the context grew 50k more", () => {
    expect(chatHandoverDue(180_000, ask, false, 152_000)).toBe(false);
    expect(chatHandoverDue(202_000, ask, false, 152_000)).toBe(true);
  });
});

describe("request and resume", () => {
  it("asks for a note and is shown as a line, not as the user's words", () => {
    const req = chatHandoverRequest(152_400);
    expect(req.startsWith(CHAT_HANDOVER_MARK)).toBe(true);
    expect(req).toContain("152k");
    const items = reduceAll([JSON.stringify({ type: "qanuvi_user", text: req })]);
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("notice");
  });

  it("reads the note and size back from the new session's first message", () => {
    const prompt = chatResumePrompt("Handover note\nGoal: totals.", "Fix the totals", 152_000);
    expect(parseResume(prompt)).toEqual({ note: "Handover note\nGoal: totals.", was: 152_000, next: "" });
    expect(parseResume("Fix the totals")).toBeNull();
    const [item] = reduceAll([JSON.stringify({ type: "qanuvi_user", text: prompt })]);
    expect(item).toMatchObject({ kind: "notice", note: "Handover note\nGoal: totals." });
    expect(item.kind === "notice" && item.text).toContain("was 152k");
  });

  it("puts a reopened chat's new message in the first turn and shows it as the user's", () => {
    const prompt =
      chatResumePrompt("Handover note\nGoal: totals.", "", 76_000, "What is the test word?") +
      "\n\n(Private details: kept out of sight.)";
    expect(prompt).toContain("do only what this message asks");
    expect(prompt).not.toContain("go on with what is left");
    expect(parseResume(prompt)).toEqual({
      note: "Handover note\nGoal: totals.",
      was: 76_000,
      next: "What is the test word?",
    });
    const items = reduceAll([JSON.stringify({ type: "qanuvi_user", text: prompt })]);
    expect(items.map((i) => i.kind)).toEqual(["notice", "user"]);
    expect(items[1]).toMatchObject({ kind: "user", text: "What is the test word?" });
  });

  it("works without a last request", () => {
    expect(parseResume(chatResumePrompt("Handover note\nAll done here.", "", 160_000))?.note).toBe(
      "Handover note\nAll done here.",
    );
  });
});

describe("noteAfter and lastRequest", () => {
  const items: Item[] = [
    { kind: "user", id: "u1", text: "Fix the totals" },
    { kind: "text", id: "t1", text: "Working on it." },
    { kind: "notice", id: "ho3", text: "Handing over", tone: "info" },
    { kind: "text", id: "t2", text: "Edit finished.\n\nHandover note\nGoal: totals. Done: cart.php. Left: tests." },
  ];

  it("takes the agent's note after the request, from its first line", () => {
    expect(noteAfter(items, 2, () => "fallback")).toBe("Handover note\nGoal: totals. Done: cart.php. Left: tests.");
  });

  it("falls back when the agent gave no note", () => {
    expect(noteAfter(items.slice(0, 3), 2, () => "fallback")).toBe("fallback");
  });

  it("finds the user's last request before the handover", () => {
    expect(lastRequest(items, 2)).toBe("Fix the totals");
    expect(lastRequest([], 0)).toBe("");
  });
});

describe("chains", () => {
  const s = [
    { id: "c", continues_from: "b" },
    { id: "b", continues_from: "a" },
    { id: "a", continues_from: null },
    { id: "x" },
  ];

  it("walks a chat back to its first session, oldest first", () => {
    expect(chainOf(s, "c").map((x) => x.id)).toEqual(["a", "b", "c"]);
    expect(chainOf(s, "x").map((x) => x.id)).toEqual(["x"]);
    expect(chainOf(s, "missing")).toEqual([]);
  });

  it("stops at a loop", () => {
    const loop = [
      { id: "a", continues_from: "b" },
      { id: "b", continues_from: "a" },
    ];
    expect(chainOf(loop, "a").map((x) => x.id)).toEqual(["b", "a"]);
  });

  it("lists only the newest session of each chat", () => {
    expect(chatsOnly(s).map((x) => x.id)).toEqual(["c", "x"]);
  });
});

describe("chatGroups", () => {
  const chats = [
    { title: "Fix the login form", saved: false },
    { title: "Cart totals", saved: true },
    { title: "Login page styles", saved: true },
    { title: "Readme" },
  ];
  const titles = (g: { title: string }[]) => g.map((c) => c.title);

  it("puts starred chats under Saved, keeping the order", () => {
    const g = chatGroups(chats);
    expect(titles(g.saved)).toEqual(["Cart totals", "Login page styles"]);
    expect(titles(g.rest)).toEqual(["Fix the login form", "Readme"]);
  });

  it("filters both groups by every word of the title, any case", () => {
    const g = chatGroups(chats, "  LOGIN ");
    expect(titles(g.saved)).toEqual(["Login page styles"]);
    expect(titles(g.rest)).toEqual(["Fix the login form"]);
    expect(titles(chatGroups(chats, "login form").rest)).toEqual(["Fix the login form"]);
    expect(chatGroups(chats, "nothing").saved).toEqual([]);
  });
});

describe("reopening a starred chat (24s part 3)", () => {
  it("goes on fresh only for a heavy starred chat that is not live and not a plan step", () => {
    const heavy = REOPEN_FRESH_FROM;
    expect(reopenFresh({ saved: true, status: "ended" }, heavy, false, "ask")).toBe(true);
    expect(reopenFresh({ saved: true, status: "ended" }, heavy, false, "auto")).toBe(true);
    expect(reopenFresh({ saved: true, status: "ended" }, heavy, false, "off")).toBe(false);
    expect(reopenFresh({ saved: true, status: "ended", handover_off: true }, heavy, false, "auto")).toBe(false);
    expect(reopenFresh({ saved: true, status: "stopped" }, heavy - 1, false, "ask")).toBe(false);
    expect(reopenFresh({ saved: false, status: "ended" }, heavy, false, "ask")).toBe(false);
    expect(reopenFresh({ saved: true, status: "idle" }, heavy, false, "ask")).toBe(false);
    expect(reopenFresh({ saved: true, status: "ended" }, heavy, true, "ask")).toBe(false);
  });

  it("keeps a written note only while the session has no newer lines", () => {
    const m = new Map<string, string>();
    const store = {
      getItem: (k: string) => m.get(k) ?? null,
      setItem: (k: string, v: string) => void m.set(k, v),
      removeItem: (k: string) => void m.delete(k),
    };
    expect(keptNote(store, "s1", 10)).toBeNull();
    keepNote(store, "s1", 10, "Handover note\nGoal: x");
    expect(keptNote(store, "s1", 10)).toBe("Handover note\nGoal: x");
    expect(keptNote(store, "s1", 11)).toBeNull();
    keepNote(store, "s1", 10, "  ");
    expect(keptNote(store, "s1", 10)).toBeNull();
    m.set("qanuvi.reopenNote.s1", "{broken");
    expect(keptNote(store, "s1", 10)).toBeNull();
    forgetNote(store, "s1");
    expect(m.size).toBe(0);
  });
});
