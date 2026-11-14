//! Small helpers that behave like the JavaScript the plan logic was written in, so the Rust port gives the same text:
//! `String(x)`, `toFixed`, `Math.round`, `slice` on UTF-16 units, and reading the JSON in a model's answer.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// The JSON object in a model's answer: the first fenced block if there is one, from its first `{` to its last `}`.
pub fn json_in(answer: &str) -> Option<Value> {
    static FENCED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)```(?:json)?\s*(.*?)```").unwrap());
    let body = FENCED
        .captures(answer)
        .and_then(|c| c.get(1))
        .map_or(answer, |m| m.as_str());
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&body[start..=end]).ok()
}

/// `String(v ?? "")`: what JavaScript makes of a value as text.
pub fn string(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number(n.as_f64().unwrap_or(0.0)),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| string(Some(x)))
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// A number as JavaScript prints it (whole numbers without ".0").
pub fn number(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e21 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// `Math.round`: halves go up.
pub fn round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `x.toFixed(digits)`: an exact tie rounds away from zero, as JavaScript does (Rust's own formatting rounds to even).
pub fn to_fixed(x: f64, digits: usize) -> String {
    // The value's exact decimal digits (a double's tie at a few places shows within 60 places).
    let exact = format!("{:.60}", x.abs());
    let frac = exact.split_once('.').map_or("", |(_, f)| f);
    let tie = frac[digits..].starts_with('5') && frac[digits + 1..].bytes().all(|b| b == b'0');
    if tie {
        let scale = 10f64.powi(digits as i32);
        let up = ((x.abs() * scale).trunc() + 1.0) / scale;
        let s = format!("{up:.digits$}");
        return if x < 0.0 { format!("-{s}") } else { s };
    }
    format!("{x:.digits$}")
}

/// `s.slice(0, n)` on UTF-16 units (a character that would be cut in half is left out).
pub fn cut(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// The length JavaScript gives a string (UTF-16 units).
pub fn len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_json_in_an_answer() {
        assert_eq!(
            json_in("Sure:\n```json\n{\"a\":1}\n```"),
            Some(json!({"a": 1}))
        );
        assert_eq!(json_in("x {\"b\": [1]} y"), Some(json!({"b": [1]})));
        assert_eq!(json_in("no json"), None);
        assert_eq!(json_in("} {"), None);
    }

    #[test]
    fn prints_like_javascript() {
        assert_eq!(string(Some(&json!(1))), "1");
        assert_eq!(string(Some(&json!(1.5))), "1.5");
        assert_eq!(string(Some(&json!(null))), "");
        assert_eq!(string(Some(&json!({"a": 1}))), "[object Object]");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(2.0, 2), "2.00");
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(cut("héllo", 2), "hé");
        assert_eq!(cut("a😀b", 2), "a");
    }
}
