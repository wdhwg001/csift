//! The layout, the lane-id gate, and the two write primitives.

use super::*;

#[test]
fn the_layout_is_one_owned_directory_under_the_sidecar() {
    let sidecar = std::path::Path::new(
        "/Users/dev/projects/-Users-dev-relay/00000000-0000-4000-8000-000000000001",
    );
    let root = channel_dir(sidecar);
    assert!(root.ends_with("csift-channel"));
    assert_eq!(root.parent(), Some(sidecar));
    assert!(messages_dir(&root).ends_with("messages"));
    assert!(outbox_path(&root).ends_with("outbox.jsonl"));
    assert!(inbox_path(&root, RECEIVER)
        .unwrap()
        .ends_with(format!("inbox/{RECEIVER}.jsonl")));
    assert!(ledger_path(&root, AGENT)
        .unwrap()
        .ends_with(format!("ledger/{AGENT}.jsonl")));
    assert!(armed_path(&root, TEAMMATE)
        .unwrap()
        .ends_with(format!("armed/{TEAMMATE}.json")));
    assert!(message_path(&root, MSG_ID)
        .unwrap()
        .ends_with(format!("messages/{MSG_ID}.json")));
}

#[test]
fn a_lane_id_is_a_uuid_or_one_of_the_two_agent_shapes() {
    assert!(is_lane_id(SESSION));
    assert!(is_lane_id(AGENT));
    assert!(is_lane_id(TEAMMATE));
    // A teammate name may itself carry dashes.
    assert!(is_lane_id("aP1-region-0123456789abcdef"));
}

#[test]
fn nothing_that_could_escape_the_directory_is_a_lane_id() {
    for bad in [
        "",
        "..",
        "../../etc/passwd",
        "a/b",
        "main",
        "0123456789abcdef.jsonl",
        "aRelay-0123456789abcdef/../x",
        "C:-Users-dev",
    ] {
        assert!(!is_lane_id(bad), "`{bad}` must not be a lane id");
        assert!(validate_lane_id(bad).is_err());
    }
}

#[test]
fn a_bad_lane_id_fails_the_path_builders_loudly() {
    let root = std::path::Path::new("/Users/dev/channel");
    let err = inbox_path(root, "../escape").unwrap_err().to_string();
    assert!(err.contains("not a lane id"), "{err}");
    assert!(ledger_path(root, "").is_err());
    assert!(armed_path(root, "nope").is_err());
}

#[test]
fn a_message_id_is_exactly_sixteen_lowercase_hex() {
    assert!(is_message_id(MSG_ID));
    for bad in [
        "",
        "0123456789ABCDEF",
        "0123456789abcde",
        "0123456789abcdef0",
        "0123456789abcdeg",
        "../0123456789ab",
    ] {
        assert!(!is_message_id(bad), "`{bad}` must not be a message id");
    }
    assert!(message_path(std::path::Path::new("/Users/dev"), "nope").is_err());
}

#[test]
fn append_creates_the_directory_and_keeps_append_order() {
    let fx = Fixture::new();
    let path = ledger_path(&fx.root, AGENT).unwrap();
    append_line(&path, "{\"n\":1}").unwrap();
    append_line(&path, "{\"n\":2}").unwrap();
    let (values, skipped) = read_jsonl(&path).unwrap();
    assert_eq!(skipped, 0);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["n"], 1);
    assert_eq!(values[1]["n"], 2);
}

#[test]
fn two_writers_appending_at_once_interleave_whole_lines() {
    let fx = Fixture::new();
    let path = ledger_path(&fx.root, AGENT).unwrap();
    // Seed the file so both threads open an existing one, the shape a second hook
    // process hits in production.
    append_line(&path, "{\"seed\":true}").unwrap();
    let rounds = 200;
    let mut handles = Vec::new();
    for writer in 0..2 {
        let p = path.clone();
        handles.push(std::thread::spawn(move || {
            for i in 0..rounds {
                let line = format!(
                    "{{\"writer\":{writer},\"i\":{i},\"pad\":\"{}\"}}",
                    "x".repeat(80)
                );
                append_line(&p, &line).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let (values, skipped) = read_jsonl(&path).unwrap();
    // Not one torn line: with O_APPEND every whole-line write lands intact, so every
    // line parses and both writers' rounds are all present.
    assert_eq!(skipped, 0, "a torn line appeared");
    assert_eq!(values.len(), 1 + 2 * rounds);
    for writer in 0..2 {
        let count = values
            .iter()
            .filter(|v| v.get("writer").and_then(serde_json::Value::as_u64) == Some(writer))
            .count();
        assert_eq!(count, rounds, "writer {writer} lost lines");
    }
}

#[test]
fn an_atomic_rewrite_replaces_the_file_and_leaves_no_temp_behind() {
    let fx = Fixture::new();
    let path = armed_path(&fx.root, AGENT).unwrap();
    write_atomic(&path, "{\"v\":1}").unwrap();
    write_atomic(&path, "{\"v\":2}").unwrap();
    assert_eq!(read_json_object(&path).unwrap()["v"], 2);
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
}

#[test]
fn a_missing_file_reads_as_empty_and_a_malformed_line_is_counted() {
    let fx = Fixture::new();
    let path = ledger_path(&fx.root, AGENT).unwrap();
    let (values, skipped) = read_jsonl(&path).unwrap();
    assert!(values.is_empty());
    assert_eq!(skipped, 0);
    assert!(read_json_object(&path).is_none());

    append_line(&path, "{\"ok\":1}").unwrap();
    append_line(&path, "this is not json").unwrap();
    append_line(&path, "").unwrap();
    append_line(&path, "{\"ok\":2}").unwrap();
    let (values, skipped) = read_jsonl(&path).unwrap();
    assert_eq!(values.len(), 2);
    // The blank line is not a failure; the garbage line is, and it is counted.
    assert_eq!(skipped, 1);
}
