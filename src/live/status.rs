//! `run_status`: resolve one target, join the surfaces, render the verdict.

use super::*;

/// Resolve the single transcript a status/wait target names (same exactly-one contract
/// as `show`, same resolver grammar as everything).
pub(crate) fn resolve_live_target(target: &[PathBuf], want_subagents: bool) -> Result<PathBuf> {
    let one = match target {
        [one] => one,
        many => bail!(
            "status/wait watch exactly ONE session per call; got {} targets — run one \
             call per session",
            many.len()
        ),
    };
    let scope = crate::path::SubagentScope::from(want_subagents);
    let files = crate::path::resolve_session_files(
        std::slice::from_ref(one),
        scope,
        crate::path::Caller::Other,
    )?;
    // The MAIN transcript anchors the watch; children are discovered from it. A
    // subagent target anchors on its own transcript.
    let main = files
        .iter()
        .find(|p| !crate::subagent::is_subagent_path(p))
        .or_else(|| files.first())
        .cloned();
    main.ok_or_else(|| anyhow::anyhow!("the target resolved to no transcript"))
}

/// Entry point for `csift status`.
pub fn run_status(args: &StatusArgs) -> Result<()> {
    let main = resolve_live_target(&args.target, args.want_subagents())?;
    let lens = BackgroundLens::from_args(
        args.lens.background_since.as_deref(),
        &args.lens.ignore_background,
    )?;
    let assessment = assess_path(&main, args.want_subagents(), &lens)?;
    let session_id = crate::subagent::session_id_from_path(&main);
    let is_subagent = crate::subagent::is_subagent_path(&main);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(&main).unwrap_or_else(|| session_id.clone());
    match args.format {
        OutputFormat::Text => render_status_text(&session_id, &assessment),
        OutputFormat::Json => {
            render_status_json(&session_id, is_subagent, &parent_session_id, &assessment)?;
        }
    }
    Ok(())
}

/// One full join over a resolved transcript path (registry, tail, pid, children,
/// sidecar, background launches under the lens, the harness task list, the last
/// messages).
pub(crate) fn assess_path(
    main: &Path,
    want_subagents: bool,
    lens: &BackgroundLens,
) -> Result<Assessment> {
    let session_id = crate::subagent::session_id_from_path(main);
    let is_subagent_target = crate::subagent::is_subagent_path(main);
    let owner_id =
        crate::subagent::parent_session_id_from_path(main).unwrap_or_else(|| session_id.clone());

    let registry = registry_row_for(&owner_id)?;
    let liveness = registry.as_ref().and_then(|r| r.pid).map(|pid| {
        probe_pid(
            pid,
            registry.as_ref().and_then(|r| r.proc_start.as_deref()),
            registry.as_ref().and_then(|r| r.pid_domain.as_deref()),
        )
    });

    let main_tail = tail_shape(main)?;
    // Background launches first: an async agent whose completion pulse already landed
    // is a SETTLED lane whatever its tail says, so the children scan takes that set.
    let background = background_report(main, want_subagents, lens)?;
    let returned: std::collections::HashSet<String> = background
        .tasks
        .iter()
        .filter(|t| t.kind == BgKind::Agent && t.state != BgState::Open)
        .filter_map(|t| t.id.clone())
        .collect();
    let children = if want_subagents && !is_subagent_target {
        children_report(main, &returned)?
    } else {
        ChildrenReport::default()
    };

    // The elicitation sidecar covers HITL (AUQ / ExitPlanMode / MCP): unresolved
    // pendings only, keyed by the top-level session.
    let pending: Vec<String> = if is_subagent_target {
        Vec::new() // the sidecar is keyed by the top-level session
    } else {
        crate::elicitation::unresolved_pending(main)?
            .0
            .iter()
            .map(|r| {
                r.csift_kind
                    .clone()
                    .unwrap_or_else(|| "elicitation".to_string())
            })
            .collect()
    };

    let mut assessment = assess_full(
        registry.as_ref(),
        liveness.as_ref(),
        &main_tail,
        &children,
        &pending,
        is_subagent_target,
        &background,
    );
    assessment.last = last_messages(main)?;
    // The task list is keyed by the top-level session (like the sidecar); a subagent
    // target gets no section instead of its parent's list under its own id.
    if !is_subagent_target {
        assessment.tasks = tasks_report(&owner_id);
    }
    Ok(assessment)
}
