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
    // Non-ASCII multi-byte UTF-8 is emitted verbatim by serde_json -> still
    // prefilter-eligible (no JSON escaping). Use a locale-neutral fixture
    // (accented Latin + an emoji, both multi-byte) to prove the bytes pass
    // through unchanged.
    assert_eq!(required_literal("café🛠"), Some("café🛠".as_bytes().to_vec()));
}

#[test]
fn quote_pattern_no_silent_drop_case_sensitive() {
    // DEFECT 2 end-to-end: a record whose DECODED text is `Say"Xello there`. The
    // raw line stores the quote escaped as \". A case-sensitive search for
    // `Say"Xello` must STILL match (no literal prefilter -> regex runs on decoded
    // text), not silently drop the hit (SPEC section 0). Build the matcher with an
    // uppercase letter so smart-case stays case-SENSITIVE (the buggy path).
    let m = build_matcher(&args("Say\"Xello")).unwrap();
    // The WHOLE literal must never anchor (the quote is stored escaped), but its
    // longest safe RUN ("Xello") survives JSON encoding verbatim and gates safely.
    assert!(
        matches!(m.prefilter, Some(Prefilter::Literal(_))),
        "the quote-free run anchors; the full literal never does"
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
    // Smart-case lowercased -> case-insensitive -> the CASELESS literal prefilter:
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
fn prefilter_space_pattern_anchors_its_longest_safe_run() {
    // `normalize_line` collapses whitespace in several render paths, so a rendered
    // "hello world" can be raw "hello\nworld" - the WHOLE space-carrying literal
    // must never anchor a byte prefilter. Its whitespace-free RUNS survive any
    // rewrite though (they sit inside unrewritten non-whitespace spans), so the
    // longest safe run gates instead of nothing at all.
    let m = build_matcher(&args("hello world")).unwrap();
    assert!(matches!(m.prefilter, Some(Prefilter::CaselessLiteral(_))));
    // Raw bytes hold the JSON-escaped newline seam; the run still hits.
    assert!(m.line_may_match(br"prefix hello\nworld suffix"));
    assert!(!m.line_may_match(b"unrelated bytes"));
    let m2 = build_matcher(&args("Hello World")).unwrap();
    assert!(
        matches!(m2.prefilter, Some(Prefilter::Literal(_))),
        "case-sensitive: byte-exact run"
    );
    assert!(m2.line_may_match(br"say Hello\nWorld now"));
    assert!(!m2.line_may_match(b"unrelated bytes"));
}

#[test]
fn required_needles_alternation_and_necessity() {
    // The motivating query shape: BOTH branches require the same literal - the
    // union dedups to ONE needle and the whole-file gate applies.
    assert_eq!(
        required_needles("TodoWrite.*legacy|legacy.*TodoWrite"),
        Some(vec!["TodoWrite".to_string()])
    );
    // Distinct branch literals union into an ANY-of set.
    // (The parser splits a literal run before a repetition: `betay+z` is
    // `beta` + `y+` + `z`, so the branch's strongest safe part is "beta".)
    assert_eq!(
        required_needles("alphax.*[0-9]|betay+z"),
        Some(vec!["alphax".to_string(), "beta".to_string()])
    );
    // One branch without a safe needle kills the gate (necessity would break).
    assert!(required_needles("alphax|[0-9]+").is_none());
    // Class-only / short-literal patterns anchor nothing.
    assert!(required_needles("[a-z]+").is_none());
    assert!(required_needles("ca.ry").is_none(), "runs under 3 bytes");
    // The plain-literal fast path is unchanged.
    assert_eq!(required_needles("carry"), Some(vec!["carry".to_string()]));
}

#[test]
fn required_needles_concat_repetition_and_runs() {
    // A concat picks its strongest part (longest worst-case needle).
    assert_eq!(
        required_needles("abc.*defgh"),
        Some(vec!["defgh".to_string()])
    );
    // min=0 repetition contributes nothing; the sibling part anchors.
    assert_eq!(
        required_needles("(foo)*barbaz"),
        Some(vec!["barbaz".to_string()])
    );
    // min>=1 repetition requires its inner.
    assert_eq!(
        required_needles("(carry)+"),
        Some(vec!["carry".to_string()])
    );
    // Anchors and word boundaries never block extraction.
    assert_eq!(
        required_needles("^\\bTodoWrite\\b$"),
        Some(vec!["TodoWrite".to_string()])
    );
    // A JSON-escapable char splits a literal into safe runs.
    assert_eq!(
        required_needles("say\"hopeful.*"),
        Some(vec!["hopeful".to_string()])
    );
}

#[test]
fn metachar_pattern_gate_composes_with_synth_markers() {
    // Needle "subagent" extracted from both branches; the notification line's raw
    // bytes lack it (the kind slug is SYNTHESIZED at render) - the marker keeps the
    // line matchable, the literal miss alone proves nothing.
    let m = build_matcher(&args("subagent.*probe|probe.*subagent")).unwrap();
    assert!(m.prefilter.is_some());
    let line = br#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>t1</task-id><summary>Agent \"probe\" completed</summary></task-notification>"}}"#;
    assert!(m.line_may_match(line));
    assert!(
        !m.line_may_match(b"{\"type\":\"user\"} nothing relevant"),
        "no needle, no marker: provably a miss"
    );
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

// -- Stage 2 of the whole-file gate re-renders from the SOURCE LINE --

/// The gate's second stage is the one place csift proves a NEGATIVE: it re-renders a
/// marker-bearing line and, on a miss, declares the whole file unmatchable. So it has to
/// re-render what the emission would render - and several renders read the source LINE,
/// because a bookkeeping line's content lives in fields `Record` does not model at all.
///
/// The instrument is the two answers side by side: the same record and the same pattern,
/// once with the line and once without. They differ, and that difference is exactly the
/// text a gate given no line would have pruned a file over.
#[test]
fn stage_two_renders_a_line_only_reachable_through_its_source_bytes() {
    let line = r#"{"type":"cost-state","sessionId":"0a1b2c3d-0000-0000-0000-000000000000","totalCostUSD":1.5,"totalDuration":900}"#;
    let record: Record = serde_json::from_str(line).expect("valid fixture record");
    let m = build_matcher(&args("totalDuration")).unwrap();
    assert!(
        m.synth_texts_match(&record, Some(line)),
        "with its own line the bookkeeping render answers, and the file is not pruned"
    );
    assert!(
        !m.synth_texts_match(&record, None),
        "without it the same record renders nothing - the hole the source line closes"
    );
}

/// One case per REGISTERED verifiable marker: a fixture line carrying the needle, and a
/// pattern that only the FABRICATED render carries, so the case exercises the stage-2
/// re-render rather than the literal scan.
struct MarkerCase {
    /// The needle as `synth_marker_finders` registers it.
    needle: &'static str,
    /// A pattern present in the marker's synthesized text and absent from the raw bytes.
    pattern: &'static str,
    /// A line carrying the needle.
    line: &'static str,
    /// True when handing the SOURCE LINE through widens the answer rather than leaving
    /// it alone - the `harness.bookkeeping` head-plus-verbatim-body law.
    widens: bool,
}

/// One case per verifiable needle, in registration order.
const MARKER_CASES: &[MarkerCase] = &[
    MarkerCase {
        needle: "<task-notification>",
        // The pulse's fabricated head: the kind slug, the task id and the status in one
        // phrase, which the raw line carries in three separate tags.
        pattern: "agent t1 completed",
        line: r#"{"type":"user","message":{"role":"user","content":"<task-notification><task-id>t1</task-id><summary>Agent \"probe\" completed</summary></task-notification>"}}"#,
        widens: false,
    },
    MarkerCase {
        needle: r#""answers""#,
        // The `Q1:` scaffold the reconstruction fabricates around the question, which the
        // raw line carries only as a key in the answers map.
        pattern: "Q1: Pick one",
        line: r#"{"type":"user","toolUseResult":{"answers":{"Pick one":"yes"}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"ok"}]}}"#,
        widens: false,
    },
    // The four answer-marker branches: no `toolUseResult`, so the exchange renders
    // through the fallback and the `[AskUserQuestion]` scaffold is entirely fabricated.
    MarkerCase {
        needle: "User has answered your questions",
        pattern: "AskUserQuestion",
        line: r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"User has answered your questions"}]}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "Your questions have been answered",
        pattern: "AskUserQuestion",
        line: r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"Your questions have been answered"}]}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "The user answered:",
        pattern: "AskUserQuestion",
        line: r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"The user answered: \"q\"=\"a\""}]}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "The user responded:",
        pattern: "AskUserQuestion",
        line: r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"The user responded: go ahead"}]}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "stopped by the user",
        pattern: "subagent stopped",
        line: r#"{"type":"user","message":{"role":"user","content":"Background agent \"probe\" was stopped by the user."}}"#,
        widens: false,
    },
    MarkerCase {
        needle: r#""fallback""#,
        pattern: "fell back from",
        line: r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"fallback","from":{"model":"m-a"},"to":{"model":"m-b"}}]}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "compact_boundary",
        pattern: "trigger=auto",
        line: r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","content":"Conversation compacted","compactMetadata":{"trigger":"auto"}}"#,
        widens: false,
    },
    MarkerCase {
        needle: "turn_duration",
        // The raw line spells the subtype with an underscore; the head spells it with a
        // space, so the pattern reaches only the fabricated text.
        pattern: "turn duration",
        line: r#"{"type":"system","subtype":"turn_duration","uuid":"t1","durationMs":64911,"messageCount":908}"#,
        widens: false,
    },
    MarkerCase {
        needle: "stop_hook_summary",
        pattern: "stop hooks",
        line: r#"{"type":"system","subtype":"stop_hook_summary","uuid":"h1","hookCount":2,"hookErrors":[],"preventedContinuation":false}"#,
        widens: false,
    },
    MarkerCase {
        needle: "file-history-",
        // A key the HEAD elides entirely: it names the tracked paths and their versions
        // and nothing else. The verbatim body is the only render that carries it.
        pattern: "snapshotMessageId",
        line: r#"{"type":"file-history-snapshot","messageId":"m1","snapshotMessageId":"sm-9","snapshot":{"timestamp":"2026-01-01T00:00:00Z","trackedFileBackups":{"/a/b.rs":{"version":3}}}}"#,
        widens: true,
    },
    MarkerCase {
        needle: r#""subtype""#,
        // `[notice warning]`: the head names the LEAF's own word, which the raw line
        // does not carry at all, and the level beside it - so the phrase exists only in
        // the fabricated head.
        pattern: "notice warning",
        line: r#"{"type":"system","subtype":"informational","level":"warning","uuid":"n1","content":"heads up"}"#,
        widens: false,
    },
];

/// Handing the source line through moves NO verdict the record alone can answer, and
/// where it does move one it may only ADD matches.
///
/// The set under test is the REGISTERED one, read back out of `synth_marker_finders`
/// rather than retyped: twelve hand-written verifiable needles, seven unconditional and
/// five behind a selector reaching their leaf, then the BOOKKEEPING group. A needle added
/// without a case here fails this test by name instead of slipping through, which is the
/// point - the earlier version of this test enumerated four of the twelve and stated a
/// universal it could not have checked.
///
/// The bookkeeping group is checked against the TABLE it is generated from rather than
/// case by case, because one render builds all of them: sixteen needles, each the quoted
/// line-type value that also admits its own leaf as a scan candidate, so a needle can only
/// go missing by the table losing an entry. The worked case that exercises the stage-2
/// re-render of such a head is `a_fabricated_bookkeeping_head_survives_the_whole_file_gate`
/// in the family's own test file.
///
/// ELEVEN of the twelve render from the record's own fields, so the extra argument is
/// inert and the two verdicts are EQUAL. The twelfth is `file-history-`, and it is a
/// `harness.bookkeeping` line: that family renders its key=value HEAD and then the
/// line's own verbatim JSON, so with the line it answers for every key the head elides.
/// The law there is a SUPERSET, never equality - handing the line through may only widen
/// the answer, because narrowing it is exactly how the whole-file gate would prune a file
/// it has to keep.
///
/// What this file does NOT cover, and where it lives: the gate re-renders EVERY VIEW a
/// record could carry, not the line alone - a record with no leaf of its own reaches the
/// render seam only through the caller's class. Every case below happens to be a line
/// WITH its own leaf or a text the engine's generic arms produce, so none of them can see
/// that half; `the_whole_file_gate_reaches_a_render_only_a_class_can_name` (tests/seam.rs)
/// is the one that does.
#[test]
fn stage_two_answers_every_registered_marker_the_same_or_wider_with_the_line() {
    // The glob reaches every leaf under `harness` on all three narrowing axes, which is
    // what arms the five conditional needles; without it the registered set is seven.
    let mut probe = args("zz");
    probe.labels = vec!["harness.*".to_string()];
    let (verifiable, _) = synth_marker_finders(&probe);
    let registered: Vec<String> = verifiable
        .iter()
        .map(|f| String::from_utf8_lossy(f.needle()).into_owned())
        .collect();
    let covered: Vec<&str> = MARKER_CASES.iter().map(|c| c.needle).collect();
    let (hand_written, bookkeeping) = registered.split_at(covered.len());
    assert_eq!(
        hand_written, covered,
        "a registered needle has no case, or the cases drifted out of registration order"
    );
    // The BOOKKEEPING group, in registration order: the candidate-gate table, then the
    // queue line's own needle (one needle, two leaves, and only the harness half
    // fabricates a head).
    let expected: Vec<String> = crate::commands::search::BOOKKEEPING_NEEDLES
        .iter()
        .map(|(n, _)| (*n).to_string())
        .chain(std::iter::once(r#""queue-operation""#.to_string()))
        .collect();
    assert_eq!(
        bookkeeping, expected,
        "the bookkeeping marker set drifted from the candidate-gate table it is built from"
    );

    for case in MARKER_CASES {
        let record: Record = serde_json::from_str(case.line).expect("valid fixture record");
        let m = build_matcher(&args(case.pattern)).unwrap();
        let with = m.synth_texts_match(&record, Some(case.line));
        let without = m.synth_texts_match(&record, None);
        assert!(
            with,
            "{}: the marker's own render should carry {}",
            case.needle, case.pattern
        );
        // The SUPERSET law, asserted for every needle: a line may only add matches.
        assert!(
            with || !without,
            "{}: the source line NARROWED the verdict - the gate would prune a file it must keep",
            case.needle
        );
        if case.widens {
            assert!(
                !without,
                "{}: this case exists because the line changes the answer; it no longer does",
                case.needle
            );
        } else {
            assert_eq!(
                with, without,
                "{}: verdict moved, and this render does not read the line",
                case.needle
            );
        }
        // The pattern must be absent from the raw BYTES, or the case would pass on the
        // literal scan and prove nothing about the re-render. (For the widening case the
        // body IS the raw line, so the head/body split is what the assertions above read.)
        if !case.widens {
            assert!(
                !case.line.contains(case.pattern),
                "{}: the pattern is a literal substring of the line - the case proves nothing",
                case.needle
            );
        }
    }

    // A marker line whose synthesized text does NOT carry the pattern still answers no,
    // with the line as without it - the gate stays able to prune.
    let line = MARKER_CASES[0].line;
    let record: Record = serde_json::from_str(line).expect("valid fixture record");
    let m = build_matcher(&args("zzabsent")).unwrap();
    assert!(!m.synth_texts_match(&record, Some(line)));
    assert!(!m.synth_texts_match(&record, None));
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
    let (_dir, p) = crate::testfs::Scratch::file("persist-test", "persisted.txt");
    std::fs::write(&p, "THE REAL PERSISTED BODY with a deep token zzqqxx").unwrap();
    let resolved = resolve_persisted_text(&p.to_string_lossy(), "<persisted-output> pointer");
    assert!(resolved.contains("zzqqxx"), "got: {resolved}");
    assert!(
        !resolved.contains("pointer"),
        "inline pointer should be replaced"
    );

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
    let (_dir, p) = crate::testfs::Scratch::file("e2e-persist", "persisted.txt");
    std::fs::write(&p, "deep file body containing the token wibblewobble here").unwrap();
    let line = format!(
        r#"{{"type":"user","uuid":"u0","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"x","content":"<persisted-output>\nOutput too large (1 KB). Full output saved to: {}\n\nPreview (first 2KB):\n(no token here)\n</persisted-output>"}}]}}}}"#,
        // JSON-escape the path: a Windows temp dir carries backslashes.
        p.to_string_lossy().replace('\\', "\\\\")
    );
    let r: Record = serde_json::from_str(&line).expect("valid record");

    // WITHOUT resolution: the deep token is not in the inline content -> no hit.
    let m = build_matcher(&args("wibblewobble")).unwrap();
    let mut no_resolve = Vec::new();
    collect_record_hits(
        &r,
        None,
        None,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        false,
        EXCERPT_MAX,
        &CallIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut no_resolve,
    );
    assert!(no_resolve.is_empty(), "deep token must NOT match inline");

    // WITH resolution: the file is read, the token is found -> exactly one hit.
    let mut with_resolve = Vec::new();
    collect_record_hits(
        &r,
        None,
        None,
        LabelFilter::new(&["agent.tool.result".to_string()], &[]),
        &m,
        true,
        EXCERPT_MAX,
        &CallIndex::default(),
        &HashMap::new(),
        &ClassifyCtx::top_level(),
        &mut with_resolve,
    );
    assert_eq!(with_resolve.len(), 1, "deep token matches after resolution");
    assert_eq!(with_resolve[0].class, Some(Class::AgentToolResult));
}

#[test]
fn any_literal_prefilter_case_sensitive_multi_needle() {
    // Uppercase pattern -> case-sensitive; two distinct branch needles -> AnyLiteral.
    let m = build_matcher(&args("Alphax.*probe|Betay.*probe")).unwrap();
    assert!(matches!(m.prefilter, Some(Prefilter::AnyLiteral(_))));
    // (Branch 2's strongest safe needle ties between "Betay" and "probe"; either
    // choice is a REQUIRED literal of that branch, so the gate stays sound.)
    assert!(m.line_may_match(b"... Alphax then something ..."));
    assert!(m.line_may_match(b"... Betay probe something ..."));
    assert!(!m.line_may_match(b"... alphax lowercase misses ..."));
    assert!(!m.line_may_match(b"... nothing relevant ..."));
    // Whole-file and prefilter-only forms run the same needle set.
    assert!(m.file_may_match(b"a whole file where Alphax appears"));
    assert!(!m.file_may_match(b"a whole file with neither"));
    assert!(m.line_prefilter_hits(b"probe present"));
    assert!(!m.line_prefilter_hits(b"nothing"));
    // The needle-set size cap: >8 branch literals -> no gate.
    let wide = "aaax1|bbbx2|cccx3|dddx4|eeex5|fffx6|gggx7|hhhx8|iiix9";
    let m2 = build_matcher(&args(wide)).unwrap();
    assert!(m2.prefilter.is_none(), "over-cap union anchors nothing");
    // A prefilter-less matcher can prove nothing.
    assert!(m2.line_prefilter_hits(b"anything"));
    // The empty pattern derives no needles (the pure filter never gates).
    assert!(required_needles("").is_none());
}

#[test]
fn needle_extraction_mutation_pins() {
    // A min-0 repetition contributes NOTHING even when its inner literal is the
    // longest part - it may match zero times, so its literal is not required.
    assert_eq!(
        required_needles("(alphaxx)*barb"),
        Some(vec!["barb".to_string()])
    );
    // A C0 control char inside an otherwise plain pattern is JSON-escaped in raw
    // lines: it splits the runs, and the short remainders anchor nothing.
    assert!(required_needles("ab\u{01}cd").is_none());
}
