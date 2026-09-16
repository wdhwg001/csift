//! PDF page provenance on `image`: a `Read` with a `pages` parameter returns one image
//! element per page and the page number lives nowhere on the element, so csift derives it
//! from the record's own `toolUseResult.firstPage` plus the element's content position.
//! Every other image - a screenshot, a carrier that names no `firstPage` - keeps a null
//! page rather than an assumed 1.

use crate::harness::*;

const PENC: &str = "-Users-dev-example-project";
const PSESS: &str = "7c6b5a49-4738-4261-8f4e-3d2c1b0a9e8d";

/// L1 prompt, L2 the Read tool_use, L3 its `parts` result (a leading text plus three page
/// images, firstPage 5), L4 an ordinary screenshot result on its own record.
fn pdf_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","parentUuid":serde_json::Value::Null,
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"read the survey"}]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","id":"m0","content":[
            {"type":"tool_use","id":"t1","name":"Read",
             "input":{"file_path":"/Users/dev/docs/report.pdf","pages":"5-7"}}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:00:09.000Z",
        "toolUseResult":{"type":"parts","firstPage":5,
            "file":{"count":3,"filePath":"/Users/dev/docs/report.pdf",
                    "originalSize":223744,"outputDir":"/Users/dev/tmp/pdf-1"}},
        "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
            {"type":"text","text":"PDF pages extracted: 3 page(s) from /Users/dev/docs/report.pdf (218.5KB)"},
            img_block("image/jpeg", PNG_1X1),
            img_block("image/jpeg", PNG_RED),
            img_block("image/jpeg", PNG_GREEN)]}]}
    });
    let r3 = serde_json::json!({
        "type":"user","uuid":"u2","parentUuid":"u1","timestamp":"2026-06-07T05:00:12.000Z",
        "toolUseResult":{"type":"image","file":{"filePath":"/Users/dev/shot.png"}},
        "message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":[
            {"type":"text","text":"shot"},
            img_block("image/png", PNG_BLUE)]}]}
    });
    h.write(
        &format!("{PENC}/{PSESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n{r3}\n"),
    );
    h
}

/// The text listing carries ` p<N>` beside the media type, numbered from the range's own
/// first page, and names the document ONCE for the run of pages from one record - never
/// the whole path, which is what the JSON row is for.
#[test]
fn text_listing_numbers_the_pages_and_names_the_document_once() {
    let h = pdf_home();
    let out = h.run(&["image", at(PSESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("L3i") || l.starts_with("L4i"))
        .collect();
    assert_eq!(rows.len(), 4, "{}", out.stdout);
    assert!(rows[0].contains("image/jpeg p5 "), "{}", rows[0]);
    assert!(rows[1].contains("image/jpeg p6 "), "{}", rows[1]);
    assert!(rows[2].contains("image/jpeg p7 "), "{}", rows[2]);
    assert!(
        !rows[3].contains(" p"),
        "the screenshot carries no page: {}",
        rows[3]
    );
    assert!(rows[0].contains("pdf report.pdf"), "{}", rows[0]);
    assert!(
        !rows[1].contains("pdf ") && !rows[2].contains("pdf "),
        "the basename rides the first row of the run only: {}\n{}",
        rows[1],
        rows[2]
    );
    assert!(
        !out.stdout.contains("/Users/dev/docs/report.pdf"),
        "the text listing never carries the whole path: {}",
        out.stdout
    );
}

/// The JSON row carries `page` and `pdf_path` - the path VERBATIM, like every other path
/// field - and both are null on an image that is not a PDF page.
#[test]
fn json_rows_carry_the_page_and_the_verbatim_path() {
    let h = pdf_home();
    let out = h.run(&["image", at(PSESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("jsonl"))
        .filter(|v: &serde_json::Value| v["kind"] == "image")
        .collect();
    assert_eq!(rows.len(), 4, "{}", out.stdout);
    for (i, page) in [5_u64, 6, 7].into_iter().enumerate() {
        assert_eq!(rows[i]["page"], serde_json::json!(page));
        assert_eq!(
            rows[i]["pdf_path"],
            serde_json::json!("/Users/dev/docs/report.pdf")
        );
    }
    assert!(rows[3]["page"].is_null(), "{}", rows[3]);
    assert!(rows[3]["pdf_path"].is_null(), "{}", rows[3]);
}

/// Everything that addressed a page before still does, and the extracted filename is the
/// locator it always was - the page is provenance, never a new id.
#[test]
fn addressing_and_out_filenames_are_unchanged_by_the_page() {
    let h = pdf_home();
    let dir = h.root.join("pages");
    let out = h.run(&[
        "image",
        at(PSESS).as_str(),
        "--id",
        "L3i2",
        "--out",
        dir.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(dir.join("7c6b5a49-L3i2.jpg").is_file(), "{}", out.stdout);
    // The pages ride one record, so they ride one turn - the fixture's only one.
    let turn = h.run(&[
        "image",
        at(PSESS).as_str(),
        "--turn",
        "-1..",
        "--format",
        "json",
    ]);
    assert!(turn.success, "stderr: {}", turn.stderr);
    let pages: Vec<serde_json::Value> = turn
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).expect("jsonl"))
        .filter(|v: &serde_json::Value| v["kind"] == "image")
        .map(|v| v["page"].clone())
        .collect();
    assert_eq!(
        pages,
        vec![
            serde_json::json!(5),
            serde_json::json!(6),
            serde_json::json!(7),
            serde_json::Value::Null
        ],
        "{}",
        turn.stdout
    );
}
