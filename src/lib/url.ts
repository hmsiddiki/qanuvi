/** Turns what the user typed in the preview's address bar into a web address, or "" if it cannot be one. */
export function normalizeUrl(raw: string): string {
  const text = raw.trim();
  if (!text) return "";
  const withScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(text)
    ? text
    : /^(localhost|127\.0\.0\.1|\[::1\]|[\w-]+\.(test|localhost))(:\d+)?(\/|$)/i.test(text)
      ? `http://${text}`
      : `https://${text}`;
  try {
    const url = new URL(withScheme);
    if (url.protocol !== "http:" && url.protocol !== "https:") return "";
    if (!url.hostname) return "";
    return url.toString();
  } catch {
    return "";
  }
}
