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

/// The longest text a usage-limit notice can be: it is one sentence, never a page of work.
const NOTICE_MAX: usize = 400;

/// The usage-limit notice in one of a session's agent lines, or None when the line cannot be one.
///
/// Only the agent's own error end counts: a turn that ended in error (`result` with `is_error`), the agent's own
/// error message (an `assistant` line marked as an API error) or plain output of the agent program that is not JSON.
/// The agent's ordinary replies, tool results, files it read and the owner's messages never do, whatever they say
/// (a step about a REST rate limit once read as the paid agent's limit and the run waited an hour for nothing).
pub fn limit_notice(line: &str) -> Option<String> {
    let text = match serde_json::from_str::<serde_json::Value>(line) {
        Ok(m) => {
            let kind = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match kind {
                "result" if m.get("is_error").and_then(|v| v.as_bool()) == Some(true) => {
                    m.get("result").and_then(|v| v.as_str())?.to_string()
                }
                "assistant"
                    if m.get("error").is_some_and(|e| !e.is_null())
                        || m.get("isApiErrorMessage").and_then(|v| v.as_bool()) == Some(true)
                        || m.pointer("/message/model").and_then(|v| v.as_str())
                            == Some("<synthetic>") =>
                {
                    m.pointer("/message/content/0/text")
                        .and_then(|v| v.as_str())?
                        .to_string()
                }
                _ => return None,
            }
        }
        Err(_) => line.to_string(),
    };
    let text = text.trim();
    (text.len() <= NOTICE_MAX && is_limit(text)).then(|| text.to_string())
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

    /// The step that once stopped a run for an hour: its work was about a REST rate limit, and a tool result quoting
    /// the plan said "rate limit" and, further on, "reached". Only the agent's own error end is a limit notice.
    #[test]
    fn only_the_agents_own_error_end_is_a_limit_notice() {
        let work =
            "skips the REST rate limit (`rest.php:434`) … the limit is reached by any visitor";
        let not_limits = [
            // A tool result (a file it edited or read).
            serde_json::json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": work}]},
                "tool_use_result": {"originalFile": work}})
            .to_string(),
            // The agent's reply and its successful end.
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": work}]}})
                .to_string(),
            serde_json::json!({"type": "result", "subtype": "success", "is_error": false, "result": work})
                .to_string(),
            // The owner's own message in the step's chat.
            serde_json::json!({"type": "qanuvi_user", "text": "You've hit your session limit? rate limit reached?"})
                .to_string(),
            // An error end that is not a limit, and a long error that only mentions one.
            serde_json::json!({"type": "result", "is_error": true, "result": "Tests failed."}).to_string(),
            serde_json::json!({"type": "result", "is_error": true, "result": format!("{} {work}", "x".repeat(400))})
                .to_string(),
        ];
        for l in &not_limits {
            assert_eq!(limit_notice(l), None, "{l}");
        }

        let limit = "You've hit your session limit · resets 5:20am (Asia/Dhaka)";
        let ended =
            serde_json::json!({"type": "result", "is_error": true, "result": limit}).to_string();
        assert_eq!(limit_notice(&ended).as_deref(), Some(limit));
        let api_error = serde_json::json!({"type": "assistant", "error": "rate_limit",
            "message": {"model": "<synthetic>", "content": [{"type": "text", "text": "Claude usage limit reached"}]}})
        .to_string();
        assert_eq!(
            limit_notice(&api_error).as_deref(),
            Some("Claude usage limit reached")
        );
        // Plain output of the agent program (not JSON).
        assert_eq!(
            limit_notice("Usage limit reached. Resets 11pm").as_deref(),
            Some("Usage limit reached. Resets 11pm")
        );
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
