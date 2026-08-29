//! The harness session registry: `<claude-home>/sessions/<pid>.json`.
//!
//! Field shape (verified on disk): `{pid, sessionId, cwd, startedAt(ms), procStart(str),
//! version, kind:"interactive", status, updatedAt, statusUpdatedAt, ...}`. `status`
//! values observed on disk: `idle` / `busy` / `shell` (the binary also carries `blocked`
//! and `waiting`). TRANSITION-writes only - never a heartbeat. Coverage: TOP-LEVEL
//! interactive sessions only; a subagent has no row anywhere and csift never fabricates
//! one.
//!
//! `procStart` renders in UTC (`Sun Aug 16 09:04:23 2026`) while `ps lstart` renders in
//! the LOCAL zone - a naive string/local comparison flags pid reuse on EVERY row. Parse
//! the registry value as UTC, the `ps` value as local, and compare instants (with a small
//! tolerance); when either side is absent or unparseable, degrade to a pid-only probe AND
//! say so in the evidence (the reuse guard was skipped, honest, never silent).

use super::*;

/// One registry row (only the liveness-relevant fields; the rest is tolerated + ignored).
#[derive(Debug, Clone)]
pub(crate) struct RegistryRow {
    pub(crate) pid: Option<u32>,
    pub(crate) status: Option<String>,
    /// Millisecond epoch of the last status TRANSITION (not a heartbeat).
    pub(crate) status_updated_at_ms: Option<i64>,
    /// The raw `procStart` string (UTC-rendered by the harness).
    pub(crate) proc_start: Option<String>,
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
        return Ok(Some(RegistryRow {
            pid: v
                .get("pid")
                .and_then(serde_json::Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            status: v
                .get("status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            status_updated_at_ms: v.get("statusUpdatedAt").and_then(serde_json::Value::as_i64),
            proc_start: v
                .get("procStart")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
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
    /// The probe is unavailable on this host (non-unix): the verdict must say so.
    /// (Constructed only under cfg(not(unix)); the allow keeps the unix build honest
    /// without a crate-wide dead_code blanket.)
    #[cfg_attr(unix, allow(dead_code))]
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReuseGuard {
    Checked,
    /// `procStart` absent/unparseable (either side): pid-only liveness, disclosed.
    Skipped,
}

/// Probe pid liveness WITHOUT signaling (and without a second `unsafe` site - the
/// crate's single-allow law outranks the raw-syscall form): one `ps -p PID -o lstart=`
/// answers both questions - a failing/empty ps = no such process; a start time within
/// tolerance of the registry's UTC `procStart` = the same process (reuse guarded).
pub(crate) fn probe_pid(pid: u32, proc_start_utc: Option<&str>) -> PidLiveness {
    #[cfg(unix)]
    {
        match ps_probe(pid) {
            PsProbe::NoProcess => PidLiveness::Dead,
            PsProbe::Alive(actual) => {
                match (proc_start_utc.and_then(parse_registry_proc_start), actual) {
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
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, proc_start_utc);
        PidLiveness::Unavailable
    }
}

/// The single-ps outcome: process absent, or alive with a (best-effort) start instant.
#[cfg(unix)]
pub(crate) enum PsProbe {
    NoProcess,
    Alive(Option<jiff::Timestamp>),
}

/// Parse the registry's `procStart` (asctime-like, UTC-rendered: `Sun Aug 16 09:04:23
/// 2026`). `None` on any mismatch - the caller degrades to pid-only + a note.
pub(crate) fn parse_registry_proc_start(s: &str) -> Option<jiff::Timestamp> {
    let bd = jiff::fmt::strtime::parse("%a %b %e %H:%M:%S %Y", s.trim()).ok()?;
    let dt = bd.to_datetime().ok()?;
    dt.to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// One `ps -p PID -o lstart=` call: a failing/empty result = no such process; success
/// yields the start instant when the LOCAL-rendered format parses (two observed field
/// orders tried), else `Alive(None)` - the caller then skips the reuse guard AND says so.
#[cfg(unix)]
pub(crate) fn ps_probe(pid: u32) -> PsProbe {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()
    else {
        return PsProbe::Alive(None); // ps itself unavailable: never claim dead on that
    };
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || text.is_empty() {
        return PsProbe::NoProcess;
    }
    let local = crate::timez::local_tz();
    for fmt in ["%a %b %e %H:%M:%S %Y", "%a %e %b %H:%M:%S %Y"] {
        if let Ok(bd) = jiff::fmt::strtime::parse(fmt, &text) {
            if let Ok(dt) = bd.to_datetime() {
                if let Ok(z) = dt.to_zoned(local.clone()) {
                    return PsProbe::Alive(Some(z.timestamp()));
                }
            }
        }
    }
    PsProbe::Alive(None)
}
