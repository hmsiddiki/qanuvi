import { describe, expect, it } from "vitest";
import { decorations, letterFor } from "./decorations";

const c = (path: string, status: string) => ({
  path,
  status,
  untracked: status === "??",
  deleted: status.includes("D") && status !== "DD",
});

describe("letterFor", () => {
  it("uses the letters VS Code shows", () => {
    expect(letterFor(c("a", " M"))).toBe("M");
    expect(letterFor(c("a", "M "))).toBe("M");
    expect(letterFor(c("a", "??"))).toBe("U");
    expect(letterFor(c("a", "A "))).toBe("A");
    expect(letterFor(c("a", " D"))).toBe("D");
    expect(letterFor(c("a", "R "))).toBe("R");
    expect(letterFor(c("a", "UU"))).toBe("!");
  });
});

describe("decorations", () => {
  it("marks files and every folder above a change", () => {
    const d = decorations([c("admin/inc/header.php", " M"), c("uploads/x.png", "??"), c("index.php", " M")]);
    expect(d.files.get("admin/inc/header.php")).toBe("M");
    expect(d.files.get("uploads/x.png")).toBe("U");
    expect([...d.folders].sort()).toEqual(["admin", "admin/inc", "uploads"]);
    expect(d.folders.has("index.php")).toBe(false);
  });
});
