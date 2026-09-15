//! The hook input object `csift deliver` reads on stdin, and the two event families the
//! delivery modes ride.
//!
//! Parsing is TOLERANT in one direction only. Unknown fields are ignored, an absent
//! optional field is `None`, and a field of the wrong JSON type reads as absent - the
//! harness adds event extras between builds and a delivery must not die on one. But the
//! two fields a delivery cannot work without are checked by the caller, not defaulted
//! here: a served or remote call carries `session_id:"served:<caller>"` and an EMPTY
//! `transcript_path`, and treating that shape as a lane would write a message into a
//! directory no receiver reads.
//!
//! The event families are the whole of the mode contract. `steer` rides any of the eight
//! delivery events; `queue` rides only a turn boundary, which includes the two re-entry
//! forms of SessionStart (a resume or a post-compaction restart is a turn boundary for
//! the lane even though the event name is the same one that fires at startup).

use serde_json::Value;

use super::{str_field, STEER_EVENTS};

/// The events that end (or restart) a turn, where a `queue` message may ride.
const QUEUE_EVENTS: [&str; 3] = ["UserPromptSubmit", "Stop", "SubagentStop"];

/// The two `SessionStart` sources that are a RE-ENTRY into an existing lane rather than a
/// fresh start, so a queue message may ride them.
const REENTRY_SOURCES: [&str; 2] = ["resume", "compact"];

/// The base hook input plus the per-event extras `deliver` acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookInput {
    pub(crate) session_id: String,
    pub(crate) transcript_path: String,
    pub(crate) cwd: Option<String>,
    /// Present only when the hook fires from within a subagent lane, and on the two
    /// subagent events, which declare it as their own required extra.
    pub(crate) agent_id: Option<String>,
    pub(crate) agent_type: Option<String>,
    pub(crate) hook_event_name: String,
    /// `SessionStart` only: `startup` | `resume` | `clear` | `compact` | `fork`.
    pub(crate) source: Option<String>,
    /// `Stop` / `SubagentStop`: true when the turn is already continuing because a stop
    /// hook blocked it, which is the harness telling this hook to stop blocking.
    pub(crate) stop_hook_active: bool,
    /// `SubagentStop` only: the CHILD lane's own transcript, distinct from
    /// `transcript_path`, which names the top-level session file in every lane.
    pub(crate) agent_transcript_path: Option<String>,
}

impl HookInput {
    /// Parse the hook payload. `None` for anything that is not a JSON object, which is
    /// the one stdin shape a delivery can say nothing about.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(raw).ok()?;
        if !v.is_object() {
            return None;
        }
        Some(HookInput {
            session_id: str_field(&v, "session_id").unwrap_or_default(),
            transcript_path: str_field(&v, "transcript_path").unwrap_or_default(),
            cwd: str_field(&v, "cwd"),
            agent_id: str_field(&v, "agent_id").filter(|s| !s.trim().is_empty()),
            agent_type: str_field(&v, "agent_type"),
            hook_event_name: str_field(&v, "hook_event_name").unwrap_or_default(),
            source: str_field(&v, "source"),
            stop_hook_active: v
                .get("stop_hook_active")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            agent_transcript_path: str_field(&v, "agent_transcript_path"),
        })
    }

    /// The lane this event belongs to: the subagent id when the payload names one, else
    /// the top-level session. `session_id` is ALWAYS the top-level uuid, in every lane,
    /// so it is the correct fallback and never a guess.
    pub(crate) fn lane(&self) -> &str {
        self.agent_id.as_deref().unwrap_or(&self.session_id)
    }

    /// True when this is a Stop-family event, the only place the exit-2 vehicle exists.
    pub(crate) fn is_stop_family(&self) -> bool {
        self.hook_event_name == "Stop" || self.hook_event_name == "SubagentStop"
    }

    /// True when this is a `SessionStart` carrying the named source.
    pub(crate) fn session_start_source(&self, want: &str) -> bool {
        self.hook_event_name == "SessionStart" && self.source.as_deref() == Some(want)
    }
}

/// True when a steer message may ride this event. Every other event exits 0 with nothing:
/// the hook line is identical on all of them, so the filter lives here rather than in what
/// the user pasted.
pub(crate) fn is_steer_event(event: &str) -> bool {
    STEER_EVENTS.contains(&event)
}

/// True when a queue message may ride this event: a turn boundary, or one of the two
/// `SessionStart` re-entry sources.
pub(crate) fn is_queue_event(event: &str, source: Option<&str>) -> bool {
    if QUEUE_EVENTS.contains(&event) {
        return true;
    }
    event == "SessionStart" && source.is_some_and(|s| REENTRY_SOURCES.contains(&s))
}
