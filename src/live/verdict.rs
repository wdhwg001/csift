//! The six-verdict join: registry -> tail -> pid, sidecars covering HITL.

use super::*;

/// The closed verdict set. Wire slugs are the kebab forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Running,
    WaitingChildren,
    WaitingHitl,
    /// The turn ended (a clean end_turn) but N background task(s) the lens counts have
    /// not returned - neither running nor stopped: by design (a dev server, a watcher)
    /// or not, csift cannot tell. Never satisfies `--until stop`.
    IdleBackgroundOpen,
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
            Verdict::IdleBackgroundOpen => "idle-background-open",
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
    /// The harness task list (attached by `assess_path`; empty default in the pure join).
    pub(crate) tasks: TasksReport,
    /// Background launches and their fate (the lens already applied).
    pub(crate) background: BackgroundReport,
    /// The newest prompt + assistant message (attached by `assess_path`).
    pub(crate) last: LastMessages,
    /// What the main lane is doing at this instant, in words (`in a Bash call for
    /// 34s` / `generating (last record 12s ago)` / `idle (last stop_reason end_turn, 5m
    /// 2s ago)`).
    pub(crate) tail_state: String,
}

/// The six-surface join without a background report (test-only convenience; production
/// goes through [`assess_full`]).
#[cfg(test)]
pub(crate) fn assess(
    registry: Option<&RegistryRow>,
    liveness: Option<&PidLiveness>,
    main_tail: &TailShape,
    children: &ChildrenReport,
    pending_elicitations: &[String],
    is_subagent_target: bool,
) -> Assessment {
    assess_full(
        registry,
        liveness,
        main_tail,
        children,
        pending_elicitations,
        is_subagent_target,
        &BackgroundReport::default(),
    )
}

/// Join the surfaces into one verdict. Precedence (each step names its evidence):
/// dead process > blocked-on-human > tool-in-flight > live children > open background
/// task under the lens > clean EoT > unknown. Never guesses: a shape the rules cannot
/// rank is `unknown` with the disagreement in the evidence rows.
pub(crate) fn assess_full(
    registry: Option<&RegistryRow>,
    liveness: Option<&PidLiveness>,
    main_tail: &TailShape,
    children: &ChildrenReport,
    pending_elicitations: &[String],
    is_subagent_target: bool,
    background: &BackgroundReport,
) -> Assessment {
    let (evidence, mut notes) = collect_evidence(
        registry,
        liveness,
        main_tail,
        children,
        pending_elicitations,
        is_subagent_target,
        background,
    );
    let verdict = rank_verdict(
        registry,
        liveness,
        main_tail,
        children,
        pending_elicitations,
        background,
        &mut notes,
    );

    // F7 honesty: a pending PERMISSION prompt leaves no transcript trace and would
    // masquerade exactly as these verdicts; only a CURRENT registry row (status
    // `waiting`) can show one, and that row is transition-written, never a heartbeat.
    if matches!(
        verdict,
        Verdict::IdleEot | Verdict::WaitingChildren | Verdict::IdleBackgroundOpen
    ) {
        notes.push(match registry.and_then(|r| r.status.as_deref()) {
            Some(s) => format!(
                "a pending permission prompt leaves no transcript trace - it would \
                 masquerade as idle; the registry row (status {s}) would read `waiting` \
                 while one is up, and that row is transition-written, so trust it only \
                 while fresh"
            ),
            None => "a pending permission prompt lives only in Claude Code process memory \
                     and is invisible to this instrument - it would masquerade as idle"
                .to_string(),
        });
    }

    Assessment {
        verdict,
        evidence,
        children: children.children.clone(),
        pending: pending_elicitations.to_vec(),
        notes,
        tasks: TasksReport::default(),
        background: background.clone(),
        last: LastMessages::default(),
        tail_state: tail_state_words(main_tail),
    }
}

/// Every surface's evidence row (plus its degradation note), in display order.
fn collect_evidence(
    registry: Option<&RegistryRow>,
    liveness: Option<&PidLiveness>,
    main_tail: &TailShape,
    children: &ChildrenReport,
    pending_elicitations: &[String],
    is_subagent_target: bool,
    background: &BackgroundReport,
) -> (Vec<Evidence>, Vec<String>) {
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
            PidLiveness::ForeignDomain(d) => (
                format!("row from another pid domain ({d})"),
                Some(
                    "the registry row was written in another pid domain (another machine \
                     or OS): its pid means nothing here - stale-dead is undecidable"
                        .to_string(),
                ),
            ),
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
    if !background.tasks.is_empty() {
        evidence.push(Evidence {
            surface: "background",
            value: background.summary_line(),
            age_secs: None,
        });
    }

    (evidence, notes)
}

/// The join: dead process > blocked-on-human > tool-in-flight > live children > open
/// background task under the lens > clean EoT > unknown, each step leaving its note.
fn rank_verdict(
    registry: Option<&RegistryRow>,
    liveness: Option<&PidLiveness>,
    main_tail: &TailShape,
    children: &ChildrenReport,
    pending_elicitations: &[String],
    background: &BackgroundReport,
    notes: &mut Vec<String>,
) -> Verdict {
    let dead = matches!(liveness, Some(PidLiveness::Dead | PidLiveness::Reused));
    // Two HITL legs beside the sidecar: the registry's `waiting` status (the binary sets
    // it whenever a dialog blocks the session: a question, a permission prompt, a plan
    // approval, a sandbox/worker request), and an unreturned AskUserQuestion/ExitPlanMode
    // at the tail. Whether a pending dialog's record reaches disk is a TIMING outcome of
    // the persistence frontier (the binary has no question-count branch and the frontier
    // is unchanged across builds): measured live, a multi-question ask was on disk while
    // its dialog was open and a single-question ask was not, so the sidecar stays the
    // instrument for the shape that stays buffered (ledger ELI-009).
    let registry_waiting = registry
        .and_then(|r| r.status.as_deref())
        .is_some_and(|s| s == "waiting");
    let tail_dialog = main_tail
        .unreturned_use
        .as_ref()
        .is_some_and(|(tool, _)| matches!(tool.as_str(), "AskUserQuestion" | "ExitPlanMode"));
    // The registry's `shell` status is IDLE WITH A BACKGROUND SHELL RUNNING (the binary
    // computes `idle` and then relabels it `shell` while a local_bash task is open) -
    // never a running shape. `busy` is the only registry running signal.
    let registry_status = registry.and_then(|r| r.status.as_deref());
    let registry_shell = registry_status == Some("shell");
    let running_shape = main_tail.unreturned_use.is_some() || registry_status == Some("busy");
    let eot_shape = main_tail.unreturned_use.is_none()
        && (main_tail
            .last_stop_reason
            .as_deref()
            .is_some_and(|s| s == "end_turn")
            || registry_shell);
    if registry_shell && !dead {
        notes.push(if background.open_counted() > 0 {
            "registry status shell: idle at end of turn with a background shell still \
             running - the background section lists it"
                .to_string()
        } else {
            "registry status shell: the harness reports a background shell still running \
             that the background section does not count (launched in an unscanned lane, \
             or excluded by the lens)"
                .to_string()
        });
    }

    if dead {
        notes.push(if main_tail.unreturned_use.is_some() {
            "the tail shows a call still open: the process died MID-TOOL".to_string()
        } else {
            "the tail is settled: the process ended after its last turn".to_string()
        });
        Verdict::StaleDead
    } else if !pending_elicitations.is_empty() || registry_waiting || tail_dialog {
        if registry_waiting {
            notes.push(
                "registry status waiting: the session is blocked on a dialog (a question, a \
                 permission prompt, a plan approval or a sandbox/worker request); the row is \
                 transition-written, so a stale one from a dead session needs the pid probe \
                 to refute it"
                    .to_string(),
            );
        }
        if tail_dialog {
            notes.push(
                "the tail holds an unreturned AskUserQuestion/ExitPlanMode call: whether a \
                 pending dialog reaches disk is a timing outcome of the write frontier \
                 (measured: a multi-question ask lands, a single-question ask stays \
                 buffered until answered; the sidecar covers the buffered shape)"
                    .to_string(),
            );
        }
        Verdict::WaitingHitl
    } else if running_shape {
        Verdict::Running
    } else if children.live_count > 0 {
        Verdict::WaitingChildren
    } else if eot_shape && background.open_counted() > 0 {
        notes.push(
            "the turn ended, but background task(s) have not returned - by design (a dev \
             server, a watcher) or not, csift cannot tell: a UI stop, a Monitor timeout or \
             agent teardown leaves no transcript marker, and Claude Code reconciles only at \
             the next session start. `--background-since` / `--ignore-background` narrow \
             what counts; kill a dead one with the tool or the shell"
                .to_string(),
        );
        Verdict::IdleBackgroundOpen
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
    }
}

/// The main lane's instant state in words (the at-exit line of `wait`, the tail row
/// of `status`).
pub(crate) fn tail_state_words(tail: &TailShape) -> String {
    let ago = |secs: Option<i64>| {
        secs.map(|s| {
            format!(
                " {} ago",
                crate::text::fmt_secs(u64::try_from(s).unwrap_or(0))
            )
        })
        .unwrap_or_default()
    };
    if let Some((tool, ts)) = &tail.unreturned_use {
        let for_ = age_secs(ts.as_deref())
            .map(|s| {
                format!(
                    " for {}",
                    crate::text::fmt_secs(u64::try_from(s).unwrap_or(0))
                )
            })
            .unwrap_or_default();
        return format!("in a {tool} call{for_}");
    }
    let age = age_secs(tail.last_ts_utc.as_deref());
    let sr = tail.last_stop_reason.as_deref().unwrap_or("(none)");
    if sr != "end_turn" && age.is_some_and(|a| a <= CHILD_RECENT_SECS) && tail.records_seen > 0 {
        format!("generating (last record{}, stop_reason {sr})", ago(age))
    } else if tail.records_seen == 0 {
        "no records readable at the tail".to_string()
    } else {
        format!("idle (last stop_reason {sr}, last record{})", ago(age))
    }
}
