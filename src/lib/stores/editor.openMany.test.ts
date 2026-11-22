import { describe, expect, it, vi } from "vitest";

// sl1 S2: a project's saved tabs are read together (one round trip's wait over a relay, not one per file), and still
// come in the order they were saved, whatever order the reads finish in.
vi.mock("@tauri-apps/api/core", () => ({ Channel: class {}, invoke: vi.fn() }));
vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {}, clear: () => {} });
vi.mock("./settings.svelte", () => ({ settings: { values: {} } }));
vi.mock("./lsp.svelte", () => ({ lsp: { saved: vi.fn(), closed: vi.fn(), opened: vi.fn() } }));
vi.mock("$lib/stores/confirm.svelte", () => ({ ask: vi.fn(async () => true) }));

const reads: { path: string; done: (ok: boolean) => void }[] = [];
vi.mock("$lib/api", async (original) => {
  const real = await original<typeof import("$lib/api")>();
  return {
    ...real,
    api: {
      ...real.api,
      files: {
        ...real.api.files,
        read: (_projectId: string, path: string) =>
          new Promise((resolve, reject) =>
            reads.push({
              path,
              done: (ok) =>
                ok
                  ? resolve({ path, hash: `h-${path}`, text: `text of ${path}`, binary: false, too_large: false })
                  : reject(new Error("The file is gone.")),
            }),
          ),
      },
    },
  };
});

const { editor, tabKey } = await import("./editor.svelte");

describe("openMany (sl1 S2)", () => {
  it("reads every file at once, adds them in the given order, keeps open ones, and marks a gone one", async () => {
    const first = editor.openMany("p1", [{ path: "a.ts" }], 0);
    await Promise.resolve();
    reads.shift()?.done(true);
    await first;
    const opening = editor.openMany(
      "p1",
      [{ path: "b.ts", line: 3 }, { path: "a.ts" }, { path: "c.ts" }, { path: "d.ts" }],
      0,
    );
    // All three new files were asked for before any answered; the open one was not read again.
    await Promise.resolve();
    expect(reads.map((r) => r.path)).toEqual(["b.ts", "c.ts", "d.ts"]);
    // They finish out of order, one of them gone.
    reads[2].done(true);
    reads[1].done(false);
    reads[0].done(true);
    await opening;
    expect(editor.tabs.map((t) => t.path)).toEqual(["a.ts", "b.ts", "c.ts", "d.ts"]);
    const b = editor.tabs.find((t) => t.key === tabKey("p1", "b.ts"));
    expect(b?.savedText).toBe("text of b.ts");
    expect(b?.revealLine).toBe(3);
    expect(editor.tabs.find((t) => t.path === "c.ts")?.error).toContain("gone");
    // The last one given is the active one, as open() one after another left it.
    expect(editor.active?.path).toBe("d.ts");
  });
});
