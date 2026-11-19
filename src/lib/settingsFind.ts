/**
 * Settings search (24n, A4): which piece of text on a settings page the search box points at, so the page can scroll
 * to it and mark it. The whole phrase wins; otherwise the text holding the most of its words (the first such one).
 * Answers the piece's index and where the marked part starts and ends in it, or null when nothing holds any word.
 */
export function firstMatch(texts: string[], query: string): { index: number; start: number; end: number } | null {
  const phrase = query.trim().toLowerCase().replace(/\s+/g, " ");
  if (!phrase) return null;
  const lower = texts.map((t) => t.toLowerCase());
  const whole = lower.findIndex((t) => t.includes(phrase));
  if (whole >= 0) {
    const start = lower[whole].indexOf(phrase);
    return { index: whole, start, end: start + phrase.length };
  }
  const words = phrase.split(" ");
  let best = -1;
  let most = 0;
  lower.forEach((t, i) => {
    const n = words.filter((w) => t.includes(w)).length;
    if (n > most) {
      most = n;
      best = i;
    }
  });
  if (best < 0) return null;
  const word = words.find((w) => lower[best].includes(w)) ?? "";
  const start = lower[best].indexOf(word);
  return { index: best, start, end: start + word.length };
}
