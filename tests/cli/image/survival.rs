//! The SURVIVAL AXIS on `image`: an image pasted into a prompt the operator later rewound
//! past is still on disk, so it stays in the listing and stays extractable - it is only
//! MARKED, and a `--turn` window (which asks about numbered turns) leaves it out.

use crate::harness::*;

const IENC: &str = "-Users-dev-example-project";
const ISESS: &str = "9e8d7c6b-5a49-4738-8261-5f4e3d2c1b0a";

/// L1 prompt with an image, L2 reply, L3 the REWOUND prompt with its own image,
/// L4 its reply, L5 the resend from the same parent with a third image.
fn rewind_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","parentUuid":serde_json::Value::Null,
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"chart the lagoon"}, img_block("image/png", PNG_1X1)]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"dredge the northern channel"}, img_block("image/png", PNG_RED)]}
    });
    let r3 = serde_json::json!({
        "type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z",
        "message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}
    });
    let r4 = serde_json::json!({
        "type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"survey the southern shoal"}, img_block("image/png", PNG_BLUE)]}
    });
    h.write(
        &format!("{IENC}/{ISESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n{r3}\n{r4}\n"),
    );
    h
}

fn images(out: &str) -> Vec<serde_json::Value> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "image")
        .collect()
}

#[test]
fn an_image_on_a_rewound_prompt_is_listed_and_marked() {
    let h = rewind_home();
    let out = h.run(&["image", at(ISESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("3 image(s)"),
        "the bytes are on disk, so every image is listed:\n{}",
        out.stdout
    );
    assert_eq!(
        out.stdout.matches("[abandoned]").count(),
        1,
        "only the rewound prompt's image is marked:\n{}",
        out.stdout
    );
    let j = h.run(&["image", at(ISESS).as_str(), "--format", "json"]);
    let imgs = images(&j.stdout);
    assert_eq!(imgs.len(), 3, "{}", j.stdout);
    let ab = imgs
        .iter()
        .find(|v| v["line"] == 3)
        .expect("the rewound prompt's image");
    assert_eq!(ab["survival"], "abandoned", "{}", j.stdout);
    for line in [1, 5] {
        let live = imgs
            .iter()
            .find(|v| v["line"] == line)
            .expect("a surviving image");
        assert_eq!(live["survival"], "live", "{}", j.stdout);
    }
}

#[test]
fn a_turn_window_resolves_against_live_numbering() {
    let h = rewind_home();
    // Two live turns remain (the first prompt and the resend); the last one is the
    // resend, whose image sits at L5. The rewound prompt belongs to no numbered turn,
    // so a turn window never admits it.
    let out = h.run(&[
        "image",
        at(ISESS).as_str(),
        "--turn",
        "-1..",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let imgs = images(&out.stdout);
    assert_eq!(imgs.len(), 1, "{}", out.stdout);
    assert_eq!(imgs[0]["line"], 5, "{}", out.stdout);
}

/// L1 live prompt, L2 its reply carrying an image, L3 the REWOUND prompt, L4 that
/// prompt's reply carrying the second image, L5 the resend from the same parent.
///
/// The abandoned image is on the REPLY, not on the opener, and that is the whole point of
/// this fixture. Every other survival fixture here puts it on the rewound PROMPT, which
/// the pre-0.12.0 grouper ALSO dropped (a superseded opener was its one exclusion), so
/// those cases cannot tell the two rules apart. An abandoned MEMBER can: the survival axis
/// gives it no numbered turn at all, while the conservative grouper keeps it as a member
/// of the turn above and a window would admit it.
fn rewound_member_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","parentUuid":serde_json::Value::Null,
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"chart the lagoon"}]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","id":"m0","content":[
            {"type":"text","text":"the lagoon"}, img_block("image/png", PNG_1X1)]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"dredge the channel"}]}
    });
    let r3 = serde_json::json!({
        "type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z",
        "message":{"role":"assistant","id":"m1","content":[
            {"type":"text","text":"the channel"}, img_block("image/png", PNG_RED)]}
    });
    let r4 = serde_json::json!({
        "type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"survey the shoal instead"}]}
    });
    h.write(
        &format!("{IENC}/{ISESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n{r3}\n{r4}\n"),
    );
    h
}

#[test]
fn a_turn_window_excludes_an_abandoned_image_that_is_a_turn_member() {
    let h = rewound_member_home();
    // The flat listing carries both images, one of them marked - the bytes are on disk.
    let all = h.run(&["image", at(ISESS).as_str(), "--format", "json"]);
    assert!(all.success, "stderr: {}", all.stderr);
    let listed = images(&all.stdout);
    assert_eq!(listed.len(), 2, "{}", all.stdout);
    let member = listed
        .iter()
        .find(|v| v["line"] == 4)
        .expect("the abandoned reply's image");
    assert_eq!(member["survival"], "abandoned", "{}", all.stdout);

    // A window over EVERY numbered turn still leaves it out, because it belongs to none.
    let out = h.run(&[
        "image",
        at(ISESS).as_str(),
        "--turn",
        "..",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let windowed: Vec<i64> = images(&out.stdout)
        .iter()
        .filter_map(|v| v["line"].as_i64())
        .collect();
    assert_eq!(
        windowed,
        vec![2],
        "a turn window admits only the live reply's image; the abandoned MEMBER at L4 \
         belongs to no numbered turn:\n{}",
        out.stdout
    );
}

#[test]
fn an_abandoned_image_extracts_to_a_real_file() {
    // The help promises an abandoned image stays extractable, because the bytes are on
    // disk and `--out` writes the same file either way. A marker that quietly cost the
    // caller the extraction would be worse than no marker.
    let h = rewind_home();
    let out_dir = h.root.join("imgs");
    let out = h.run(&[
        "image",
        at(ISESS).as_str(),
        "--id",
        "L3i1",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("extracted 1 image(s)"),
        "{}",
        out.stdout
    );
    let f = out_dir.join("9e8d7c6b-L3i1.png");
    let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "the abandoned image decodes to real PNG bytes"
    );
}
