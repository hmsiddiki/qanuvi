/**
 * Migration M1c: with a master's project on screen, `localhost` in the preview means that master's localhost. Such
 * an address loads through a tunnel (`port_forward`) on any free port of this computer, so it never clashes with this
 * computer's own dev servers. The address bar keeps the master's address; public addresses load directly.
 */

/** The port on the machine a loopback address points at (80/443 when none is typed), or null when not loopback. */
export function loopbackPort(raw: string): number | null {
  let url: URL;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return null;
  const host = url.hostname.toLowerCase();
  const loopback =
    host === "localhost" || host.endsWith(".localhost") || host === "[::1]" || /^127(\.\d{1,3}){3}$/.test(host);
  if (!loopback) return null;
  return url.port ? Number(url.port) : url.protocol === "https:" ? 443 : 80;
}

/**
 * The same address through the tunnel's local port. Names (`localhost`, `site.localhost`) are kept, so the master's
 * server still sees the site name it serves; the tunnel listens on 127.0.0.1 only, so number addresses become it.
 */
export function throughTunnel(raw: string, local: number): string {
  const url = new URL(raw);
  const host = url.hostname.toLowerCase();
  if (host === "[::1]" || /^127(\.\d{1,3}){3}$/.test(host)) url.hostname = "127.0.0.1";
  url.port = String(local);
  return url.toString();
}

/**
 * Text handed to the agent (page text, a picked element) names the tunnel's address; the agent works on the master,
 * where that port means nothing. Put the master's own address back (`tunnelled` is what loaded, `shown` the bar's).
 */
export function machineText(text: string, tunnelled: string, shown: string): string {
  if (!tunnelled || tunnelled === shown) return text;
  let from: string;
  let to: string;
  try {
    from = new URL(tunnelled).origin;
    to = new URL(shown).origin;
  } catch {
    return text;
  }
  if (from === to) return text;
  const escaped = from.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text.replace(new RegExp(`${escaped}(?!\\d)`, "g"), to);
}
