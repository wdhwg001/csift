//! Time-window parsing for `search --since`/`--until` (SPEC §6.2).
//!
//! A bound is EITHER an absolute ISO8601 instant/date OR a relative form
//! (`2h`, `3d`, `90m`, `45s`, `1w`, `2mo`, `1y`) interpreted as "that long ago" relative to
//! **now in the system-local timezone** (auto-detected via [`crate::timez::local_tz`]),
//! then converted to UTC for comparison, OR the token `now` (the instant the command
//! started - the lens form `--background-since now` means "only what launches after
//! I started"). A leading `-` on a relative form is tolerated (`-1d` == `1d`: the only
//! direction is "ago"). `mo` = 30 days and `y` = 365 days, calendar-approximate. A record's `timestamp` (raw UTC ISO8601) is
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
    /// INTERSECTS the window - the `list` rule: a session is admitted when ANY part of its
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
/// ISO8601 (full instant; bare datetime = system-local wall clock; bare date =
/// system-local midnight).
pub(crate) fn parse_bound(s: &str) -> Result<Timestamp> {
    let s = s.trim();
    // `now`: the command's own start instant (resolved once, at parse time).
    if s.eq_ignore_ascii_case("now") {
        return Ok(Timestamp::now());
    }
    if let Some(ts) = parse_relative(s)? {
        return Ok(ts);
    }
    parse_absolute(s)
}

/// Relative form: an integer followed by a unit (`s` `m` `h` `d` `w` `mo` `y`), with
/// an optional leading `-` (tolerated: "ago" is the only direction). `Ok(None)` when
/// `s` is not in this shape (so the caller falls through to absolute parsing).
/// Resolved against now in the system-local timezone → UTC.
fn parse_relative(s: &str) -> Result<Option<Timestamp>> {
    let body = s.strip_prefix('-').unwrap_or(s);
    // Digits, then the unit (one or two ASCII letters).
    let digits_end = body.bytes().take_while(u8::is_ascii_digit).count();
    if digits_end == 0 || digits_end == body.len() {
        return Ok(None);
    }
    let (num_part, unit) = body.split_at(digits_end);
    let n: i64 = num_part
        .parse()
        .with_context(|| format!("invalid relative-time quantity in {s:?}"))?;

    let span = match unit {
        "s" => Span::new().try_seconds(n),
        "m" => Span::new().try_minutes(n),
        "h" => Span::new().try_hours(n),
        "d" => Span::new().try_days(n),
        "w" => Span::new().try_weeks(n),
        // Calendar-approximate: a month is 30 days, a year 365 - a cutoff, not a ledger.
        "mo" => n.checked_mul(30).map_or_else(
            || Span::new().try_days(i64::MAX),
            |d| Span::new().try_days(d),
        ),
        "y" => n.checked_mul(365).map_or_else(
            || Span::new().try_days(i64::MAX),
            |d| Span::new().try_days(d),
        ),
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

/// Absolute ISO8601: a full instant (`2026-06-01T05:00:00Z` / `…+10:00`), a bare civil
/// DATETIME (`2026-06-01T05:00:00` - system-LOCAL wall-clock time, the same local
/// convention as a bare date), or a bare date (`2026-06-01`, system-local midnight).
fn parse_absolute(s: &str) -> Result<Timestamp> {
    if let Ok(ts) = s.parse::<Timestamp>() {
        return Ok(ts);
    }
    // Bare civil DATETIME (no offset / `Z`) → system-local wall clock → UTC. This arm MUST
    // precede the Date arm: jiff's civil-Date parser ACCEPTS a full datetime string and
    // keeps only its date part, so with Date tried first a bare "2026-07-13T20:00:00"
    // silently collapsed to local MIDNIGHT - a bounded window that looked exactly like a
    // quiet time period (the R9 silent-wrong-answer bug; the worst failure shape a
    // time-window flag can produce). The offset guard keeps this arm honest: jiff's civil
    // parsers also IGNORE a trailing offset, so a string that CARRIES one but failed the
    // Timestamp parse above is malformed and must fall through to the bail, never be
    // re-read as local wall-clock time.
    if !has_offset_indicator(s) {
        if let Ok(dt) = s.parse::<jiff::civil::DateTime>() {
            let zoned = dt.to_zoned(local_tz()).with_context(|| {
                format!("cannot place datetime {s:?} in the system-local timezone")
            })?;
            return Ok(zoned.timestamp());
        }
    }
    // Bare civil date → system-local midnight → UTC.
    if let Ok(date) = s.parse::<jiff::civil::Date>() {
        let zoned = date
            .to_zoned(local_tz())
            .with_context(|| format!("cannot place date {s:?} in the system-local timezone"))?;
        return Ok(zoned.timestamp());
    }
    bail!(
        "cannot parse time bound {s:?}: expected ISO8601 — `2026-06-01` (bare date = \
         system-local midnight), `2026-06-01T05:00:00` (bare datetime = system-LOCAL \
         wall-clock time), `2026-06-01T05:00:00Z` / `…+10:00` (explicit zone) — or a \
         relative form (e.g. 2h, 3d, 90m, 2mo, 1y; a leading `-` is tolerated) — or `now`"
    )
}

/// True when the TIME part of an ISO8601-ish string carries a zone indicator (`Z`/`z`, `+`,
/// or a `-` AFTER the date/time separator - the date part's own dashes don't count).
fn has_offset_indicator(s: &str) -> bool {
    match s.find(['T', 't', ' ']) {
        Some(sep) => s[sep + 1..].contains(['Z', 'z', '+', '-']),
        None => false,
    }
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
    fn bare_datetime_is_local_wall_clock_not_midnight_collapse() {
        // R9: jiff's civil-Date parser accepts a full datetime string (keeping only the
        // date), so a bare datetime used to collapse silently to local midnight. The
        // DateTime arm must yield date-midnight + the stated time-of-day, in the SAME
        // local zone - assert the delta, which is TZ-independent (no DST transition on
        // 2026-06-01 in any mainstream zone between 00:00 and 05:00).
        let midnight = parse_bound("2026-06-01").unwrap();
        let five_am = parse_bound("2026-06-01T05:00:00").unwrap();
        let delta = five_am.as_second() - midnight.as_second();
        assert_eq!(
            delta,
            5 * 3600,
            "time-of-day must be honored, not discarded"
        );
        // And two different times of day must differ (the collapse made them equal).
        let eight_pm = parse_bound("2026-06-01T20:00:00").unwrap();
        assert_ne!(five_am, eight_pm);
    }

    #[test]
    fn malformed_offset_still_fails_loud() {
        // A string CARRYING a zone indicator that Timestamp rejects must bail, never be
        // silently re-read as local wall-clock (jiff's civil parsers ignore offsets).
        assert!(parse_bound("2026-06-01T05:00:00+99:00").is_err());
        assert!(parse_bound("2026-13-99T99:99:99").is_err());
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
        // from jiff for the SYSTEM zone (tz-agnostic - holds on any machine / CI),
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
        // a [since=1h-ago, ∞) window only if it's BEFORE the bound - future is in.
        let w = TimeWindow::from_args(Some("1h"), None).unwrap();
        assert!(!w.is_unbounded());
        // A timestamp from the distant past is before "1 hour ago" → excluded.
        assert!(!w.contains(Some("2000-01-01T00:00:00Z")));
        // A timestamp far in the future is after "1 hour ago" → included.
        assert!(w.contains(Some("2999-01-01T00:00:00Z")));
    }

    #[test]
    fn relative_units_all_parse() {
        for u in ["1s", "30m", "2h", "3d", "1w", "2mo", "1y", "-1d", "-30m"] {
            assert!(
                TimeWindow::from_args(Some(u), None).is_ok(),
                "unit failed: {u}"
            );
        }
    }

    #[test]
    fn month_and_year_are_calendar_approximate_and_ordered() {
        // 2mo = 60 days and 1y = 365 days ago: each bound sits where the equivalent
        // day count sits (to the second), and the sign is always "ago".
        let mo = parse_bound("2mo").unwrap();
        let d60 = parse_bound("60d").unwrap();
        assert!(
            (mo.as_second() - d60.as_second()).abs() <= 1,
            "{mo} vs {d60}"
        );
        let y = parse_bound("1y").unwrap();
        let d365 = parse_bound("365d").unwrap();
        assert!(
            (y.as_second() - d365.as_second()).abs() <= 1,
            "{y} vs {d365}"
        );
        assert!(y < mo && mo < parse_bound("1d").unwrap());
        // `5m` stays MINUTES (the single-letter unit wins over any prefix of `mo`).
        let m5 = parse_bound("5m").unwrap();
        assert!(m5.as_second() - parse_bound("300s").unwrap().as_second() <= 1);
    }

    #[test]
    fn leading_minus_is_tolerated_and_now_is_the_start_instant() {
        let plain = parse_bound("1d").unwrap();
        let minus = parse_bound("-1d").unwrap();
        assert!((plain.as_second() - minus.as_second()).abs() <= 1);
        let before = Timestamp::now();
        let now = parse_bound("now").unwrap();
        let after = Timestamp::now();
        assert!(before <= now && now <= after);
        assert!(parse_bound("NOW").is_ok());
        // A bare minus, or a minus with no unit, is not a relative form.
        assert!(TimeWindow::from_args(Some("-"), None).is_err());
        assert!(TimeWindow::from_args(Some("-5"), None).is_err());
        // Overflowing the month/year multiplier still errors instead of wrapping.
        assert!(TimeWindow::from_args(Some(&format!("{}y", i64::MAX)), None).is_err());
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
        // `5q` - digits + an unrecognized unit → parse_relative returns None
        // (the `_ => return Ok(None)` arm) and absolute parsing then fails. So does a
        // three-letter unit.
        assert!(TimeWindow::from_args(Some("5q"), None).is_err());
        assert!(TimeWindow::from_args(Some("5mon"), None).is_err());
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
