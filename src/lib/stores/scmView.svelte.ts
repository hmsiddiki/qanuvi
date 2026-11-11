/**
 * What the Git view shows (15h): the change chosen in the project panel's list, or the history being looked at. Shared
 * by the list (in the panel) and the diff and history (in the work area).
 */
class ScmView {
  selected = $state<{ path: string; side: "staged" | "changes" } | null>(null);
  /** History shown in the work area: a file's path, "" for the whole project, null when closed. */
  historyFor = $state<string | null>(null);
}

export const scmView = new ScmView();
