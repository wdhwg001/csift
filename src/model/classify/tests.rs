//! The classify engine's own cases, one file per child of `model/classify/`.
//!
//! The `classify_*.rs` files under `model/tests/` cover the engine from the OUTSIDE -
//! the label a whole record carries, the comm view, the schedule arms - while these
//! pin one family table each.

use super::*;

/// Parse one raw jsonl line into a [`Record`]; the fixtures here are hand-written
/// from the on-disk shapes the family tables name.
pub(super) fn parse(line: &str) -> Record {
    serde_json::from_str(line).expect("valid record")
}

/// A spawn lookup stub: `toolu_spawn` spawned `child-abc`; the teammate name `VSRepro`
/// resolves to its name-embedded id. Everything else is unknown (graceful degrade path).
pub(super) struct FakeSpawn;
impl SpawnLookup for FakeSpawn {
    fn child_for_spawn_tool_use_id(&self, id: &str) -> Option<String> {
        (id == "toolu_spawn").then(|| "child-abc".to_string())
    }
    fn child_for_spawn_name(&self, name: &str) -> Option<String> {
        (name == "VSRepro").then(|| "aVSRepro-deadbeef".to_string())
    }
}

/// The leaf set one raw line classifies to, under a bare top-level context.
pub(super) fn labels(line: &str) -> Vec<Class> {
    parse(line).classify(&ClassifyCtx::top_level())
}

mod attachments;
mod bookkeeping;
mod channel;
mod injections;
mod resume;
mod support;
mod system;
mod thinking;
