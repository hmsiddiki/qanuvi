import { describe, expect, it } from "vitest";
import { folderHints, shownName } from "./projectNames";

const p = (id: string, name: string, path: string) => ({ id, name, path });

describe("projects with the same name", () => {
  it("shows the folder that tells them apart", () => {
    const list = [
      p("a", "qanuvi", "/home/u/qc-scratch/y9/qanuvi"),
      p("b", "qanuvi", "/home/u/qc-scratch/y10/qanuvi"),
      p("c", "site", "/home/u/site"),
    ];
    const hints = folderHints(list);
    expect(shownName(list[0], hints)).toBe("qanuvi (y9)");
    expect(shownName(list[1], hints)).toBe("qanuvi (y10)");
    expect(shownName(list[2], hints)).toBe("site");
  });

  it("goes up the path until the folders differ", () => {
    const list = [p("a", "app", "E:\\work\\one\\src\\app"), p("b", "app", "E:\\work\\two\\src\\app")];
    const hints = folderHints(list);
    expect(hints.get("a")).toBe("one/src");
    expect(hints.get("b")).toBe("two/src");
  });

  it("matches names whatever their case", () => {
    const hints = folderHints([p("a", "Shop", "/x/a/Shop"), p("b", "shop", "/x/b/shop")]);
    expect(hints.get("a")).toBe("a");
    expect(hints.get("b")).toBe("b");
  });
});
