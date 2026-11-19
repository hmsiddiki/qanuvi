// What the terminal screen reads out of a shell's output and its mouse: links (z6) and copy requests (z7).

/** Links open on Ctrl+click (Cmd+click on a Mac) with the left button, as in VS Code: a selection or a right-click
 * that ends over a link never opens it. */
export function opensLink(e: Pick<MouseEvent, "button" | "ctrlKey" | "metaKey">): boolean {
  return e.button === 0 && (e.ctrlKey || e.metaKey);
}

export interface FoundUrl {
  /** 0-based column of the first character. */
  start: number;
  /** 0-based column just after the last character. */
  end: number;
  url: string;
}

/** http(s) addresses in one line of the screen, without the punctuation that usually follows them in a sentence. */
export function urlsIn(text: string): FoundUrl[] {
  const found: FoundUrl[] = [];
  for (const m of text.matchAll(/https?:\/\/[^\s"'<>`]+/g)) {
    let url = m[0];
    while (/[.,;:!?)\]}]$/.test(url)) {
      // A closing bracket that belongs to the address (wiki-style links) stays.
      const close = url.at(-1)!;
      const open = { ")": "(", "]": "[", "}": "{" }[close];
      if (open && url.split(open).length > url.split(close).length - 1) break;
      url = url.slice(0, -1);
    }
    if (url.length > "https://".length) found.push({ start: m.index, end: m.index + url.length, url });
  }
  return found;
}

/** The most text one copy request may put on the clipboard. */
const COPY_MAX = 1_000_000;

/**
 * The text of a shell's "copy this" request (OSC 52: `<targets>;<base64>`), or null. A request to read the clipboard
 * (`?`) is never answered, so a program in the terminal cannot see what the owner copied.
 */
export function osc52Copy(data: string): string | null {
  const semi = data.indexOf(";");
  if (semi < 0) return null;
  const payload = data.slice(semi + 1);
  if (payload === "?" || payload.length === 0 || payload.length > (COPY_MAX * 4) / 3 + 4) return null;
  try {
    const bytes = Uint8Array.from(atob(payload), (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

/** What a paste types (z5): one line copied with its line break comes without it, so it waits on the prompt for an
 * edit and Enter; text of several lines stays as it is. */
export function pasteText(text: string): string {
  const trimmed = text.replace(/(\r?\n)+$/, "");
  return /[\r\n]/.test(trimmed) ? text : trimmed;
}

/**
 * What the screen sends before its terminal is known (v3): typing, a paste or the screen's own answers while the
 * shell is still being started (a slow link, a busy cold start). Held in order and sent as one once the id comes,
 * never dropped; once the id is known everything goes straight on.
 */
export class EarlyInput {
  private held: string[] = [];

  /** Holds `data` while there is no id; answers whether it was held. */
  hold(id: string | null, data: string): boolean {
    if (id) return false;
    this.held.push(data);
    return true;
  }

  /** Everything held, in order (empty when nothing was), and nothing held after. */
  take(): string {
    const all = this.held.join("");
    this.held = [];
    return all;
  }
}
