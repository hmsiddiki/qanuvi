/** File tree decorations from git status, as in VS Code: a letter per changed file and a dot on folders. */
import type { GitChange } from "$lib/api";

export type Letter = "M" | "A" | "U" | "D" | "R" | "!";

export interface Decorations {
  files: Map<string, Letter>;
  /** Folders that contain a change somewhere below them. */
  folders: Set<string>;
}

export function letterFor(change: Pick<GitChange, "status" | "untracked" | "deleted">): Letter {
  const s = change.status;
  if (s === "UU" || s === "AA" || s === "DD" || s.includes("U")) return change.untracked ? "U" : "!";
  if (change.untracked) return "U";
  if (change.deleted) return "D";
  if (s.startsWith("R")) return "R";
  if (s.startsWith("A")) return "A";
  return "M";
}

export function decorations(changes: Pick<GitChange, "path" | "status" | "untracked" | "deleted">[]): Decorations {
  const files = new Map<string, Letter>();
  const folders = new Set<string>();
  for (const c of changes) {
    const path = c.path.replace(/\/$/, "");
    files.set(path, letterFor(c));
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) folders.add(parts.slice(0, i).join("/"));
  }
  return { files, folders };
}
