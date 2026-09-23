//! Text/JSON rendering across detail levels, footers, separators.

use super::*;
// -- scan_one_file branch coverage (mmap-None, skipped-line counting) --

#[test]
fn render_text_all_detail_levels_run() {
    let muts = extract(&fixture());
    for d in [
        FilesDetail::Summary,
        FilesDetail::ByDir,
        FilesDetail::ByFile,
        FilesDetail::Timeline,
    ] {
        render_text(&outcome(d, muts.clone())).expect("text render");
    }
}

#[test]
fn render_text_empty_prints_none() {
    // The empty-mutations branch of render_text (-> "no file mutations found").
    render_text(&outcome(FilesDetail::Summary, Vec::new())).expect("text render");
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
fn the_footer_names_skipped_lines_only_when_there_are_some() {
    // The malformed note is a DISCLOSURE, so it must appear exactly when lines were
    // skipped: printing it at zero claims a loss that did not happen, and suppressing it
    // above zero is the silent truncation the output contract forbids.
    let mut o = outcome(FilesDetail::Summary, extract(&fixture()));
    o.skipped_lines = 0;
    let mut buf: Vec<u8> = Vec::new();
    write_footer(&mut buf, &o).expect("footer");
    let text = String::from_utf8(buf).expect("utf8");
    assert!(!text.contains("malformed"), "{text}");

    o.skipped_lines = 3;
    let mut buf: Vec<u8> = Vec::new();
    write_footer(&mut buf, &o).expect("footer");
    let text = String::from_utf8(buf).expect("utf8");
    assert!(text.contains("3 malformed line(s) skipped"), "{text}");
}

#[test]
fn a_session_header_names_its_id_domain_and_the_separator_sits_between() {
    // Two things the grouping owes a reader. A SUBAGENT group is branded as one beside its
    // re-feedable parent, because a bare agent hex read as `SESSION` is not a target any
    // command accepts; and the blank line separates two groups rather than leading the
    // first, so a `| head` read opens on the header.
    let ln: Vec<usize> = (1..=fixture().len()).collect();
    let fx = fixture();
    let mut a = extract_mutations("aaaa-sess", &fx, &ln, &view_of(&fx));
    let b = extract_mutations("bbbb-sess", &fx, &ln, &view_of(&fx));
    a.extend(b);
    let mut buf: Vec<u8> = Vec::new();
    render_summary(&mut buf, &outcome(FilesDetail::Summary, a)).expect("summary");
    let text = String::from_utf8(buf).expect("utf8");
    assert!(
        text.starts_with("SESSION aaaa-sess\n"),
        "no leading blank, and a top-level group is a SESSION: {text}"
    );
    assert!(text.contains("\n\nSESSION bbbb-sess\n"), "{text}");

    // The same two mutations branded as a subagent take the other header arm.
    let mut sub = extract_mutations("cccc-agent", &fx, &ln, &view_of(&fx));
    for m in &mut sub {
        m.is_subagent = true;
        m.parent_session_id = "dddd-parent".to_string();
    }
    let mut buf: Vec<u8> = Vec::new();
    render_summary(&mut buf, &outcome(FilesDetail::Summary, sub)).expect("summary");
    let text = String::from_utf8(buf).expect("utf8");
    assert!(
        text.starts_with("SUBAGENT cccc-agent  ·  parent SESSION dddd-parent\n"),
        "{text}"
    );
}

#[test]
fn timeline_renders_heuristic_and_non_heuristic() {
    // The timeline heuristic-label ternary both arms: a Write (no label) and a
    // Bash mutation (heuristic label) in the same session.
    render_timeline(
        &mut Vec::new(),
        &outcome(FilesDetail::Timeline, extract(&fixture())),
    )
    .expect("timeline");
}

#[test]
fn json_grouped_emits_per_group_with_timestamps() {
    // A by-dir JSON render where groups carry first/last timestamps (the
    // `first_local`/`last_local` Some arms via local_iso) + the distinct-file count.
    render_json(&outcome(FilesDetail::ByDir, extract(&fixture()))).expect("json");
}
