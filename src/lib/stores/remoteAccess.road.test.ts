import { describe, expect, it } from "vitest";
import { connectedText, freeRelayAsk, sizeText, usageText } from "./remoteAccess.svelte";

describe("relay road (24rl R3)", () => {
  it("says which road the link took, and nothing extra for older masters", () => {
    expect(connectedText({ road: "own port" })).toBe("Connected to the relay (its own port)");
    expect(connectedText({ road: "443" })).toBe("Connected to the relay (through port 443)");
    expect(connectedText({})).toBe("Connected to the relay");
    // fb2: an address with its own port, or plain ws://, was said to go "through port 443".
    expect(connectedText({ road: "443", relay: "wss://relay.qanuvi.com" })).toBe("Connected to the relay (through port 443)");
    expect(connectedText({ road: "443", relay: "wss://relay.qanuvi.com:47443" })).toBe("Connected to the relay");
    expect(connectedText({ road: "443", relay: "ws://127.0.0.1:47391" })).toBe("Connected to the relay");
    expect(connectedText({ road: "443", relay: "wss://relay.example.com:443/" })).toBe("Connected to the relay (through port 443)");
    // Relay kit K2: a pinned address (IP, own port) is read without its #pin= part.
    const pinned = `wss://203.0.113.5:47443#pin=${"a".repeat(43)}`;
    expect(connectedText({ road: "443", relay: pinned })).toBe("Connected to the relay");
  });
});

describe("daily share of the relay (24rl F4)", () => {
  it("says nothing without a cap, the use under it, and when slowing ends over it", () => {
    expect(usageText(null).text).toBe("");
    expect(usageText({ used: 5 << 20, cap: 0, slowedUntil: 0 }).text).toBe("");
    expect(usageText({ used: 120 << 20, cap: 2 * 2 ** 30, slowedUntil: 0 })).toEqual({
      text: "Today through the relay: 120 MB of 2 GB",
      over: false,
    });
    const over = usageText({ used: 3 * 2 ** 30, cap: 2 * 2 ** 30, slowedUntil: 2_000_000_000 }, 1_999_990_000);
    expect(over.over).toBe(true);
    expect(over.text).toMatch(/^Slowed until .+: today's share of this relay \(2 GB\) is used\./);
    // A slow time that has passed is not shown as slowed (the next day started).
    expect(usageText({ used: 3 * 2 ** 30, cap: 2 * 2 ** 30, slowedUntil: 100 }, 200).over).toBe(false);
  });
  it("writes sizes as people read them", () => {
    expect(sizeText(512)).toBe("512 B");
    expect(sizeText(1536)).toBe("1.5 KB");
    expect(sizeText(120 << 20)).toBe("120 MB");
    expect(sizeText(2 * 2 ** 30)).toBe("2 GB");
  });
});

describe("free relays (24rl F2)", () => {
  it("asks with who runs it and what it can see", () => {
    const ask = freeRelayAsk({ name: "Example", runBy: "Test volunteer" });
    expect(ask).toMatch(/^Example \(run by Test volunteer\) will carry this connection\./);
    expect(ask).toMatch(/cannot read anything/);
    expect(ask).toMatch(/pair again here\. Use it\?$/);
    expect(freeRelayAsk({ name: "Example", runBy: "" })).toMatch(/^Example will carry/);
  });
});
