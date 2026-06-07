//! Time-window parsing for `search --since`/`--until` (SPEC §6.2).
//!
//! A bound is EITHER an absolute ISO8601 instant/date OR a relative form
//! (`2h`, `3d`, `90m`, `45s`, `1w`) interpreted as "that long ago" relative to
//! **now in Australia/Sydney**, then converted to UTC for comparison. A record's
//! `timestamp` (raw UTC ISO8601) is compared against the resolved bounds; a record
//! with no timestamp NEVER falls inside a bounded window (SPEC §6.2).
//!
//! `--since` is inclusive lower, `--until` is inclusive upper. An unbounded window
//! (neither set) admits every record, including timestamp-less ones.

use anyhow::{bail, Context, Result};
use jiff::{Span, Timestamp, Zoned};

const SYDNEY: &str = "Australia/Sydney";

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
/// ISO8601 (full instant, or a bare date interpreted at Sydney midnight).
fn parse_bound(s: &str) -> Result<Timestamp> {
    let s = s.trim();
    if let Some(ts) = parse_relative(s)? {
        return Ok(ts);
    }
    parse_absolute(s)
}

/// Relative form: an optional-sign-free integer followed by a single unit char.
/// `Ok(None)` when `s` is not in this shape (so the caller falls through to
/// absolute parsing). Resolved against now in Sydney → UTC.
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

    let now = now_sydney()?;
    // "2h" means two hours AGO.
    let then = now
        .checked_sub(span)
        .with_context(|| format!("relative-time underflow computing {s:?} ago"))?;
    Ok(Some(then.timestamp()))
}

/// Absolute ISO8601: a full instant (`2026-06-01T00:00:00Z`) or a bare date
/// (`2026-06-01`, taken at Sydney local midnight).
fn parse_absolute(s: &str) -> Result<Timestamp> {
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    // Bare civil date → Sydney local midnight → UTC.
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        let tz = jiff::tz::TimeZone::get(SYDNEY)
            .with_context(|| "Australia/Sydney timezone unavailable")?;
        let zoned = date
            .to_zoned(tz)
            .with_context(|| format!("cannot place date {s:?} in Australia/Sydney"))?;
        return Ok(zoned.timestamp());
    }
    bail!(
        "cannot parse time bound {s:?}: expected ISO8601 (e.g. 2026-06-01 or \
         2026-06-01T05:00:00Z) or a relative form (e.g. 2h, 3d, 90m)"
    )
}

fn now_sydney() -> Result<Zoned> {
    let tz =
        jiff::tz::TimeZone::get(SYDNEY).with_context(|| "Australia/Sydney timezone unavailable")?;
    Ok(Timestamp::now().to_zoned(tz))
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
    fn bare_date_is_sydney_midnight() {
        // 2026-06-01 Sydney (AEST +10) midnight == 2026-05-31T14:00:00Z.
        let w = TimeWindow::from_args(Some("2026-06-01"), None).unwrap();
        // A record exactly at the Sydney-midnight UTC instant is included.
        assert!(w.contains(Some("2026-05-31T14:00:00Z")));
        // One second before is excluded.
        assert!(!w.contains(Some("2026-05-31T13:59:59Z")));
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
}
