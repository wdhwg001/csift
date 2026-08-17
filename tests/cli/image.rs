use crate::harness::*;

#[test]
fn image_extracts_real_bytes_to_dir() {
    let h = image_home();
    let out_dir = h.root.join("imgs");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("extracted 3 image(s)"),
        "{}",
        out.stdout
    );
    // <sess8>-L1i1.png must be a REAL decoded PNG (magic bytes).
    let f = out_dir.join("0a1b2c3d-L1i1.png");
    let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        "not a PNG"
    );
    // media_type drives the extension: image/jpeg → .jpg.
    assert!(out_dir.join("0a1b2c3d-L3i1.jpg").exists());
    assert!(out_dir.join("0a1b2c3d-L3i2.png").exists());
}

#[test]
fn image_hash_n_disambiguators_resolve_to_one() {
    let h = ambiguous_hash_home();
    // Each disambiguator narrows `#1` to a unique image: turn, time window, uuid, exact locator.
    let by_turn = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--turn",
        "1..1",
        "--id",
        "1",
        "--format",
        "json",
    ]);
    assert!(by_turn.success, "stderr: {}", by_turn.stderr);
    assert!(
        by_turn.stdout.contains("\"id\": \"L3i1\"") || by_turn.stdout.contains("\"id\":\"L3i1\""),
        "turn 1..1 → the line-3 blue #1:\n{}",
        by_turn.stdout
    );

    // Time window: r2 is at 06:00, r0 at 05:00 → --since 05:30 isolates the line-3 one.
    let by_time = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--since",
        "2026-06-07T05:30:00Z",
        "--id",
        "1",
    ]);
    assert!(by_time.success, "stderr: {}", by_time.stderr);
    assert!(by_time.stdout.contains("L3i1"), "{}", by_time.stdout);

    // uuid prefix → the line-3 record.
    let by_uuid = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--uuid",
        "u1deadbe",
        "--id",
        "1",
    ]);
    assert!(by_uuid.success, "stderr: {}", by_uuid.stderr);
    assert!(by_uuid.stdout.contains("L3i1"), "{}", by_uuid.stdout);

    // Exact locator extracts the line-3 (blue) image; filename carries `#1` + the locator.
    let out_dir = h.root.join("d_imgs");
    let ex = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L3i1",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(ex.success, "stderr: {}", ex.stderr);
    let f = out_dir.join("0a1b2c3d-img1-L3i1.png");
    let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
}

#[test]
fn image_converts_by_out_path_extension() {
    // The --out path's EXTENSION drives the format (the `convert in out.jpg` idiom): a single
    // image written to a file with a recognized image extension is CONVERTED to it; a png source
    // → a .png path is a raw passthrough. There is no separate `--as` flag.
    let h = image_home(); // r0 L1i1 is a real PNG.
    let png_magic = &[0x89u8, b'P', b'N', b'G'][..];
    for (ext, magic, len) in [
        ("jpg", &[0xFFu8, 0xD8, 0xFF][..], 3usize),
        ("gif", &b"GIF8"[..], 4),
        ("webp", &b"RIFF"[..], 4),
        ("png", png_magic, 4),
    ] {
        let f = h.root.join(format!("shot.{ext}"));
        let out = h.run(&[
            "image",
            at(SESS).as_str(),
            "--no-subagents",
            "--id",
            "L1i1",
            "--out",
            f.to_str().unwrap(),
        ]);
        assert!(out.success, ".{ext} stderr: {}", out.stderr);
        let bytes = std::fs::read(&f).unwrap_or_else(|_| panic!("missing {}", f.display()));
        assert_eq!(&bytes[..len], magic, ".{ext} produced wrong magic bytes");
    }
    // WebP carries the "WEBP" fourcc at offset 8 (lossy VP8, via libwebp).
    let wb = std::fs::read(h.root.join("shot.webp")).unwrap();
    assert_eq!(&wb[8..12], b"WEBP");

    // A single-file path with >1 image selected is an error (can't write many to one file).
    let many = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--out",
        h.root.join("x.png").to_str().unwrap(),
    ]);
    assert!(
        !many.success,
        "single file + many images must error: {}",
        many.stdout
    );
    assert!(
        many.stderr.contains("single") && many.stderr.contains("directory"),
        "error names the file/dir distinction: {}",
        many.stderr
    );

    // A directory path (no image extension) keeps the SOURCE format, auto-named — no conversion.
    let dir = h.root.join("imgs");
    let d = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L1i1",
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(d.success, "stderr: {}", d.stderr);
    assert!(
        dir.join("0a1b2c3d-L1i1.png").exists(),
        "source-format auto-name in the directory"
    );
}

#[test]
fn image_animated_gif_to_still_takes_first_frame() {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"an animation [Image #1]"},
            img_block("image/gif", ANIM_GIF_3F)]}
    });
    h.write(&format!("{ENC}/{SESS}.jsonl"), &format!("{r0}\n"));

    // A .png out path flattens the animated GIF to a still → FIRST frame + a warning (frames + s).
    let f = h.root.join("frame.png");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "1",
        "--out",
        f.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("first frame") && out.stdout.contains("3 frames"),
        "first-frame warning with frame count:\n{}",
        out.stdout
    );
    let bytes = std::fs::read(&f).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );

    // A .gif out path (same format as source) is a raw passthrough — animation preserved, no warning.
    let g = h.root.join("keep.gif");
    let keep = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "1",
        "--out",
        g.to_str().unwrap(),
    ]);
    assert!(keep.success, "stderr: {}", keep.stderr);
    assert!(
        !keep.stdout.contains("first frame"),
        "no flatten note: {}",
        keep.stdout
    );
    let gb = std::fs::read(&g).unwrap();
    assert_eq!(&gb[..4], b"GIF8");
}

#[test]
fn image_id_selection_json_and_unresolved() {
    let h = image_home();
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L3i2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(objs
        .iter()
        .any(|o| o.get("id").and_then(|v| v.as_str()) == Some("L3i2")));
    assert!(objs.iter().any(|o| o.get("images").is_some())); // trailing summary
                                                             // A nonexistent id is an explicit error, never a silent miss.
    let miss = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L999i9",
    ]);
    assert!(!miss.success);
    assert!(miss.stderr.contains("L999i9"), "stderr: {}", miss.stderr);
    // v0.6.2: the miss explains itself — inventory (this fixture's images carry no
    // [Image #N] markers, so all are unnumbered) + the paste-time numbering provenance.
    assert!(
        miss.stderr.contains("unnumbered image(s)"),
        "stderr: {}",
        miss.stderr
    );
    assert!(
        miss.stderr.contains("paste-time"),
        "stderr: {}",
        miss.stderr
    );
}

// A `#N` miss on a transcript whose handles carry HOLES (inherited paste-time numbers
// starting past #1) must name the handles that DO exist and call the miss a source gap —
// never a bare "matched no image" that reads like a csift drop.
#[test]
fn image_id_miss_names_present_handles_and_explains_holes() {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "version":"2.1.0","timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"see [Image #2] and [Image #4]"},
            img_block("image/png", PNG_RED), img_block("image/png", PNG_GREEN)]}
    });
    h.write(&format!("{ENC}/{SESS}.jsonl"), &format!("{r0}\n"));
    let miss = h.run(&["image", at(SESS).as_str(), "--no-subagents", "--id", "1"]);
    assert!(!miss.success);
    assert!(
        miss.stderr.contains("--id matched no image: #1"),
        "stderr: {}",
        miss.stderr
    );
    assert!(
        miss.stderr.contains("present here: #2 #4"),
        "stderr: {}",
        miss.stderr
    );
    assert!(
        miss.stderr.contains("source gap"),
        "stderr: {}",
        miss.stderr
    );
}

#[test]
fn image_extract_single_by_id() {
    let h = image_home();
    let out_dir = h.root.join("one");
    let out = h.run(&[
        "image",
        at(SESS).as_str(),
        "--no-subagents",
        "--id",
        "L1i1",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out_dir.join("0a1b2c3d-L1i1.png").exists());
    let n = std::fs::read_dir(&out_dir).unwrap().count();
    assert_eq!(n, 1, "only the selected image should be written");
}
