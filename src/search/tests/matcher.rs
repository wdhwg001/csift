//! Pattern compilation, smart case, the literal prefilter and synth-marker gate, persisted-output resolution.

use super::*;

#[test]
fn smart_case_lowercase_is_insensitive() {
    let m = build_matcher(&args("carry")).unwrap();
    assert!(m.is_match("the CARRY logic"));
    assert!(m.is_match("the carry logic"));
}

#[test]
fn smart_case_uppercase_is_sensitive() {
    let m = build_matcher(&args("Carry")).unwrap();
    assert!(m.is_match("the Carry logic"));
    assert!(!m.is_match("the carry logic"));
}

#[test]
fn ignore_case_overrides_smart_case() {
    let mut a = args("Carry");
    a.ignore_case = true;
    let m = build_matcher(&a).unwrap();
    assert!(m.is_match("the carry logic"));
}

#[test]
fn empty_pattern_is_pure_filter_matches_all() {
    let m = build_matcher(&args("")).unwrap();
    assert!(m.is_pure_filter());
    assert!(m.is_match("literally anything"));
}

#[test]
fn multiline_dot_crosses_newline() {
    let mut a = args("foo.*bar");
    a.multiline = true;
    let m = build_matcher(&a).unwrap();
    assert!(m.is_match("foo\nmiddle\nbar"));
    let m2 = build_matcher(&args("foo.*bar")).unwrap();
    assert!(!m2.is_match("foo\nbar"));
}

#[test]
fn required_literal_only_for_plain_patterns() {
    assert_eq!(required_literal("carry"), Some(b"carry".to_vec()));
    assert!(required_literal("ca.ry").is_none());
    assert!(required_literal("a|b").is_none());
}

#[test]
fn required_literal_rejects_json_escaped_chars() {
    // DEFECT 2: a pattern char that JSON escapes inside a string ('"', control
    // chars, DEL) does not appear verbatim in the raw JSON line, so a memmem
    // prefilter for it would silently drop a line whose decoded text matches.
    // Such patterns must NOT get a literal prefilter (fall back to regex).
    assert!(
        required_literal("Say\"Xello").is_none(),
        "a quote-containing literal must not be prefiltered"
    );
    assert!(required_literal("a\tb").is_none(), "tab is JSON-escaped");
    assert!(
        required_literal("a\nb").is_none(),
        "newline is JSON-escaped"
    );
    // Non-ASCII multi-byte UTF-8 is emitted verbatim by serde_json → still
    // prefilter-eligible (no JSON escaping). Use a locale-neutral fixture
    // (accented Latin + an emoji, both multi-byte) to prove the bytes pass
    // through unchanged.
    assert_eq!(required_literal("café🛠"), Some("café🛠".as_bytes().to_vec()));
}

#[test]
fn quote_pattern_no_silent_drop_case_sensitive() {
    // DEFECT 2 end-to-end: a record whose DECODED text is `Say"Xello there`. The
    // raw line stores the quote escaped as \". A case-sensitive search for
    // `Say"Xello` must STILL match (no literal prefilter → regex runs on decoded
    // text), not silently drop the hit (SPEC §0). Build the matcher with an
    // uppercase letter so smart-case stays case-SENSITIVE (the buggy path).
    let m = build_matcher(&args("Say\"Xello")).unwrap();
    assert!(
        m.prefilter.is_none(),
        "no byte prefilter for a quote-containing literal"
    );
    // The decoded text matches the regex.
    assert!(m.is_match("Say\"Xello there"));
    // And the raw-line gate must NOT drop the carrier (can't prove absence).
    let raw = br#"{"type":"user","message":{"role":"user","content":"Say\"Xello there"}}"#;
    assert!(
        m.line_may_match(raw),
        "without a literal prefilter the line passes to the regex stage"
    );
}

#[test]
fn prefilter_drops_lines_without_literal() {
    // Smart-case lowercased → case-insensitive → the CASELESS literal prefilter:
    // still a raw-byte gate, but folding case (any-case occurrences pass).
    let m = build_matcher(&args("carry")).unwrap();
    assert!(matches!(m.prefilter, Some(Prefilter::CaselessLiteral(_))));
    assert!(m.line_may_match(b"...the CARRY logic..."));
    assert!(m.line_may_match(b"...the carry logic..."));
    assert!(!m.line_may_match(b"...nothing relevant..."));
    assert!(!m.file_may_match(b"a whole file without the needle"));
    assert!(m.file_may_match(b"prefix bytes then Carry appears"));
    // A case-sensitive plain literal gets the byte-exact memmem prefilter.
    let m2 = build_matcher(&args("Carry")).unwrap();
    assert!(matches!(m2.prefilter, Some(Prefilter::Literal(_))));
    assert!(m2.line_may_match(b"...the Carry logic..."));
    assert!(!m2.line_may_match(b"...the CARRY logic..."));
    assert!(!m2.line_may_match(b"...nothing relevant..."));
}

#[test]
fn prefilter_whitespace_literal_is_ineligible() {
    // `normalize_line` collapses whitespace in several render paths (genuine-user
    // text, peer bodies, notification reports), so a rendered "hello world" can be
    // raw "hello\nworld" - a space-carrying literal must NOT anchor a byte
    // prefilter in EITHER case mode.
    let m = build_matcher(&args("hello world")).unwrap();
    assert!(m.prefilter.is_none());
    let m2 = build_matcher(&args("Hello World")).unwrap();
    assert!(m2.prefilter.is_none(), "case-sensitive too");
    // No prefilter ⇒ nothing is provably a miss.
    assert!(m.line_may_match(b"unrelated bytes"));
    assert!(m.file_may_match(b"unrelated bytes"));
}

#[test]
fn synth_marker_keeps_line_and_file_matchable_without_literal() {
    // A `<task-notification>` record renders a FABRICATED kind slug ("subagent") +
    // status that appear nowhere in its raw bytes; the marker must keep the
    // line/file in the match pipeline even though the literal prefilter misses.
    let m = build_matcher(&args("subagent")).unwrap();
    assert!(m.prefilter.is_some());
    let line = br#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>t1</task-id><summary>Agent \"probe\" completed</summary></task-notification>"}}"#;
    assert!(m.line_may_match(line));
    assert!(m.file_may_match(line));
    // The rejection reconstruction appends a `[plan: …]` pointer resolved from a
    // DIFFERENT record - its marker keeps the carrier matchable too.
    let rej = br#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"To tell you how to proceed, the user said:\ngo"}]}}"#;
    let m_plan = build_matcher(&args("plan")).unwrap();
    assert!(m_plan.line_may_match(rej));
    // A line with neither literal nor marker is still provably a miss.
    assert!(!m.line_may_match(b"{\"type\":\"user\"} nothing relevant"));
    assert!(!m.file_may_match(b"a whole file with nothing relevant"));
}

#[test]
fn resolve_persisted_flag_adds_pointer_markers() {
    let mut a = args("zzguarded");
    a.resolve_persisted = true;
    let m = build_matcher(&a).unwrap();
    // A persisted-output pointer line can match EXTERNAL file content, so under
    // `--resolve-persisted` it must stay matchable despite lacking the literal.
    let line = br#"{"toolUseResult":{"persistedOutputPath":"/tmp/x.txt"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":"Full output saved to: /tmp/x.txt"}]}}"#;
    assert!(m.line_may_match(line));
    // Without the flag the same line is provably a miss.
    let m2 = build_matcher(&args("zzguarded")).unwrap();
    assert!(!m2.line_may_match(line));
}

#[test]
fn resolve_persisted_text_reads_file_or_notes_failure() {
    // Success: the resolved text is the file content (not the inline pointer).
    let dir = std::env::temp_dir();
    let p = dir.join(format!("csift-persist-test-{}.txt", std::process::id()));
    std::fs::write(&p, "THE REAL PERSISTED BODY with a deep token zzqqxx").unwrap();
    let resolved = resolve_persisted_text(&p.to_string_lossy(), "<persisted-output> pointer");
    assert!(resolved.contains("zzqqxx"), "got: {resolved}");
    assert!(
        !resolved.contains("pointer"),
        "inline pointer should be replaced"
    );
    std::fs::remove_file(&p).ok();

    // Failure: a missing file keeps the inline text + an explicit note (never fatal).
    let missing = resolve_persisted_text("/no/such/csift/file.txt", "inline preview text");
    assert!(missing.contains("inline preview text"));
    assert!(missing.contains("could not resolve persisted output"));
}

#[test]
fn resolve_persisted_end_to_end_matches_deep_token() {
    // Build a carrier whose inline tool_result is a <persisted-output> pointer to
    // a temp file; a token that lives ONLY in the file (not inline) must match
    // ONLY when --resolve-persisted is set. This is the discriminating test.
    let dir = std::env::temp_dir();
    let p = dir.join(format!("csift-e2e-persist-{}.txt", std::process::id()));
    std::fs::write(&p, "deep file body containing the token wibblewobble here").unwrap();
    let line = format!(
        r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: {}\n\nPreview (first 2KB):\n(no token here)\n</persisted-output>"}}]}}}}"#,
        // JSON-escape the path: a Windows temp dir carries backslashes.
        p.to_string_lossy().replace('\\', "\\\\")
    );
    let r: Record = serde_json::from_str(&line).expect("valid record");

    // WITHOUT resolution: the deep token is not in the inline content → no hit.
    let m = build_matcher(&args("wibblewobble")).unwrap();
    let mut no_resolve = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut no_resolve,
    );
    assert!(no_resolve.is_empty(), "deep token must NOT match inline");

    // WITH resolution: the file is read, the token is found → exactly one hit.
    let mut with_resolve = Vec::new();
    collect_record_hits(
        &r,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        true,
        EXCERPT_MAX,
        &PlanIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut with_resolve,
    );
    assert_eq!(with_resolve.len(), 1, "deep token matches after resolution");
    assert_eq!(with_resolve[0].class, Class::AgentToolResult);

    std::fs::remove_file(&p).ok();
}
