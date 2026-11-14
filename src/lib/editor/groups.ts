/**
 * Editor groups (15i part 4): the rules for which tabs each group holds, as plain functions so they can be tested
 * without a window. At most two groups, side by side or one above the other. A file can be open in both.
 */

export interface Group {
  /** Tab keys in the order shown. */
  keys: string[];
  active: string | null;
}

export type SplitDirection = "right" | "down";

export const MAX_GROUPS = 2;

const copy = (groups: Group[]): Group[] => groups.map((g) => ({ keys: [...g.keys], active: g.active }));

/** Adds a key to a group (at the end, or before `before`) and makes it the group's active tab. */
export function addTo(groups: Group[], g: number, key: string, before: string | null = null): Group[] {
  const out = copy(groups);
  while (out.length <= g) out.push({ keys: [], active: null });
  const keys = out[g].keys.filter((k) => k !== key);
  const at = before === null ? -1 : keys.indexOf(before);
  if (at < 0) keys.push(key);
  else keys.splice(at, 0, key);
  out[g] = { keys, active: key };
  return out;
}

/**
 * Takes a key out of a group. When it was the group's active tab, the neighbour among the tabs `shown` (this
 * project's) becomes active: the one that took its place, else the one before it.
 */
export function removeFrom(groups: Group[], g: number, key: string, shown: (k: string) => boolean = () => true) {
  const out = copy(groups);
  const group = out[g];
  if (!group) return out;
  const before = group.keys.filter(shown);
  const index = before.indexOf(key);
  group.keys = group.keys.filter((k) => k !== key);
  if (group.active === key) {
    const rest = group.keys.filter(shown);
    group.active = index < 0 ? (rest[rest.length - 1] ?? null) : (rest[Math.min(index, rest.length - 1)] ?? null);
  }
  return out;
}

/** Moves a tab within a group or to the other one (to `groups.length` makes a new group, if there is room). */
export function move(
  groups: Group[],
  key: string,
  from: number,
  to: number,
  before: string | null = null,
  shown: (k: string) => boolean = () => true,
): Group[] {
  if (to > groups.length || to >= MAX_GROUPS) to = groups.length - 1;
  if (from === to) return addTo(groups, to, key, before);
  return addTo(removeFrom(groups, from, key, shown), to, key, before);
}

/** Drops empty groups (one always stays) and keeps the focused index inside the list. */
export function tidy(groups: Group[], focused: number): { groups: Group[]; focused: number } {
  let out = copy(groups).filter((g) => g.keys.length > 0);
  if (out.length === 0) out = [{ keys: [], active: null }];
  // The focused group's new place: the groups kept before it (a dropped one hands focus to its neighbour).
  const f = groups.slice(0, Math.max(0, focused)).filter((g) => g.keys.length > 0).length;
  return { groups: out, focused: Math.min(f, out.length - 1) };
}

/** Puts the second group's tabs into the first (no tab twice); one group is left. */
export function join(groups: Group[]): Group[] {
  if (groups.length < 2) return copy(groups);
  const keys = [...groups[0].keys];
  for (const k of groups[1].keys) if (!keys.includes(k)) keys.push(k);
  return [{ keys, active: groups[0].active ?? groups[1].active }];
}

/** The groups that have something to show for this project (the first group when none has). */
export function shownGroups(groups: Group[], shown: (k: string) => boolean): number[] {
  const out: number[] = [];
  groups.forEach((g, i) => g.keys.some(shown) && out.push(i));
  return out.length ? out : [0];
}

/** How much of the space the first group takes; kept between 15% and 85%. */
export function cleanRatio(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(0.85, Math.max(0.15, value)) : 0.5;
}
