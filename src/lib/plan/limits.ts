/**
 * Usage limits (a paid plan's session or weekly limit): "You've hit your session limit · resets 5:20am (Asia/Dhaka)".
 * A run that meets one waits until the reset and goes on, or hands steps to free models until then.
 */

/** Whether a text is a usage-limit message. */
export function isLimit(text: string): boolean {
  return /hit your (session|weekly|usage|daily|5-hour|opus|fable)?\s*limit|usage limit reached|rate[_ ]limit.*reached/i.test(
    text,
  );
}

/** The minutes from now until a wall-clock time in a time zone (the next time it comes round). */
function minutesUntil(hour: number, minute: number, zone: string | undefined, now: Date): number {
  let parts: Intl.DateTimeFormatPart[];
  try {
    parts = new Intl.DateTimeFormat("en-GB", {
      timeZone: zone,
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(now);
  } catch {
    parts = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" }).formatToParts(
      now,
    );
  }
  const h = Number(parts.find((p) => p.type === "hour")?.value ?? 0);
  const m = Number(parts.find((p) => p.type === "minute")?.value ?? 0);
  const diff = (hour * 60 + minute - (h * 60 + m) + 1440) % 1440;
  return diff === 0 ? 1440 : diff;
}

/**
 * When the limit resets, from its message. Times without a date are the next time that hour comes round in the
 * zone given (or this computer's). Unreadable: an hour from now, then the run looks again.
 */
export function limitResetAt(text: string, now = new Date()): Date | null {
  if (!isLimit(text)) return null;
  const m = text.match(/resets?\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\s*(?:\(([^)]+)\))?/i);
  if (!m) return new Date(now.getTime() + 60 * 60_000);
  let hour = Number(m[1]) % 24;
  const minute = Number(m[2] ?? 0);
  const ap = m[3]?.toLowerCase();
  if (ap === "pm" && hour < 12) hour += 12;
  if (ap === "am" && hour === 12) hour = 0;
  // A minute's margin after the reset.
  return new Date(now.getTime() + (minutesUntil(hour, minute, m[4]?.trim(), now) + 1) * 60_000);
}
