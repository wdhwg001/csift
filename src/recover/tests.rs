//! Unit tests for `recover`: per-arm branch-completeness over lightweight fixtures, in
//! the style of `files.rs` / `parse.rs`. Locale-neutral multi-byte tokens only
//! (accented Latin / emoji - `café🛠`), the house fixture style.

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("valid fixture record")
}

fn extract_events(records: &[(usize, Record)], file: &str) -> Vec<FileEvent> {
    let recs: Vec<&Record> = records.iter().map(|(_, r)| r).collect();
    let turns = group_turn_indices_deduped(&recs, |r| *r);
    extract_with_turns(records, &turns, Some(file))
}

fn numbered(lines: &[&str]) -> Vec<(usize, Record)> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i + 1, rec(l)))
        .collect()
}

mod bash_anchors;
mod boundaries;
mod coverage;
mod diff;
mod disclosure;
mod events;
mod patching;
mod render;
mod replay;
mod signals;
mod string_edits;
