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

    // 2. The scope's OWN bindings (status line + the bound_by_owner check base), each paired
    //    with the BINDING FACTS its own records carry: the slug's change points, whether the
    //    bound file is on disk, plan text held without a binding, and the slug against the
    //    file's birth instant. One walk per transcript, beside the binding resolution it
    //    already runs.
    let audited: Vec<Audited> = session_files
        .par_iter()
        .map(|p| -> Result<Audited> {
            Ok(Audited {
                session_id: crate::subagent::session_id_from_path(p),
                binding: resolve_session_plan(p)?,
                facts: binding_facts(p)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
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
        OutputFormat::Text => render_audit_text(&audited, &owners, &rows, skipped),
        OutputFormat::Json => render_audit_json(&audited, &rows, skipped)?,
    }
    Ok(())
}

/// One in-scope transcript: its binding, if any, and the facts its own records carry about
/// that binding. A transcript with NO binding is still audited, because "holds plan text with
/// nothing bound" is precisely a state that has no binding to hang off.
#[derive(Debug)]
pub(crate) struct Audited {
    pub(crate) session_id: String,
    pub(crate) binding: Option<PlanRef>,
    pub(crate) facts: BindingFacts,
}

fn render_audit_text(
    audited: &[Audited],
    owners: &BTreeSet<String>,
    rows: &[EditRow],
    skipped: usize,
) {
    println!("PLAN AUDIT");
    if audited.iter().all(|a| a.binding.is_none()) {
        println!("binds    none (no Plan Mode in the resolved scope)");
    }
    for a in audited {
        if let Some(r) = &a.binding {
            render_binding_text(r, &a.facts);
        }
        render_unbound_text(a);
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

/// One binding, with the three facts that qualify it: whether the bound file is on disk
/// (check b), where the slug came from (check a) and how it sits against the file's birth
/// instant (check d).
fn render_binding_text(r: &PlanRef, facts: &BindingFacts) {
    let slug = r
        .slug
        .as_deref()
        .map(|s| format!(", slug {s}"))
        .unwrap_or_default();
    // (b) The same `Path::is_file` verdict `csift plan` prints as `[exists]`/`[missing]`,
    // read off the SAME `PlanRef.plan_exists` field the forward view uses. The plan name is
    // minted at Plan-Mode entry and the file lands only when content is first written
    // (PLAN-014), so `[missing]` is an ordinary state and not a fault.
    println!(
        "binds    {} -> {}  [{}]  (L{}{slug})",
        r.session_id,
        r.plan_file,
        if r.plan_exists { "exists" } else { "missing" },
        r.line_no
    );
    // (a) The change points: where the binding key came into being, and any later move.
    match facts.changes.as_slice() {
        [] => println!(
            "slug     none carried by this transcript's records (the binding is the \
             attachment's, not a slug's)"
        ),
        changes => {
            for c in changes {
                println!("slug     L{}  {}", c.line, change_arrow(c));
            }
        }
    }
    // (d) Which came first, the binding or the file.
    let v = slug_vs_plan_file(facts.first_slug_utc.as_deref(), &r.plan_file);
    let tail = match v.reason() {
        Some(why) => format!("unknown - {why}"),
        None => format!(
            "the first slug-carrying record is {} the plan file's birth instant",
            v.token()
        ),
    };
    println!("birth    {tail}");
}

/// (c) A transcript holding a re-injected plan while NO record carries a slug: plan text with
/// nothing bound to it. Only the BOUND plan comes back in full after a compaction (PLAN-016),
/// so this text will not.
fn render_unbound_text(a: &Audited) {
    if a.facts.plan_ref_lines.is_empty() || !a.facts.no_slug() {
        return;
    }
    println!(
        "warning: {} holds a plan_file_reference attachment (L{}) while NO record carries a \
         slug - plan text without a binding. Only the BOUND plan is re-injected in full after \
         a compaction, so this text is not coming back.",
        a.session_id,
        a.facts
            .plan_ref_lines
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", L")
    );
}

/// One change point as text. `from: None` is the MINT, which is what all but a moved slug is.
fn change_arrow(c: &SlugChange) -> String {
    let to = c.to.as_deref().unwrap_or("none");
    match c.from.as_deref() {
        None => format!("none -> {to}  (the slug is minted here)"),
        Some(from) => format!("{from} -> {to}  (the binding MOVED here)"),
    }
}

fn render_audit_json(audited: &[Audited], rows: &[EditRow], skipped: usize) -> Result<()> {
    let header = crate::text::envelope_header("plan", json!({"mode": "audit"}));
    println!("{}", serde_json::to_string(&header)?);
    for a in audited {
        if let Some(r) = &a.binding {
            println!("{}", serde_json::to_string(&binding_json(r, &a.facts))?);
        }
        if !a.facts.plan_ref_lines.is_empty() && a.facts.no_slug() {
            // (c) One row per transcript in that state, so a consumer can act on it without
            // reading the warning prose.
            let obj = json!({
                "kind": "plan-unbound-text",
                "session_id": a.session_id,
                "plan_file_reference_lines": a.facts.plan_ref_lines,
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
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
    let unbound = audited
        .iter()
        .filter(|a| !a.facts.plan_ref_lines.is_empty() && a.facts.no_slug())
        .count();
    let summary = crate::text::envelope_summary(json!({
        "bindings": audited.iter().filter(|a| a.binding.is_some()).count(),
        "plan_files_touched": rows.len(),
        "warnings": rows.iter().filter(|r| !r.bound_by_owner).count() + unbound,
        // Transcripts holding a re-injected plan with no slug anywhere (check c).
        "unbound_plan_text": unbound,
        "skipped_lines": skipped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}

/// One binding row, with the three qualifying facts beside it.
fn binding_json(r: &PlanRef, facts: &BindingFacts) -> serde_json::Value {
    let v = slug_vs_plan_file(facts.first_slug_utc.as_deref(), &r.plan_file);
    json!({
        "kind": "binding",
        "session_id": r.session_id,
        "is_subagent": r.is_subagent,
        "parent_session_id": r.parent_session_id,
        "plan_file": r.plan_file,
        "line": r.line_no,
        "slug": r.slug,
        // (b) The same `Path::is_file` verdict the forward `csift plan` view prints.
        "plan_exists": r.plan_exists,
        // (a) Every point the slug changed, in file order; `from: null` is the mint.
        "slug_changes": facts.changes.iter().map(|c| json!({
            "line": c.line,
            "from": c.from,
            "to": c.to,
        })).collect::<Vec<_>>(),
        "first_slug_line": facts.first_slug_line,
        "first_slug_utc": facts.first_slug_utc,
        "first_slug_local": facts.first_slug_utc.as_deref().and_then(crate::timez::local_iso),
        // (d) The first slug-carrying record against the plan file's birth instant, with the
        // reason on the `unknown` arm rather than a guessed direction.
        "slug_vs_plan_file": v.token(),
        "slug_vs_plan_file_reason": v.reason(),
    })
}
