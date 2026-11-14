//! Usage limits (a paid plan's session or weekly limit), ported from `src/lib/plan/limits.ts`:
//! "You've hit your session limit · resets 5:20am (Asia/Dhaka)". A run that meets one waits until the reset and goes
//! on, or hands steps to free models until then.

use jiff::{Timestamp, tz::TimeZone};
use regex::Regex;
use std::sync::LazyLock;

/// Whether a text is a usage-limit message.
pub fn is_limit(text: &str) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)hit your (session|weekly|usage|daily|5-hour|opus|fable)?\s*limit|usage limit reached|rate[_ ]limit.*reached").unwrap()
    });
    RE.is_match(text)
}

/// The minutes from now until a wall-clock time in a time zone (the next time it comes round). An unknown zone, or
/// none, is this computer's.
fn minutes_until(hour: i64, minute: i64, zone: Option<&str>, now: Timestamp) -> i64 {
    let tz = zone
        .and_then(|z| TimeZone::get(z).ok())
        .unwrap_or_else(TimeZone::system);
    let t = now.to_zoned(tz);
    let (h, m) = (t.hour() as i64, t.minute() as i64);
    let diff = (hour * 60 + minute - (h * 60 + m) + 1440).rem_euclid(1440);
    if diff == 0 { 1440 } else { diff }
}

/// When the limit resets (milliseconds since 1970), from its message. Times without a date are the next time that
/// hour comes round in the zone given (or this computer's). Unreadable: an hour from now, then the run looks again.
pub fn limit_reset_at(text: &str, now: Timestamp) -> Option<i64> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)resets?\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\s*(?:\(([^)]+)\))?")
            .unwrap()
    });
    if !is_limit(text) {
        return None;
    }
    let now_ms = now.as_millisecond();
    let Some(c) = RE.captures(text) else {
        return Some(now_ms + 60 * 60_000);
    };
    let mut hour = c[1].parse::<i64>().unwrap_or(0) % 24;
    let minute = c.get(2).map_or(0, |m| m.as_str().parse().unwrap_or(0));
    match c.get(3).map(|m| m.as_str().to_lowercase()).as_deref() {
        Some("pm") if hour < 12 => hour += 12,
        Some("am") if hour == 12 => hour = 0,
        _ => {}
    }
    let zone = c.get(4).map(|m| m.as_str().trim());
    // A minute's margin after the reset.
    Some(now_ms + (minutes_until(hour, minute, zone, now) + 1) * 60_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn knows_a_limit_message() {
        assert!(is_limit(
            "You've hit your session limit · resets 5:20am (Asia/Dhaka)"
        ));
        assert!(!is_limit("Tests failed."));
    }

    #[test]
    fn finds_the_reset_time_in_the_zone_given() {
        // 03:00 in Dhaka (UTC+6) is 21:00 UTC the day before.
        let now = at("2026-11-11T21:00:00Z");
        let mins = |t: Option<i64>, now: Timestamp| (t.unwrap() - now.as_millisecond()) / 60_000;
        let r = limit_reset_at(
            "You've hit your session limit · resets 5:20am (Asia/Dhaka)",
            now,
        );
        assert_eq!(mins(r, now), 141);
        let now2 = at("2026-11-12T22:30:00Z");
        assert_eq!(
            mins(
                limit_reset_at("hit your weekly limit · resets 11pm (UTC)", now2),
                now2
            ),
            31
        );
        assert_eq!(limit_reset_at("no limit here", now), None);
        assert_eq!(
            mins(limit_reset_at("You've hit your usage limit", now), now),
            60
        );
    }
}
