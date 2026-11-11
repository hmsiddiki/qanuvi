/** What a piece of work changed in a project: the text of changed files before it, and the diff since (24b/24e). */
import { api } from "$lib/api";
import { stepDiff } from "$lib/core/decider";

/**
 * The text of the files that differ from the last commit now. Earlier work that is not committed yet would show in a
 * diff against the commit, so a later look compares with this instead.
 */
export async function snapshotChanges(projectId: string): Promise<Map<string, string>> {
  const out = new Map<string, string>();
  const st = await api.git.status(projectId).catch(() => null);
  if (!st?.is_repo) return out;
  for (const c of st.changes.slice(0, 40)) {
    const f = await api.files.read(projectId, c.path).catch(() => null);
    out.set(c.path, f && !f.binary && !f.too_large ? f.text : "");
  }
  return out;
}

/** The diffs of what changed since `before` (files `skip` names left out). Empty: nothing changed. */
export async function changesSince(
  projectId: string,
  before: Map<string, string>,
  skip: (path: string) => boolean = () => false,
): Promise<string[]> {
  const after = await snapshotChanges(projectId);
  const parts: string[] = [];
  for (const [path, text] of after) {
    if (skip(path)) continue;
    const was = before.get(path);
    if (was === text) continue;
    // A file that was clean before: its diff against the commit is this work's change.
    const own = was === undefined ? null : stepDiff(path, was, text);
    parts.push(own ?? (await api.git.diff(projectId, path).catch(() => "")));
  }
  return parts;
}
