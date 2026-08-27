//! `plan --audit`: the target scope's plan-file edits joined against plan BINDINGS.
//!
//! Why this audit exists: a session may freely `Edit`/`Write` ANOTHER session's plan
//! file (it is an ordinary tool call on a path), but after a compaction only the
//! session's OWN bound plan is re-injected in full - content parked in an unbound plan
//! file does not come back. The audit finds every structured mutation the target scope
//! made to a file that SOME session binds as its plan, and warns when the mutating
//! session does not bind that file itself.
//!
//! Identification is a JOIN against the corpus's `plan_mode` bindings (one scan of
//! every project, `plan_mode`-prefiltered so it parses almost nothing), never a plans
//! directory guess (`plansDirectory` is configurable). Bash-side edits are outside
//! this audit: structured `Write`/`Edit`/`MultiEdit`/`NotebookEdit` only.

use super::*;
use serde_json::json;

/// One audited (owner session, plan file) edit aggregate.
#[derive(Debug)]
struct EditRow {
    /// Owning parent session id of the mutating transcript(s).
    owner: String,
    /// The mutated plan file (verbatim tool-input path).
    path: String,
    mutations: usize,
    /// True when some transcript of `owner` binds `path` as its plan.
    bound_by_owner: bool,
    /// The corpus binder chosen for display (top-level first); `None` only if the
    /// binder set was empty, which cannot happen for an emitted row.
    binder: Option<PlanRef>,
}

/// Structured-mutation candidate prefilter: every `Write`/`Edit`/`MultiEdit` input
/// carries the `file_path` key, `NotebookEdit` the `notebook_path` key (quoted key
/// needles: serialization-tolerant, and an in-content quote is escaped in raw bytes).
fn line_is_structured_mutation_candidate(line: &[u8]) -> bool {
    static FILE_PATH: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"file_path\""));
    static NOTEBOOK_PATH: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"notebook_path\""));
    FILE_PATH.find(line).is_some() || NOTEBOOK_PATH.find(line).is_some()
}

pub(crate) fn run_plan_audit(args: &PlanArgs, session_files: &[PathBuf]) -> Result<()> {
    // 1. Structured plan-candidate mutations in the target scope, keyed by
    //    (owning parent session, path). Malformed lines are counted (the law).
    let mut edits: std::collections::BTreeMap<(String, String), usize> =
        std::collections::BTreeMap::new();
    let mut skipped = 0usize;
    for p in session_files {
        let owner = crate::subagent::parent_session_id_from_path(p)
            .unwrap_or_else(|| crate::subagent::session_id_from_path(p));
        let Some(mmap) = mmap_bytes(p)? else {
            continue;
        };
        let (records, s) =
            crate::parse::parse_candidates_parallel(&mmap, line_is_structured_mutation_candidate);
        skipped += s;
        for (_line, rec) in &records {
            for m in rec.structured_tool_mutations() {
                *edits.entry((owner.clone(), m.path)).or_insert(0) += 1;
            }
        }
    }

    // 2. The scope's OWN bindings (status line + the bound_by_owner check base).
    let own: Vec<PlanRef> = session_files
        .par_iter()
        .map(|p| resolve_session_plan(p))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let owners: BTreeSet<String> = session_files
        .iter()
        .map(|p| {
            crate::subagent::parent_session_id_from_path(p)
                .unwrap_or_else(|| crate::subagent::session_id_from_path(p))
        })
        .collect();

    // 3. Corpus binder map: plan file → every session bound to it. Paid only when the
    //    scope mutated anything (the join is what identifies a "plan file").
    let mut binder_map: std::collections::BTreeMap<String, Vec<PlanRef>> =
        std::collections::BTreeMap::new();
    if !edits.is_empty() {
        let all = path::resolve_session_files(
            &[],
            crate::path::SubagentScope::WithSubagents,
            path::Caller::Other,
        )?;
        let all_refs: Vec<PlanRef> = all
            .par_iter()
            .map(|p| resolve_session_plan(p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        for r in all_refs {
            binder_map.entry(r.plan_file.clone()).or_default().push(r);
        }
        // Top-level binders first within each path (display preference).
        for binders in binder_map.values_mut() {
            binders.sort_by(|a, b| {
                a.is_subagent
                    .cmp(&b.is_subagent)
                    .then_with(|| a.parent_session_id.cmp(&b.parent_session_id))
            });
        }
    }

    // 4. Rows: only mutated paths that ARE some session's bound plan.
    let rows: Vec<EditRow> = edits
        .into_iter()
        .filter_map(|((owner, path), mutations)| {
            let binders = binder_map.get(&path)?;
            let bound_by_owner = binders.iter().any(|b| b.parent_session_id == owner);
            let binder = binders
                .iter()
                .find(|b| b.parent_session_id != owner)
                .or_else(|| binders.first())
                .cloned();
            Some(EditRow {
                owner,
                path,
                mutations,
                bound_by_owner,
                binder,
            })
        })
        .collect();

    match args.format {
        OutputFormat::Text => render_audit_text(&own, &owners, &rows, skipped),
        OutputFormat::Json => render_audit_json(&own, &rows, skipped)?,
    }
    Ok(())
}

fn render_audit_text(own: &[PlanRef], owners: &BTreeSet<String>, rows: &[EditRow], skipped: usize) {
    println!("PLAN AUDIT");
    if own.is_empty() {
        println!("binds    none (no Plan Mode in the resolved scope)");
    }
    for r in own {
        let slug = r
            .slug
            .as_deref()
            .map(|s| format!(", slug {s}"))
            .unwrap_or_default();
        println!(
            "binds    {} -> {}  (L{}{slug})",
            r.session_id, r.plan_file, r.line_no
        );
    }
    if rows.is_empty() {
        println!(
            "edits    none: no structured mutation in scope touches any session's bound \
             plan file (bash-side edits are outside this audit)"
        );
    }
    let mut warnings = 0usize;
    for r in rows {
        let owner = if owners.len() > 1 {
            format!(" by {}", r.owner)
        } else {
            String::new()
        };
        let verdict = if r.bound_by_owner {
            "bound by this session"
        } else {
            warnings += 1;
            "NOT bound by the mutating session"
        };
        println!(
            "edits    {}  {} mutation(s){owner}  [{verdict}]",
            r.path, r.mutations
        );
    }
    for r in rows.iter().filter(|r| !r.bound_by_owner) {
        if let Some(b) = &r.binder {
            println!(
                "warning: {} mutation(s) to {} by session {}, which does NOT bind it \
                 (bound by {}, L{}). Only the BOUND plan is re-injected in full after a \
                 compaction.",
                r.mutations, r.path, r.owner, b.parent_session_id, b.line_no
            );
        }
    }
    if warnings == 0 && !rows.is_empty() {
        println!("ok: every audited plan-file edit targets the mutating session's own plan");
    }
    if skipped > 0 {
        println!("({})", crate::text::malformed_note(skipped));
    }
}

fn render_audit_json(own: &[PlanRef], rows: &[EditRow], skipped: usize) -> Result<()> {
    let header = crate::text::envelope_header("plan", json!({"mode": "audit"}));
    println!("{}", serde_json::to_string(&header)?);
    for r in own {
        let obj = json!({
            "kind": "binding",
            "session_id": r.session_id,
            "is_subagent": r.is_subagent,
            "parent_session_id": r.parent_session_id,
            "plan_file": r.plan_file,
            "line": r.line_no,
            "slug": r.slug,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    for r in rows {
        let obj = json!({
            "kind": "plan-edit",
            "owner_session_id": r.owner,
            "path": r.path,
            "mutations": r.mutations,
            "bound_by_owner": r.bound_by_owner,
            "binder_session_id": r.binder.as_ref().map(|b| b.parent_session_id.clone()),
            "binder_line": r.binder.as_ref().map(|b| b.line_no),
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "bindings": own.len(),
        "plan_files_touched": rows.len(),
        "warnings": rows.iter().filter(|r| !r.bound_by_owner).count(),
        "skipped_lines": skipped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
