//! How `show` names the `--line` addresses it could not honour: a miss spells a range in the
//! grammar's own `A..B`, the form `--line` accepts back.

use crate::harness::*;

/// A range wholly past the end of the file is a miss in BOTH fetch modes, and each names it
/// `L<a>..<b>`. `A-B` is refused on input, so the dash spelling handed the reader an address
/// the parser rejects.
#[test]
fn a_range_miss_is_spelled_in_the_range_grammar() {
    let h = populated_home();
    let target = at(SESS);
    for extra in [&[][..], &["--raw"][..]] {
        let mut argv = vec!["show", target.as_str(), "--line", "900..1000"];
        argv.extend_from_slice(extra);
        let out = h.run(&argv);
        assert!(
            !out.success,
            "a range past the end is a miss: {}",
            out.stdout
        );
        assert!(
            out.stderr.contains("L900..1000") && !out.stderr.contains("L900-1000"),
            "the miss names the range as `--line` spells it ({extra:?}): {}",
            out.stderr
        );
    }
}

/// An OPEN range resolves its end against the file, and the miss prints the resolved pair in
/// the same grammar rather than the dash.
#[test]
fn an_open_range_miss_prints_the_resolved_pair_with_two_dots() {
    let h = populated_home();
    let out = h.run(&["show", at(SESS).as_str(), "--line", "900.."]);
    assert!(!out.success, "{}", out.stdout);
    assert!(
        out.stderr.contains("L900..") && !out.stderr.contains("L900-"),
        "{}",
        out.stderr
    );
}

/// The fixture transcript's physical line count, the domain `--line` resolves against.
fn fixture_lines(h: &Home) -> usize {
    let path = h.projects().join(ENC).join(format!("{SESS}.jsonl"));
    std::fs::read_to_string(path)
        .expect("fixture transcript")
        .lines()
        .count()
}

/// A range that runs past the end is CLAMPED to the file, and the lines it named past the end
/// are counted as what they are - `lines_past_end` - never as `non_record_lines`, which counts
/// lines that exist and render no record.
#[test]
fn a_range_past_the_end_is_clamped_and_counted_apart_from_non_record_lines() {
    let h = populated_home();
    let n = fixture_lines(&h);
    let out = h.run(&[
        "show",
        at(SESS).as_str(),
        "--line",
        "6..1000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let summary = json_summary(&out.stdout);
    assert_eq!(summary["lines_past_end"], serde_json::json!(1000 - n));
    let non_record = summary["non_record_lines"].as_u64().expect("a count") as usize;
    assert!(
        non_record <= n - 5,
        "only lines L6..L{n} can be non-record lines, read {non_record}"
    );
}

/// The text footer and the `--raw` stderr note say the same thing, in the same words.
#[test]
fn the_clamp_is_disclosed_in_the_text_footer_and_the_raw_note() {
    let h = populated_home();
    let n = fixture_lines(&h);
    let want = format!(
        "{} addressed line(s) lie past the end of the file (L{n} is its last line)",
        1000 - n
    );
    let text = h.run(&["show", at(SESS).as_str(), "--line", "6..1000"]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(text.stdout.contains(&want), "{}", text.stdout);
    let raw = h.run(&["show", at(SESS).as_str(), "--line", "6..1000", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(raw.stderr.contains(&want), "{}", raw.stderr);
    assert!(
        !raw.stdout.contains("past the end"),
        "stdout stays the file's bytes"
    );
}

/// Two ranges running past the end overlap there, so the count is the widest overhang and
/// never the sum; a range wholly inside the file reports none and prints no clamp line.
#[test]
fn overlapping_overhangs_count_once_and_an_in_file_range_counts_none() {
    let h = populated_home();
    let n = fixture_lines(&h);
    let both = h.run(&[
        "show",
        at(SESS).as_str(),
        "--line",
        "6..500,7..1000",
        "--format",
        "json",
    ]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert_eq!(
        json_summary(&both.stdout)["lines_past_end"],
        serde_json::json!(1000 - n)
    );
    let inside = h.run(&["show", at(SESS).as_str(), "--line", &format!("6..{n}")]);
    assert!(inside.success, "stderr: {}", inside.stderr);
    assert!(!inside.stdout.contains("past the end"), "{}", inside.stdout);
}

/// A line or a range that starts past the end names the file's length in the miss, in both
/// fetch modes, so the reader learns the domain from the error.
#[test]
fn a_miss_past_the_end_names_the_file_length_in_both_modes() {
    let h = populated_home();
    let n = fixture_lines(&h);
    let want = format!("(file has {n} lines)");
    for extra in [&[][..], &["--raw"][..]] {
        for addr in ["999", "900..1000"] {
            let target = at(SESS);
            let mut argv = vec!["show", target.as_str(), "--line", addr];
            argv.extend_from_slice(extra);
            let out = h.run(&argv);
            assert!(!out.success, "{addr} {extra:?}: {}", out.stdout);
            assert!(
                out.stderr.contains(&want),
                "{addr} {extra:?}: {}",
                out.stderr
            );
        }
    }
}
