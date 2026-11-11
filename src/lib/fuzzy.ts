/**
 * Fuzzy file matching for "Go to file": the letters of the query must appear in order. Matches in the file name, at
 * the start of words and in a row score higher.
 */
export function fuzzyScore(query: string, path: string): number | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return 0;
  const p = path.toLowerCase();
  const nameStart = p.lastIndexOf("/") + 1;
  let score = 0;
  let pi = 0;
  let prev = -2;
  for (const ch of q) {
    const found = p.indexOf(ch, pi);
    if (found < 0) return null;
    score += 1;
    if (found === prev + 1) score += 3;
    if (found >= nameStart) score += 2;
    const before = p[found - 1];
    if (found === 0 || before === "/" || before === "." || before === "-" || before === "_") score += 2;
    prev = found;
    pi = found + 1;
  }
  // Prefer short paths, and an exact file name above all.
  if (p.slice(nameStart) === q) score += 50;
  return score - path.length * 0.01;
}

export function fuzzyFilter(query: string, paths: string[], limit = 60): string[] {
  if (!query.trim()) return paths.slice(0, limit);
  const scored: { path: string; score: number }[] = [];
  for (const path of paths) {
    const score = fuzzyScore(query, path);
    if (score !== null) scored.push({ path, score });
  }
  scored.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
  return scored.slice(0, limit).map((s) => s.path);
}
