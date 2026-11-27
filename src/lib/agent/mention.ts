/**
 * @ mentions in the message box: typing "@" (at the start or after a space) opens a picker of the project's files and
 * symbols; the pick replaces the "@query" with a mention the agent reads.
 */

/** The "@query" the caret is in, if any: where it starts and what is typed after the "@". */
export function mentionAt(text: string, caret: number): { start: number; query: string } | null {
  const before = text.slice(0, caret);
  const m = /(^|\s)@([^\s@]*)$/.exec(before);
  if (!m) return null;
  return { start: before.length - m[2].length - 1, query: m[2] };
}

/** The text with the "@query" at `start` (up to the caret) replaced by the mention, and where the caret goes. */
export function insertMention(
  text: string,
  start: number,
  caret: number,
  mention: string,
): { text: string; caret: number } {
  const after = text.slice(caret).replace(/^\S*/, "");
  const inserted = `${mention} `;
  return {
    text: text.slice(0, start) + inserted + after.replace(/^ /, ""),
    caret: start + inserted.length,
  };
}

/** A mention of a file: "@path". */
export function fileMention(path: string): string {
  return `@${path}`;
}

/** A mention of a symbol: the file with the name and line, so the agent goes straight there. */
export function symbolMention(path: string, name: string, line: number): string {
  return `@${path} (${name}, line ${line})`;
}

/** Whether a dropped file is inside the project; its path relative to the project when it is. */
export function inProject(root: string, path: string): string | null {
  const r = root.replace(/\\/g, "/").replace(/\/$/, "");
  const p = path.replace(/\\/g, "/");
  return p.toLowerCase().startsWith(`${r.toLowerCase()}/`) ? p.slice(r.length + 1) : null;
}

/**
 * r1 (Mac drive): a file an agent's step names, relative to the project. Claude Code names full paths; Qanuvi's own
 * agent names paths relative to the project ("hello.js"), which inProject refused, so Revert said "outside the
 * project". A relative path counts when it does not climb out of the project.
 */
export function agentFileInProject(root: string, path: string): string | null {
  const p = path.replace(/\\/g, "/");
  const absolute = p.startsWith("/") || /^[a-z]:\//i.test(p);
  if (absolute) return inProject(root, p);
  const rel = p.replace(/^(\.\/)+/, "");
  if (!rel || rel.split("/").includes("..")) return null;
  return rel;
}

/** Image files by name (dropped images become attachments the agent sees). */
export function isImagePath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp)$/i.test(path);
}
