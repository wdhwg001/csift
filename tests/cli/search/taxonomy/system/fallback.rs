//! A turn the API served from a FALLBACK model: the marker both a scanned unit and an
//! addressed one carry, and the honest banner edge beside it.

use super::*;

/// A turn the API served from a fallback model: an addressed fetch RENDERS it under the
/// plain message leaf with both model ids in the label zone, and the honest banner edge
/// beside them.
#[test]
fn a_fallback_served_turn_is_addressable_and_names_both_models() {
    let h = system_home();
    let show = h.run(&["show", &at(SESS), "--line", "12"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout.contains("agent.message.plain")
            && show
                .stdout
                .contains("[served by model-beta, fell back from model-alpha]"),
        "{}",
        show.stdout
    );
    // The banner edge is the HONEST unknown: the banner names this record, not the
    // reverse, and a uuid map cannot walk down.
    assert!(
        show.stdout.contains(
            "[for: banner unknown (the banner names this record, not the reverse; a \
             one-to-many descent needs the provenance index)]"
        ),
        "{}",
        show.stdout
    );
    let js = h.run(&["show", &at(SESS), "--line", "12", "--format", "json"]);
    let row = json_rows(&js.stdout, "record")
        .into_iter()
        .next()
        .expect("one record row");
    assert_eq!(row["label"], serde_json::json!("agent.message.plain"));
    assert_eq!(row["served_by"], serde_json::json!("model-beta"));
    assert_eq!(row["fell_back_from"], serde_json::json!("model-alpha"));
    assert_eq!(row["for"][0]["kind"], serde_json::json!("banner"));
    assert_eq!(row["for"][0]["target"], serde_json::Value::Null);
    // A pattern SCAN emits nothing for it - the API wrote the block in place of the
    // text, so there is no unit to match or excerpt.
    let scan = h.run(&["search", "model-alpha", &at(SESS), "-t", "agent", "-c"]);
    assert_eq!(scan.stdout.trim(), "0", "{}", scan.stdout);
}

/// A turn that fell back AND SPOKE carries the marker on its scanned unit too.
///
/// The ids come off the parsed BLOCK rather than off the source line, and that is the
/// whole point: a scan hands a family no source line for a role-bearing record, so a
/// marker read off the line appears under `show --line` and vanishes under `search`.
/// This is the address-versus-scan split the one stamping site exists to prevent, pinned
/// from both sides on one record.
#[test]
fn a_fallback_turn_that_spoke_carries_the_marker_on_a_scan_too() {
    let h = Home::new();
    let sess = "6d6d6d6d-7e7e-4f4f-8a8a-9b9b9b9b9b9b";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzask the harbor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","model":"model-beta","content":[{"type":"fallback","from":{"model":"model-alpha"},"to":{"model":"model-beta"}},{"type":"text","text":"zzspoke and fell back"}]}}"#, "\n",
        ),
    );
    let marker = "[served by model-beta, fell back from model-alpha]";
    let scan = h.run(&["search", "zzspoke", &at(sess)]);
    assert!(scan.success, "stderr: {}", scan.stderr);
    assert!(
        scan.stdout.contains(marker),
        "a SCANNED unit carries the marker:\n{}",
        scan.stdout
    );
    let show = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(
        show.success && show.stdout.contains(marker),
        "and the addressed unit renders the same:\n{}",
        show.stdout
    );
    let js = h.run(&["search", "zzspoke", &at(sess), "--format", "json"]);
    let hit = hits_of(&js.stdout).into_iter().next().expect("one hit");
    assert_eq!(hit["served_by"], serde_json::json!("model-beta"));
    assert_eq!(hit["fell_back_from"], serde_json::json!("model-alpha"));
}
