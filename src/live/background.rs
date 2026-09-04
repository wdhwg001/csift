//! Background tasks: the off-turn work a session launched and whether it ever came
//! back. Three kinds, measured on the corpus (v0.10.0):
//!
//! - a backgrounded SHELL: a `Bash` tool_use with `input.run_in_background:true`; its
//!   tool_result arrives within milliseconds ("Command running in background with ID:
//!   <id>. Output is being written to: <path> ..."), so the tail state machine pairs it
//!   at once - it is invisible to the unreturned-call logic by construction;
//! - an async AGENT: a tool_result whose `toolUseResult` is `{isAsync:true,
//!   status:"async_launched", agentId, description, outputFile}`;
//! - a MONITOR: the `Monitor` tool_use (a command whose stdout lines are events, or a
//!   websocket), armed on disk as an immediately-paired pair whose result reads `Monitor
//!   started (task <id>, …)` with `toolUseResult.taskId`. It shares the `b…` id namespace
//!   with backgrounded shells (both are `local_bash` tasks in the harness), so only the
//!   tool name tells them apart. Event pulses (`Monitor event: …`, no `<status>`) never
//!   close it; a termination notice (`<status>completed</status>`, summary opening
//!   `Monitor`) or a timeout event does; a PERSISTENT monitor never returns by design
//!   (measured: 30% of armed monitors produced no notification at all).
//!
//! Completion is a `<task-notification>` whose `<tool-use-id>` equals the launching
//! tool_use id (an exact join; the 9-char `backgroundTaskId` is a second key, absent on
//! 43% of subagent-lane launches), with `<status>` completed | failed | killed | stopped.
//! It rides THREE carriers: a `type:"user"` string record when the session was idle,
//! or (40% of returned shells) a `queue-operation` enqueue + remove and a
//! `queued_command` attachment when it landed mid-turn - never a user record. A shell
//! launched from a SUBAGENT lane is completed in the PARENT main transcript (607/618
//! measured; zero notifications exist in any subagent transcript), so this scan reads
//! launches from every lane and completions from the main file.
//!
//! At the next session start Claude Code reconciles orphans itself: one notification
//! carrying several `<task-id>` tags plus `__orphan_summary__:shell`, status `stopped`,
//! whose summary says the tasks "may have been stopped (via the UI, Monitor timeout, or
//! agent teardown - these leave no transcript marker)". That sentence is the honesty
//! bound: NOT RETURNED IS NOT PROOF OF STILL RUNNING. The `<output-file>` from the
//! launch is a real file (an agent's is a symlink to its transcript); its size and
//! mtime are an independent "still producing output" signal, one `stat` per open task.
//!
//! Never-returned launches sit 56-375 MB before EOF on real files, so this is a whole-
//! file scan behind a five-needle byte prefilter (measured +0.2-0.4 s worst case), not a
//! tail read.

use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BgKind {
    Shell,
    Agent,
    Monitor,
}

impl BgKind {
    #[must_use]
    pub(crate) fn slug(self) -> &'static str {
        match self {
            BgKind::Shell => "shell",
            BgKind::Agent => "agent",
            BgKind::Monitor => "monitor",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BgState {
    /// Launched, no completion carrier names it yet.
    Open,
    Completed,
    Failed,
    Killed,
    /// Claude Code's own orphan reconciliation at the next session start, or an
    /// explicit `stopped` status.
    Stopped,
    /// A Monitor whose timeout fired (the `[Monitor timed out …]` event).
    TimedOut,
    /// The remote-agent notifier's fifth terminal value (`<task-id> is blocked: ...`;
    /// Claude Code 2.1.258 emits it from the cloud-agent poll branch). Not seen on disk
    /// in a local-only corpus; a producer-only claim in the ledger (BG-011).
    Blocked,
    /// A status literal csift does not know. Rendered as its own bucket so a new harness
    /// value is disclosed instead of being booked as completed (v0.10.3).
    Other,
}

impl BgState {
    #[must_use]
    pub(crate) fn slug(self) -> &'static str {
        match self {
            BgState::Open => "open",
            BgState::Completed => "completed",
            BgState::Failed => "failed",
            BgState::Killed => "killed",
            BgState::Stopped => "stopped",
            BgState::TimedOut => "timed-out",
            BgState::Blocked => "blocked",
            BgState::Other => "other",
        }
    }

    pub(crate) fn from_status(status: Option<&str>) -> Self {
        match status.map(str::trim) {
            None | Some("") | Some("completed") => BgState::Completed,
            Some("failed") => BgState::Failed,
            Some("killed") => BgState::Killed,
            Some("stopped") => BgState::Stopped,
            Some("blocked") => BgState::Blocked,
            Some(_) => BgState::Other,
        }
    }
}

/// One launched background task.
#[derive(Debug, Clone)]
pub(crate) struct BgTask {
    pub(crate) kind: BgKind,
    /// The harness id: the 9-char shell task id or the 17-char agent id.
    pub(crate) id: Option<String>,
    pub(crate) tool_use_id: String,
    pub(crate) description: Option<String>,
    /// The shell command (verbatim; shells only).
    pub(crate) command: Option<String>,
    pub(crate) launched_utc: Option<String>,
    /// The transcript that launched it (a session uuid or a bare agent hex).
    pub(crate) lane: String,
    pub(crate) output_file: Option<String>,
    pub(crate) state: BgState,
    pub(crate) returned_utc: Option<String>,
    /// `stat` of the output file for an OPEN task: bytes and seconds since last write.
    pub(crate) output_bytes: Option<u64>,
    pub(crate) output_age_secs: Option<i64>,
    /// The lens rule that excluded this OPEN task from the verdict, when one did.
    pub(crate) ignored_by: Option<String>,
}

impl BgTask {
    /// The text the `--ignore-background` regex runs over: description + command.
    fn haystack(&self) -> String {
        let mut s = self.description.clone().unwrap_or_default();
        if let Some(c) = &self.command {
            s.push(' ');
            s.push_str(c);
        }
        s
    }

    pub(crate) fn is_open(&self) -> bool {
        self.state == BgState::Open
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BackgroundReport {
    /// Open counted tasks first (newest launch first), then open ignored, then closed.
    pub(crate) tasks: Vec<BgTask>,
    /// Unjoinable facts (an agents-stopped notice names no id).
    pub(crate) notes: Vec<String>,
    pub(crate) scanned_files: usize,
}

impl BackgroundReport {
    pub(crate) fn open_counted(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.is_open() && t.ignored_by.is_none())
            .count()
    }

    pub(crate) fn open_ignored(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.is_open() && t.ignored_by.is_some())
            .count()
    }

    /// `(completed, failed, killed, stopped, timed_out)`.
    pub(crate) fn closed_counts(&self) -> (usize, usize, usize, usize, usize) {
        let n = |st: BgState| self.tasks.iter().filter(|t| t.state == st).count();
        (
            n(BgState::Completed),
            n(BgState::Failed),
            n(BgState::Killed),
            n(BgState::Stopped),
            n(BgState::TimedOut),
        )
    }

    /// `(blocked, other)`: the two buckets outside the classic five, rendered only when
    /// non-zero.
    pub(crate) fn rare_counts(&self) -> (usize, usize) {
        let n = |st: BgState| self.tasks.iter().filter(|t| t.state == st).count();
        (n(BgState::Blocked), n(BgState::Other))
    }

    /// The one-line evidence value for the verdict table.
    pub(crate) fn summary_line(&self) -> String {
        let (c, f, k, s, t) = self.closed_counts();
        let (b, o) = self.rare_counts();
        let rare = [(b, "blocked"), (o, "with an unknown status")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, w)| format!(", {n} {w}"))
            .collect::<String>();
        let ignored = self.open_ignored();
        let ignored = if ignored > 0 {
            format!(" (+{ignored} ignored by the lens)")
        } else {
            String::new()
        };
        let timed = if t > 0 {
            format!(", {t} timed out")
        } else {
            String::new()
        };
        format!(
            "{} open{ignored}; {c} completed, {f} failed, {k} killed, {s} stopped{timed}{rare}",
            self.open_counted()
        )
    }
}

/// The operator's lens over open tasks: a launch-time cutoff and command patterns.
#[derive(Debug, Default)]
pub(crate) struct BackgroundLens {
    pub(crate) since: Option<jiff::Timestamp>,
    pub(crate) since_raw: Option<String>,
    pub(crate) ignore: Vec<(String, regex::Regex)>,
}

impl BackgroundLens {
    /// Build from the raw `--background-since` / `--ignore-background` values.
    pub(crate) fn from_args(since: Option<&str>, ignore: &[String]) -> Result<Self> {
        let since_ts = since
            .map(crate::time_window::parse_bound)
            .transpose()
            .map_err(|e| anyhow::anyhow!("--background-since: {e}"))?;
        let mut compiled = Vec::new();
        for raw in ignore {
            let re = regex::Regex::new(raw)
                .map_err(|e| anyhow::anyhow!("--ignore-background: bad regex `{raw}`: {e}"))?;
            compiled.push((raw.clone(), re));
        }
        Ok(Self {
            since: since_ts,
            since_raw: since.map(str::to_string),
            ignore: compiled,
        })
    }

    pub(crate) fn is_active(&self) -> bool {
        self.since.is_some() || !self.ignore.is_empty()
    }

    fn ignored_by(&self, t: &BgTask) -> Option<String> {
        if let (Some(since), Some(raw)) = (self.since, t.launched_utc.as_deref()) {
            if let Ok(ts) = raw.parse::<jiff::Timestamp>() {
                if ts < since {
                    return Some(format!(
                        "launched before --background-since {}",
                        self.since_raw.as_deref().unwrap_or("?")
                    ));
                }
            }
        }
        let hay = t.haystack();
        for (raw, re) in &self.ignore {
            if re.is_match(&hay) {
                return Some(format!("matches --ignore-background {raw}"));
            }
        }
        None
    }
}

/// The MAIN transcript a lane belongs to: the file itself for a top-level session, the
/// `<uuid>.jsonl` beside the `<uuid>/subagents/` tree for a subagent transcript.
pub(crate) fn main_transcript_for(path: &Path) -> PathBuf {
    if !crate::subagent::is_subagent_path(path) {
        return path.to_path_buf();
    }
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.file_name().and_then(|n| n.to_str()) == Some("subagents") {
            if let Some(session_dir) = d.parent() {
                return session_dir.with_extension("jsonl");
            }
        }
        dir = d.parent();
    }
    path.to_path_buf()
}

/// Scan the session for background launches and their completions, then apply the lens.
pub(crate) fn background_report(
    target: &Path,
    want_subagents: bool,
    lens: &BackgroundLens,
) -> Result<BackgroundReport> {
    let main = main_transcript_for(target);
    let mut files: Vec<PathBuf> = vec![main.clone()];
    if crate::subagent::is_subagent_path(target) {
        files.push(target.to_path_buf());
    } else if want_subagents {
        files.extend(crate::subagent::subagent_transcript_files(&main).unwrap_or_default());
    }

    let mut tasks: BTreeMap<String, BgTask> = BTreeMap::new();
    let mut carriers: Vec<Carrier> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    for file in &files {
        let Some(mmap) = mmap_bytes(file)? else {
            continue;
        };
        scanned += 1;
        let bytes: &[u8] = &mmap;
        let lane = crate::subagent::session_id_from_path(file);
        let is_main = *file == main;
        let mut pos = 0usize;
        while pos < bytes.len() {
            let end = memchr::memchr(b'\n', &bytes[pos..]).map_or(bytes.len(), |i| pos + i);
            let line = &bytes[pos..end];
            pos = end + 1;
            if !line_is_bg_candidate(line) {
                continue;
            }
            let Ok(Some(rec)) = crate::parse::parse_line(line) else {
                continue;
            };
            ingest_launches(&rec, &lane, &mut tasks);
            if is_main {
                ingest_carriers(&rec, &mut carriers, &mut notes);
            }
        }
    }
    resolve_carriers(&mut tasks, &carriers, &mut notes);

    let mut list: Vec<BgTask> = tasks.into_values().collect();
    for t in &mut list {
        if t.is_open() {
            t.ignored_by = lens.ignored_by(t);
            stat_output(t);
        }
    }
    // Open counted first, then open ignored, then closed; newest launch first within each.
    list.sort_by(|a, b| {
        let rank = |t: &BgTask| match (t.is_open(), t.ignored_by.is_some()) {
            (true, false) => 0,
            (true, true) => 1,
            _ => 2,
        };
        (rank(a), std::cmp::Reverse(a.launched_utc.clone()))
            .cmp(&(rank(b), std::cmp::Reverse(b.launched_utc.clone())))
    });
    Ok(BackgroundReport {
        tasks: list,
        notes,
        scanned_files: scanned,
    })
}
