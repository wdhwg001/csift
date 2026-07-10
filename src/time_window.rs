//! Time-window parsing for `search --since`/`--until` (SPEC §6.2).
//!
//! A bound is EITHER an absolute ISO8601 instant/date OR a relative form
//! (`2h`, `3d`, `90m`, `45s`, `1w`) interpreted as "that long ago" relative to
//! **now in the system-local timezone** (auto-detected via [`crate::timez::local_tz`]),
//! then converted to UTC for comparison. A record's `timestamp` (raw UTC ISO8601) is
//! compared against the resolved bounds; a record with no timestamp NEVER falls
//! inside a bounded window (SPEC §6.2).
//!
//! `--since` is inclusive lower, `--until` is inclusive upper. An unbounded window
//! (neither set) admits every record, including timestamp-less ones.

use anyhow::{bail, Context, Result};
use jiff::{Span, Timestamp, Zoned};

use crate::timez::local_tz;

/// A resolved `[since, until]` window in absolute UTC timestamps.
#[derive(Debug, Clone, Default)]
pub struct TimeWindow {
    since: Option<Timestamp>,
    until: Option<Timestamp>,
}

impl TimeWindow {
    /// Build from the raw `--since`/`--until` strings (either may be absent).
    pub fn from_args(since: Option<&str>, until: Option<&str>) -> Result<Self> {
        let since = since.map(parse_bound).transpose()?;
        let until = until.map(parse_bound).transpose()?;
        if let (Some(s), Some(u)) = (since, until) {
            if u < s {
                bail!("--until is before --since (since={s}, until={u})");
            }
        }
        Ok(Self { since, until })
    }

    /// True when no bound is set (every record admitted, timestamp or not).
    #[must_use]
    pub fn is_unbounded(&self) -> bool {
        self.since.is_none() && self.until.is_none()
    }

    /// True if the inclusive activity span `[first, last]` (raw UTC ISO timestamps)
    /// INTERSECTS the window — the `list` rule: a session is admitted when ANY part of its
    /// [first-activity, last-activity] span falls inside `[since, until]` (a long-running
    /// session that straddles the whole window still matches). One missing/unparseable
    /// endpoint degrades the span to a point; both missing ⇒ a bounded window never admits
    /// (the SPEC §6.2 timestamp-less rule).
    #[must_use]
    pub fn intersects_span(&self, first: Option<&str>, last: Option<&str>) -> bool {
        if self.is_unbounded() {
            return true;
        }
        let parse = |raw: Option<&str>| raw.and_then(|r| r.parse::<Timestamp>().ok());
        let (a, b) = match (parse(first), parse(last)) {
            (Some(a), Some(b)) => (a.min(b), a.max(b)),
            (Some(a), None) | (None, Some(a)) => (a, a),
            (None, None) => return false,
        };
        if let Some(s) = self.since {
            if b < s {
                return false;
            }
        }
        if let Some(u) = self.until {
            if a > u {
                return false;
            }
        }
        true
    }

    /// True if a record's raw timestamp falls inside the window. An unbounded
    /// window always returns true (even for `None`). A bounded window NEVER admits
    /// a record with no/unparseable timestamp (SPEC §6.2).
    #[must_use]
    pub fn contains(&self, raw_ts: Option<&str>) -> bool {
        if self.is_unbounded() {
            return true;
        }
        let Some(raw) = raw_ts else {
            return false;
        };
        let Ok(ts) = raw.parse::<Timestamp>() else {
            return false;
        };
        if let Some(s) = self.since {
            if ts < s {
                return false;
            }
        }
        if let Some(u) = self.until {
            if ts > u {
                return false;
            }
        }
        true
    }
}

/// Parse one bound: try the relative form first (`<N><unit>`), then absolute
/// ISO8601 (full instant, or a bare date interpreted at system-local midnight).
fn parse_bound(s: &str) -> Result<Timestamp> {
    let s = s.trim();
    if let Some(ts) = parse_relative(s)? {
        return Ok(ts);
    }
    parse_absolute(s)
}

/// Relative form: an optional-sign-free integer followed by a single unit char.
/// `Ok(None)` when `s` is not in this shape (so the caller falls through to
/// absolute parsing). Resolved against now in the system-local timezone → UTC.
fn parse_relative(s: &str) -> Result<Option<Timestamp>> {
    // Must be all-ASCII-digits then exactly one unit letter.
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return Ok(None);
    }
    let (num_part, unit) = s.split_at(s.len() - 1);
    if num_part.is_empty() || !num_part.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let n: i64 = num_part
        .parse()
        .with_context(|| format!("invalid relative-time quantity in {s:?}"))?;

    let span = match unit {
        "s" => Span::new().try_seconds(n),
        "m" => Span::new().try_minutes(n),
        "h" => Span::new().try_hours(n),
        "d" => Span::new().try_days(n),
        "w" => Span::new().try_weeks(n),
        _ => return Ok(None), // not a recognized unit → let absolute parsing try
    }
    .with_context(|| format!("relative-time span out of range in {s:?}"))?;

    let now = now_local();
    // "2h" means two hours AGO.
    let then = now
        .checked_sub(span)
        .with_context(|| format!("relative-time underflow computing {s:?} ago"))?;
    Ok(Some(then.timestamp()))
}

/// Absolute ISO8601: a full instant (`2026-06-01T00:00:00Z`) or a bare date
/// (`2026-06-01`, taken at system-local midnight).
fn parse_absolute(s: &str) -> Result<Timestamp> {
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    // Bare civil date → system-local midnight → UTC.
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        let zoned = date
            .to_zoned(local_tz())
            .with_context(|| format!("cannot place date {s:?} in the system-local timezone"))?;
        return Ok(zoned.timestamp());
    }
    bail!(
        "cannot parse time bound {s:?}: expected ISO8601 (e.g. 2026-06-01 or \
         2026-06-01T05:00:00Z) or a relative form (e.g. 2h, 3d, 90m)"
    )
}

/// `now` in the system-local timezone (auto-detected). [`local_tz`] is infallible,
/// so this never errors.
fn now_local() -> Zoned {
    Timestamp::now().to_zoned(local_tz())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbounded_admits_everything() {
        let w = TimeWindow::default();
        assert!(w.is_unbounded());
        assert!(w.contains(Some("2026-06-07T05:00:00Z")));
        assert!(w.contains(None));
    }

    #[test]
    fn bounded_excludes_timestampless_records() {
        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        assert!(!w.is_unbounded());
        assert!(!w.contains(None));
    }

    #[test]
    fn since_until_absolute_instant() {
        let w = TimeWindow::from_args(Some("2026-06-01T00:00:00Z"), Some("2026-06-30T23:59:59Z"))
            .unwrap();
        assert!(w.contains(Some("2026-06-07T05:00:00Z")));
        assert!(!w.contains(Some("2026-05-31T23:59:59Z")));
        assert!(!w.contains(Some("2026-07-01T00:00:00Z")));
    }

    #[test]
    fn bare_date_is_system_local_midnight() {
        // A bare date resolves to local midnight. Derive the expected UTC instant
        // from jiff for the SYSTEM zone (tz-agnostic — holds on any machine / CI),
        // rather than hardcoding a single zone's offset.
        let midnight_utc = "2026-06-01"
            .parse::<jiff::civil::Date>()
            .unwrap()
            .to_zoned(local_tz())
            .unwrap()
            .timestamp();
        let one_sec_before = midnight_utc - jiff::SignedDuration::from_secs(1);

        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        // A record exactly at the local-midnight UTC instant is included.
        assert!(w.contains(Some(&midnight_utc.to_string())));
        // One second before is excluded.
        assert!(!w.contains(Some(&one_sec_before.to_string())));
    }

    #[test]
    fn relative_form_resolves_to_past() {
        // "1h" ago must be in the past relative to "now"; a far-future ts is out of
        // a [since=1h-ago, ∞) window only if it's BEFORE the bound — future is in.
        let w = TimeWindow::from_args(Some("1h"), None).unwrap();
        assert!(!w.is_unbounded());
        // A timestamp from the distant past is before "1 hour ago" → excluded.
        assert!(!w.contains(Some("2000-01-01T00:00:00Z")));
        // A timestamp far in the future is after "1 hour ago" → included.
        assert!(w.contains(Some("2999-01-01T00:00:00Z")));
    }

    #[test]
    fn relative_units_all_parse() {
        for u in ["1s", "30m", "2h", "3d", "1w"] {
            assert!(
                TimeWindow::from_args(Some(u), None).is_ok(),
                "unit failed: {u}"
            );
        }
    }

    #[test]
    fn invalid_bound_errors() {
        assert!(TimeWindow::from_args(Some("not-a-time"), None).is_err());
        assert!(TimeWindow::from_args(Some("2h"), Some("garbage")).is_err());
    }

    #[test]
    fn until_before_since_errors() {
        assert!(
            TimeWindow::from_args(Some("2026-06-30T00:00:00Z"), Some("2026-06-01T00:00:00Z"))
                .is_err()
        );
    }

    // ── Branch-completeness ──

    #[test]
    fn contains_unparseable_timestamp_excluded_from_bounded() {
        // A bounded window with a record whose timestamp does not parse → excluded
        // (the `Err(_) => false` arm in `contains`).
        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        assert!(!w.contains(Some("not-a-timestamp")));
    }

    #[test]
    fn contains_until_upper_bound_excludes_after() {
        // An UNTIL-only window: a ts AFTER the bound is excluded (the `ts > u` true
        // arm, distinct from the since lower-bound path other tests cover).
        let w = TimeWindow::from_args(None, Some("2026-06-07T00:00:00Z")).unwrap();
        assert!(w.contains(Some("2026-06-06T23:59:59Z")));
        assert!(!w.contains(Some("2026-06-07T00:00:01Z")));
    }

    #[test]
    fn both_bounds_set_window() {
        // Both since AND until present (the `(Some, Some)` arm in from_args + both
        // arms of contains).
        let w = TimeWindow::from_args(Some("2026-06-01T00:00:00Z"), Some("2026-06-02T00:00:00Z"))
            .unwrap();
        assert!(w.contains(Some("2026-06-01T12:00:00Z")));
        assert!(!w.contains(Some("2026-05-31T00:00:00Z"))); // before since
        assert!(!w.contains(Some("2026-06-03T00:00:00Z"))); // after until
    }

    #[test]
    fn relative_short_string_is_not_relative() {
        // A single-char string (len < 2) cannot be a relative form → it falls through
        // to absolute parsing, which fails for "x" (so an error), but "1" alone is
        // also < 2 chars and not absolute either.
        assert!(TimeWindow::from_args(Some("x"), None).is_err());
        assert!(TimeWindow::from_args(Some("1"), None).is_err());
    }

    #[test]
    fn relative_non_digit_quantity_falls_through() {
        // `ah` has a unit letter but a non-digit quantity → not relative; falls to
        // absolute, which also fails → error.
        assert!(TimeWindow::from_args(Some("ah"), None).is_err());
    }

    #[test]
    fn relative_unrecognized_unit_falls_through_to_absolute() {
        // `5y` — digits + an unrecognized unit letter → parse_relative returns None
        // (the `_ => return Ok(None)` arm) and absolute parsing then fails.
        assert!(TimeWindow::from_args(Some("5y"), None).is_err());
    }

    #[test]
    fn relative_quantity_overflow_errors() {
        // A quantity too large for i64 → the `.parse::<i64>()` with_context error path.
        let huge = format!("{}h", "9".repeat(40));
        assert!(TimeWindow::from_args(Some(&huge), None).is_err());
    }

    #[test]
    fn relative_span_out_of_range_errors() {
        // A numerically-valid i64 quantity whose span is out of jiff's range → the
        // span `.with_context` error arm (weeks magnify the value the most).
        let big = format!("{}w", i64::MAX);
        assert!(TimeWindow::from_args(Some(&big), None).is_err());
    }

    #[test]
    fn absolute_full_instant_path() {
        // A full ISO instant takes the `s.parse::<Timestamp>()` Ok arm in
        // parse_absolute (distinct from the bare-date arm other tests cover).
        let w = TimeWindow::from_args(Some("2026-06-07T05:00:00Z"), None).unwrap();
        assert!(w.contains(Some("2026-06-07T06:00:00Z")));
    }
}
