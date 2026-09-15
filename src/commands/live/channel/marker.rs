//! `armed/<lane>.json` - the runtime proof that delivery hooks are actually installed on
//! a lane.
//!
//! A sender can read the receiver's SETTINGS and count how many `csift deliver --slot k`
//! entries are configured, but configuration is not arming: a hook may be configured in
//! a scope the receiver's process never merged, or the process may predate the edit.
//! This marker is written by `deliver` itself on every event it handles, so it is the
//! one file that says a slot has really run in that lane. Configured slots and armed
//! slots are therefore reported as two different numbers, never conflated.
//!
//! It is rewritten whole (temp plus rename) rather than appended: it holds current
//! state, and a torn read of current state is worse than a slightly stale one.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use super::{armed_path, read_json_object, str_field, write_atomic};

/// The armed marker of one lane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ArmedMarker {
    /// Every slot number that has run in this lane, ascending and deduplicated.
    pub(crate) slots_seen: BTreeSet<u32>,
    pub(crate) last_event: Option<String>,
    pub(crate) last_ts_utc: Option<String>,
    pub(crate) hook_session: Option<String>,
    pub(crate) claude_code_version: Option<String>,
}

impl ArmedMarker {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "slots_seen": self.slots_seen.iter().copied().collect::<Vec<u32>>(),
            "last_event": self.last_event,
            "last_ts_utc": self.last_ts_utc,
            "hook_session": self.hook_session,
            "claude_code_version": self.claude_code_version,
        })
    }

    pub(crate) fn from_json(v: &Value) -> Self {
        let slots_seen = v
            .get("slots_seen")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_u64)
                    .filter_map(|n| u32::try_from(n).ok())
                    .collect()
            })
            .unwrap_or_default();
        ArmedMarker {
            slots_seen,
            last_event: str_field(v, "last_event"),
            last_ts_utc: str_field(v, "last_ts_utc"),
            hook_session: str_field(v, "hook_session"),
            claude_code_version: str_field(v, "claude_code_version"),
        }
    }
}

/// Read a lane's armed marker. `None` means no delivery hook has ever run in this lane
/// (or the file is unreadable) - which is a fact a sender needs, not an error.
pub(crate) fn read_armed(root: &Path, lane: &str) -> Result<Option<ArmedMarker>> {
    let path = armed_path(root, lane)?;
    Ok(read_json_object(&path).map(|v| ArmedMarker::from_json(&v)))
}

/// Record that `slot` ran on `event` in this lane, and return the marker as written.
///
/// The slot set ACCUMULATES: slots fire one per hook entry and per event, so a marker
/// refreshed by slot 2 must not erase the knowledge that slot 1 exists. A field the
/// caller does not know (the Claude Code version, for one) keeps its previous value
/// instead of being overwritten with null.
pub(crate) fn refresh_armed(
    root: &Path,
    lane: &str,
    slot: u32,
    event: &str,
    ts_utc: &str,
    hook_session: Option<&str>,
    claude_code_version: Option<&str>,
) -> Result<ArmedMarker> {
    let path = armed_path(root, lane)?;
    let mut marker =
        read_json_object(&path).map_or_else(ArmedMarker::default, |v| ArmedMarker::from_json(&v));
    marker.slots_seen.insert(slot);
    marker.last_event = Some(event.to_string());
    marker.last_ts_utc = Some(ts_utc.to_string());
    if let Some(session) = hook_session {
        marker.hook_session = Some(session.to_string());
    }
    if let Some(version) = claude_code_version {
        marker.claude_code_version = Some(version.to_string());
    }
    write_atomic(&path, &serde_json::to_string(&marker.to_json())?)?;
    Ok(marker)
}
