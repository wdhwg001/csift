//! WHAT each injection was written FOR: one `for` relation per leaf, read from the key
//! the producer leaves on the record and resolved through the chain's own uuid map.

use super::*;

#[test]
fn every_injection_names_the_record_it_was_written_for() {
    let h = inject_home();
    // One RELATION per leaf, each read from the key the producer leaves on the record.
    // The line comes from the chain's own uuid map, so a reader does not have to search
    // for the uuid the edge names.
    let cases: &[(&str, &str, &str, u64)] = &[
        // (marker, leaf, relation, the LINE the edge resolves to)
        ("zzcaveat", "harness.command.caveat", "attached_to", 6),
        ("zzstopfeedback", "harness.hook.feedback", "blocked", 7),
        (
            "zzenforceso",
            "harness.enforce.structured-output",
            "redoes",
            11,
        ),
        ("zzskillbody", "harness.context.skill", "attached_to", 10),
        (
            "zzotherreminder",
            "harness.context.reminder",
            "attached_to",
            3,
        ),
        ("zzprefloor", "harness.schedule.fire", "fires", 20),
    ];
    for &(marker, leaf, relation, line) in cases {
        let out = h.run(&["search", marker, &at(SESS), "-t", leaf]);
        assert!(out.success, "{marker}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(&format!("[for: {relation} L{line}]")),
            "{marker} -> [for: {relation} L{line}]:\n{}",
            out.stdout
        );
        let json = h.run(&["search", marker, &at(SESS), "-t", leaf, "--format", "json"]);
        let hit = first_hit(&json.stdout);
        let edge = &hit["for"][0];
        assert_eq!(edge["kind"], relation, "{hit}");
        // The parent link says where the record SITS, so the basis is position - never
        // `key`, which would claim the producer chose the target.
        assert_eq!(edge["basis"], "position", "{hit}");
        assert_eq!(edge["target"]["line"], line, "{hit}");
    }
    // The FRAME reports on the tool_use its wrapped pulse names. That id is not a record
    // uuid, so the edge keeps the id and says the line is not here rather than inventing
    // one.
    let frame = h.run(&["search", "zzframe", &at(SESS), "--format", "json"]);
    let edge = first_hit(&frame.stdout)["for"][0].clone();
    assert_eq!(edge["kind"], "reports", "{edge}");
    assert_eq!(edge["basis"], "key", "{edge}");
    assert_eq!(edge["target"]["uuid"], "toolu_zzlaunch", "{edge}");
    assert!(edge["target"]["line"].is_null(), "{edge}");
}
