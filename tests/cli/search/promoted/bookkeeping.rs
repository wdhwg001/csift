//! The MATCHABLE-TEXT LAW at the surface: a `harness.bookkeeping` unit renders its
//! key=value HEAD and then the line's own verbatim JSON, so a key the head elides
//! stays findable - and the whole-file gate has to read that body the same way the
//! emission does, down to how each decodes an invalid byte.

use super::*;

/// The MATCHABLE-TEXT LAW at the surface: a bookkeeping line renders its key=value head
/// AND its own bytes, so a key the head elides is still findable.
///
/// `snapshotMessageId` joins a file-history delta to the snapshot it follows, and the
/// head names only the path and the version - so a render that stopped at the head would
/// make that join unsearchable on every surface. The head has to survive too: it is the
/// legible half, and it is what a reader scans a result list by.
#[test]
fn a_bookkeeping_key_the_head_elides_is_still_searchable() {
    let h = promoted_home();
    let hit = h.run(&[
        "search",
        "snapshotMessageId",
        &at(SESS),
        "-t",
        "harness.bookkeeping.file-history",
    ]);
    assert!(hit.success, "stderr: {}", hit.stderr);
    assert!(
        hit.stdout.contains("harness.bookkeeping.file-history"),
        "the key in the line's own bytes is matchable:\n{}",
        hit.stdout
    );
    // And the unit still carries the head, under an address where nothing is clipped.
    let shown = h.run(&["show", &at(SESS), "--line", "10"]);
    assert!(shown.success, "stderr: {}", shown.stderr);
    assert!(
        shown.stdout.contains("[file-history delta at ")
            && shown.stdout.contains("snapshotMessageId"),
        "head then body, both present:\n{}",
        shown.stdout
    );
}

/// The whole-file gate decodes a marker line's bytes exactly as the emission does.
///
/// A bookkeeping line's body IS the source line, and csift decodes it LOSSILY - so a line
/// carrying one invalid byte renders a replacement character that appears nowhere in its
/// raw bytes. That is the one case where the body is NOT a byte substring of the line, so
/// the literal prefilter misses and stage 2 of the gate is what decides the file. Decoded
/// strictly, stage 2 saw no source line at all, re-rendered the head alone, and pruned the
/// file - a silent drop of a record the emission path would have printed, which is the
/// section 7 landmine on the one path allowed to prove a negative.
#[test]
fn the_whole_file_gate_decodes_an_invalid_byte_as_the_emission_does() {
    let h = Home::new();
    let p = h.write(&format!("{ENC}/{SESS}.jsonl"), "placeholder\n");
    // The invalid byte sits in a field `Record` does not model, so serde skips it and the
    // line still parses - which is what makes this reachable at all.
    let mut line: Vec<u8> = br#"{"type":"file-history-snapshot","messageId":"al"#.to_vec();
    line.push(0xff);
    line.extend_from_slice(
        br#"pha","snapshot":{"timestamp":"2026-06-07T05:00:00.500Z","trackedFileBackups":{"/proj/notes.md":{"version":3}}}}"#,
    );
    line.push(b'\n');
    std::fs::write(&p, &line).unwrap();

    // The pattern reaches the record ONLY through the replacement character, so it is
    // absent from the raw bytes: the literal prefilter misses, the `file-history-` marker
    // keeps the line, and the re-render is what answers.
    let out = h.run(&[
        "search",
        "al\u{fffd}pha",
        &at(SESS),
        "-t",
        "harness.bookkeeping.file-history",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.bookkeeping.file-history"),
        "the gate kept the file its own emission path can render:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("al\u{fffd}pha"),
        "the emitted body carries the same text the gate matched:\n{}",
        out.stdout
    );
}
