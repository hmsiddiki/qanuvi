import { describe, expect, it } from "vitest";
import { loopbackPort, machineText, throughTunnel } from "./previewTunnel";

describe("preview tunnel (migration M1c)", () => {
  it("finds the port a loopback address points at", () => {
    expect(loopbackPort("http://localhost:5173/")).toBe(5173);
    expect(loopbackPort("http://localhost/my-site/")).toBe(80);
    expect(loopbackPort("https://localhost/")).toBe(443);
    expect(loopbackPort("http://127.0.0.1:8000/a")).toBe(8000);
    expect(loopbackPort("http://[::1]:3000/")).toBe(3000);
    expect(loopbackPort("http://shop.localhost:8080/")).toBe(8080);
    expect(loopbackPort("http://LOCALHOST:81/")).toBe(81);
  });

  it("leaves public and other addresses alone", () => {
    expect(loopbackPort("https://example.com/")).toBeNull();
    expect(loopbackPort("http://site.test/")).toBeNull();
    expect(loopbackPort("http://192.168.1.5:3000/")).toBeNull();
    expect(loopbackPort("http://localhost.example.com/")).toBeNull();
    expect(loopbackPort("file:///C:/a.html")).toBeNull();
    expect(loopbackPort("not an address")).toBeNull();
  });

  it("swaps in the tunnel's port and keeps the path, the query and site names", () => {
    expect(throughTunnel("http://localhost:5173/a/b?x=1#top", 53001)).toBe("http://localhost:53001/a/b?x=1#top");
    expect(throughTunnel("http://localhost/my-site/", 53001)).toBe("http://localhost:53001/my-site/");
    expect(throughTunnel("https://shop.localhost/", 53002)).toBe("https://shop.localhost:53002/");
  });

  it("uses 127.0.0.1 for number addresses, where the tunnel listens", () => {
    expect(throughTunnel("http://[::1]:3000/", 53003)).toBe("http://127.0.0.1:53003/");
    expect(throughTunnel("http://127.0.0.2:3000/", 53003)).toBe("http://127.0.0.1:53003/");
  });

  it("puts the master's address back into text for the agent", () => {
    const tunnelled = "http://localhost:33211/";
    const shown = "http://localhost:5179/";
    expect(machineText("Page: M1c - http://localhost:33211/ (HTTP 200)", tunnelled, shown)).toBe(
      "Page: M1c - http://localhost:5179/ (HTTP 200)",
    );
    expect(machineText("Element on http://localhost:33211/a?b=1 (x): #t", "http://localhost:33211/a", shown)).toBe(
      "Element on http://localhost:5179/a?b=1 (x): #t",
    );
    // A longer port that only starts with the tunnel's digits is left alone.
    expect(machineText("see http://localhost:332110/", tunnelled, shown)).toBe("see http://localhost:332110/");
    // Number addresses became 127.0.0.1 in the tunnel; the bar's own form comes back.
    expect(machineText("at http://127.0.0.1:40001/x", "http://127.0.0.1:40001/x", "http://[::1]:3000/x")).toBe(
      "at http://[::1]:3000/x",
    );
    // Default port: the bar's address has none.
    expect(machineText("at http://localhost:40002/", "http://localhost:40002/", "http://localhost/")).toBe(
      "at http://localhost/",
    );
  });

  it("leaves text alone when nothing was tunnelled", () => {
    expect(machineText("Page: http://localhost:5179/", "http://localhost:5179/", "http://localhost:5179/")).toBe(
      "Page: http://localhost:5179/",
    );
    expect(machineText("Page: x", "", "http://localhost:5179/")).toBe("Page: x");
  });
});
