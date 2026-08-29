//! Hit rendering: glyphs, labels, excerpt centering and truncation, sibling caps.

use super::*;

// ── Branch-completeness for the pure render helpers ──

#[test]
fn class_path_and_role_glyph_cover_every_leaf() {
    // Every Class::ALL leaf round-trips through path() (the rendered/JSON label) and maps to a
    // role glyph (◂ user, ▸ agent, ⚙ harness) - the cutover replacement for the old flat
    // category_label/glyph table.
    for &c in Class::ALL {
        assert!(!c.path().is_empty());
        let g = role_glyph(c);
        assert!(matches!(g, '◂' | '▸' | '⚙'), "{} -> {g}", c.path());
    }
    assert_eq!(role_glyph(Class::UserMessage), '◂');
    assert_eq!(role_glyph(Class::AgentMessage), '▸');
    assert_eq!(role_glyph(Class::CommInbox), '▸'); // comm is agent-side
    assert_eq!(role_glyph(Class::NotificationWorkflow), '⚙');
}

#[test]
fn render_label_decorates_pairing_and_direction() {
    // ▹ pairing: a paired tool.use renders the two-sided form; a pending use / orphan result
    // render their notes. ⇨ direction: a comm hit appends `from ⇨ to`.
    let paired = Hit {
        class: Class::AgentToolUse,
        labels: vec!["agent.tool.use"],
        excerpt: String::new(),
        timestamp_utc: None,
        tool_name: None,
        model: None,
        attachment_type: None,
        version: None,
        is_error: None,
        direction: None,
        tool_use_id: Some("t1".into()),
        pair: Some(Pairing::Paired),
        line: 0,
        uuid: None,
        raw: None,
        image_ids: Vec::new(),
        from_sidecar: false,
        truncated: false,
    };
    assert_eq!(render_label(&paired), "agent.tool.use ▹ agent.tool.result");
    let pending = Hit {
        pair: Some(Pairing::PendingNoResult),
        ..paired.clone()
    };
    assert_eq!(
        render_label(&pending),
        "agent.tool.use (no result — pending)"
    );
    let orphan = Hit {
        class: Class::AgentToolResult,
        pair: Some(Pairing::OrphanResult),
        ..paired.clone()
    };
    assert_eq!(
        render_label(&orphan),
        "agent.tool.result (use not in scope)"
    );
    let comm = Hit {
        class: Class::CommInbox,
        direction: Some(("VSMultiRegion".into(), "self".into())),
        tool_use_id: None,
        pair: None,
        ..paired.clone()
    };
    assert_eq!(
        render_label(&comm),
        "agent.communication.inbox  VSMultiRegion ⇨ self"
    );
}

#[test]
fn render_tool_use_name_only_input_only_both_neither() {
    assert_eq!(render_tool_use(Some("Bash"), None), "Bash");
    let v = serde_json::json!({"k":"v"});
    // input only (no name) → leading space then the json.
    assert_eq!(render_tool_use(None, Some(&v)), " {\"k\":\"v\"}");
    // both
    assert_eq!(
        render_tool_use(Some("Read"), Some(&v)),
        "Read {\"k\":\"v\"}"
    );
    // neither → empty
    assert_eq!(render_tool_use(None, None), "");
}

#[test]
fn truncate_excerpt_long_and_short() {
    assert_eq!(truncate_excerpt("short"), "short");
    let s = "y".repeat(EXCERPT_MAX + 3);
    let out = truncate_excerpt(&s);
    assert!(out.ends_with("… (+3 chars)"), "got: {out}");
}

#[test]
fn match_excerpt_centers_on_a_deep_match() {
    // The needle sits ~800 chars in - far past EXCERPT_MAX. The OLD head-only
    // excerpt hid it entirely (the bug that forced raw-jsonl reads); centering
    // must surface it, with explicit clipping markers on both sides.
    // Synthetic multi-byte placeholder (neutral emoji) + neutral padding.
    let needle = "🤖🎉✅🚀🌟";
    let text = format!("{}{needle}{}", "🔵".repeat(800), "🟥".repeat(800));
    let m = build_matcher(&args(needle)).unwrap();
    let span = m.locate(&text).expect("matches").expect("has a span");
    let (ex, truncated) = match_excerpt(&text, Some(span), EXCERPT_MAX);
    assert!(ex.contains(needle), "excerpt must show the match: {ex}");
    assert!(
        ex.starts_with('…'),
        "content precedes the window → leading …: {ex}"
    );
    assert!(
        ex.contains("chars)"),
        "content follows → trailing count: {ex}"
    );
    assert!(truncated, "a clipped match-centered window is truncated");
}

#[test]
fn match_excerpt_short_message_is_shown_whole() {
    let text = "a short hit here";
    let m = build_matcher(&args("hit")).unwrap();
    let span = m.locate(text).unwrap();
    let (ex, truncated) = match_excerpt(text, span, EXCERPT_MAX);
    assert_eq!(ex, "a short hit here");
    assert!(!truncated, "a message that fits the cap is not truncated");
}

#[test]
fn match_excerpt_early_match_keeps_the_head() {
    let text = format!("needle {}", "z".repeat(EXCERPT_MAX));
    let m = build_matcher(&args("needle")).unwrap();
    let span = m.locate(&text).unwrap();
    let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
    assert!(!ex.starts_with('…'), "match at char 0 → no leading …: {ex}");
    assert!(ex.starts_with("needle"), "got: {ex}");
    assert!(truncated, "the tail past the window was dropped");
}

#[test]
fn match_excerpt_pure_filter_falls_back_to_head() {
    let text = "X".repeat(EXCERPT_MAX + 50);
    let m = build_matcher(&args("")).unwrap(); // empty pattern = pure filter
    let span = m.locate(&text).expect("pure filter matches");
    assert_eq!(span, None, "pure filter has no locatable span");
    let (ex, truncated) = match_excerpt(&text, span, EXCERPT_MAX);
    assert!(!ex.starts_with('…'), "head form has no leading …");
    assert!(ex.ends_with("… (+50 chars)"), "got: {ex}");
    assert!(truncated, "the head form clipped 50 chars");
}

#[test]
fn match_excerpt_full_budget_emits_whole_message() {
    // `--no-truncate` passes `usize::MAX` as the budget: a message longer than EXCERPT_MAX is
    // emitted whole, with NO truncation marker - whereas the default budget truncates.
    let n = EXCERPT_MAX + 200;
    let text = "🤖".repeat(n);
    let (capped, capped_truncated) = match_excerpt(&text, None, EXCERPT_MAX);
    assert!(
        capped.contains("… (+"),
        "default budget truncates: {capped}"
    );
    assert!(capped_truncated, "default budget reports truncation");
    let (full, full_truncated) = match_excerpt(&text, None, usize::MAX);
    assert!(
        !full.contains("… (+"),
        "full budget has no truncation marker"
    );
    assert!(
        !full_truncated,
        "--no-truncate's usize::MAX budget never truncates — the signal the caution note keys on"
    );
    assert_eq!(full.chars().count(), n, "full text length preserved");
}

#[test]
fn sibling_cap_policy_is_fixed_and_message_classes_uncapped() {
    // Message classes always render (None = uncapped); chattier machinery is capped.
    assert_eq!(sibling_cap(Class::UserMessage), None);
    assert_eq!(sibling_cap(Class::AgentMessage), None);
    assert_eq!(sibling_cap(Class::CommInbox), None);
    assert_eq!(sibling_cap(Class::AgentThinking), Some(2));
    assert_eq!(sibling_cap(Class::AgentToolUse), Some(3));
    assert_eq!(sibling_cap(Class::AgentToolResult), Some(3));
    assert_eq!(sibling_cap(Class::CommandStdout), Some(2));
}
