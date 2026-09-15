//! Unit tests for `recover`: per-arm branch-completeness over lightweight fixtures, in
//! the style of `files.rs` / `parse.rs`. Locale-neutral multi-byte tokens only
//! (accented Latin / emoji - `café🛠`), the house fixture style.

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).expect("valid fixture record")
}

/// The survival view over a COMPLETE fixture record list - every line is a record here,
/// so the spine is empty and the chain sees the whole DAG already.
fn view_of(records: &[(usize, Record)]) -> ChainView {
    ChainView::build(records, &[])
}

fn extract_events(records: &[(usize, Record)], file: &str) -> Vec<FileEvent> {
    extract_with_turns(records, &view_of(records), Some(file)).0
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
mod snapshots;
mod string_edits;
