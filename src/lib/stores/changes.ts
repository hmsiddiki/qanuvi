/**
 * What a piece of work changed in a project (24b/24e), from Qanuvi's own snapshots in the core: with or without git,
 * in folders the project's git ignores and in repositories nested in the project. Git is the user's choice per
 * project (Project settings: "Plan step changes from"); files git cannot see still come from the snapshots.
 */
import { api, errorText, type Project } from "$lib/api";

/** A snapshot taken before a piece of work, and the folders it follows whatever ignore rules say. */
export interface Snap {
  id: string;
  also: string[];
  /** Of `also`, the folders no git repository sees. */
  gitBlind?: string[];
  files?: number;
  truncated?: boolean;
}

/** What changed since a snapshot. `error`: the changes could not be read (the caller must not take it as "none"). */
export interface Changed {
  parts: string[];
  files: string[];
  /** With git chosen: changed files git cannot see. */
  unseen: string[];
  error?: string;
}

export function wantsGit(project: Pick<Project, "settings">): boolean {
  return project.settings.changes_from === "git";
}

/** The folder of a project-relative file ("" for one at the top). */
export function folderOf(path: string): string {
  const i = path.lastIndexOf("/");
  return i > 0 ? path.slice(0, i) : "";
}

/** Records the project's files now; `also`: folders followed even where ignore rules leave them out. */
export async function snapshotChanges(projectId: string, also: string[] = []): Promise<Snap | null> {
  const keep = also.filter(Boolean);
  const t = await api.files.snapshot(projectId, keep).catch(() => null);
  return t ? { id: t.id, also: keep, gitBlind: t.git_blind ?? [], files: t.files, truncated: t.truncated } : null;
}

/** The diffs of what changed since `before` (files `skip` names left out). */
export async function changesSince(
  project: Pick<Project, "id" | "settings">,
  before: Snap | null,
  skip: (path: string) => boolean = () => false,
): Promise<Changed> {
  if (!before) return { parts: [], files: [], unseen: [], error: "no snapshot was taken before this work" };
  try {
    const c = await api.files.changesSince(project.id, before.id, before.also, wantsGit(project));
    const kept = c.changes.filter((x) => !skip(x.path));
    return {
      parts: kept.map((x) => x.diff).filter(Boolean),
      files: kept.map((x) => x.path),
      unseen: c.unseen.filter((p) => !skip(p)),
    };
  } catch (e) {
    return { parts: [], files: [], unseen: [], error: errorText(e) };
  }
}
