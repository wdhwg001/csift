//! Per-turn Bash content-anchor extraction: pair each gated command shape
//! (`bash_mutations::bash_anchor`) with its result carrier and emit first-class
//! content events.
//!
//! The lexical classifier decides WHAT a command shape means; this layer supplies
//! the transcript-side gates:
//! - the command's RESULT must not be an error (`failed_ids` - a failed write never
//!   landed, a failed read proves nothing);
//! - a READ anchor additionally needs the top-level `toolUseResult` echo with EMPTY
//!   stderr, `interrupted` false, and NO persisted-output pointer (an externalized
//!   stdout is not the inline text). The echo is a per-LANE fact, not a per-level one
//!   (re-measured at CC 2.1.258, v0.10.1): WORKFLOW lanes never carry it (0 of 3867
//!   Bash results), built-in Task/Agent and teammate lanes carry it 97-99% of the
//!   time, the main lane always - so read anchors reach every lane that carries the
//!   echo, by construction, while WRITE anchors (input-side) work in every lane;
//! - the operand resolves against the recording shell's cwd through the same
//!   machinery every heuristic row uses (`BashMutation::resolve`).
//!
//! An admitted WRITE/APPEND anchor SUPERSEDES the heuristic `BashTouch` row the
//! lexical mutation layer emits for the same command: the caller drops that row via
//! the returned suppression keys, so one command never yields both a content event
//! and a self-inflicted boundary.

use super::*;
use crate::bash_mutations::{bash_anchors, parse_bash_mutations, AnchorCmd, BashMutation, CwdAt};

/// The outcome of one turn's bash-anchor pass.
#[derive(Debug, Default)]
pub(crate) struct TurnBashAnchors {
    pub(crate) events: Vec<FileEvent>,
    /// `(line_no, resolved_path)` of admitted WRITE/APPEND anchors: the caller
    /// removes the matching heuristic `BashTouch` rows.
    pub(crate) suppress: Vec<(usize, String)>,
}

/// One turn's Bash tool_use commands, keyed by id.
struct BashUse<'a> {
    line_no: usize,
    ts: Option<String>,
    cmd: &'a str,
    cwd: Option<&'a str>,
}

/// Collect the turn's admissible bash content anchors.
pub(crate) fn collect_turn_bash_anchors(
    records: &[(usize, Record)],
    idxs: &[usize],
    target_file: Option<&str>,
    failed_ids: &std::collections::HashSet<String>,
) -> TurnBashAnchors {
    let mut out = TurnBashAnchors::default();
    if target_file.is_none() {
        return out;
    }
    let mut uses: BTreeMap<&str, BashUse> = BTreeMap::new();
    for &i in idxs {
        let (line_no, rec) = (&records[i].0, &records[i].1);
        let Some(blocks) = rec.blocks() else { continue };
        for b in blocks {
            if let Block::ToolUse {
                id: Some(id),
                name: Some(name),
                input: Some(input),
            } = b
            {
                if name == "Bash" {
                    if let Some(cmd) = input.get("command").and_then(serde_json::Value::as_str) {
                        uses.insert(
                            id.as_str(),
                            BashUse {
                                line_no: *line_no,
                                ts: rec.timestamp.clone(),
                                cmd,
                                cwd: rec.cwd.as_deref(),
                            },
                        );
                    }
                }
            }
        }
    }
    if uses.is_empty() {
        return out;
    }

    for (id, bu) in &uses {
        if failed_ids.contains(*id) {
            continue;
        }
        let anchors = bash_anchors(bu.cmd);
        if anchors.read.is_none() && anchors.writes.is_empty() {
            continue;
        }
        // The whole-command CLEAN gate for compound-command writes: exit ok is only
        // the LAST segment's verdict in a `;`/newline chain, but a failing
        // cat/tee/echo always writes stderr - a clean echo proves the write landed.
        let clean_echo =
            anchors.writes.is_empty() || !anchors.multi_segment || clean_result(records, idxs, id);
        // The command's OTHER mutation rows, resolved: an anchor whose path any
        // other part of the command also touches is refused (the later touch could
        // rewrite it and segment order is not replayed within one line).
        let resolved_rows: Vec<(String, String)> = parse_bash_mutations(bu.cmd)
            .into_iter()
            .filter(|r| !crate::bash_mutations::is_class_marker(&r.path))
            .map(|r| {
                let (res, _) = r.resolve(bu.cwd);
                (r.path, res)
            })
            .collect();
        for anchor in anchors.writes {
            if !clean_echo {
                break;
            }
            let (operand, content, heredoc, append) = match anchor {
                AnchorCmd::WriteFull {
                    operand,
                    content,
                    heredoc,
                } => (operand, content, heredoc, false),
                AnchorCmd::Append {
                    operand,
                    content,
                    heredoc,
                } => (operand, content, heredoc, true),
                AnchorCmd::ReadFull { .. } | AnchorCmd::ReadWindow { .. } => continue,
            };
            let (resolved, _res) = BashMutation {
                path: operand.clone(),
                verb: "anchor",
                cwd_at: CwdAt::Spawn,
            }
            .resolve(bu.cwd);
            // Same-path collision: the anchor's own redirect/tee row matches its
            // operand; ANY OTHER row resolving to the same file kills the anchor.
            let hits = resolved_rows
                .iter()
                .filter(|(p, res)| *p == operand || *res == resolved)
                .count();
            if hits > 1 {
                continue;
            }
            if !path_matches(target_file, &resolved) && !path_matches(target_file, &operand) {
                continue;
            }
            if append {
                out.events.push(FileEvent {
                    line_no: bu.line_no,
                    turn_index: 0, // stamped by the caller
                    timestamp_utc: bu.ts.clone(),
                    kind: EventKind::BashAppend { content },
                });
            } else {
                out.events.push(FileEvent {
                    line_no: bu.line_no,
                    turn_index: 0,
                    timestamp_utc: bu.ts.clone(),
                    kind: EventKind::FullSnapshot {
                        total_lines: line_count(&content),
                        content,
                        source: if heredoc {
                            SnapSource::BashHeredoc
                        } else {
                            SnapSource::BashWrite
                        },
                    },
                });
            }
            out.suppress.push((bu.line_no, resolved));
            out.suppress.push((bu.line_no, operand));
        }
        let Some(read) = anchors.read else { continue };
        let operand = match &read {
            AnchorCmd::ReadFull { operand } | AnchorCmd::ReadWindow { operand, .. } => {
                operand.clone()
            }
            _ => continue,
        };
        let (resolved, _res) = BashMutation {
            path: operand.clone(),
            verb: "anchor",
            cwd_at: CwdAt::Spawn,
        }
        .resolve(bu.cwd);
        if !path_matches(target_file, &resolved) && !path_matches(target_file, &operand) {
            continue;
        }
        match read {
            AnchorCmd::ReadFull { .. } => {
                if let Some((line_no, ts, stdout)) = gated_stdout(records, idxs, id) {
                    out.events.push(FileEvent {
                        line_no,
                        turn_index: 0,
                        timestamp_utc: ts,
                        kind: EventKind::FullSnapshot {
                            total_lines: line_count(&stdout),
                            content: stdout,
                            source: SnapSource::BashCat,
                        },
                    });
                }
            }
            AnchorCmd::WriteFull { .. } | AnchorCmd::Append { .. } => {}
            AnchorCmd::ReadWindow { start, end, .. } => {
                let Some((line_no, ts, stdout)) = gated_stdout(records, idxs, id) else {
                    continue;
                };
                let lines = crate::recover::split_lines(&stdout);
                if lines.is_empty() {
                    continue; // window past EOF / empty print: nothing placeable.
                }
                if let Some(e) = end {
                    let expected = e - start + 1;
                    if lines.len() > expected {
                        continue; // more lines than the window can print: not ours.
                    }
                }
                // A window from line 1 that hit EOF (fewer lines than asked, or an
                // explicit to-EOF script) is the WHOLE file - stdout verbatim.
                let hit_eof = end.is_none_or(|e| lines.len() < e - start + 1);
                if start == 1 && hit_eof {
                    out.events.push(FileEvent {
                        line_no,
                        turn_index: 0,
                        timestamp_utc: ts,
                        kind: EventKind::FullSnapshot {
                            total_lines: line_count(&stdout),
                            content: stdout,
                            source: SnapSource::BashCat,
                        },
                    });
                } else {
                    out.events.push(FileEvent {
                        line_no,
                        turn_index: 0,
                        timestamp_utc: ts,
                        kind: EventKind::BashWindowRead {
                            start_line: start,
                            lines,
                        },
                    });
                }
            }
        }
    }
    out
}

/// True when the command's result echo is CLEAN: the `toolUseResult` exists (a
/// top-level lane), stderr is empty, and the command was not interrupted. The
/// compound-command write gate: a failing cat/tee/echo always writes stderr, so a
/// clean echo proves every segment's write landed even where the exit code only
/// reflects the last segment. Carrier-less lanes cannot prove it: `false`.
fn clean_result(records: &[(usize, Record)], idxs: &[usize], id: &str) -> bool {
    for &i in idxs {
        let rec = &records[i].1;
        let Some(blocks) = rec.blocks() else { continue };
        let carries = blocks
            .iter()
            .any(|b| matches!(b, Block::ToolResult { tool_use_id: Some(tid), .. } if tid == id));
        if !carries {
            continue;
        }
        let Some(tur) = rec.tool_use_result_value() else {
            return false;
        };
        let stderr_clean = tur
            .get("stderr")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|s| s.trim().is_empty());
        let interrupted = tur
            .get("interrupted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        return stderr_clean && !interrupted;
    }
    false
}

/// The COMPLETENESS-gated stdout of a Bash result: the carrier record in this turn
/// whose `toolUseResult` echoes the command output. `None` when the carrier is
/// absent (subagent lanes), stderr is non-empty, the command was interrupted, or
/// the output was externalized to a persisted file (the inline text is not the
/// whole stdout then).
fn gated_stdout(
    records: &[(usize, Record)],
    idxs: &[usize],
    id: &str,
) -> Option<(usize, Option<String>, String)> {
    for &i in idxs {
        let (line_no, rec) = (&records[i].0, &records[i].1);
        let Some(blocks) = rec.blocks() else { continue };
        let carries = blocks
            .iter()
            .any(|b| matches!(b, Block::ToolResult { tool_use_id: Some(tid), .. } if tid == id));
        if !carries {
            continue;
        }
        let tur = rec.tool_use_result_value()?;
        let stdout = tur.get("stdout").and_then(serde_json::Value::as_str)?;
        let stderr_clean = tur
            .get("stderr")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|s| s.trim().is_empty());
        let interrupted = tur
            .get("interrupted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let persisted = tur.get("persistedOutputPath").is_some();
        if stderr_clean && !interrupted && !persisted {
            return Some((*line_no, rec.timestamp.clone(), stdout.to_string()));
        }
        return None;
    }
    None
}
