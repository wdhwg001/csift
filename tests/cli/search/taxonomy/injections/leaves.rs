//! WHICH LEAF each isMeta template reaches, and which shapes take none at all.
//!
//! The census is the first assertion because it is the one that fails as a NUMBER: a
//! template that stops reaching its leaf shows as a count rather than as a missing
//! string somewhere in a rendered page.

use super::*;

#[test]
fn every_injection_template_reaches_its_own_leaf() {
    let h = inject_home();
    let out = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let c = census(&out.stdout);
    let expect: &[(&str, usize)] = &[
        // The roster reminder and the generic reminder share the family's last arm.
        ("harness.context.reminder", 2),
        // Only the TEXT reference emits on a scan; the image-ONLY record renders no text,
        // so it is reachable by address alone (asserted separately below).
        ("harness.context.image", 1),
        ("harness.command.caveat", 1),
        // Two of the four blocking-hook names, one leaf.
        ("harness.hook.feedback", 2),
        // BOTH inbound relays - the coordinator's and the peer's.
        ("agent.communication.inbox", 2),
        ("harness.context.skill", 1),
        ("harness.enforce.structured-output", 1),
        ("harness.enforce.brief", 1),
        ("harness.enforce.handback", 1),
        // The equality nudge AND the edit-failed notice, one leaf.
        ("harness.enforce.retry", 2),
        ("harness.notification.background-command", 1),
        ("harness.schedule.fire", 1),
        // The human's own prompt, the relayed message, and the quoting message.
        ("user.message", 3),
    ];
    for &(leaf, n) in expect {
        assert_eq!(c.get(leaf).copied(), Some(n), "{leaf}:\n{}", out.stdout);
    }
    // The residue carries no leaf, so it is in no census key at all.
    let residue = h.run(&["search", "zzresidue", &at(SESS)]);
    assert!(residue.success, "stderr: {}", residue.stderr);
    assert!(
        residue.stdout.contains("no matching exchanges"),
        "a scan never emits an unmodeled unit:\n{}",
        residue.stdout
    );
}

#[test]
fn the_residue_is_an_unmodeled_unit_under_an_address() {
    let h = inject_home();
    let out = h.run(&["show", &at(SESS), "--line", "19"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The 1:1 law: an address never reports a parseable line as absent, and the unit
    // NAMES the shape rather than leaving the reader with an empty label.
    assert!(
        out.stdout.contains("? unmodeled: user/isMeta"),
        "the residue names its shape:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("zzresidue"),
        "and still renders its body:\n{}",
        out.stdout
    );
    let json = h.run(&["show", &at(SESS), "--line", "19", "--format", "json"]);
    let rows = json_rows(&json.stdout, "record");
    assert_eq!(rows.len(), 1, "{}", json.stdout);
    assert!(rows[0]["label"].is_null(), "{}", rows[0]);
    assert_eq!(rows[0]["unmodeled"], "user/isMeta", "{}", rows[0]);
}

#[test]
fn the_image_only_record_is_reachable_by_address() {
    let h = inject_home();
    // It emits nothing on a scan (no text section), which is the exemption the emission
    // law keys BY SPECIMEN - but an address resolves it and it keeps its leaf.
    let out = h.run(&["show", &at(SESS), "--line", "6"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.context.image"),
        "the image-only record keeps its leaf under an address:\n{}",
        out.stdout
    );
    // The extraction pointer is what makes the unit useful: the bytes are on disk.
    assert!(
        out.stdout.contains("csift image "),
        "and points at the extraction:\n{}",
        out.stdout
    );
    // The record renders NO text, so the unit falls through to the engine's key=value
    // excerpt of its own top-level keys. The counts are ADDED to the label zone beside
    // that excerpt - they are what says what the record is; the excerpt stays, and the
    // case below pins both halves.
    assert!(
        out.stdout
            .contains("[context image-only: 1 image block(s), 0 document block(s)]"),
        "the block counts render:\n{}",
        out.stdout
    );
    let json = h.run(&["show", &at(SESS), "--line", "6", "--format", "json"]);
    let rows = json_rows(&json.stdout, "record");
    assert_eq!(rows.len(), 1, "{}", json.stdout);
    assert_eq!(rows[0]["image_only"]["images"], 1, "{}", rows[0]);
    assert_eq!(rows[0]["image_only"]["documents"], 0, "{}", rows[0]);
}

#[test]
fn the_image_only_counts_are_added_beside_the_key_value_excerpt() {
    let h = inject_home();
    // The counts do NOT replace the unit's body. A record with no renderable text takes
    // the engine's key=value excerpt of its own top-level keys - the same body the 1:1
    // law gives every textless line - and the family's counts ride the LABEL ZONE in
    // front of it. Both halves are asserted here so neither can be described as the
    // other: a doc saying the counts render "instead of" the excerpt fails this case.
    let out = h.run(&["show", &at(SESS), "--line", "6"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find(|l| l.contains("harness.context.image"))
        .unwrap_or_else(|| panic!("no unit line:\n{}", out.stdout));
    let counts = "[context image-only: 1 image block(s), 0 document block(s)]";
    let at_counts = line
        .find(counts)
        .unwrap_or_else(|| panic!("the counts ride the label zone:\n{line}"));
    // The excerpt is the record's own top-level keys, rendered key=value in on-disk
    // order, so it names the fields an address is asked for.
    for key in [
        "cwd=/Users/dev/inject-project",
        "isMeta=true",
        "type=user",
        "uuid=11111111-2222-4333-8444-000000000006",
    ] {
        let at_key = line
            .find(key)
            .unwrap_or_else(|| panic!("the key=value excerpt still carries {key}:\n{line}"));
        assert!(
            at_counts < at_key,
            "the counts lead the excerpt rather than replacing it ({key}):\n{line}"
        );
    }
    // The image PAYLOAD is not among them, and its siblings are. The block's base64
    // `data` reaches the excerpt as a value of the `message` key like any other, and it
    // is elided by name: a blob's head identifies nothing while the per-value budget is
    // 80 characters, so printing it costs the keys that DO say what the shape is.
    assert!(
        line.contains(r#""data":"<elided>""#) && !line.contains("iVBORw0KGgo="),
        "the base64 payload is elided, not printed:\n{line}"
    );
    assert!(
        line.contains("image/png"),
        "and the key that identifies the shape prints in its place:\n{line}"
    );
    // A machine reader gets both halves too, and from the one address: the counts as
    // their own key, the excerpt as the unit's text.
    let json = h.run(&["show", &at(SESS), "--line", "6", "--format", "json"]);
    let rows = json_rows(&json.stdout, "record");
    assert_eq!(rows.len(), 1, "{}", json.stdout);
    assert_eq!(rows[0]["image_only"]["images"], 1, "{}", rows[0]);
    let text = rows[0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("isMeta=true") && text.contains(r#""data":"<elided>""#),
        "the JSON text is the same key=value excerpt:\n{}",
        rows[0]
    );
    assert!(
        !text.contains("image block(s)"),
        "and the counts are a fact beside it, never folded into the text:\n{}",
        rows[0]
    );
}

#[test]
fn the_image_only_leaf_is_gated_on_the_authorship_flag() {
    let h = inject_home();
    // The SAME content shape without `isMeta` is a person's record, and the harness leaf
    // would be a false statement about who wrote it - so it carries none, and the address
    // renders it as the shape it is.
    let out = h.run(&["show", &at(SESS), "--line", "25"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("? unmodeled: user/image"),
        "a non-isMeta image-only record takes no harness leaf:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("harness.context.image"),
        "and never the image leaf:\n{}",
        out.stdout
    );
    // A DOCUMENT-only isMeta record is the other half of the same ruling: the arm names
    // image blocks, so this one carries no leaf either rather than borrowing one. Its
    // residue is named by the AUTHORSHIP flag rather than by the block, because who wrote
    // a record is what a reader of a residue unit needs; the same record WITHOUT the flag
    // is named by its block (the fetch-domain suite pins that half). The excerpt carries
    // the flag verbatim, which is the pair the shape name and the excerpt make - it does
    // NOT reliably carry the block type, because the per-value cut is 80 characters and a
    // base64 source spends them.
    let doc = h.run(&["show", &at(SESS), "--line", "24"]);
    assert!(
        doc.stdout.contains("? unmodeled: user/isMeta"),
        "a document-only record carries no leaf by design:\n{}",
        doc.stdout
    );
    assert!(
        doc.stdout.contains("isMeta=true"),
        "and the excerpt carries the flag the shape name reports:\n{}",
        doc.stdout
    );
}

#[test]
fn the_pre_stamp_fired_prompt_is_labelled_by_its_parent_link() {
    let h = inject_home();
    // L21 carries NO `promptSource` and a 2.1.159 version stamp - the shape roughly two
    // thirds of the fired prompts on disk have, all written below the stamp's floor. The
    // gate is the parent link, so the leaf does not depend on the field.
    let out = h.run(&[
        "search",
        "zzprefloor",
        &at(SESS),
        "-t",
        "harness.schedule.fire",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.schedule.fire")
            && out.stdout.contains("[scheduled fire Jun 7 5:00am]"),
        "the pre-stamp prompt labels and dates:\n{}",
        out.stdout
    );
    // The armed text renders verbatim - there is no marker in it to match.
    assert!(
        out.stdout.contains("zzprefloor sweep the queue"),
        "{}",
        out.stdout
    );
}

#[test]
fn the_notification_frame_classifies_by_the_pulse_it_wraps() {
    let h = inject_home();
    let out = h.run(&["search", "zzframe", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    // The frame is a second CARRIER, not a second shape: the inner pulse decides the kind,
    // and the render is the automation label, never the frame's warning prose.
    assert!(
        out.stdout
            .contains("harness.notification.background-command"),
        "the frame's pulse keeps its own kind:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("[background-command b1a2c3d4e completed]"),
        "and renders through the automation label:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("NOT USER INPUT"),
        "the frame text is not the body:\n{}",
        out.stdout
    );
}

#[test]
fn the_enforce_family_splits_four_ways() {
    let h = inject_home();
    for (marker, leaf) in [
        ("zzenforceso", "harness.enforce.structured-output"),
        ("zzenforcebrief", "harness.enforce.brief"),
        ("zzenforcehandback", "harness.enforce.handback"),
        ("zzenforceretryedit", "harness.enforce.retry"),
    ] {
        let out = h.run(&["search", marker, &at(SESS), "-t", leaf]);
        assert!(out.success, "{marker}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(leaf) && out.stdout.contains(marker),
            "{marker} -> {leaf}:\n{}",
            out.stdout
        );
    }
    // A bare category reaches all four, because every enforce leaf is text the model read.
    // Five RECORDS over four leaves: the retry leaf carries two templates.
    let all = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.enforce",
        "--count-by",
        "label",
    ]);
    assert!(all.success, "stderr: {}", all.stderr);
    let c = census(&all.stdout);
    assert_eq!(c.len(), 4, "four enforce leaves:\n{}", all.stdout);
    assert_eq!(
        c.values().sum::<usize>(),
        5,
        "five records under them:\n{}",
        all.stdout
    );
}

#[test]
fn both_blocking_hook_names_take_the_feedback_leaf() {
    let h = inject_home();
    for marker in ["zzstopfeedback", "zzidlefeedback"] {
        let out = h.run(&["search", marker, &at(SESS), "-t", "harness.hook.feedback"]);
        assert!(out.success, "{marker}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains("harness.hook.feedback"),
            "{marker} -> harness.hook.feedback:\n{}",
            out.stdout
        );
    }
}

#[test]
fn a_bundled_skill_body_with_no_directory_line_reaches_the_skill_leaf() {
    // The registrar prepends `Base directory for this skill:` only for a skill that ships
    // FILES, so a bundled body arrives bare - and its own head is what the leaf answers
    // to. Its own fixture, because the shared one addresses records by line.
    let h = Home::new();
    let sess = "11111111-2222-4333-8444-555555555504";
    let lines = [
        r#"{"type":"user","uuid":"s0","sessionId":"11111111-2222-4333-8444-555555555504","cwd":"/Users/dev/bundled","version":"2.1.278","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"/workflow-authoring zzbundled"}}"#,
        r##"{"type":"user","uuid":"s1","parentUuid":"s0","isMeta":true,"turnCompanion":true,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"# Workflow authoring reference\n\nzzbundled a workflow structures work across many agents."}]}}"##,
        r#"{"type":"assistant","uuid":"s2","parentUuid":"s1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzbundled read it"}]}}"#,
    ];
    h.write(
        &format!("-Users-dev-bundled/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    // A FLAGLESS pattern scan reaches it: the record rides a `type:"user"` line and the
    // leaf is visible, so nothing gates it.
    let out = h.run(&["search", "zzbundled", &at(sess), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.context.skill"),
        "the bundled body reaches the skill leaf: {}",
        out.stdout
    );
    // And it is not the person, nor a turn of its own: the record carries both authorship
    // flags, so the transcript has ONE turn.
    let turns = h.run(&[
        "search",
        "",
        &at(sess),
        "--no-subagents",
        "--count-by",
        "turn",
    ]);
    assert!(turns.success, "stderr: {}", turns.stderr);
    let keys = turns
        .stdout
        .lines()
        .filter(|l| l.trim_start().starts_with(char::is_numeric))
        .count();
    assert_eq!(keys, 1, "one turn over the fixture: {}", turns.stdout);
}
