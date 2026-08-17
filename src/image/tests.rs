use super::*;

/// A minimal [`ImageRef`] carrying only what `ext()` consults.
fn media_ref(mt: &str) -> ImageRef {
    ImageRef {
        session_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
        is_subagent: false,
        parent_session_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into(),
        line_no: 1,
        img_index: 1,
        seq: None,
        fingerprint: String::new(),
        source_kind: "base64".into(),
        media_type: mt.into(),
        b64_len: 0,
        est_bytes: 0,
        url: None,
        ts_utc: None,
        record_uuid: None,
        data: None,
    }
}

#[test]
fn media_type_to_ext_mapping_pinned() {
    // Mutation pin: every media-type arm, plus the never-fabricated `bin` fallback.
    for (mt, ext) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/jpg", "jpg"),
        ("image/gif", "gif"),
        ("image/webp", "webp"),
        ("image/svg+xml", "svg"),
        ("image/bmp", "bmp"),
        ("image/tiff", "tiff"),
        ("image/heic", "heic"),
        ("image/avif", "avif"),
        ("application/octet-stream", "bin"),
    ] {
        assert_eq!(media_ref(mt).ext(), ext, "{mt}");
    }
}

#[test]
fn human_bytes_boundaries() {
    // Mutation pin: the KB/MB thresholds and both formatting shapes.
    assert_eq!(human_bytes(0), "0 B");
    assert_eq!(human_bytes(1023), "1023 B");
    assert_eq!(human_bytes(1024), "1 KB");
    assert_eq!(human_bytes(294_912), "288 KB");
    assert_eq!(human_bytes(1_048_576), "1.0 MB");
    assert_eq!(human_bytes(1_258_291), "1.2 MB");
}

#[test]
fn base64_roundtrip_standard_and_padded() {
    // "hi" → "aGk=", "caf\u{e9}" bytes → padded; whitespace tolerated.
    assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
    assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(decode_base64("a G V s\nbG8=").unwrap(), b"hello");
    // A 1x1 transparent PNG header decodes to the PNG magic bytes.
    let png = decode_base64("iVBORw0KGgo=").unwrap();
    assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}

#[test]
fn base64_rejects_invalid_char() {
    assert!(decode_base64("not base64 !!!@").is_none());
}

#[test]
fn est_decoded_len_matches_real_decode() {
    for s in ["aGk=", "aGVsbG8=", "iVBORw0KGgo=", "Zm9vYmFy"] {
        assert_eq!(est_decoded_len(s), decode_base64(s).unwrap().len(), "{s}");
    }
}

#[test]
fn id_and_filename_are_stable() {
    let mut r = image_ref_from_source(
        &json!({"type":"base64","media_type":"image/png","data":"aGk="}),
        false,
    )
    .unwrap();
    r.session_id = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".into();
    r.line_no = 6812;
    r.img_index = 2;
    assert_eq!(r.id(), "L6812i2");
    assert_eq!(r.out_filename_with_ext(r.ext()), "0a1b2c3d-L6812i2.png");
    // A converted target overrides only the extension, keeping the unique locator.
    assert_eq!(r.out_filename_with_ext("jpg"), "0a1b2c3d-L6812i2.jpg");
    assert_eq!(r.ext(), "png");
}

#[test]
fn parse_id_selection_forms() {
    // Locators (L<line>i<n>) AND the session `#N` handle (with `#` or bare number).
    assert_eq!(
        parse_id_selection(&["L6812i2,L99i1".into(), "L1i1".into()]).unwrap(),
        vec![Sel::Loc(6812, 2), Sel::Loc(99, 1), Sel::Loc(1, 1)]
    );
    // The '#'-form is REJECTED with a pointed fix (unquoted `--id #32` becomes a
    // shell comment - bare digits are the one input form; display stays `#N`).
    let err = parse_id_selection(&["#32".into()]).unwrap_err();
    assert!(err.to_string().contains("drop the '#'"), "{err}");
    assert_eq!(
        parse_id_selection(&["32,33".into(), "34".into()]).unwrap(),
        vec![Sel::Seq(32), Sel::Seq(33), Sel::Seq(34)]
    );
    // A bare number is a `#N` handle, not a locator (no `i`).
    assert_eq!(
        parse_id_selection(&["7".into()]).unwrap(),
        vec![Sel::Seq(7)]
    );
    assert!(parse_id_selection(&["nope".into()]).is_err());
    assert!(parse_id_selection(&["L6812".into()]).is_err()); // missing `i<n>`
}

#[test]
fn seq_extracted_by_positional_zip_of_image_markers() {
    // Text lists `[Image #2] [Image #3]` and the record has 2 image blocks → positional.
    let r: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"see [Image #2] and [Image #3] please"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}}
            ]}}"#,
        )
        .unwrap();
    let imgs = record_images(&r, false);
    assert_eq!(imgs.len(), 2);
    assert_eq!(imgs[0].seq, Some(2));
    assert_eq!(imgs[1].seq, Some(3));
    assert_eq!(imgs[1].media_type, "image/jpeg"); // NOT assumed PNG
                                                  // A count mismatch (1 marker, 2 images) leaves seq unassigned - no misattribution.
    let r2: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"text","text":"only [Image #9] referenced"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}
            ]}}"#,
        )
        .unwrap();
    assert!(record_images(&r2, false).iter().all(|i| i.seq.is_none()));
}

#[test]
fn url_source_has_no_bytes() {
    let r = image_ref_from_source(
        &json!({"type":"url","url":"https://example.test/x.png","media_type":"image/png"}),
        true,
    )
    .unwrap();
    assert_eq!(r.source_kind, "url");
    assert_eq!(r.b64_len, 0);
    assert_eq!(r.url.as_deref(), Some("https://example.test/x.png"));
    assert!(r.data.is_none());
}

#[test]
fn excerpt_around_centers_on_marker_and_is_char_safe() {
    // Centers on the marker, normalizes whitespace, and never slices a multi-byte char.
    let text = "café münster ☕ [Image #7] dolor sit amet consectetur";
    let ex = excerpt_around(text, "[Image #7]", 8);
    assert!(ex.contains("[Image #7]"), "centered on the marker: {ex}");
    assert!(
        ex.contains('☕') || ex.contains("dolor"),
        "window around it: {ex}"
    );
    // No marker → head fallback, still char-safe (no panic on the multi-byte head).
    let head = excerpt_around("résumé piñata ☕ über no marker here", "[Image #9]", 4);
    assert!(!head.is_empty());
}

#[test]
fn format_of_media_type_maps_the_four_api_types() {
    assert_eq!(format_of_media_type("image/png"), Some(ImageOutFormat::Png));
    assert_eq!(
        format_of_media_type("image/jpeg"),
        Some(ImageOutFormat::Jpeg)
    );
    assert_eq!(
        format_of_media_type("image/jpg"),
        Some(ImageOutFormat::Jpeg)
    );
    assert_eq!(format_of_media_type("image/gif"), Some(ImageOutFormat::Gif));
    assert_eq!(
        format_of_media_type("image/webp"),
        Some(ImageOutFormat::Webp)
    );
    assert_eq!(format_of_media_type("image/heic"), None);
}
