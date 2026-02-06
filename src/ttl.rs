//! TTL (time-to-live) support for memory items.
//!
//! Provides client-side expiry filtering since FerridynDB has no native TTL.
//! Items with an `expires_at` attribute (RFC 3339 timestamp) are filtered out
//! on read when the timestamp is in the past.

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use serde_json::Value;

/// Default TTL for scratchpad items: 24 hours.
pub const SCRATCHPAD_DEFAULT_TTL: Duration = Duration::hours(24);

/// Default TTL for sessions items: 7 days.
pub const SESSIONS_DEFAULT_TTL: Duration = Duration::days(7);

/// Default TTL for interactions items: 90 days.
pub const INTERACTIONS_DEFAULT_TTL: Duration = Duration::days(90);

/// Parse a TTL duration string into a [`chrono::Duration`].
///
/// Supported formats:
/// - `"1h"`, `"24h"` — hours
/// - `"1d"`, `"7d"`, `"30d"` — days
/// - `"1w"`, `"2w"` — weeks (7 days each)
///
/// Returns an error if the format is unrecognized or the number is invalid.
pub fn parse_ttl(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("TTL string is empty".into());
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| format!("Invalid TTL number: '{num_str}'"))?;

    if num <= 0 {
        return Err(format!("TTL must be positive, got {num}"));
    }

    match unit {
        "h" => Ok(Duration::hours(num)),
        "d" => Ok(Duration::days(num)),
        "w" => Ok(Duration::weeks(num)),
        _ => Err(format!(
            "Unknown TTL unit '{unit}'. Use h (hours), d (days), or w (weeks)"
        )),
    }
}

/// Compute an `expires_at` timestamp from now + duration.
///
/// Returns an RFC 3339 string suitable for storing as a STRING attribute.
pub fn compute_expires_at(ttl: Duration) -> String {
    (Utc::now() + ttl).to_rfc3339()
}

/// Check if an item is expired.
///
/// An item is expired if it has an `expires_at` attribute whose value is a
/// valid RFC 3339 timestamp in the past. Items without `expires_at` are never
/// considered expired (they are LTM).
pub fn is_expired(item: &Value) -> bool {
    match item.get("expires_at").and_then(|v| v.as_str()) {
        Some(expires_str) => match DateTime::parse_from_rfc3339(expires_str) {
            Ok(expires) => Utc::now() > expires,
            Err(_) => false, // Unparseable — treat as not expired.
        },
        None => false, // No expires_at — LTM, never expires.
    }
}

/// Filter a list of items, removing expired ones.
pub fn filter_expired(items: Vec<Value>) -> Vec<Value> {
    items.into_iter().filter(|item| !is_expired(item)).collect()
}

/// Auto-compute an `expires_at` for the `events` category based on the `date`
/// attribute.
///
/// If the item has a `date` attribute (ISO 8601 date string like "2026-02-10"),
/// returns an `expires_at` set to the end of that day (23:59:59 UTC).
/// Returns `None` if no date attribute is present or parsing fails.
pub fn auto_ttl_from_date(item: &Value) -> Option<String> {
    let date_str = item.get("date").and_then(|v| v.as_str())?;
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()?;
    let end_of_day = date
        .and_time(NaiveTime::from_hms_opt(23, 59, 59)?)
        .and_utc();
    Some(end_of_day.to_rfc3339())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- parse_ttl ---

    #[test]
    fn test_parse_ttl_hours() {
        let d = parse_ttl("24h").unwrap();
        assert_eq!(d, Duration::hours(24));
    }

    #[test]
    fn test_parse_ttl_days() {
        let d = parse_ttl("7d").unwrap();
        assert_eq!(d, Duration::days(7));
    }

    #[test]
    fn test_parse_ttl_weeks() {
        let d = parse_ttl("2w").unwrap();
        assert_eq!(d, Duration::weeks(2));
    }

    #[test]
    fn test_parse_ttl_single_hour() {
        let d = parse_ttl("1h").unwrap();
        assert_eq!(d, Duration::hours(1));
    }

    #[test]
    fn test_parse_ttl_invalid_unit() {
        assert!(parse_ttl("5x").is_err());
    }

    #[test]
    fn test_parse_ttl_empty() {
        assert!(parse_ttl("").is_err());
    }

    #[test]
    fn test_parse_ttl_zero() {
        assert!(parse_ttl("0h").is_err());
    }

    #[test]
    fn test_parse_ttl_negative() {
        assert!(parse_ttl("-1d").is_err());
    }

    #[test]
    fn test_parse_ttl_no_number() {
        assert!(parse_ttl("d").is_err());
    }

    // --- compute_expires_at ---

    #[test]
    fn test_compute_expires_at_in_future() {
        let expires = compute_expires_at(Duration::hours(1));
        let parsed = DateTime::parse_from_rfc3339(&expires).unwrap();
        assert!(parsed > Utc::now());
    }

    // --- is_expired ---

    #[test]
    fn test_not_expired_no_field() {
        let item = json!({"category": "notes", "key": "test", "content": "hello"});
        assert!(!is_expired(&item));
    }

    #[test]
    fn test_not_expired_future() {
        let future = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let item = json!({"category": "notes", "key": "test", "expires_at": future});
        assert!(!is_expired(&item));
    }

    #[test]
    fn test_expired_past() {
        let past = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let item = json!({"category": "notes", "key": "test", "expires_at": past});
        assert!(is_expired(&item));
    }

    #[test]
    fn test_not_expired_invalid_string() {
        let item = json!({"category": "notes", "key": "test", "expires_at": "not-a-date"});
        assert!(!is_expired(&item));
    }

    // --- filter_expired ---

    #[test]
    fn test_filter_expired_removes_past() {
        let past = (Utc::now() - Duration::hours(1)).to_rfc3339();
        let future = (Utc::now() + Duration::hours(1)).to_rfc3339();
        let items = vec![
            json!({"key": "alive", "expires_at": future}),
            json!({"key": "dead", "expires_at": past}),
            json!({"key": "permanent"}), // no expires_at = LTM
        ];
        let filtered = filter_expired(items);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0]["key"], "alive");
        assert_eq!(filtered[1]["key"], "permanent");
    }

    // --- auto_ttl_from_date ---

    #[test]
    fn test_auto_ttl_from_date_valid() {
        let item = json!({"category": "events", "key": "meeting", "date": "2030-06-15"});
        let expires = auto_ttl_from_date(&item).unwrap();
        let parsed = DateTime::parse_from_rfc3339(&expires).unwrap();
        assert_eq!(
            parsed.date_naive(),
            NaiveDate::from_ymd_opt(2030, 6, 15).unwrap()
        );
    }

    #[test]
    fn test_auto_ttl_from_date_no_date() {
        let item = json!({"category": "events", "key": "meeting", "content": "standup"});
        assert!(auto_ttl_from_date(&item).is_none());
    }

    #[test]
    fn test_auto_ttl_from_date_invalid() {
        let item = json!({"category": "events", "key": "meeting", "date": "not-a-date"});
        assert!(auto_ttl_from_date(&item).is_none());
    }
}
