//! verbatim --slice/--slices: fixed-fleet chunking and reassembly.

use crate::harness::*;

#[test]
fn turns_slice_reassembles_out_document_within_window() {
    // --slice paginates the SAME verbatim document `--out` writes into ≤window-CHAR chunks with
    // NO chrome. Assert: every chunk ≤ window, concatenating slices 1..K reproduces the `--out`
    // document byte-for-byte (the zero-drift contract between build_document_body and out_blob),
    // and an out-of-range slice is empty (exit 0).
    let h = turns_home();
    let window = 500usize;

    let out_path = h.root.join("turns_doc.md");
    let r = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "20000",
        "--no-subagents",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(r.success, "stderr: {}", r.stderr);
    let document = std::fs::read_to_string(&out_path).expect("out document written");
    assert!(!document.is_empty(), "fixture yields a non-empty document");
    assert!(
        document.chars().count() > window,
        "document must exceed one window to exercise multi-slice ({} chars)",
        document.chars().count()
    );

    let win = window.to_string();
    let mut reassembled = String::new();
    let mut n = 1usize;
    loop {
        let ns = n.to_string();
        let s = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--budget",
            "20000",
            "--no-subagents",
            "--window",
            &win,
            "--slice",
            &ns,
        ]);
        assert!(s.success, "slice {n} stderr: {}", s.stderr);
        if s.stdout.is_empty() {
            break; // out-of-range slice → empty → done
        }
        assert!(
            s.stdout.chars().count() <= window,
            "slice {n} exceeds the {window}-char window ({} chars)",
            s.stdout.chars().count()
        );
        reassembled.push_str(&s.stdout);
        n += 1;
        assert!(n < 1000, "runaway slice loop");
    }
    assert!(
        n > 2,
        "fixture should span at least two slices, got {}",
        n - 1
    );
    assert_eq!(
        reassembled, document,
        "concatenated slices must reproduce the --out document byte-for-byte"
    );
}

#[test]
fn turns_slices_pins_emitted_count_to_the_fleet() {
    // `--slices N` makes the slice COUNT the hard constraint: it emits AT MOST N chunks no matter
    // how many a char budget would have produced, and each chunk stays within the window. A 2-slice
    // fleet over this multi-block fixture: slices 1-2 are within window, and any index > 2 is empty
    // - the count can never drift to 3/4/5 as the turns grow.
    let h = turns_home();
    let win = 1500usize;
    for i in 1..=2 {
        let o = h.run(&[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--slices",
            "2",
            "--window",
            "1500",
            "--slice",
            &i.to_string(),
        ]);
        assert!(o.success, "stderr: {}", o.stderr);
        assert!(
            o.stdout.chars().count() <= win,
            "slice {i} exceeds the window: {}",
            o.stdout.chars().count()
        );
    }
    let s1 = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "1",
    ]);
    assert!(
        !s1.stdout.is_empty(),
        "slice 1 of a filled 2-fleet is non-empty"
    );
    let s3 = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--slices",
        "2",
        "--window",
        "1500",
        "--slice",
        "3",
    ]);
    assert!(
        s3.stdout.is_empty(),
        "an index beyond the fixed fleet must be empty, got: {}",
        s3.stdout
    );
}

#[test]
fn turns_slices_keeps_newest_discards_oldest() {
    // The fleet fills newest-first; the oldest turns that don't fit are DISCARDED (not truncated).
    // A tight 2-slice fleet keeps the live tail and drops the oldest round-trip ("the very first
    // ask … café").
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=2 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "2",
                "--window",
                "1500",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        !doc.contains("the very first ask"),
        "the oldest turn must be discarded by a small fleet: {doc}"
    );
    assert!(
        doc.contains("final committed answer")
            || doc.contains("TAILuser")
            || doc.contains("short live ask")
            || doc.contains("do the final thing"),
        "the newest turns must be kept: {doc}"
    );
}

#[test]
fn turns_slices_keeps_user_turns_whole_no_role_cap() {
    // The defect a peer session caught: budget mode middle-truncates a USER turn at the 600-char
    // role cap even with budget to spare. In `--slices` mode the only cap is the WINDOW, so a
    // multi-hundred-char user directive survives VERBATIM. The fixture's huge_user (≈817 chars)
    // appears whole (no mid-cut) when the window comfortably exceeds it.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "9000",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    let whole = format!("HEADuser {} TAILuser", "u".repeat(800));
    assert!(
        doc.contains(&whole),
        "a long user turn must be kept whole in --slices mode (it was gutted at the 600 cap?)"
    );
    // Contrast: the SAME fixture under budget mode STILL applies the 600 user cap (legacy behavior
    // is untouched) - so the verbatim user body is NOT present and the elision marker IS.
    let budgeted = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(
        !budgeted.stdout.contains(&whole),
        "budget mode must still apply the 600 user cap (legacy unchanged)"
    );
    assert!(
        budgeted.stdout.contains("chars elided") || budgeted.stdout.contains("chars]"),
        "budget mode shows the elision marker"
    );
}

#[test]
fn turns_slices_ellipsizes_only_a_turn_bigger_than_one_window() {
    // The ONLY content cut in --slices mode is a single turn that ALONE exceeds one window. With a
    // small window the big assistant turn (≈3000 chars) is middle-elided, while the shorter user
    // turn (≈817) in the same fleet is kept whole.
    let h = turns_home();
    let mut doc = String::new();
    for i in 1..=8 {
        doc.push_str(
            &h.run(&[
                "verbatim",
                at(SESS).as_str(),
                "--no-subagents",
                "--slices",
                "8",
                "--window",
                "1200",
                "--slice",
                &i.to_string(),
            ])
            .stdout,
        );
    }
    assert!(
        doc.contains("chars elided") || doc.contains("chars]"),
        "a turn larger than one window is ellipsized: {doc}"
    );
    assert!(
        doc.contains(&format!("HEADuser {} TAILuser", "u".repeat(800))),
        "a turn that fits within one window is kept whole alongside it: {doc}"
    );
}
