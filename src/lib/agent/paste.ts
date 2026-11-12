/**
 * Images (and text files) pasted into the message box with Ctrl+V become attachments, like the + menu's: a
 * screenshot, an image copied from a browser or an image editor, or files copied in the file manager. Same limits as
 * attachments from disk (images up to 5 MB, text up to 200 KB).
 */
import type { Attachment } from "$lib/api";

const IMAGE_TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
};
const MAX_IMAGE = 5 * 1024 * 1024;
const MAX_TEXT = 200 * 1024;

export interface Pasted {
  attachments: Attachment[];
  errors: string[];
  /** Files were on the clipboard: the paste is handled here (no file name pasted as text). */
  handled: boolean;
}

/** The files on a clipboard (or a drop), without doubles: some sources put the same file in both lists. */
export function clipboardFiles(data: DataTransfer | null): File[] {
  if (!data) return [];
  const out: File[] = [];
  for (const item of Array.from(data.items ?? [])) {
    if (item.kind !== "file") continue;
    const f = item.getAsFile();
    if (f) out.push(f);
  }
  if (!out.length) out.push(...Array.from(data.files ?? []));
  return out;
}

/** A name for a pasted file: its own, unless it is the clipboard's generic "image.png". */
export function pastedName(file: File, n: number): string {
  const ext = IMAGE_TYPES[file.type];
  if (ext && (!file.name || /^image\.\w+$/i.test(file.name))) return `Pasted image ${n}.${ext}`;
  return file.name || `Pasted file ${n}`;
}

async function base64(file: Blob): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let bin = "";
  // In slices: one call with megabytes of arguments overflows the stack.
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}

/** Turns pasted files into attachments. `already`: attachments the message has, to number the new images. */
export async function pastedAttachments(files: File[], already = 0): Promise<Pasted> {
  const attachments: Attachment[] = [];
  const errors: string[] = [];
  let n = already;
  for (const file of files) {
    n += 1;
    const name = pastedName(file, n);
    if (IMAGE_TYPES[file.type]) {
      if (file.size > MAX_IMAGE) {
        errors.push(`${name} is over 5 MB; paste a smaller image.`);
        continue;
      }
      attachments.push({ name, kind: "image", mediaType: file.type, data: await base64(file) });
      continue;
    }
    if (file.type.startsWith("image/")) {
      errors.push(`${name}: only PNG, JPEG, GIF and WebP images can be attached.`);
      continue;
    }
    if (file.size > MAX_TEXT) {
      errors.push(`${name} is over 200 KB; attach a smaller part, or mention it from the project.`);
      continue;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
      attachments.push({ name, kind: "text", mediaType: "", data: text });
    } catch {
      errors.push(`${name} is not text or an image (PNG, JPEG, GIF, WebP); it cannot be attached yet.`);
    }
  }
  return { attachments, errors, handled: files.length > 0 };
}
