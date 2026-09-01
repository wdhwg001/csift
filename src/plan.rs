//! Plan-file resolution + the `csift plan` subcommand.
//!
//! ## What binds a plan file to a session
//!
//! Claude Code stores Plan-Mode plans flat under `~/.claude/plans/` with a random
//! three-word name (`nested-prancing-popcorn.md`); a subagent's plan gets an
//! `-agent-<hex>` suffix. The name is NOT derivable from the session id - it is bound
//! to the session by a record the transcript writes on entering Plan Mode:
//!
//! ```text
//! {"type":"attachment","attachment":{"type":"plan_mode",
//!    "planFilePath":"/Users/…/.claude/plans/nested-prancing-popcorn.md",
//!    "isSubAgent":false,"planExists":false}, …}
//! ```
//!
//! This `plan_mode` attachment is the AUTHORITATIVE binding. Crucially it is the *only*
//! reliable one: a session may freely `Edit`/`Write` OTHER sessions' plan files (they
//! show up as ordinary tool calls on a `~/.claude/plans/…` path), so "any plans/ path the
//! session touched" is NOT the session's own plan. The bound plan is the one named in the
//! `plan_mode` attachment, full stop - no path heuristics.
//!
//! Within one transcript every `plan_mode` attachment carries the same `planFilePath`
//! (only `planExists` flips `false→true` once the plan is first written); we take the
//! LATEST occurrence as the current binding.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rayon::prelude::*;

use crate::cli::{OutputFormat, PlanArgs};
use crate::parse::mmap_bytes;
use crate::path;

mod audit;
pub(crate) use audit::*;

/// The magic `--file` value that tells `recover` to reconstruct the session-bound plan
/// file instead of an explicit path. Bash-safe (no shell metacharacters → no escaping in
/// a mixed script) and consistent with `--at`'s `@line:` / `@turn:` sigils.
pub const PLAN_SIGIL: &str = "@plan";

/// The plan file a session is bound to, read from its `plan_mode` attachment.
#[derive(Debug, Clone)]
pub struct PlanRef {
    /// The transcript's own id (a bare hex for a subagent, a uuid for a top-level session).
    pub session_id: String,
    /// True when the binding came from a SUBAGENT transcript (plan file has the
    /// `-agent-<hex>` suffix); its `session_id` is not a re-feedable `@<uuid>` target.
    pub is_subagent: bool,
    /// The re-feedable parent session uuid (= `session_id` for a top-level transcript).
    pub parent_session_id: String,
    /// Absolute path to the bound plan file, verbatim from `planFilePath`.
    pub plan_file: String,
    /// Whether that plan file currently exists on disk (a recover target need NOT exist -
    /// recovering a deleted plan from the transcript is the whole point).
    pub plan_exists: bool,
    /// JSONL line number of the (latest) `plan_mode` attachment, for provenance.
    pub line_no: usize,
    /// The session's plan slug (the harness derives the plan file name from it).
    /// Read from the binding record itself - the slug is minted at Plan-Mode entry,
    /// so it is absent from every earlier record (including the whole head window),
    /// but always present on the `plan_mode` attachment record. `None` when the
    /// record predates the field.
    pub slug: Option<String>,
    /// How the binding was established: `plan_mode` (the explicit attachment, path
    /// verbatim) or `slug-only` (no `plan_mode` anywhere; the FIRST slug-carrying
    /// record binds `<plans-dir>/<slug>.md` - Claude Code's own binding law, which a
    /// forked/background session reaches because the fork strips its history's
    /// attachments, and any compaction mints a slug even without Plan Mode).
    pub binding_source: &'static str,
    /// True when the first slug-carrying record is a `compact_boundary` itself: the
    /// slug was MINTED at that compaction (Plan Mode never ran; Claude Code will
    /// still inject or rebuild the file it names) - the forked-session signature.
    pub minted_at_compaction: bool,
}

/// Tight byte prefilter for the plan-resolution pre-pass: `plan_mode` is a rare token, so
/// a giant transcript parses only its handful of attachment lines (the scan still splits
/// newlines over the whole file, but `serde_json` runs on almost nothing).
fn line_is_plan_candidate(line: &[u8]) -> bool {
    // Built ONCE (per-line hot path - the stateless form rebuilt its searcher every call).
    static PLAN_MODE: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"plan_mode"));
    PLAN_MODE.find(line).is_some()
}

/// Resolve the plan file BOUND to one session transcript (the latest `plan_mode`
/// attachment's `planFilePath`), or `None` if the session never entered Plan Mode.
pub fn resolve_session_plan(path: &Path) -> Result<Option<PlanRef>> {
    let session_id = crate::subagent::session_id_from_path(path);
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(None);
    };
    let bytes: &[u8] = &mmap;
    let (records, _skipped) =
        crate::parse::parse_candidates_parallel(bytes, line_is_plan_candidate);

    // File order == line order; overwriting keeps the LATEST plan_mode binding.
    let mut latest: Option<PlanRef> = None;
    for (line_no, rec) in &records {
        let Some(att) = rec.attachment_value() else {
            continue;
        };
        if att.get("type").and_then(serde_json::Value::as_str) != Some("plan_mode") {
            continue;
        }
        let Some(plan_file) = att.get("planFilePath").and_then(serde_json::Value::as_str) else {
            continue;
        };
        // The attachment's own isSubAgent agrees with the path-derived one; prefer the
        // path (authoritative for the transcript's id domain), falling back to the field.
        let plan_is_subagent = att
            .get("isSubAgent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(is_subagent);
        latest = Some(PlanRef {
            session_id: session_id.clone(),
            is_subagent: plan_is_subagent,
            parent_session_id: parent_session_id.clone(),
            plan_file: plan_file.to_string(),
            plan_exists: Path::new(plan_file).is_file(),
            line_no: *line_no,
            slug: rec.slug.clone(),
            binding_source: "plan_mode",
            minted_at_compaction: false,
        });
    }
    // No `plan_mode` anywhere: fall back to Claude Code's ACTUAL binding law - the
    // FIRST record carrying a `slug` binds `<plans-dir>/<slug>.md` (the harness's
    // getSlugFromLog takes the first slug in the log, no attachment consulted). A
    // forked/background session reaches this state by construction (the clone strips
    // attachment history), and its slug is often minted by the first own compaction -
    // reporting "no plan" there is a WRONG answer: CC will inject/rebuild that file.
    if latest.is_none() {
        if let Some((line_no, rec)) = first_slug_record(bytes) {
            if let Some(slug) = rec.slug.as_deref().filter(|s| slug_is_valid(s)) {
                let plan_file = plans_dir().join(format!("{slug}.md"));
                latest = Some(PlanRef {
                    session_id: session_id.clone(),
                    is_subagent,
                    parent_session_id: parent_session_id.clone(),
                    plan_exists: plan_file.is_file(),
                    plan_file: plan_file.display().to_string(),
                    line_no,
                    slug: Some(slug.to_string()),
                    binding_source: "slug-only",
                    minted_at_compaction: rec.is_compact_boundary(),
                });
            }
        }
    }
    Ok(latest)
}

/// The FIRST record carrying a `slug` field (Claude Code's binding key), by a
/// sequential early-exit walk - the fallback runs only when no `plan_mode` exists, and
/// a slugged session's first carrier normally sits early after the mint point.
fn first_slug_record(bytes: &[u8]) -> Option<(usize, crate::model::Record)> {
    static SLUG: std::sync::LazyLock<memchr::memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memchr::memmem::Finder::new(b"\"slug\""));
    let mut line_no = 0usize;
    for line in bytes.split(|&b| b == b'\n') {
        line_no += 1;
        if SLUG.find(line).is_none() {
            continue;
        }
        if let Ok(Some(rec)) = crate::parse::parse_line(line) {
            if rec.slug.is_some() {
                return Some((line_no, rec));
            }
        }
    }
    None
}

/// Claude Code's slug validity rule (lowercase alnum head, alnum/dash tail, <=120
/// chars) - a stray tolerated `slug` value that CC itself would reject never binds.
fn slug_is_valid(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(head) = chars.next() else {
        return false;
    };
    s.len() <= 120
        && (head.is_ascii_lowercase() || head.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The plans directory: `plansDirectory` from `<claude-home>/settings.json` when set
/// (absolute as-is, relative joined to the config home), else `<claude-home>/plans`.
fn plans_dir() -> PathBuf {
    let Ok(home) = crate::path::claude_home() else {
        return PathBuf::from("plans");
    };
    if let Ok(raw) = std::fs::read_to_string(home.join("settings.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(d) = v.get("plansDirectory").and_then(serde_json::Value::as_str) {
                let p = PathBuf::from(d);
                return if p.is_absolute() { p } else { home.join(p) };
            }
        }
    }
    home.join("plans")
}

/// Resolve the single plan file to use for `recover --file @plan` across the in-scope
/// session files. Prefers the TOP-LEVEL session's own plan; bails (never guesses) when no
/// session in scope is bound to a plan, or when top-level sessions disagree on which plan.
pub fn resolve_plan_target(session_files: &[PathBuf]) -> Result<PlanRef> {
    let mut refs: Vec<PlanRef> = Vec::new();
    for p in session_files {
        if let Some(r) = resolve_session_plan(p)? {
            refs.push(r);
        }
    }
    if refs.is_empty() {
        bail!(
            "--file {PLAN_SIGIL}: no plan file is bound to the target session(s) — no \
             `plan_mode` attachment found (a session has a bound plan only if it entered \
             Plan Mode). To recover an ordinary file, pass its path to --file instead."
        );
    }
    // Prefer the top-level session's own plan; fall back to subagent plans only if that's
    // all there is.
    let top_level: Vec<&PlanRef> = refs.iter().filter(|r| !r.is_subagent).collect();
    let pool: Vec<&PlanRef> = if top_level.is_empty() {
        refs.iter().collect()
    } else {
        top_level
    };
    let distinct: BTreeSet<&str> = pool.iter().map(|r| r.plan_file.as_str()).collect();
    if distinct.len() > 1 {
        let mut paths: Vec<&str> = distinct.into_iter().collect();
        paths.sort_unstable();
        bail!(
            "--file {PLAN_SIGIL}: the target spans sessions with different bound plan files \
             ({}). Pass `@<uuid>` to select one.",
            paths.join(", ")
        );
    }
    let chosen = distinct.into_iter().next().unwrap_or_default();
    Ok(pool
        .into_iter()
        .find(|r| r.plan_file == chosen)
        .cloned()
        .expect("chosen path came from pool"))
}

/// REVERSE plan lookup: which session(s) are bound to `plan_file`. Scans the resolved scope
/// (default every project; narrow with a PATH target) for transcripts whose `plan_mode`
/// attachment names this exact plan file (absolute-path identity), and reports the bound
/// session/subagent id(s). The inverse of the default session→plan direction.
fn run_plan_reverse(args: &PlanArgs, plan_file: &Path) -> Result<()> {
    let want = path::absolutize(plan_file)?;
    // No session pin (the whole point is we don't know the session); paths narrow scope.
    let session_files = path::resolve_targets_with_session_list(
        &args.paths,
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;
    // Resolve every in-scope transcript's plan binding IN PARALLEL, then keep the ones bound to
    // `want`. UNSCOPED `--reverse` scans every session in every project (many 300MB+ transcripts),
    // so the old serial `for` loop read them one at a time on a single core (measured 12.7s on a
    // multi-GB corpus). par_iter overlaps the reads+parses across cores; the explicit sort below is
    // a total order, so output is BYTE-IDENTICAL regardless of completion order. Mirrors the forward
    // path + recover/search/files. (`want` is borrowed read-only - Sync across the pool.)
    let mut hits: Vec<PlanRef> = session_files
        .par_iter()
        .map(|p| -> Result<Option<PlanRef>> {
            Ok(resolve_session_plan(p)?.filter(|r| {
                path::absolutize(Path::new(&r.plan_file))
                    .map(|a| a == want)
                    .unwrap_or(false)
            }))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    hits.sort_by(|a, b| {
        a.is_subagent
            .cmp(&b.is_subagent)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    match args.format {
        OutputFormat::Text => render_reverse_text(&want, &hits),
        OutputFormat::Json => render_reverse_json(&want, &hits)?,
    }
    Ok(())
}

/// Reverse text: the plan file, then each bound session/subagent (with its parent).
fn render_reverse_text(plan_file: &Path, hits: &[PlanRef]) {
    println!("plan     {}", plan_file.display());
    if hits.is_empty() {
        eprintln!("note: no session in scope is bound to this plan file (no `plan_mode` binding).");
        return;
    }
    for r in hits {
        let tag = if r.is_subagent { "  (subagent)" } else { "" };
        println!("session  {}{}", r.session_id, tag);
        if r.is_subagent {
            println!("parent   {}", r.parent_session_id);
        }
        if let Some(slug) = &r.slug {
            println!("slug     {slug}");
        }
        println!("bound at jsonl L{}", r.line_no);
    }
}

/// Reverse JSON: one object per bound session (the id-domain discriminators + provenance).
fn render_reverse_json(plan_file: &Path, hits: &[PlanRef]) -> Result<()> {
    let _ = plan_file; // the per-hit `plan_file` (the binding's stored path) is the faithful value
    println!(
        "{}",
        crate::text::envelope_header("plan", serde_json::json!({}))
    );
    for r in hits {
        let obj = serde_json::json!({
            "kind": "plan",
            "plan_file": r.plan_file,
            "session_id": r.session_id,
            "is_subagent": r.is_subagent,
            "parent_session_id": r.parent_session_id,
            "line": r.line_no,
            "slug": r.slug,
            "binding_source": r.binding_source,
            "minted_at_compaction": r.minted_at_compaction,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    println!(
        "{}",
        crate::text::envelope_summary(serde_json::json!({"plans": hits.len()}))
    );
    Ok(())
}

/// Entry point for `csift plan`.
pub fn run_plan(args: &PlanArgs) -> Result<()> {
    // REVERSE: given a plan file, find the session(s) bound to it.
    if let Some(plan_file) = &args.reverse {
        return run_plan_reverse(args, plan_file);
    }
    // With NO target at all (no positional, no --sessions-from), resolve the CALLING session
    // (like `whoami`) - `csift plan` inside a Claude Code session answers "what is MY plan
    // file". Never scan every project (ambiguous + expensive); error with guidance when the
    // env signal is absent. A positional target (`@<uuid>` / `*.jsonl` / a project path) or a
    // `--sessions-from` list flows through the shared resolver's grammar instead.
    let session_paths: Vec<PathBuf> = if args.paths.is_empty() && args.sessions_from.is_none() {
        match crate::whoami::detect_session_id() {
            // The env id becomes an `@<uuid>` positional - the same path every other
            // session is reached by now that `--session` is gone.
            Some(id) => vec![PathBuf::from(format!("@{id}"))],
            None => bail!("{}", crate::whoami::AMBIGUOUS_GUIDANCE),
        }
    } else {
        args.paths.clone()
    };

    let session_files = path::resolve_targets_with_session_list(
        &session_paths,
        args.sessions_from.as_deref(),
        args.want_subagents().into(),
        path::Caller::Other,
    )?;

    // AUDIT: the scope's plan-file edits joined against corpus bindings.
    if args.audit {
        return run_plan_audit(args, &session_files);
    }

    // Resolve each transcript's plan binding IN PARALLEL (rayon pool = CPU count) - mirrors
    // recover/search/files. A big session's plan lives in its 300MB+ top-level transcript, and a
    // `.`/multi-session target adds many more; scanning them serially (the old `for` loop) left one
    // core idle on the dominant file. The explicit sort below makes the output order deterministic
    // regardless of completion order, so this is byte-identical - pure execution strategy.
    let mut refs: Vec<PlanRef> = session_files
        .par_iter()
        .map(|p| resolve_session_plan(p))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    // Top-level first, then subagents; stable, id-sorted within each band.
    refs.sort_by(|a, b| {
        a.is_subagent
            .cmp(&b.is_subagent)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    match args.format {
        OutputFormat::Text => render_text(&refs),
        OutputFormat::Json => render_json(&refs)?,
    }
    Ok(())
}

fn render_text(refs: &[PlanRef]) {
    if refs.is_empty() {
        // An honest empty result is not an error: the session(s) simply never planned.
        eprintln!("note: no plan file is bound to the resolved session(s) (no Plan Mode).");
        return;
    }
    for r in refs {
        let tag = if r.is_subagent { "  (subagent)" } else { "" };
        println!("session  {}{}", r.session_id, tag);
        println!(
            "plan     {}  [{}]",
            r.plan_file,
            if r.plan_exists { "exists" } else { "missing" }
        );
        if let Some(slug) = &r.slug {
            println!("slug     {slug}");
        }
        if r.binding_source == "slug-only" {
            println!(
                "binding  slug only — no plan_mode attachment; Claude Code binds by the \
                 first slug-carrying record{}",
                if r.minted_at_compaction {
                    " (slug MINTED at a compaction boundary — Plan Mode never ran; CC \
                     still injects/rebuilds this file)"
                } else {
                    ""
                }
            );
        }
        if r.is_subagent {
            println!("parent   {}", r.parent_session_id);
        }
        println!("line     L{}", r.line_no);
    }
}

fn render_json(refs: &[PlanRef]) -> Result<()> {
    use serde_json::json;
    println!("{}", crate::text::envelope_header("plan", json!({})));
    for r in refs {
        let obj = json!({
            "kind": "plan",
            "session_id": r.session_id,
            "is_subagent": r.is_subagent,
            "parent_session_id": r.parent_session_id,
            "plan_file": r.plan_file,
            "plan_exists": r.plan_exists,
            "line": r.line_no,
            "slug": r.slug,
            "binding_source": r.binding_source,
            "minted_at_compaction": r.minted_at_compaction,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    println!(
        "{}",
        crate::text::envelope_summary(json!({"plans": refs.len()}))
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_sigil_is_bash_safe_and_at_prefixed() {
        // No shell metacharacter → no escaping needed in a mixed script; `@`-sigil matches
        // the `--at @line:`/`@turn:` convention.
        assert_eq!(PLAN_SIGIL, "@plan");
        assert!(PLAN_SIGIL.starts_with('@'));
        assert!(!PLAN_SIGIL
            .chars()
            .any(|c| matches!(c, '$' | '!' | '*' | '`' | ' ' | '"' | '\'' | '\\')));
    }

    #[test]
    fn prefilter_matches_only_plan_mode_lines() {
        assert!(line_is_plan_candidate(
            br#"{"attachment":{"type":"plan_mode","planFilePath":"/x.md"}}"#
        ));
        // An ordinary Edit of a plans/ file is NOT a plan_mode binding.
        assert!(!line_is_plan_candidate(
            br#"{"message":{"content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/Users/x/.claude/plans/foo.md"}}]}}"#
        ));
    }

    /// `resolve_session_plan` must bind ONLY on a real `plan_mode` attachment - an empty
    /// file, a mere mention of the word, a different attachment type, and a `plan_mode`
    /// with no `planFilePath` all resolve to `None` (never a false binding).
    #[test]
    fn resolve_session_plan_binds_only_on_a_real_plan_mode_attachment() {
        let dir = std::env::temp_dir().join(format!("csift-plan-ut-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, body: &str| {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            p
        };

        // (a) empty transcript → no binding (mmap returns None).
        let empty = write("empty.jsonl", "");
        assert!(resolve_session_plan(&empty).unwrap().is_none());

        // (b) the word "plan_mode" appears in ordinary USER TEXT (passes the byte prefilter)
        //     but there is no attachment at all → no binding.
        let decoy = write(
            "decoy.jsonl",
            "{\"type\":\"user\",\"timestamp\":\"2026-06-07T05:00:00Z\",\"message\":{\"role\":\"user\",\"content\":\"what does plan_mode do\"}}\n",
        );
        assert!(resolve_session_plan(&decoy).unwrap().is_none());

        // (c) an attachment of a DIFFERENT type that happens to contain the token → skipped.
        let other_att = write(
            "other.jsonl",
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"file\",\"note\":\"see plan_mode\"},\"timestamp\":\"2026-06-07T05:00:00Z\"}\n",
        );
        assert!(resolve_session_plan(&other_att).unwrap().is_none());

        // (d) a real plan_mode attachment but with NO planFilePath → skipped.
        let no_path = write(
            "nopath.jsonl",
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"plan_mode\",\"isSubAgent\":false,\"planExists\":false},\"timestamp\":\"2026-06-07T05:00:00Z\"}\n",
        );
        assert!(resolve_session_plan(&no_path).unwrap().is_none());

        // (e) a real, complete plan_mode attachment → bound. The binding record's own
        //     top-level `slug` rides along (the harness mints it at Plan-Mode entry, so
        //     the bind record always carries it on current CC).
        let real = write(
            "real.jsonl",
            "{\"type\":\"attachment\",\"slug\":\"quiet-harbor-relay\",\"attachment\":{\"type\":\"plan_mode\",\"isSubAgent\":false,\"planFilePath\":\"/x/p.md\",\"planExists\":false},\"timestamp\":\"2026-06-07T05:00:00Z\"}\n",
        );
        let got = resolve_session_plan(&real)
            .unwrap()
            .expect("a real plan_mode binds");
        assert_eq!(got.plan_file, "/x/p.md");
        assert!(!got.is_subagent);
        assert_eq!(got.slug.as_deref(), Some("quiet-harbor-relay"));

        // (f) an OLDER binding record without the slug field → None, never fabricated.
        let no_slug = write(
            "noslug.jsonl",
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"plan_mode\",\"isSubAgent\":false,\"planFilePath\":\"/x/q.md\",\"planExists\":false},\"timestamp\":\"2026-06-07T05:00:00Z\"}\n",
        );
        assert_eq!(resolve_session_plan(&no_slug).unwrap().unwrap().slug, None);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
