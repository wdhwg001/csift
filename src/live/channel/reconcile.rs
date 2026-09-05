//! Ledger-against-transcript reconciliation: the join `msg` and `ack` are built on.
//!
//! The channel keeps two records of a delivery and they answer different questions. The
//! per-lane LEDGER is csift's own INTENT: an `emit` line says this binary printed a chunk
//! at a hook point. The receiver's TRANSCRIPT is the FACT: a delivered chunk lands there
//! as a `hook_additional_context` attachment whose content opens with the envelope header,
//! so a record carrying `id=<id>` proves the text reached that lane's context.
//!
//! Promoting intent into fact is exactly the failure the official team mailbox cannot
//! avoid - it deletes an entry on consume and never flips its `read` flag, so afterwards
//! nothing on disk distinguishes "delivered" from "never written". Here the two halves stay
//! separate files and the verdict names which of them is missing.
//!
//! The transcript half is a byte scan, not a parse of every line: the envelope needle and
//! the id are both raw ASCII substrings of the injected content, so a line that cannot
//! carry the delivery is rejected before any JSON is built.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use memchr::memmem;

use crate::model::Record;
use crate::parse::{
    mmap_bytes, non_candidate_verdict, parse_line, scan_lines_parallel, LineVerdict,
};

use super::{
    is_expired, now_utc, parse_header, read_inbox, read_ledger, read_message, states, InboxLine,
    LedgerLine, Message, MessageState,
};

/// Where one message stands once intent and fact are joined. A closed set: every other
/// shape a caller might imagine (half-emitted, redelivered) folds into one of these seven,
/// and the row's own fields carry the detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MsgVerdict {
    /// Emitted AND present in the receiver lane's transcript.
    Delivered,
    /// Emitted, with no transcript record: a crash window, or a flush that has not landed.
    IntentOnly,
    /// Enqueued, nothing emitted yet.
    Queued,
    /// A hold with no later emit.
    Held,
    /// An `expired` ledger line, or the ttl elapsed with nothing emitted.
    Expired,
    /// The receiver said it read the message.
    Acked,
    /// csift declined to act on it.
    Refused,
}

impl MsgVerdict {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MsgVerdict::Delivered => "DELIVERED",
            MsgVerdict::IntentOnly => "INTENT-ONLY",
            MsgVerdict::Queued => "QUEUED",
            MsgVerdict::Held => "HELD",
            MsgVerdict::Expired => "EXPIRED",
            MsgVerdict::Acked => "ACKED",
            MsgVerdict::Refused => "REFUSED",
        }
    }
}

/// The transcript half: where a delivery of one message id was found in the receiver
/// lane's own file. The FIRST chunk record wins - a multi-part delivery lands as several
/// records and the earliest one is the moment the lane saw the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fact {
    pub(crate) line: usize,
    pub(crate) uuid: Option<String>,
}

/// One reconciled message: the message source (when it survives), its inbox line, its
/// folded ledger state and the transcript proof.
#[derive(Debug, Clone)]
pub(crate) struct MsgReport {
    pub(crate) id: String,
    pub(crate) verdict: MsgVerdict,
    pub(crate) lane: String,
    pub(crate) session: String,
    pub(crate) message: Option<Message>,
    pub(crate) inbox: Option<InboxLine>,
    pub(crate) state: MessageState,
    pub(crate) emits: Vec<LedgerLine>,
    pub(crate) fact: Option<Fact>,
}

impl MsgReport {
    /// The instant the row sorts on: when the message was enqueued, falling back to the
    /// first ledger line for a message whose inbox line is gone.
    pub(crate) fn sort_key(&self) -> &str {
        self.inbox
            .as_ref()
            .map_or_else(
                || self.state.first_ts_utc.as_deref(),
                |i| Some(i.enqueued_utc.as_str()),
            )
            .unwrap_or("")
    }
}

/// The lane a channel command operates on, with everything derived from it once.
#[derive(Debug, Clone)]
pub(crate) struct LaneCtx {
    /// The lane's own transcript id (a session uuid, or an agent id for a child lane).
    pub(crate) lane: String,
    /// The OWNING top-level session uuid: the channel root is keyed by session, the files
    /// inside it by lane.
    pub(crate) session: String,
    /// The lane's own transcript - the file the FACT is read from.
    pub(crate) transcript: PathBuf,
    /// `<session sidecar>/csift-channel`. Derived, never required to exist: a reader
    /// treats an absent root as an empty channel.
    pub(crate) root: PathBuf,
}

/// Resolve a lane from an `@`-target through the shared resolver, so every id form csift
/// prints (uuid, uuid prefix, agent id, teammate routing form, `@main`, `@trap:`) names a
/// lane here too.
///
/// A leading `@` is optional: a lane is always an id, so there is no path form to confuse
/// it with, and requiring the sigil would only turn a natural invocation into an error.
pub(crate) fn lane_from_target(target: &str) -> Result<LaneCtx> {
    let token = if target.starts_with('@') {
        target.to_string()
    } else {
        format!("@{target}")
    };
    let files = crate::path::resolve_session_files(
        &[PathBuf::from(&token)],
        crate::path::SubagentScope::TopLevelOnly,
        crate::path::Caller::Other,
    )?;
    match files.len() {
        0 => bail!("no transcript found for lane `{token}`"),
        1 => lane_from_transcript(&files[0]),
        _ => {
            let ids: Vec<String> = files
                .iter()
                .map(|p| crate::subagent::session_id_from_path(p))
                .collect();
            bail!(
                "`{token}` names {} lanes ({}) - a channel command operates on exactly one \
                 lane, so name it by its own transcript id",
                files.len(),
                ids.join(", ")
            )
        }
    }
}

/// Build the lane context from a concrete transcript path.
pub(crate) fn lane_from_transcript(transcript: &Path) -> Result<LaneCtx> {
    let lane = crate::subagent::session_id_from_path(transcript);
    if lane.is_empty() {
        bail!("cannot read a lane id from {}", transcript.display());
    }
    let session =
        crate::subagent::parent_session_id_from_path(transcript).unwrap_or_else(|| lane.clone());
    let Some(sidecar) = session_sidecar_dir(transcript) else {
        bail!(
            "cannot locate the session sidecar directory for {}",
            transcript.display()
        );
    };
    Ok(LaneCtx {
        lane,
        session,
        transcript: transcript.to_path_buf(),
        root: super::channel_dir(&sidecar),
    })
}

/// The `<projects>/<encoded>/<session-uuid>/` directory beside `subagents/`, derived from
/// any lane's transcript path.
///
/// Derived rather than looked up through [`crate::subagent::sidecar_dir_for_session`],
/// which requires the directory to already exist: a top-level session that has never been
/// written to has no sidecar yet, and a reader must read that as an empty channel while a
/// writer creates it on demand.
fn session_sidecar_dir(transcript: &Path) -> Option<PathBuf> {
    // A child lane lives at `<sidecar>/subagents/[workflows/wf_*/]agent-<id>.jsonl`, so the
    // sidecar is the parent of the `subagents` component however deeply the lane is nested.
    for dir in transcript.ancestors() {
        if dir.file_name().and_then(|s| s.to_str()) == Some("subagents") {
            return dir.parent().map(Path::to_path_buf);
        }
    }
    let stem = transcript.file_stem()?.to_str()?;
    Some(transcript.parent()?.join(stem))
}

/// Every message id the lane's channel files know about, with its inbox line and folded
/// ledger state. The union of the two files: a message can be enqueued with no ledger line
/// (nothing has run yet) and a ledger line can outlive a hand-pruned inbox.
pub(crate) struct LaneView {
    pub(crate) inboxes: BTreeMap<String, InboxLine>,
    pub(crate) states: BTreeMap<String, MessageState>,
    pub(crate) emits: BTreeMap<String, Vec<LedgerLine>>,
    /// Channel lines the current schema could not read, counted and never dropped in
    /// silence.
    pub(crate) skipped_lines: usize,
}

impl LaneView {
    pub(crate) fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.inboxes.keys().cloned().collect();
        for id in self.states.keys() {
            if !self.inboxes.contains_key(id) {
                ids.push(id.clone());
            }
        }
        ids.sort();
        ids.dedup();
        ids
    }
}

/// Read a lane's inbox and ledger once.
pub(crate) fn read_lane(ctx: &LaneCtx) -> Result<LaneView> {
    let (inbox_lines, inbox_skipped) = read_inbox(&ctx.root, &ctx.lane)?;
    let (ledger_lines, ledger_skipped) = read_ledger(&ctx.root, &ctx.lane)?;
    let mut inboxes: BTreeMap<String, InboxLine> = BTreeMap::new();
    for line in inbox_lines {
        // Append-only: a re-enqueued id keeps the FIRST line, which is when the lane was
        // actually asked to carry the message.
        inboxes.entry(line.id.clone()).or_insert(line);
    }
    let mut emits: BTreeMap<String, Vec<LedgerLine>> = BTreeMap::new();
    for line in &ledger_lines {
        if matches!(line, LedgerLine::Emit { .. }) {
            if let Some(id) = line.id() {
                emits.entry(id.to_string()).or_default().push(line.clone());
            }
        }
    }
    Ok(LaneView {
        inboxes,
        states: states(&ledger_lines),
        emits,
        skipped_lines: inbox_skipped + ledger_skipped,
    })
}

/// Scan a lane's transcript for csift-channel deliveries, returning the FIRST record of
/// each message id plus the count of unreadable lines.
///
/// `only` narrows the byte prefilter to one id, which is what makes a single-message
/// reconciliation cheap on a large transcript: both the envelope needle and `id=<id>` are
/// plain ASCII runs of the injected content with no JSON-escaped character, so a line
/// missing either cannot carry that delivery and is never parsed.
///
/// An unreadable transcript is an ERROR, never an empty result: absence of the file is not
/// absence of the record, and reading it as one would report a delivered message as
/// intent-only.
pub(crate) fn scan_facts(
    transcript: &Path,
    only: Option<&str>,
) -> Result<(BTreeMap<String, Fact>, usize)> {
    let Some(map) = mmap_bytes(transcript)? else {
        return Ok((BTreeMap::new(), 0));
    };
    let channel = memmem::Finder::new(Record::CSIFT_CHANNEL_NEEDLE.as_bytes());
    let id_pattern = only.map(|id| format!("id={id}"));
    let id_needle = id_pattern
        .as_ref()
        .map(|p| memmem::Finder::new(p.as_bytes()));
    let (hits, skipped) = scan_lines_parallel(&map[..], |line, line_no| {
        if channel.find(line).is_none()
            || id_needle.as_ref().is_some_and(|f| f.find(line).is_none())
        {
            // The never-silent law: a non-candidate line still gets the O(1) shape check,
            // so corrupt bytes are counted rather than exempted by the prefilter.
            return non_candidate_verdict(line);
        }
        match parse_line(line) {
            Ok(Some(rec)) => match fact_id(&rec) {
                Some(id) if only.is_none_or(|want| want == id) => LineVerdict::Keep((
                    id,
                    Fact {
                        line: line_no,
                        uuid: rec.uuid.clone(),
                    },
                )),
                _ => LineVerdict::Ignore,
            },
            Ok(None) => LineVerdict::Ignore,
            Err(_) => LineVerdict::Skip,
        }
    });
    let mut out: BTreeMap<String, Fact> = BTreeMap::new();
    for (id, fact) in hits {
        // Keep the FIRST record of each id: a multi-part delivery lands as several chunks
        // and the earliest one is when the lane saw the header.
        out.entry(id).or_insert(fact);
    }
    Ok((out, skipped))
}

/// The message id a record proves the delivery of: the envelope header parsed out of a
/// hook-context attachment's own text. A record that merely quotes an id in prose carries
/// no header at the start of its content and yields `None`.
fn fact_id(rec: &Record) -> Option<String> {
    let text = rec.csift_channel_text()?;
    parse_header(&text).map(|h| h.id)
}

/// Join one message's intent and fact into a report.
pub(crate) fn reconcile(
    ctx: &LaneCtx,
    id: &str,
    view: &LaneView,
    fact: Option<Fact>,
) -> Result<Option<MsgReport>> {
    let inbox = view.inboxes.get(id).cloned();
    let state = view.states.get(id).cloned();
    let message = read_message(&ctx.root, id)?;
    if inbox.is_none() && state.is_none() {
        return Ok(None);
    }
    let state = state.unwrap_or_default();
    let emits = view.emits.get(id).cloned().unwrap_or_default();
    let verdict = verdict_for(&state, inbox.as_ref(), fact.as_ref(), &now_utc());
    Ok(Some(MsgReport {
        id: id.to_string(),
        verdict,
        lane: ctx.lane.clone(),
        session: ctx.session.clone(),
        message,
        inbox,
        state,
        emits,
        fact,
    }))
}

/// True when the message can no longer be delivered on its own schedule: either csift
/// wrote an `expired` line, or the inbox line's deadline has passed.
///
/// Both sources count because the ledger line is only written by a delivery that ran: a
/// lane whose hooks never fire accumulates messages whose ttl elapsed with nothing on the
/// ledger at all, and reporting those as merely QUEUED would read as "still on its way".
pub(crate) fn is_message_expired(
    state: &MessageState,
    inbox: Option<&InboxLine>,
    now: &str,
) -> bool {
    if state.expired {
        return true;
    }
    inbox
        .and_then(|i| i.expires_utc.as_deref())
        .is_some_and(|deadline| is_expired(deadline, now))
}

/// The verdict ladder. Terminal states outrank transient ones, and the transcript is
/// consulted only once something was actually emitted - a message nothing emitted cannot
/// be DELIVERED however many records mention its id.
fn verdict_for(
    state: &MessageState,
    inbox: Option<&InboxLine>,
    fact: Option<&Fact>,
    now: &str,
) -> MsgVerdict {
    if state.acked {
        return MsgVerdict::Acked;
    }
    let emitted = !state.emitted_parts.is_empty();
    if !emitted && !state.refused_reasons.is_empty() {
        return MsgVerdict::Refused;
    }
    if !emitted && is_message_expired(state, inbox, now) {
        return MsgVerdict::Expired;
    }
    if !emitted && !state.held_reasons.is_empty() {
        return MsgVerdict::Held;
    }
    if emitted {
        return if fact.is_some() {
            MsgVerdict::Delivered
        } else {
            MsgVerdict::IntentOnly
        };
    }
    MsgVerdict::Queued
}
