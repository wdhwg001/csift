//! The harness session registry: `<claude-home>/sessions/<pid>.json`.
//!
//! Field shape (verified on disk, CC 2.1.258): `{pid, sessionId, cwd, startedAt(ms),
//! procStart(str), version, kind:"interactive", entrypoint:"cli"|"sdk-cli", pidDomain,
//! status, updatedAt, statusUpdatedAt, bridgeSessionId?, tmux?, ...}`. `status` is the
//! binary's closed set `busy | shell | idle | waiting`: `waiting` whenever the session is
//! blocked on a dialog (a question, a permission prompt, a plan approval, a sandbox or
//! worker request), `busy` while loading or delegating, else `idle`. A print-mode
//! (`claude -p`) session writes a row too, with `status: null` that never transitions.
//! TRANSITION-writes only - never a heartbeat. Coverage: TOP-LEVEL sessions only; a
//! subagent has no row anywhere and csift never fabricates one.
//!
//! `procStart` is the OWNER PROCESS's creation instant in a PLATFORM-SPECIFIC rendering:
//! on unix an asctime string in UTC (`Sun Aug 16 09:04:23 2026`), on Windows a FILETIME
//! integer (100ns ticks since 1601-01-01, e.g. `134328101803820142`). `pidDomain` names
//! the pid space the row was written in (`darwin`, `linux`, or `win32:<hostname>`); a row
//! from another domain cannot be probed here and the verdict says so. `ps lstart` renders
//! under the caller's locale and zone (the harness pins `LC_ALL=C` and `TZ=UTC` for its
//! own acquisition, and so does csift's probe - an inherited locale changes the field
//! order and the weekday and month names, an inherited zone shifts the clock). Parse
//! both sides to UTC instants and compare with a small tolerance; when either side is
//! absent or unparseable, degrade to a pid-only probe AND say so in the evidence (the
//! reuse guard was skipped, honest, never silent).

use super::*;

/// One registry row (only the liveness-relevant fields; the rest is tolerated + ignored).
#[derive(Debug, Clone)]
pub(crate) struct RegistryRow {
    pub(crate) pid: Option<u32>,
    pub(crate) status: Option<String>,
    /// Millisecond epoch of the last status TRANSITION (not a heartbeat).
    pub(crate) status_updated_at_ms: Option<i64>,
    /// The raw `procStart` string (asctime UTC on unix, a FILETIME integer on Windows).
    pub(crate) proc_start: Option<String>,
    /// The raw `pidDomain` (`darwin` | `linux` | `win32:<host>`); absent on older rows.
    pub(crate) pid_domain: Option<String>,
}

/// Scan the registry dir for the row whose `sessionId` matches. `Ok(None)` when the dir
/// is absent or no row matches (a subagent target, a non-interactive session, an old CC).
pub(crate) fn registry_row_for(session_id: &str) -> Result<Option<RegistryRow>> {
    let dir = crate::path::claude_home()?.join("sessions");
    if !dir.is_dir() {
        return Ok(None);
    }
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&p) else {
            continue; // a mid-write or unreadable row is not this session's problem
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if v.get("sessionId").and_then(serde_json::Value::as_str) != Some(session_id) {
            continue;
        }
        let str_field = |k: &str| {
            v.get(k)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        };
        return Ok(Some(RegistryRow {
            pid: v
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            status: str_field("status"),
            status_updated_at_ms: v.get("statusUpdatedAt").and_then(serde_json::Value::as_i64),
            // The ROW writer stamps `procStart` unconditionally on every platform (the
            // Windows session measured live carried the FILETIME under that key); the
            // two-key `procStartFt` schema belongs to the `<pid>.<hash>.key` companion,
            // not to the row. Reading `procStartFt` first is a harmless superset of
            // what the row writer emits, and both renderings parse (ledger WIN-007).
            proc_start: str_field("procStartFt").or_else(|| str_field("procStart")),
            pid_domain: str_field("pidDomain"),
        }));
    }
    Ok(None)
}

/// The owner-pid probe's verdict, with the reuse-guard state named explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PidLiveness {
    /// Process alive AND the start-time guard matched (or the caller chose to trust
    /// pid-only; the flag says which).
    Alive { reuse_guard: ReuseGuard },
    /// No process with that pid.
    Dead,
    /// Pid alive but its start time mismatches the registry's `procStart`: the pid was
    /// REUSED by another process - the session is dead.
    Reused,
    /// The row was written in another pid domain (`pidDomain` names it): the pid means
    /// nothing here and the verdict must say so.
    ForeignDomain(String),
    /// No process probe exists on this host (neither `ps`, `/proc`, PowerShell nor
    /// `tasklist` answered): the verdict must say so.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReuseGuard {
    Checked,
    /// `procStart` absent/unparseable (either side): pid-only liveness, disclosed.
    Skipped,
}

/// The pid domain this csift runs in, in the registry's own vocabulary (the prefix
/// before any `:<hostname>` suffix).
#[must_use]
pub(crate) fn local_pid_domain() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(windows) {
        "win32"
    } else {
        "unknown"
    }
}

/// Probe pid liveness WITHOUT signaling (and without a second `unsafe` site - the
/// crate's single-allow law outranks the raw-syscall form): one process query answers
/// both questions - a failing/empty query = no such process; a start time within
/// tolerance of the registry's `procStart` = the same process (reuse guarded). A row
/// from another pid domain is never probed (its pid belongs to another machine or OS).
pub(crate) fn probe_pid(
    pid: u32,
    proc_start: Option<&str>,
    pid_domain: Option<&str>,
) -> PidLiveness {
    if let Some(d) = pid_domain {
        let head = d.split(':').next().unwrap_or(d);
        if head != local_pid_domain() {
            return PidLiveness::ForeignDomain(d.to_string());
        }
    }
    match ps_probe(pid) {
        PsProbe::Unavailable => PidLiveness::Unavailable,
        PsProbe::NoProcess => PidLiveness::Dead,
        PsProbe::Alive(actual) => match (proc_start.and_then(parse_registry_proc_start), actual) {
            (Some(reg), Some(act)) => {
                if (reg.as_second() - act.as_second()).abs() <= 2 {
                    PidLiveness::Alive {
                        reuse_guard: ReuseGuard::Checked,
                    }
                } else {
                    PidLiveness::Reused
                }
            }
            _ => PidLiveness::Alive {
                reuse_guard: ReuseGuard::Skipped,
            },
        },
    }
}

/// The single-query outcome: process absent, alive with a (best-effort) start instant,
/// or no probe tool on this host at all.
pub(crate) enum PsProbe {
    NoProcess,
    Alive(Option<jiff::Timestamp>),
    /// Constructed only where no probe tool exists (never on unix, where `ps` failing
    /// falls through to `/proc`); the allow keeps the unix build honest without a
    /// crate-wide dead_code blanket.
    #[cfg_attr(unix, allow(dead_code))]
    Unavailable,
}

/// Seconds between the FILETIME epoch (1601-01-01) and the unix epoch.
const FILETIME_UNIX_OFFSET_SECS: i64 = 11_644_473_600;

/// Parse the registry's `procStart`: an asctime-like UTC string (`Sun Aug 16 09:04:23
/// 2026`, unix) or a FILETIME integer (`134328101803820142`, Windows: 100ns ticks since
/// 1601). `None` on any mismatch - the caller degrades to pid-only + a note.
pub(crate) fn parse_registry_proc_start(s: &str) -> Option<jiff::Timestamp> {
    let s = s.trim();
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
        return filetime_to_timestamp(s.parse::<u64>().ok()?);
    }
    let bd = jiff::fmt::strtime::parse("%a %b %e %H:%M:%S %Y", s).ok()?;
    let dt = bd.to_datetime().ok()?;
    dt.to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// A Windows FILETIME (100ns ticks since 1601-01-01 UTC) as an instant; `None` when the
/// value cannot be a real creation time (before the unix epoch or absurdly far out).
pub(crate) fn filetime_to_timestamp(ticks: u64) -> Option<jiff::Timestamp> {
    let secs = i64::try_from(ticks / 10_000_000).ok()? - FILETIME_UNIX_OFFSET_SECS;
    if secs < 0 {
        return None;
    }
    let nanos = i32::try_from((ticks % 10_000_000) * 100).ok()?;
    jiff::Timestamp::new(secs, nanos).ok()
}

/// One `ps -p PID -o lstart=` call: a failing/empty result = no such process; success
/// yields the start instant when the rendering parses, else `Alive(None)` - the caller
/// then skips the reuse guard AND says so. The call PINS `LC_ALL=C` and `TZ=UTC`,
/// exactly as the harness pins its own `procStart` acquisition: `lstart` is the
/// platform utility's `%c` under the caller's `LC_TIME`, so an inherited locale
/// renders a different field order and non-English names (de_DE gives `Fr  4 Sep`,
/// fr_FR `Ven  4 sep`), and the guard was silently skipped on every such host
/// (v0.10.5, ledger MISC-024). Under `C`/`UTC` the rendering is the asctime form in
/// UTC, the same clock the registry's `procStart` is written in; the second field
/// order is kept as a belt for a `C` locale that still renders day-first.
#[cfg(unix)]
pub(crate) fn ps_probe(pid: u32) -> PsProbe {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
    else {
        return PsProbe::Alive(None); // ps itself unavailable: never claim dead on that
    };
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || text.is_empty() {
        // busybox ps (Alpine and friends) rejects `-p`/`lstart` outright, so the probe
        // fails for a LIVE pid too. On Linux `/proc/<pid>` answers liveness directly:
        // present = alive with the start time unknown (the reuse-guard skip is
        // disclosed); absent (or no /proc at all, as on macOS where the ps form is
        // reliable) = the no-such-process verdict stands.
        if std::path::Path::new(&format!("/proc/{pid}")).is_dir() {
            return PsProbe::Alive(None);
        }
        return PsProbe::NoProcess;
    }
    for fmt in ["%a %b %e %H:%M:%S %Y", "%a %e %b %H:%M:%S %Y"] {
        if let Ok(bd) = jiff::fmt::strtime::parse(fmt, &text) {
            if let Ok(dt) = bd.to_datetime() {
                if let Ok(z) = dt.to_zoned(jiff::tz::TimeZone::UTC) {
                    return PsProbe::Alive(Some(z.timestamp()));
                }
            }
        }
    }
    PsProbe::Alive(None)
}

/// Windows: one PowerShell call answers both questions - `Get-Process -Id` fails (exit
/// 3 by our script) for a missing pid, and `StartTime.ToFileTimeUtc()` yields the
/// creation FILETIME (the registry's own rendering) when the process is ours to inspect;
/// `NOSTART` when the start time is not readable (another user's process) = alive, guard
/// skipped. When PowerShell cannot be spawned, `tasklist` answers liveness alone; when
/// neither tool exists the probe is unavailable and the verdict says so.
#[cfg(windows)]
pub(crate) fn ps_probe(pid: u32) -> PsProbe {
    let script = format!(
        "$ErrorActionPreference='Stop'; try {{ $p = Get-Process -Id {pid} }} catch {{ exit 3 }}; \
         try {{ [Console]::Out.Write($p.StartTime.ToFileTimeUtc()) }} catch {{ [Console]::Out.Write('NOSTART') }}"
    );
    if let Ok(out) = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
    {
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.code() == Some(3) {
            return PsProbe::NoProcess;
        }
        if out.status.success() {
            if text.bytes().all(|b| b.is_ascii_digit()) && !text.is_empty() {
                return PsProbe::Alive(text.parse::<u64>().ok().and_then(filetime_to_timestamp));
            }
            return PsProbe::Alive(None);
        }
    }
    // PowerShell missing or broken: tasklist answers liveness (no start time).
    let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
    else {
        return PsProbe::Unavailable;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    if text.lines().any(|l| l.trim_start().starts_with('"')) {
        PsProbe::Alive(None)
    } else {
        PsProbe::NoProcess
    }
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn ps_probe(_pid: u32) -> PsProbe {
    PsProbe::Unavailable
}
