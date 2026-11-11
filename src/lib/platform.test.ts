import { describe, expect, it } from "vitest";
import { platformLabel } from "./platform";

describe("platformLabel", () => {
  it("names the desktop and mobile systems", () => {
    expect(platformLabel({ os: "windows", arch: "x86_64" })).toBe("Windows x86_64");
    expect(platformLabel({ os: "linux", arch: "aarch64" })).toBe("Linux aarch64");
    expect(platformLabel({ os: "macos", arch: "aarch64" })).toBe("macOS aarch64");
    expect(platformLabel({ os: "android", arch: "aarch64" })).toBe("Android aarch64");
    expect(platformLabel({ os: "ios", arch: "aarch64" })).toBe("iOS aarch64");
  });

  it("shows an unknown system as reported and leaves out an empty architecture", () => {
    expect(platformLabel({ os: "freebsd", arch: "x86_64" })).toBe("freebsd x86_64");
    expect(platformLabel({ os: "linux", arch: "" })).toBe("Linux");
  });
});
