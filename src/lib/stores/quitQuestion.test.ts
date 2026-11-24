import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({ getCurrentWebviewWindow: vi.fn() }));
vi.mock("$lib/stores/confirm.svelte", () => ({ choose: vi.fn() }));

import { quitAnswer, quitButtons } from "./quitQuestion";

describe("the Quit question (s10)", () => {
  it("offers Go on next time as the main button only with plan runs going", () => {
    const later = quitButtons({ id: 1, text: "", later: true });
    expect(later.okLabel).toBe("Go on next time");
    expect(later.otherLabel).toBe("Quit");
    expect(later.cancelLabel).toBe("Keep running");
    const plain = quitButtons({ id: 2, text: "" });
    expect(plain.okLabel).toBe("Quit");
    expect(plain.otherLabel).toBe("");
  });

  it("answers the master by the button pressed", () => {
    const q = { id: 1, text: "", later: true };
    expect(quitAnswer(q, true)).toEqual({ yes: true, later: true });
    expect(quitAnswer(q, "other")).toEqual({ yes: true, later: false });
    expect(quitAnswer(q, false)).toEqual({ yes: false, later: false });
    const plain = { id: 2, text: "" };
    expect(quitAnswer(plain, true)).toEqual({ yes: true, later: false });
    expect(quitAnswer(plain, false)).toEqual({ yes: false, later: false });
  });
});
