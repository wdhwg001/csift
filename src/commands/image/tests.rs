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
        survival: "live",
        page: None,
        pdf_path: None,
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
    // "hi" -> "aGk=", "caf\u{e9}" bytes -> padded; whitespace tolerated.
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
    // Text lists `[Image #2] [Image #3]` and the record has 2 image blocks -> positional.
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
fn a_tool_result_screenshot_is_collected_in_document_order() {
    // The second carrier: an {type:"image"} element nested in a tool_result's content
    // ARRAY. It shares the record's 1-based `img_index` run with any direct block, so
    // the `L<line>i<n>` locator stays stable.
    let r: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"tool_result","tool_use_id":"t1","content":[
                    {"type":"text","text":"shot"},
                    {"type":"image","source":{"type":"base64","media_type":"image/webp","data":"aGVsbG8="}},
                    {"type":"image","source":{"type":"base64","media_type":"image/gif","data":"eWVz"}}
                ]}
            ]}}"#,
        )
        .unwrap();
    let imgs = record_images(&r, false);
    assert_eq!(imgs.len(), 3, "the direct block and both nested elements");
    assert_eq!(
        imgs.iter().map(|i| i.img_index).collect::<Vec<_>>(),
        vec![1, 2, 3],
        "one ascending run across both carriers"
    );
    assert_eq!(
        imgs.iter()
            .map(|i| i.media_type.as_str())
            .collect::<Vec<_>>(),
        vec!["image/png", "image/webp", "image/gif"],
        "document order, and the nested types are read from their own source"
    );
    // A non-image element of the same array contributes nothing.
    let r2: Record = serde_json::from_str(
            r#"{"type":"user","message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"no image here"}]}
            ]}}"#,
        )
        .unwrap();
    assert!(record_images(&r2, false).is_empty());
}

#[test]
fn seq_follows_image_paste_ids_not_marker_text_order() {
    // The operator typed the markers out of order; the image blocks (and imagePasteIds)
    // are in ascending id order, so the join is by NUMBER, not by marker position.
    let r: Record = serde_json::from_str(
            r#"{"type":"user","imagePasteIds":[28,29,30],"message":{"role":"user","content":[
                {"type":"text","text":"compare [Image #30] with [Image #28] and [Image #29]"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}},
                {"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"eWVz"}}
            ]}}"#,
        )
        .unwrap();
    let seqs: Vec<Option<usize>> = record_images(&r, false).iter().map(|i| i.seq).collect();
    assert_eq!(seqs, vec![Some(28), Some(29), Some(30)]);
    // An id array that does not cover every image block falls back to the positional
    // zip (under its own count guard); here the counts agree, so the markers win.
    let r2: Record = serde_json::from_str(
            r#"{"type":"user","imagePasteIds":[7],"message":{"role":"user","content":[
                {"type":"text","text":"[Image #5] then [Image #4]"},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGk="}},
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}
            ]}}"#,
        )
        .unwrap();
    let seqs: Vec<Option<usize>> = record_images(&r2, false).iter().map(|i| i.seq).collect();
    assert_eq!(seqs, vec![Some(5), Some(4)]);
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
    // No marker -> head fallback, still char-safe (no panic on the multi-byte head).
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

/// A `parts` Read result, built from the writer's documented mapping: element 0 is the
/// leading text and every element after it is one requested page, in page order.
/// `elements` is the JSON after that leading text; `tur` is the record's own result object.
fn parts_record(tur: serde_json::Value, elements: &str) -> Record {
    let line = format!(
        r#"{{"type":"user","message":{{"role":"user","content":[
            {{"type":"tool_result","tool_use_id":"t1","content":[
                {{"type":"text","text":"PDF pages extracted: N page(s) from /Users/dev/docs/report.pdf (218.5KB)"}}
                {elements}
            ]}}
        ]}},"toolUseResult":{tur}}}"#
    );
    serde_json::from_str(&line).expect("fixture parses")
}

/// One `{type:"image"}` element, as the mapper writes a successfully rendered page.
const PAGE: &str =
    r#",{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGk="}}"#;

/// The result object of a Read that carried a `pages` parameter.
fn parts_tur(first_page: u64, count: u64) -> serde_json::Value {
    json!({
        "type": "parts",
        "firstPage": first_page,
        "file": {
            "count": count,
            "filePath": "/Users/dev/docs/report.pdf",
            "originalSize": 223_744,
            "outputDir": "/Users/dev/tmp/pdf-1"
        }
    })
}

/// The page number is `firstPage` plus the element's CONTENT POSITION, so a range that
/// does not start at page 1 numbers from where it started - never from 1, and never from
/// the image ordinal.
#[test]
fn pdf_page_is_first_page_plus_content_position() {
    for (first, count) in [(1_u64, 3_u64), (7, 2)] {
        let r = parts_record(
            parts_tur(first, count),
            &PAGE.repeat(usize::try_from(count).unwrap()),
        );
        let imgs = record_images(&r, false);
        assert_eq!(imgs.len(), usize::try_from(count).unwrap());
        let pages: Vec<Option<usize>> = imgs.iter().map(|i| i.page).collect();
        let want: Vec<Option<usize>> = (0..count)
            .map(|k| Some(usize::try_from(first + k).unwrap()))
            .collect();
        assert_eq!(pages, want, "firstPage {first}, {count} page(s)");
        assert!(
            imgs.iter()
                .all(|i| i.pdf_path.as_deref() == Some("/Users/dev/docs/report.pdf")),
            "the document rides every page row, verbatim"
        );
    }
}

/// A page the harness could not render is a TEXT element in the same sequence, so the
/// pages after it keep their own numbers. Counting IMAGES instead of content positions
/// would name every later page one too low - which is the whole reason the derivation
/// counts positions.
#[test]
fn a_failed_page_keeps_the_later_pages_on_their_own_numbers() {
    let elements = format!(
        r#"{PAGE},{{"type":"text","text":"[Page 8 could not be processed as an image]"}}{PAGE}"#
    );
    let r = parts_record(parts_tur(7, 3), &elements);
    let imgs = record_images(&r, false);
    assert_eq!(
        imgs.iter().map(|i| i.page).collect::<Vec<_>>(),
        vec![Some(7), Some(9)],
        "positions 1 and 3 of the content array"
    );
    assert_eq!(
        imgs.iter().map(|i| i.img_index).collect::<Vec<_>>(),
        vec![1, 2],
        "the image ordinal keeps counting images - it is not the page"
    );
}

/// A carrier that names no `firstPage` names no page. Three shapes reach this: the REPL
/// tool, which re-emits inner-Read page images in its own result and carries no
/// `firstPage` at all; a `parts` result from a build before the key existed; and an
/// ordinary screenshot. None of them may be read as page 1.
#[test]
fn a_carrier_without_first_page_names_no_page() {
    let repl = parts_record(
        json!({"type":"repl","images":2,"failedPages":[]}),
        &PAGE.repeat(2),
    );
    let older = parts_record(
        json!({"type":"parts","file":{"count":2,"filePath":"/Users/dev/docs/report.pdf"}}),
        &PAGE.repeat(2),
    );
    let untyped = parts_record(json!("a bare string result"), PAGE);
    for (what, rec) in [
        ("repl", repl),
        ("pre-firstPage", older),
        ("string", untyped),
    ] {
        let imgs = record_images(&rec, false);
        assert!(!imgs.is_empty(), "{what}: the images are still listed");
        assert!(
            imgs.iter()
                .all(|i| i.page.is_none() && i.pdf_path.is_none()),
            "{what}: no page and no document, rather than an assumed page 1"
        );
    }
}

/// The TYPE half of the gate, pinned on its own: a result object that is NOT `parts` but
/// DOES carry a top-level `firstPage` names no page. That shape is the REPL carrier's
/// (IMG-018) - it re-emits inner-Read page images and can hold an offset of its own name,
/// while its element positions map to nothing, several documents having been flattened
/// into one capped list. Reading `firstPage` without the type check would number those.
#[test]
fn a_result_that_is_not_parts_names_no_page_even_carrying_first_page() {
    let r = parts_record(
        json!({"type":"repl","firstPage":5,"file":{"filePath":"/Users/dev/docs/report.pdf"}}),
        &PAGE.repeat(2),
    );
    assert!(record_pdf_pages(&r).is_none());
    let imgs = record_images(&r, false);
    assert_eq!(imgs.len(), 2, "the images are still listed");
    assert!(imgs
        .iter()
        .all(|i| i.page.is_none() && i.pdf_path.is_none()));
}

/// A `parts` result describes ONE tool call, so a record carrying two tool_result blocks
/// could not say which one the pages belong to. The images are still listed; only the
/// derivation stands down.
#[test]
fn two_tool_result_blocks_refuse_the_derivation() {
    let line = format!(
        r#"{{"type":"user","message":{{"role":"user","content":[
            {{"type":"tool_result","tool_use_id":"t1","content":[
                {{"type":"text","text":"PDF pages extracted: 1 page(s) from /Users/dev/docs/report.pdf (1KB)"}}{PAGE}
            ]}},
            {{"type":"tool_result","tool_use_id":"t2","content":[
                {{"type":"text","text":"shot"}}{PAGE}
            ]}}
        ]}},"toolUseResult":{}}}"#,
        parts_tur(1, 1)
    );
    let r: Record = serde_json::from_str(&line).expect("fixture parses");
    let imgs = record_images(&r, false);
    assert_eq!(imgs.len(), 2);
    assert!(imgs.iter().all(|i| i.page.is_none()));
}

/// A whole-PDF Read returns a `{type:"document"}` block, not an image, and `image` lists
/// images. It stays unlisted - the surface would otherwise extract a PDF payload under an
/// `ext()` that has no `application/pdf` row.
#[test]
fn the_whole_pdf_document_block_is_not_listed() {
    let r = parts_record(
        json!({"type":"pdf","file":{"filePath":"/Users/dev/docs/report.pdf","originalSize":173_363}}),
        r#",{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"aGk="}}"#,
    );
    assert!(record_images(&r, false).is_empty());
}

/// A `parts` result whose content array holds ONLY the leading text maps no page, so the
/// reader answers `None` rather than a zero-page range. The distinction is not academic:
/// a zero-page range's `last` sits one BELOW its `first`, which the tool-result unit would
/// print as `[pdf pages 5-4 of report.pdf]`.
#[test]
fn a_parts_result_with_no_page_element_maps_no_page() {
    let r = parts_record(parts_tur(5, 0), "");
    assert!(record_pdf_pages(&r).is_none());
    assert!(record_images(&r, false).is_empty());
}

/// The position-to-page map at both ends, and the basename split.
#[test]
fn page_at_boundaries_and_basename_separators() {
    let p = PdfPages {
        first: 7,
        count: 3,
        path: Some("/Users/dev/docs/report.pdf".into()),
    };
    assert_eq!(p.page_at(0), None, "element 0 is the leading text");
    assert_eq!(p.page_at(1), Some(7));
    assert_eq!(p.page_at(3), Some(9), "the last mapped position");
    assert_eq!(p.page_at(4), None, "past the pages the result carries");
    assert_eq!(basename("/Users/dev/docs/report.pdf"), "report.pdf");
    assert_eq!(basename(r"C:\Users\dev\docs\report.pdf"), "report.pdf");
    assert_eq!(basename("report.pdf"), "report.pdf");
    assert_eq!(basename("/Users/dev/docs/"), "docs", "a trailing separator");
    assert_eq!(basename("/"), "/", "nothing but separators renders whole");
}
