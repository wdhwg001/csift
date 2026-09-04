//! `run_wait`: the poll loop - baselines, incremental reads, first hit wins.

use super::*;
use std::io::Write as _;

/// Adaptive poll bounds (ms): floor when things are moving, ceiling when quiet.
const POLL_FLOOR_MS: u64 = 200;
const POLL_CEIL_MS: u64 = 2000;

/// The GNU `timeout` convention - the ONE documented exception to the crate's
/// 0-vs-non-zero exit law (a monitor's timeout is a normal outcome scripts branch on).
const TIMEOUT_EXIT: u8 = 124;

/// Per-file incremental cursor: bytes before `offset` are HISTORY (never matched).
struct Cursor {
    path: PathBuf,
    offset: u64,
    /// The transcript's own id (activity is censused per lane).
    lane: String,
}

/// Entry point for `csift wait`.
pub fn run_wait(args: &WaitArgs) -> Result<()> {
    let conds: Vec<(String, Cond)> = args
        .until
        .iter()
        .map(|s| parse_condition(s).map(|c| (s.clone(), c)))
        .collect::<Result<Vec<_>>>()?;
    let needs_verdict = conds.iter().any(|(_, c)| c.needs_verdict());
    let main = resolve_live_target(&args.target, args.want_subagents())?;
    let lens = BackgroundLens::from_args(
        args.lens.background_since.as_deref(),
        &args.lens.ignore_background,
    )?;
    // The timeout is REQUIRED (v0.10.0): a background task may never return by design
    // (a dev server, a watcher), so an unbounded wait on `stop` is a bug, not a wait.
    let Some(timeout_secs) = args.timeout else {
        bail!(
            "wait needs --timeout <SECS>: a background task can be designed never to return \
             (a dev server, a watcher), so a wait without a bound never ends. Pick a bound, \
             branch on exit 124, and read the at-exit report; narrow what counts with \
             --background-since now / --ignore-background <RE>"
        );
    };
    let is_subagent_target = crate::subagent::is_subagent_path(&main);

    // ── Baselines: snapshot every currently-watched file's length. Only bytes appended
    //    AFTER these offsets are events; history is `search`'s job. ──
    let mut cursors: Vec<Cursor> = Vec::new();
    let seed = |path: PathBuf, cursors: &mut Vec<Cursor>| {
        if cursors.iter().any(|c| c.path == path) {
            return;
        }
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let lane = crate::subagent::session_id_from_path(&path);
        cursors.push(Cursor { path, offset, lane });
    };
    seed(main.clone(), &mut cursors);
    if args.want_subagents() && !is_subagent_target {
        for sub in crate::subagent::subagent_transcript_files(&main).unwrap_or_default() {
            seed(sub, &mut cursors);
        }
    }
    if !is_subagent_target {
        if let Some(sc) = crate::elicitation::sidecar_path(&main) {
            seed(sc, &mut cursors);
        }
    }

    // ── Readiness line (stderr): the caller can order its own actions AFTER this - the
    //    line is what makes a scripted wait race-free against its own trigger. ──
    let start = std::time::Instant::now();
    let deadline = std::time::Duration::from_secs(timeout_secs);
    eprintln!(
        "csift: watching {} file(s) from byte offsets; conditions: {}; timeout {timeout_secs}s{}",
        cursors.len(),
        args.until.join(" | "),
        if lens.is_active() {
            format!(
                "; lens: since {}, {} ignore pattern(s)",
                lens.since_raw.as_deref().unwrap_or("-"),
                lens.ignore.len()
            )
        } else {
            String::new()
        }
    );

    let mut activity = Activity::default();
    let mut interval = args.interval.unwrap_or(POLL_FLOOR_MS);
    loop {
        // ── New-child discovery: a lane spawned after start joins the watch set with
        //    baseline 0 (its whole content is post-start). ──
        if args.want_subagents() && !is_subagent_target {
            for sub in crate::subagent::subagent_transcript_files(&main).unwrap_or_default() {
                if !cursors.iter().any(|c| c.path == sub) {
                    let lane = crate::subagent::session_id_from_path(&sub);
                    cursors.push(Cursor {
                        path: sub,
                        offset: 0,
                        lane,
                    });
                }
            }
        }
        // ── Sidecar discovery: the sidecar DIR is typically born mid-wait (the first
        //    pending ask creates it), and its path resolves only once the dir exists -
        //    so re-attempt each poll. A file born after start is wholly post-start, so
        //    baseline 0 is exact. ──
        if !is_subagent_target {
            if let Some(sc) = crate::elicitation::sidecar_path(&main) {
                if !cursors.iter().any(|c| c.path == sc) {
                    let lane = crate::subagent::session_id_from_path(&main);
                    cursors.push(Cursor {
                        path: sc,
                        offset: 0,
                        lane,
                    });
                }
            }
        }

        // ── Incremental reads: appended COMPLETE lines only (a torn tail is held at its
        //    offset and re-read next poll - the atomic-append measurements make this the
        //    only guard needed). ──
        let mut anything_grew = false;
        for cur in &mut cursors {
            let len = std::fs::metadata(&cur.path).map(|m| m.len()).unwrap_or(0);
            if len <= cur.offset {
                continue;
            }
            anything_grew = true;
            let Some(mmap) = mmap_bytes(&cur.path)? else {
                continue;
            };
            let bytes: &[u8] = &mmap;
            let start_at = usize::try_from(cur.offset).unwrap_or(0).min(bytes.len());
            let mut pos = start_at;
            while pos < bytes.len() {
                let Some(nl) = memchr::memchr(b'\n', &bytes[pos..]) else {
                    break; // torn tail: hold the cursor here
                };
                let line = &bytes[pos..pos + nl];
                pos += nl + 1;
                let Ok(Some(rec)) = crate::parse::parse_line(line) else {
                    continue;
                };
                activity.fold(&rec, &cur.lane);
                for (raw, cond) in &conds {
                    if record_matches(cond, &rec) {
                        return finish(args, raw, &main, &lens, &activity, start.elapsed());
                    }
                }
            }
            cur.offset = pos as u64;
        }

        // ── Verdict-class conditions: re-join the surfaces (bounded tail work). ──
        if needs_verdict {
            let assessment = assess_path(&main, args.want_subagents(), &lens)?;
            for (raw, cond) in &conds {
                if verdict_matches(cond, assessment.verdict) {
                    return finish(args, raw, &main, &lens, &activity, start.elapsed());
                }
            }
        }

        if start.elapsed() >= deadline {
            return finish_timeout(args, &main, &lens, &activity, start.elapsed());
        }

        // Adaptive cadence: floor while moving, back off toward the ceiling when quiet.
        if args.interval.is_none() {
            interval = if anything_grew {
                POLL_FLOOR_MS
            } else {
                (interval * 3 / 2).min(POLL_CEIL_MS)
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(interval));
    }
}

/// A condition fired: render + exit 0.
fn finish(
    args: &WaitArgs,
    fired: &str,
    main: &Path,
    lens: &BackgroundLens,
    activity: &Activity,
    waited: std::time::Duration,
) -> Result<()> {
    let assessment = assess_path(main, args.want_subagents(), lens)?;
    emit_wait(args, fired, main, &assessment, activity, waited)?;
    Ok(())
}

/// The timeout elapsed: render `fired:"timeout"` + exit 124 (never through the error
/// path - a timeout is a NORMAL monitor outcome, just a distinguishable one).
fn finish_timeout(
    args: &WaitArgs,
    main: &Path,
    lens: &BackgroundLens,
    activity: &Activity,
    waited: std::time::Duration,
) -> Result<()> {
    let assessment = assess_path(main, args.want_subagents(), lens)?;
    emit_wait(args, "timeout", main, &assessment, activity, waited)?;
    let _ = std::io::stdout().flush();
    std::process::exit(i32::from(TIMEOUT_EXIT));
}

fn emit_wait(
    args: &WaitArgs,
    fired: &str,
    main: &Path,
    a: &Assessment,
    activity: &Activity,
    waited: std::time::Duration,
) -> Result<()> {
    let session_id = crate::subagent::session_id_from_path(main);
    match args.format {
        OutputFormat::Text => {
            println!("fired    {fired}");
            println!("verdict  {}", a.verdict.slug());
            println!("waited   {}s", waited.as_secs());
            println!("at exit  {}", a.tail_state);
            println!("activity {}", activity.summary_line());
            for e in &a.evidence {
                println!("  {:<9} {}", e.surface, e.value);
            }
            render_background_text(&a.background);
            render_last_text(&session_id, &a.last);
            for n in a.notes.iter().chain(a.background.notes.iter()) {
                println!("  note: {n}");
            }
        }
        OutputFormat::Json => {
            let obj = serde_json::json!({
                "kind": "wait",
                "fired": fired,
                "verdict": a.verdict.slug(),
                "waited_secs": waited.as_secs(),
                "at_exit": a.tail_state,
                "activity": activity.json(),
                "evidence": a.evidence.iter().map(|e| serde_json::json!({
                    "surface": e.surface,
                    "value": e.value,
                    "age_secs": e.age_secs,
                })).collect::<Vec<_>>(),
                "background": background_json(&a.background),
                "last": last_json(&a.last),
                "notes": a.notes.iter().chain(a.background.notes.iter()).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
    }
    Ok(())
}
