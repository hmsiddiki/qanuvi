import { describe, expect, it } from "vitest";
import { pasteMayHoldPicture, pastedAttachments, pastedName, pngFile } from "./paste";

const png = (name = "image.png", size = 8) => new File([new Uint8Array(size).fill(7)], name, { type: "image/png" });

describe("pasted files", () => {
  it("names a screenshot by number and keeps a real file name", () => {
    expect(pastedName(png(), 1)).toBe("Pasted image 1.png");
    expect(pastedName(png(""), 2)).toBe("Pasted image 2.png");
    expect(pastedName(png("diagram.png"), 3)).toBe("diagram.png");
  });

  it("turns an image into an attachment the agent sees", async () => {
    const r = await pastedAttachments([png()], 1);
    expect(r.handled).toBe(true);
    expect(r.errors).toEqual([]);
    expect(r.attachments).toHaveLength(1);
    const a = r.attachments[0];
    expect(a).toMatchObject({ name: "Pasted image 2.png", kind: "image", mediaType: "image/png" });
    expect(atob(a.data)).toBe("\x07".repeat(8));
  });

  it("reads text files, refuses what cannot be sent", async () => {
    const text = new File(["hello"], "notes.txt", { type: "text/plain" });
    const bin = new File([new Uint8Array([0xff, 0xfe, 0x00, 0x81])], "a.bin");
    const bmp = new File([new Uint8Array(4)], "a.bmp", { type: "image/bmp" });
    const big = png("big.png", 5 * 1024 * 1024 + 1);
    const r = await pastedAttachments([text, bin, bmp, big]);
    expect(r.attachments).toEqual([{ name: "notes.txt", kind: "text", mediaType: "", data: "hello" }]);
    expect(r.errors).toHaveLength(3);
    expect(r.errors.join(" ")).toMatch(/not text.*PNG, JPEG, GIF and WebP.*over 5 MB/s);
  });

  it("leaves a text-only paste to the box", async () => {
    expect((await pastedAttachments([])).handled).toBe(false);
  });
});

describe("a paste that carried no file (lx1, Linux)", () => {
  /** Enough of a DataTransfer for the paste checks. */
  const transfer = (text: string, files: File[] = []) =>
    ({
      items: files.map((f) => ({ kind: "file", getAsFile: () => f })),
      files,
      getData: (t: string) => (t === "text/plain" ? text : ""),
    }) as unknown as DataTransfer;

  it("reads the clipboard's picture only when neither a file nor text came", () => {
    expect(pasteMayHoldPicture(transfer(""))).toBe(true);
    expect(pasteMayHoldPicture(null)).toBe(true);
    expect(pasteMayHoldPicture(transfer("some text"))).toBe(false);
    expect(pasteMayHoldPicture(transfer("", [png()]))).toBe(false);
  });

  it("the picture read becomes a pasted screenshot like any other", async () => {
    const file = pngFile(btoa("\x89PNG-bytes"));
    expect(file.type).toBe("image/png");
    const r = await pastedAttachments([file], 0);
    expect(r.attachments[0]).toMatchObject({ name: "Pasted image 1.png", kind: "image", mediaType: "image/png" });
    expect(atob(r.attachments[0].data)).toBe("\x89PNG-bytes");
  });
});
