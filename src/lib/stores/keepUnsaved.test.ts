import { describe, expect, it, vi } from "vitest";
import type { Draft } from "$lib/api";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.mock("./editor.svelte", () => ({ editor: { unsaved: () => [] } }));
vi.mock("./inbox.svelte", () => ({ inbox: { addNow: vi.fn() } }));
vi.mock("./projects.svelte", () => ({ projects: { list: [] } }));
vi.mock("./switcher.svelte", () => ({ switcher: { held: null } }));

const { keptNote } = await import("./keepUnsaved");
const { nameMachines } = await import("$lib/remote");
const { beforeWindowGoes, windowGoing } = await import("./closing");

const d = (path: string, projectId = "p1", meta = ""): Draft => ({ projectId, path, text: "x", hash: "h", meta });

describe("unsaved text kept when a window goes (i1)", () => {
  it("names the files and the machine they are kept on", () => {
    const note = keptNote([d("src/a.ts"), d("b.php")], null, () => "shop");
    expect(note?.title).toBe("Unsaved changes to 2 files kept as drafts on This PC");
    expect(note?.text).toContain("not saved to disk");
    expect(note?.text).toContain("- src/a.ts (shop)\n- b.php (shop)");
  });

  it("names an agent's configuration file by its label, counts the rest past eight, says nothing for none", () => {
    nameMachines((id) => (id === "vps" ? "VPS" : id));
    const many = [
      d("cfg", "", JSON.stringify({ label: "Agent settings" })),
      ...Array.from({ length: 9 }, (_, i) => d(`f${i}.js`)),
    ];
    const note = keptNote(many, "vps", () => "shop");
    expect(note?.title).toBe("Unsaved changes to 10 files kept as drafts on VPS");
    expect(note?.text).toContain("- Agent settings\n");
    expect(note?.text).toContain("- and 2 more");
    expect(keptNote([], null, () => "")).toBeNull();
  });

  it("runs every close step in turn, a failing one included, and never waits past the limit", async () => {
    const ran: string[] = [];
    beforeWindowGoes(async () => void ran.push("one"));
    beforeWindowGoes(async () => {
      throw new Error("no answer");
    });
    beforeWindowGoes(async () => void ran.push("three"));
    await windowGoing();
    expect(ran).toEqual(["one", "three"]);
    beforeWindowGoes(() => new Promise(() => {}));
    const t = Date.now();
    await windowGoing(50);
    expect(Date.now() - t).toBeLessThan(1000);
  });
});
