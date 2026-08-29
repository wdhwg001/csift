//! The six-verdict join: registry -> tail -> pid, sidecars covering HITL.

use super::*;

/// The closed verdict set. Wire slugs are the kebab forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Running,
    WaitingChildren,
    WaitingHitl,
    IdleEot,
    StaleDead,
    Unknown,
}

impl Verdict {
    #[must_use]
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Verdict::Running => "running",
            Verdict::WaitingChildren => "waiting-children",
            Verdict::WaitingHitl => "waiting-hitl",
            Verdict::IdleEot => "idle-eot",
            Verdict::StaleDead => "stale-dead",
            Verdict::Unknown => "unknown",
        }
    }
}

/// One evidence row: which surface said what, and how old that statement is.
#[derive(Debug, Clone)]
pub(crate) struct Evidence {
    pub(crate) surface: &'static str,
    pub(crate) value: String,
    pub(crate) age_secs: Option<i64>,
}

/// Everything the join produced: the verdict plus every row that fed it.
#[derive(Debug, Clone)]
pub(crate) struct Assessment {
    pub(crate) verdict: Verdict,
    pub(crate) evidence: Vec<Evidence>,
    pub(crate) children: Vec<ChildState>,
    pub(crate) pending: Vec<String>,
    pub(crate) notes: Vec<String>,
}

/// Join the surfaces into one verdict. Precedence (each step names its evidence):
/// dead process > blocked-on-human > tool-in-flight > live children > clean EoT >
/// unknown. Never guesses: a shape the rules cannot rank is `unknown` with the
/// disagreement in the evidence rows.
pub(crate) fn assess(
    registry: Option<&RegistryRow>,
    liveness: Option<&PidLiveness>,
    main_tail: &TailShape,
    children: &ChildrenReport,
    pending_elicitations: &[String],
    is_subagent_target: bool,
) -> Assessment {
    let mut evidence: Vec<Evidence> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if let Some(r) = registry {
        evidence.push(Evidence {
            surface: "registry",
            value: format!(
                "status {} (pid {})",
                r.status.as_deref().unwrap_or("(absent)"),
                r.pid.map_or_else(|| "?".to_string(), |p| p.to_string())
            ),
            age_secs: r
                .status_updated_at_ms
                .map(|ms| (jiff::Timestamp::now().as_millisecond() - ms).max(0) / 1000),
        });
    } else if is_subagent_target {
        notes.push(
            "the registry covers top-level interactive sessions only - a subagent has no \
             row; verdict from tail + children evidence"
                .to_string(),
        );
    } else {
        notes.push(
            "no registry row for this session (not currently registered, or an older \
             Claude Code) - verdict from tail + children evidence"
                .to_string(),
        );
    }
    if let Some(l) = liveness {
        let (v, note): (String, Option<String>) = match l {
            PidLiveness::Alive {
                reuse_guard: ReuseGuard::Checked,
            } => ("alive (start-time guard matched)".to_string(), None),
            PidLiveness::Alive {
                reuse_guard: ReuseGuard::Skipped,
            } => (
                "alive (pid only)".to_string(),
                Some(
                    "the pid-reuse guard was skipped (procStart absent or unparseable on \
                     one side)"
                        .to_string(),
                ),
            ),
            PidLiveness::Dead => ("dead".to_string(), None),
            PidLiveness::Reused => ("pid reused by another process".to_string(), None),
            PidLiveness::Unavailable => (
                "probe unavailable on this host".to_string(),
                Some("pid liveness cannot be checked here - stale-dead is undecidable".to_string()),
            ),
        };
        evidence.push(Evidence {
            surface: "pid",
            value: v,
            age_secs: None,
        });
        if let Some(n) = note {
            notes.push(n);
        }
    }
    match &main_tail.unreturned_use {
        Some((tool, ts)) => evidence.push(Evidence {
            surface: "tail",
            value: format!("unreturned {tool} call"),
            age_secs: age_secs(ts.as_deref()),
        }),
        None => evidence.push(Evidence {
            surface: "tail",
            value: format!(
                "no pending call; last stop_reason {}",
                main_tail.last_stop_reason.as_deref().unwrap_or("(none)")
            ),
            age_secs: age_secs(main_tail.last_ts_utc.as_deref()),
        }),
    }
    if children.live_count > 0 || !children.children.is_empty() {
        evidence.push(Evidence {
            surface: "children",
            value: format!(
                "{} lane(s), {} live{}",
                children.children.len(),
                children.live_count,
                if children.journal_in_flight > 0 {
                    format!(
                        " ({} workflow agent(s) in flight)",
                        children.journal_in_flight
                    )
                } else {
                    String::new()
                }
            ),
            age_secs: None,
        });
    }
    for p in pending_elicitations {
        evidence.push(Evidence {
            surface: "sidecar",
            value: format!("pending elicitation: {p}"),
            age_secs: None,
        });
    }

    // ── The join ──
    let dead = matches!(liveness, Some(PidLiveness::Dead | PidLiveness::Reused));
    let running_shape = main_tail.unreturned_use.is_some()
        || registry
            .and_then(|r| r.status.as_deref())
            .is_some_and(|s| s == "busy" || s == "shell");
    let eot_shape = main_tail.unreturned_use.is_none()
        && main_tail
            .last_stop_reason
            .as_deref()
            .is_some_and(|s| s == "end_turn");

    let verdict = if dead {
        notes.push(if main_tail.unreturned_use.is_some() {
            "the tail shows a call still open: the process died MID-TOOL".to_string()
        } else {
            "the tail is settled: the process ended after its last turn".to_string()
        });
        Verdict::StaleDead
    } else if !pending_elicitations.is_empty() {
        Verdict::WaitingHitl
    } else if running_shape {
        Verdict::Running
    } else if children.live_count > 0 {
        Verdict::WaitingChildren
    } else if eot_shape {
        Verdict::IdleEot
    } else if main_tail.records_seen == 0 {
        notes.push("no records readable at the tail".to_string());
        Verdict::Unknown
    } else {
        notes.push(format!(
            "tail shape unranked: no pending call, last stop_reason {} - not an end_turn, \
             not provably running",
            main_tail.last_stop_reason.as_deref().unwrap_or("(none)")
        ));
        Verdict::Unknown
    };

    // F7 honesty: a pending PERMISSION prompt is invisible without a sidecar and would
    // masquerade exactly as these two verdicts.
    if matches!(verdict, Verdict::IdleEot | Verdict::WaitingChildren) {
        notes.push(
            "a pending permission prompt lives only in Claude Code process memory and is \
             invisible to this instrument - it would masquerade as idle"
                .to_string(),
        );
    }

    Assessment {
        verdict,
        evidence,
        children: children.children.clone(),
        pending: pending_elicitations.to_vec(),
        notes,
    }
}
