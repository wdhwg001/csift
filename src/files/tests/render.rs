//! Text/JSON rendering across detail levels, footers, separators.

use super::*;
// ── scan_one_file branch coverage (mmap-None, skipped-line counting) ──

#[test]
fn render_text_all_detail_levels_run() {
    let muts = extract(&fixture());
    for d in [
        FilesDetail::Summary,
        FilesDetail::ByDir,
        FilesDetail::ByFile,
        FilesDetail::Timeline,
    ] {
        render_text(&outcome(d, muts.clone()));
    }
}

#[test]
fn render_text_empty_prints_none() {
    // The empty-mutations branch of render_text (→ "no file mutations found").
    render_text(&outcome(FilesDetail::Summary, Vec::new()));
}

#[test]
fn render_json_all_detail_levels_run() {
    let muts = extract(&fixture());
    for d in [
        FilesDetail::Summary,
        FilesDetail::ByDir,
        FilesDetail::ByFile,
        FilesDetail::Timeline,
    ] {
        render_json(&outcome(d, muts.clone())).expect("json render");
    }
}

#[test]
fn footer_filter_context_all_arms() {
    let muts = extract(&fixture());
    // turn arm.
    let mut o = outcome(FilesDetail::Summary, muts.clone());
    o.turn_range = Some("0..1".to_string());
    assert_eq!(filter_context(&o), "turn=0..1");
    // bounded time-window arm.
    let mut o2 = outcome(FilesDetail::Summary, muts.clone());
    o2.time_window_bounded = true;
    assert_eq!(filter_context(&o2), "time-window");
    // unbounded "all turns" arm.
    let o3 = outcome(FilesDetail::Summary, muts);
    assert_eq!(filter_context(&o3), "all turns");
}

#[test]
fn footer_reports_skipped_lines() {
    // The skipped-lines footer branch (> 0) fires.
    let mut o = outcome(FilesDetail::Summary, extract(&fixture()));
    o.skipped_lines = 3;
    print_footer(&o); // exercises the `skipped_lines > 0` true arm
}

#[test]
fn render_multi_session_separator() {
    // Two sessions → the per_session blank-line separator arm (`!first`) fires.
    let ln: Vec<usize> = (1..=fixture().len()).collect();
    let mut a = extract_mutations("aaaa-sess", &fixture(), &ln);
    let b = extract_mutations("bbbb-sess", &fixture(), &ln);
    a.extend(b);
    render_text(&outcome(FilesDetail::Summary, a));
}

#[test]
fn timeline_renders_heuristic_and_non_heuristic() {
    // The timeline heuristic-label ternary both arms: a Write (no label) and a
    // Bash mutation (heuristic label) in the same session.
    render_timeline(&outcome(FilesDetail::Timeline, extract(&fixture())));
}

#[test]
fn json_grouped_emits_per_group_with_timestamps() {
    // A by-dir JSON render where groups carry first/last timestamps (the
    // `first_local`/`last_local` Some arms via local_iso) + the distinct-file count.
    render_json(&outcome(FilesDetail::ByDir, extract(&fixture()))).expect("json");
}
