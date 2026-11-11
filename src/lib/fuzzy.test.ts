import { describe, expect, it } from "vitest";
import { fuzzyFilter, fuzzyScore } from "./fuzzy";

const files = [
  "src/Cart/CartService.php",
  "src/Cart/CartProductResolver.php",
  "src/Option/SelectionStore.php",
  "templates/cart.php",
  "DEVELOPMENT-TASKLIST.md",
  "docs/OPTIONS.md",
];

describe("fuzzyScore", () => {
  it("needs the letters in order", () => {
    expect(fuzzyScore("cps", "src/Cart/CartProductResolver.php")).not.toBeNull();
    expect(fuzzyScore("zzz", "src/Cart/CartService.php")).toBeNull();
  });
});

describe("fuzzyFilter", () => {
  it("puts the best file-name match first", () => {
    expect(fuzzyFilter("cartservice", files)[0]).toBe("src/Cart/CartService.php");
    expect(fuzzyFilter("cart.php", files)[0]).toBe("templates/cart.php");
    expect(fuzzyFilter("tasklist", files)[0]).toBe("DEVELOPMENT-TASKLIST.md");
  });

  it("returns everything for an empty query, up to the limit", () => {
    expect(fuzzyFilter("", files, 3)).toHaveLength(3);
  });
});
