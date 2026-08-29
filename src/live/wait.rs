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
    is_main: bool,
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
    let is_subagent_target = crate::subagent::is_subagent_path(&main);

    // ── Baselines: snapshot every currently-watched file's length. Only bytes appended
    //    AFTER these offsets are events; history is `search`'s job. ──
    let mut cursors: Vec<Cursor> = Vec::new();
    let seed = |path: PathBuf, is_main: bool, cursors: &mut Vec<Cursor>| {
        if cursors.iter().any(|c| c.path == path) {
            return;
        }
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        cursors.push(Cursor {
            path,
            offset,
            is_main,
        });
    };
    seed(main.clone(), true, &mut cursors);
    if args.want_subagents() && !is_subagent_target {
        for sub in crate::subagent::subagent_transcript_files(&main).unwrap_or_default() {
            seed(sub, false, &mut cursors);
        }
    }
    if !is_subagent_target {
        if let Some(sc) = crate::elicitation::sidecar_path(&main) {
            seed(sc, true, &mut cursors);
        }
    }

    // ── Readiness line (stderr): the caller can order its own actions AFTER this - the
    //    line is what makes a scripted wait race-free against its own trigger. ──
    let start = std::time::Instant::now();
    let deadline = args.timeout.map(std::time::Duration::from_secs);
    eprintln!(
        "csift: watching {} file(s) from byte offsets; conditions: {}{}",
        cursors.len(),
        args.until.join(" | "),
        match args.timeout {
            Some(t) => format!("; timeout {t}s"),
            None => "; no --timeout given - waiting until a condition fires".to_string(),
        }
    );

    let mut interval = args.interval.unwrap_or(POLL_FLOOR_MS);
    loop {
        // ── New-child discovery: a lane spawned after start joins the watch set with
        //    baseline 0 (its whole content is post-start). ──
        if args.want_subagents() && !is_subagent_target {
            for sub in crate::subagent::subagent_transcript_files(&main).unwrap_or_default() {
                if !cursors.iter().any(|c| c.path == sub) {
                    cursors.push(Cursor {
                        path: sub,
                        offset: 0,
                        is_main: false,
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
                for (raw, cond) in &conds {
                    if record_matches(cond, &rec, cur.is_main) {
                        return finish(args, raw, &main, start.elapsed());
                    }
                }
            }
            cur.offset = pos as u64;
        }

        // ── Verdict-class conditions: re-join the surfaces (bounded tail work). ──
        if needs_verdict {
            let assessment = assess_path(&main, args.want_subagents())?;
            for (raw, cond) in &conds {
                if verdict_matches(cond, assessment.verdict) {
                    return finish(args, raw, &main, start.elapsed());
                }
            }
        }

        if let Some(d) = deadline {
            if start.elapsed() >= d {
                return finish_timeout(args, &main, start.elapsed());
            }
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
fn finish(args: &WaitArgs, fired: &str, main: &Path, waited: std::time::Duration) -> Result<()> {
    let assessment = assess_path(main, args.want_subagents())?;
    emit_wait(args, fired, &assessment, waited)?;
    Ok(())
}

/// The timeout elapsed: render `fired:"timeout"` + exit 124 (never through the error
/// path - a timeout is a NORMAL monitor outcome, just a distinguishable one).
fn finish_timeout(args: &WaitArgs, main: &Path, waited: std::time::Duration) -> Result<()> {
    let assessment = assess_path(main, args.want_subagents())?;
    emit_wait(args, "timeout", &assessment, waited)?;
    let _ = std::io::stdout().flush();
    std::process::exit(i32::from(TIMEOUT_EXIT));
}

fn emit_wait(
    args: &WaitArgs,
    fired: &str,
    a: &Assessment,
    waited: std::time::Duration,
) -> Result<()> {
    match args.format {
        OutputFormat::Text => {
            println!("fired    {fired}");
            println!("verdict  {}", a.verdict.slug());
            println!("waited   {}s", waited.as_secs());
            for e in &a.evidence {
                println!("  {:<9} {}", e.surface, e.value);
            }
        }
        OutputFormat::Json => {
            let obj = serde_json::json!({
                "kind": "wait",
                "fired": fired,
                "verdict": a.verdict.slug(),
                "waited_secs": waited.as_secs(),
                "evidence": a.evidence.iter().map(|e| serde_json::json!({
                    "surface": e.surface,
                    "value": e.value,
                    "age_secs": e.age_secs,
                })).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string(&obj)?);
        }
    }
    Ok(())
}
