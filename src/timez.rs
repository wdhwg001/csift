//! Shared timestamp rendering in the system-local timezone - ONE canonical text form.
//!
//! csift auto-detects the machine's local timezone via [`jiff::tz::TimeZone::system`]
//! (powered by jiff's default `tz-system` feature: it reads `$TZ`, then
//! `/etc/localtime` on Unix, the registry on Windows, etc.) and renders EVERY text
//! timestamp as `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)` - e.g.
//! `2026-07-11 15:33:37 AEST(UTC+10)`. The marker is a FORMAT, not a value: both the
//! abbreviation and the offset derive from the system zone AT THAT INSTANT
//! (DST-correct - a January instant in Sydney renders `AEDT(UTC+11)`, a July one
//! `AEST(UTC+10)`; an Indian machine renders `IST(UTC+05:30)`), never hardcoded.
//!
//! Design intent (v0.5): an LLM reader gets the zone name AND its offset together, so
//! the only mental step left is "shift by the given offset" - never "recall what
//! offset this zone name maps to". The former dual form (`… AEST (2026-…Z)`) invited
//! exactly the UTC-conversion arithmetic LLMs get wrong; the raw UTC lives in JSON
//! (`ts_utc`) and in `--raw` bytes, never in text.
//!
//! This module is the single place the local-timezone choice lives.

use jiff::tz::TimeZone;

/// The machine's local timezone, auto-detected from the OS (`$TZ` / `/etc/localtime`
/// / platform equivalent). [`TimeZone::system`] is infallible - if detection fails
/// it falls back to a UTC-equivalent zone - so callers never need an error branch.
#[must_use]
pub fn local_tz() -> TimeZone {
    TimeZone::system()
}

/// The canonical timezone marker for one zoned instant - `<TZAB>(UTC±offset)`.
/// Whole-hour offsets render compact (`UTC+10`, `UTC-7`); fractional offsets carry
/// zero-padded minutes (`UTC+05:30`, `UTC+09:30`). A zone with no usable abbreviation
/// (jiff yields a bare numeric like `+10:00`) degrades to `(UTC±offset)` alone.
#[must_use]
pub(crate) fn tz_marker(zoned: &jiff::Zoned) -> String {
    let secs = zoned.offset().seconds();
    let sign = if secs < 0 { '-' } else { '+' };
    let abs = secs.unsigned_abs();
    let (h, m) = (abs / 3600, (abs % 3600) / 60);
    let off = if m == 0 {
        format!("UTC{sign}{h}")
    } else {
        format!("UTC{sign}{h:02}:{m:02}")
    };
    let ab = zoned.strftime("%Z").to_string();
    if ab.is_empty() || ab.starts_with('+') || ab.starts_with('-') {
        format!("({off})")
    } else {
        format!("{ab}({off})")
    }
}

/// The shared canonical renderer: `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)`.
/// Absent → `-`; present but unparseable → the raw bytes surfaced with `(unparsed)`
/// (never a panic, never a fabricated time, never a silent drop).
fn render_local(raw: Option<&str>, millis: bool) -> String {
    let Some(raw) = raw else {
        return "—".to_string();
    };
    match raw.parse::<jiff::Timestamp>() {
        Ok(ts) => {
            let z = ts.to_zoned(local_tz());
            let base = if millis {
                z.strftime("%Y-%m-%d %H:%M:%S.%3f").to_string()
            } else {
                z.strftime("%Y-%m-%d %H:%M:%S").to_string()
            };
            format!("{base} {}", tz_marker(&z))
        }
        Err(_) => format!("{raw} (unparsed)"),
    }
}

/// Render a raw ISO8601 UTC timestamp in the canonical local form, second precision:
/// `2026-07-11 15:33:37 AEST(UTC+10)`. (The pre-v0.5 `… <TZ> (<raw UTC>)` dual form is
/// gone - the UTC copy invited LLM conversion errors; machine consumers read JSON
/// `ts_utc`.)
#[must_use]
pub fn format_timestamp(raw: Option<&str>) -> String {
    render_local(raw, false)
}

/// Render a raw ISO8601 UTC timestamp in the canonical local form with milliseconds:
/// `2026-07-11 15:33:37.442 AEST(UTC+10)` - the ordering-precision variant `search`/
/// `show` headers use. Same marker, same rules.
#[must_use]
pub fn format_local_compact(raw: Option<&str>) -> String {
    render_local(raw, true)
}

/// System-local time as an ISO8601-with-offset string (for JSON `ts_local`), or
/// `None` if the raw UTC is missing/unparseable.
#[must_use]
pub fn local_iso(raw: &str) -> Option<String> {
    let ts = raw.parse::<jiff::Timestamp>().ok()?;
    Some(
        ts.to_zoned(local_tz())
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The expected system-local rendering of an instant, derived from jiff in-test
    /// (NOT hardcoded to any one zone) so the assertions hold on any machine / CI.
    fn expected_local(raw: &str, fmt: &str) -> String {
        let ts: jiff::Timestamp = raw.parse().expect("parseable test instant");
        ts.to_zoned(local_tz()).strftime(fmt).to_string()
    }

    fn expected_marker(raw: &str) -> String {
        let ts: jiff::Timestamp = raw.parse().expect("parseable test instant");
        tz_marker(&ts.to_zoned(local_tz()))
    }

    #[test]
    fn format_timestamp_is_canonical_local_with_marker_and_no_utc_copy() {
        let raw = "2026-06-07T05:48:22.880Z";
        let out = format_timestamp(Some(raw));
        let local = expected_local(raw, "%Y-%m-%d %H:%M:%S");
        assert_eq!(out, format!("{local} {}", expected_marker(raw)));
        // The marker carries the offset inline; the raw UTC copy is GONE.
        assert!(out.contains("(UTC"), "marker missing: {out}");
        assert!(!out.contains(raw), "must not echo the raw UTC: {out}");
    }

    #[test]
    fn format_timestamp_missing_is_em_dash() {
        assert_eq!(format_timestamp(None), "—");
    }

    #[test]
    fn format_local_compact_adds_millis_same_marker() {
        let raw = "2026-06-07T05:48:22.880Z";
        let out = format_local_compact(Some(raw));
        let local = expected_local(raw, "%Y-%m-%d %H:%M:%S.%3f");
        assert_eq!(out, format!("{local} {}", expected_marker(raw)));
        assert!(out.contains(".880") || out.contains('.'), "millis: {out}");
        assert!(!out.contains(raw), "must not echo the raw UTC: {out}");
    }

    #[test]
    fn tz_marker_offset_forms() {
        use jiff::tz::TimeZone;
        let ts: jiff::Timestamp = "2026-07-11T00:00:00Z".parse().unwrap();
        // Whole-hour offset: compact form, name(offset).
        let syd = TimeZone::get("Australia/Sydney").unwrap();
        assert_eq!(tz_marker(&ts.to_zoned(syd.clone())), "AEST(UTC+10)");
        // DST flips BOTH halves by instant (January in Sydney = AEDT, UTC+11).
        let jan: jiff::Timestamp = "2026-01-11T00:00:00Z".parse().unwrap();
        assert_eq!(tz_marker(&jan.to_zoned(syd)), "AEDT(UTC+11)");
        // Fractional offset: zero-padded minutes.
        let ist = TimeZone::get("Asia/Kolkata").unwrap();
        assert_eq!(tz_marker(&ts.to_zoned(ist)), "IST(UTC+05:30)");
        // Negative whole-hour.
        let den = TimeZone::get("America/Denver").unwrap();
        assert_eq!(tz_marker(&ts.to_zoned(den)), "MDT(UTC-6)");
    }

    #[test]
    fn format_local_compact_missing_and_unparseable() {
        assert_eq!(format_local_compact(None), "—");
        let out = format_local_compact(Some("not-a-time"));
        assert!(
            out.contains("not-a-time") && out.contains("unparsed"),
            "{out}"
        );
    }

    #[test]
    fn format_timestamp_unparseable_surfaces_raw() {
        let out = format_timestamp(Some("not-a-time"));
        assert!(out.contains("not-a-time"));
        assert!(out.contains("unparsed"));
    }

    #[test]
    fn local_iso_matches_system_tz_offset() {
        let raw = "2026-06-07T05:48:22.880Z";
        let out = local_iso(raw).expect("local iso");
        // Derive the expected offset string from jiff (tz-agnostic), not a literal.
        let expected = expected_local(raw, "%Y-%m-%dT%H:%M:%S%:z");
        assert_eq!(out, expected);
        // It must carry an offset (`+HH:MM` / `-HH:MM` / `+00:00`), never bare.
        assert!(
            out.contains('+') || out.contains('-'),
            "offset missing: {out}"
        );
    }

    #[test]
    fn local_iso_none_for_unparseable() {
        assert!(local_iso("garbage").is_none());
    }

    #[test]
    fn format_timestamp_none_and_some_both_exercised_here() {
        // Pin BOTH arms of `format_timestamp`'s `let Some(raw) = raw else` in this
        // (the bin-test) binary: the None arm (→ em dash) and a Some arm (→ rendered).
        assert_eq!(format_timestamp(None), "—");
        let some = format_timestamp(Some("2026-06-07T05:00:00Z"));
        assert!(some.contains("2026-06-07"));
    }
}
