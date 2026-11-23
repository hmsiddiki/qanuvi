/**
 * Two projects with the same name (y9/qanuvi and y10/qanuvi) are told apart by their folder: the shortest end of the
 * parent path that differs from the others with that name. Projects with a name of their own get no hint.
 */
export interface Named {
  id: string;
  name: string;
  path: string;
  master?: string | null;
}

function parents(path: string): string[] {
  const parts = path.replace(/\\/g, "/").replace(/\/+$/, "").split("/").filter(Boolean);
  return parts.slice(0, -1);
}

/** The folder hint for each project id that shares its name with another; missing for the rest. */
export function folderHints(list: Named[]): Map<string, string> {
  const hints = new Map<string, string>();
  const byName = new Map<string, Named[]>();
  for (const p of list) {
    const key = p.name.toLowerCase();
    byName.set(key, [...(byName.get(key) ?? []), p]);
  }
  for (const same of byName.values()) {
    if (same.length < 2) continue;
    const ups = same.map((p) => parents(p.path));
    const most = Math.max(...ups.map((u) => u.length));
    for (const [i, p] of same.entries()) {
      let hint = "";
      for (let depth = 1; depth <= Math.max(most, 1); depth++) {
        hint = ups[i].slice(-depth).join("/");
        const clash = ups.some((u, j) => j !== i && u.slice(-depth).join("/") === hint);
        if (!clash) break;
      }
      // Same folder on two machines: the server icon already tells them apart; keep the folder anyway.
      if (hint) hints.set(p.id, hint);
    }
  }
  return hints;
}

/** "qanuvi (y9)" when the name is shared, else the name. */
export function shownName(p: Named, hints: Map<string, string>): string {
  const hint = hints.get(p.id);
  return hint ? `${p.name} (${hint})` : p.name;
}
