import { describe, expect, it } from "vitest";
import { parseWorkspace } from "./workspace.svelte";

describe("parseWorkspace", () => {
  it("keeps tabs, cursor, view, terminals, session and preview", () => {
    const w = parseWorkspace(
      JSON.stringify({
        tabs: [
          { path: "src/cart.js", line: 12, column: 5 },
          { path: "README.md", line: 1, column: 1 },
        ],
        active: "src/cart.js",
        view: "diff",
        terminals: ["powershell", "cmd"],
        session: "s1",
        preview: "http://localhost/shop/",
      }),
    );
    expect(w.tabs[0]).toEqual({ path: "src/cart.js", line: 12, column: 5 });
    expect(w.active).toBe("src/cart.js");
    expect(w.view).toBe("diff");
    expect(w.terminals).toEqual(["powershell", "cmd"]);
    expect(w.session).toBe("s1");
    expect(w.preview).toBe("http://localhost/shop/");
  });

  it("drops what does not make sense", () => {
    const w = parseWorkspace(
      JSON.stringify({
        tabs: [{ path: "../secret", line: 3 }, { path: "a.js", line: -4, column: "x" }, { path: "a.js" }, { nope: 1 }],
        active: "gone.js",
        view: "servers",
        preview: "javascript:alert(1)",
      }),
    );
    expect(w.tabs).toEqual([{ path: "a.js", line: 1, column: 1 }]);
    expect(w.active).toBeNull();
    expect(w.view).toBeNull();
    expect(w.preview).toBeNull();
    expect(parseWorkspace("not json").tabs).toEqual([]);
    expect(parseWorkspace(null).view).toBeNull();
  });
});
