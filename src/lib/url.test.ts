import { describe, expect, it } from "vitest";
import { normalizeUrl } from "./url";

describe("normalizeUrl", () => {
  it("keeps full addresses", () => {
    expect(normalizeUrl("http://localhost/amirularham/")).toBe("http://localhost/amirularham/");
    expect(normalizeUrl("https://example.com/a?b=1")).toBe("https://example.com/a?b=1");
  });

  it("adds http for local addresses and https for the rest", () => {
    expect(normalizeUrl("localhost:8000")).toBe("http://localhost:8000/");
    expect(normalizeUrl("127.0.0.1/site")).toBe("http://127.0.0.1/site");
    expect(normalizeUrl("shop.test")).toBe("http://shop.test/");
    expect(normalizeUrl("example.com")).toBe("https://example.com/");
  });

  it("refuses anything that is not a web address", () => {
    expect(normalizeUrl("")).toBe("");
    expect(normalizeUrl("javascript:alert(1)")).toBe("");
    expect(normalizeUrl("file:///C:/Windows/win.ini")).toBe("");
    expect(normalizeUrl("ftp://host/file")).toBe("");
  });
});
