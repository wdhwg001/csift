//! Shared timestamp rendering in the system-local timezone alongside raw UTC.
//!
//! csift auto-detects the machine's local timezone via [`jiff::tz::TimeZone::system`]
//! (powered by jiff's default `tz-system` feature: it reads `$TZ`, then
//! `/etc/localtime` on Unix, the registry on Windows, etc.) and renders every
//! timestamp as `YYYY-MM-DD HH:MM:SS <TZ> (RAW_UTC_ISO8601)`. The `<TZ>` abbrev is
//! whatever the detected zone yields (e.g. `PST`, `CET`, `AEST`), so output
//! auto-labels for the user's actual locale with no hardcoded zone.
//!
//! `list` and `search` both render timestamps; this module is the single place the
//! local-timezone choice lives, so the system-tz behaviour is defined once.

use jiff::tz::TimeZone;

/// The machine's local timezone, auto-detected from the OS (`$TZ` / `/etc/localtime`
/// / platform equivalent). [`TimeZone::system`] is infallible — if detection fails
/// it falls back to a UTC-equivalent zone — so callers never need an error branch.
#[must_use]
pub fn local_tz() -> TimeZone {
    TimeZone::system()
}

/// Render a raw ISO8601 UTC timestamp as `YYYY-MM-DD HH:MM:SS <TZ> (RAW_UTC)` in the
/// system-local timezone. If the timestamp is absent, `—` is shown; if it is present
/// but unparseable, the raw bytes are surfaced rather than dropped (never a panic,
/// never a fabricated time).
#[must_use]
pub fn format_timestamp(raw: Option<&str>) -> String {
    let Some(raw) = raw else {
        return "—".to_string();
    };
    match raw.parse::<jiff::Timestamp>() {
        Ok(ts) => {
            let local = ts.to_zoned(local_tz()).strftime("%Y-%m-%d %H:%M:%S %Z");
            format!("{local} ({raw})")
        }
        // Unparseable timestamp: surface the raw bytes rather than drop them.
        Err(_) => format!("{raw} (unparsed)"),
    }
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

    #[test]
    fn format_timestamp_preserves_raw_and_uses_system_local() {
        let raw = "2026-06-07T05:48:22.880Z";
        let out = format_timestamp(Some(raw));
        // The raw UTC is always preserved verbatim.
        assert!(out.contains(raw), "raw missing: {out}");
        // The local portion equals what the system tz itself yields for this instant.
        let local = expected_local(raw, "%Y-%m-%d %H:%M:%S %Z");
        assert!(out.contains(&local), "expected local {local:?} in {out:?}");
        // Output parses into "<local> (<raw>)".
        assert_eq!(out, format!("{local} ({raw})"));
    }

    #[test]
    fn format_timestamp_missing_is_em_dash() {
        assert_eq!(format_timestamp(None), "—");
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
