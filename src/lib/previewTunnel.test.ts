import { describe, expect, it } from "vitest";
import { loopbackPort, throughTunnel } from "./previewTunnel";

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
});
