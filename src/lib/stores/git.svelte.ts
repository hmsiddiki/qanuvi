/** Tells views that the repository changed (a commit, staging, a branch switch), so they refresh. */
class GitStore {
  /** Goes up after every git action Qanuvi makes. */
  changed = $state(0);

  touch(): void {
    this.changed++;
  }
}

export const gitState = new GitStore();

/** "3 days ago" for a time in seconds since 1970. */
export function ago(seconds: number, now = Date.now()): string {
  const s = Math.max(0, Math.round(now / 1000 - seconds));
  const units: [number, string][] = [
    [31536000, "year"],
    [2592000, "month"],
    [604800, "week"],
    [86400, "day"],
    [3600, "hour"],
    [60, "minute"],
  ];
  for (const [size, name] of units) {
    const n = Math.floor(s / size);
    if (n >= 1) return `${n} ${name}${n > 1 ? "s" : ""} ago`;
  }
  return "just now";
}
