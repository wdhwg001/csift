//! Settings-family external writes: the file-history snapshot instrument applied to
//! the `files` timeline.
//!
//! Claude Code rewrites its settings files IN-PROCESS (/model, /config, theme,
//! permission "always allow", plugin toggles) - no tool record, usually no printed
//! trace. The instrument: CC snapshots every Edit/Write-tracked file per prompt and
//! bumps `trackedFileBackups[<path>].version` only when the bytes changed; a version
//! JUMP with no tool write of that path in the interval is an external write.
//!
//! SCOPE LAW (operator-ruled): reported ONLY for the settings family - basename
//! `settings.json` / `settings.local.json` under a `.claude` parent. The tracked set
//! spans thousands of ordinary source paths and CC writes bookkeeping constantly;
//! listing "harness writes" beyond settings would flood every timeline (measured
//! corpus: 1701 tracked paths, 11 settings-family).
//!
//! HONEST LIMITS (documented, disclosed in SPEC/SKILL): the version counter RESETS
//! mid-session (a process restart starts a new generation) - jumps are only read
//! within a generation, so a write hiding across a reset is not reported; a tool
//! write and a silent write in the SAME snapshot interval merge into one bump and
//! the silent half is invisible here (recover's content comparison is the complete
//! form); a session that never tool-touched the file has no tracking at all.

use super::*;

/// True for the settings family: `settings.json` / `settings.local.json` directly
/// under a `.claude` directory (absolute or relative spelling alike).
pub(crate) fn is_settings_family(path: &str) -> bool {
    let mut parts = path.rsplit(['/', '\\']);
    let base = parts.next().unwrap_or_default();
    let parent = parts.next().unwrap_or_default();
    matches!(base, "settings.json" | "settings.local.json") && parent == ".claude"
}

/// Synthesize `external write` timeline rows for the settings family from the
/// snapshot version sequences. `mutations` are this transcript's tool-extracted
/// rows (line-ordered per path is NOT required; membership in the interval is by
/// jsonl line number).
pub(crate) fn extract_external_writes(
    session_id: &str,
    records: &[Record],
    line_nos: &[usize],
    mutations: &[TaggedMutation],
) -> Vec<TaggedMutation> {
    // Per settings-family path: the (line, turn, ts, version) sequence.
    type VersionSeq = Vec<(usize, usize, Option<String>, u64)>;
    let mut seqs: BTreeMap<String, VersionSeq> = BTreeMap::new();
    let turns = group_turn_indices_deduped(records, |r| r);
    let turn_of = |idx: usize| -> usize {
        turns
            .iter()
            .position(|t| t.contains(&idx))
            .unwrap_or_default()
    };
    for (idx, rec) in records.iter().enumerate() {
        let Some(snap) = rec.snapshot.as_ref() else {
            continue;
        };
        let Some(tfb) = snap.get("trackedFileBackups").and_then(|v| v.as_object()) else {
            continue;
        };
        let ts = snap
            .get("timestamp")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        for (path, entry) in tfb {
            if !is_settings_family(path) {
                continue;
            }
            let Some(version) = entry.get("version").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            seqs.entry(path.clone()).or_default().push((
                line_nos[idx],
                turn_of(idx),
                ts.clone(),
                version,
            ));
        }
    }

    let mut out = Vec::new();
    for (path, seq) in &seqs {
        let mut prev: Option<(usize, u64)> = None; // (line, version)
        for (line, turn, ts, version) in seq {
            if let Some((prev_line, prev_v)) = prev {
                // A DECREASE = a generation reset (the counter restarts on process
                // restart); only same-generation jumps are readable here.
                if *version > prev_v {
                    let touched = mutations.iter().any(|m| {
                        m.line_no > prev_line
                            && m.line_no <= *line
                            && (crate::recover::path_matches(Some(path), &m.mutation.path)
                                || crate::recover::path_matches(Some(&m.mutation.path), path))
                    });
                    if !touched {
                        out.push(TaggedMutation {
                            session_id: session_id.to_string(),
                            is_subagent: false,
                            parent_session_id: session_id.to_string(),
                            turn_index: *turn,
                            line_no: *line,
                            mutation: FileMutation {
                                path: path.clone(),
                                op: FileOp::ExternalWrite,
                                timestamp_utc: ts.clone(),
                                is_create: false,
                                path_verbatim: None,
                                resolution: None,
                                command_errored: false,
                                detail: Some(format!(
                                    "inferred: file-history v{prev_v}->v{version} with no \
                                     tool record in L{prev_line}..L{line}"
                                )),
                            },
                        });
                    }
                }
            }
            prev = Some((*line, *version));
        }
    }
    out
}
