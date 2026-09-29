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
