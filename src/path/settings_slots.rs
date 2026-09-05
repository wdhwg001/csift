//! The delivery-slot census over a merged settings model: which `csift deliver --slot k`
//! command hooks one event carries, read by a token walk that cannot fail.

use super::settings::{hooks_for_event, Merged};

/// The delivery slot numbers configured on one event: the `k` of every
/// `csift deliver --slot k` command hook, sorted and deduplicated because a slot is a
/// position in the delivery chain, so the same k under two matchers is one slot.
pub(crate) fn deliver_slots(m: &Merged, event: &str) -> Vec<u32> {
    let mut out: Vec<u32> = hooks_for_event(m, event)
        .iter()
        .filter_map(|h| deliver_slot_of(&h.command))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The `k` of a `csift deliver --slot k` command line, by a whitespace token walk (the
/// binary token bare or path-prefixed, the three words adjacent): a library path never
/// constructs a regex that could fail.
fn deliver_slot_of(command: &str) -> Option<u32> {
    let toks: Vec<&str> = command.split_whitespace().collect();
    let i = toks.iter().position(|t| {
        let t = t.trim_end_matches(".exe");
        t == "csift" || t.ends_with("/csift") || t.ends_with("\\csift")
    })?;
    if toks.get(i + 1) != Some(&"deliver") || toks.get(i + 2) != Some(&"--slot") {
        return None;
    }
    toks.get(i + 3)?.parse::<u32>().ok()
}
