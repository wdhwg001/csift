//! The channel five point at each other.
//!
//! `send`, `msg`, `ack`, `deliver` and `whoami` are one surface split across five pages: a
//! caller who found any of them has to be able to reach the other four without knowing they
//! exist. Each page therefore carries a SEE ALSO block naming the rest, and this pins it.

use crate::harness::*;

/// The five pages, and the commands each must point at. `whoami` is in the set because its
/// `--to` prediction and its lane sections are the read half of the same channel.
const PAGES: [(&str, [&str; 4]); 5] = [
    ("send", ["msg", "ack", "deliver", "whoami"]),
    ("msg", ["send", "ack", "deliver", "whoami"]),
    ("ack", ["send", "msg", "deliver", "whoami"]),
    ("deliver", ["send", "msg", "ack", "whoami"]),
    ("whoami", ["send", "msg", "ack", "deliver"]),
];

#[test]
fn every_channel_page_has_a_see_also_naming_the_other_four() {
    let h = Home::new();
    for (page, others) in PAGES {
        let out = h.run(&[page, "--help"]);
        assert!(out.success, "`csift {page} --help`: {}", out.stderr);
        let block: String = out
            .stdout
            .lines()
            .skip_while(|l| l.trim() != "SEE ALSO")
            .skip(1)
            .take_while(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !block.is_empty(),
            "`csift {page} --help` carries no SEE ALSO block:\n{}",
            out.stdout
        );
        for other in others {
            assert!(
                block.contains(&format!("csift {other}")),
                "`csift {page} --help` SEE ALSO does not name `csift {other}`:\n{block}"
            );
        }
    }
}
