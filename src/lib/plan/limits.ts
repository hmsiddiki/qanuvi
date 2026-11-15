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
