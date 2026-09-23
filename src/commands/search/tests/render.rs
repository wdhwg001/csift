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
        let g = role_glyph(Some(c));
        assert!(matches!(g, '◂' | '▸' | '⚙'), "{} -> {g}", c.path());
    }
    assert_eq!(role_glyph(Some(Class::UserMessage)), '◂');
    assert_eq!(role_glyph(Some(Class::AgentMessagePlain)), '▸');
    assert_eq!(role_glyph(Some(Class::CommInbox)), '▸'); // comm is agent-side
    assert_eq!(role_glyph(Some(Class::NotificationWorkflow)), '⚙');
    // An addressed record with no modeled leaf takes its own marker (the refetch law).
    assert_eq!(role_glyph(None), '?');
}

#[test]
fn render_label_decorates_pairing_and_direction() {
    // ▹ pairing: a paired tool.use renders the two-sided form; a pending use / orphan result
    // render their notes. ⇨ direction: a comm hit appends `from ⇨ to`.
    let paired = Hit {
        class: Some(Class::AgentToolUse),
        labels: vec!["agent.tool.use"],
        excerpt: String::new(),
        body: None,
        timestamp_utc: None,
        tool_name: None,
        model: None,
        attachment_type: None,
        version: None,
        is_error: None,
        denial_kind: None,
        direction: None,
        tool_use_id: Some("t1".into()),
        pair: Some(Pairing::Paired),
        line: 0,
        uuid: None,
        raw: None,
        image_ids: Vec::new(),
        from_sidecar: false,
        queue_operation: None,
        queue_reason: None,
        task_ids: Vec::new(),
        orphan_kind: None,
        delivery: None,
        resume_paired: None,
        scheduled_at: None,
        survival: crate::model::Survival::Live,
        rewound_branch: false,
        replay_copy_of: None,
        compaction: None,
        truncated: false,
        for_root: crate::model::RootLink::default(),
        markers: Vec::new(),
        facts: Vec::new(),
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
        class: Some(Class::AgentToolResult),
        pair: Some(Pairing::OrphanResult),
        ..paired.clone()
    };
    assert_eq!(
        render_label(&orphan),
        "agent.tool.result (use not in scope)"
    );
    let comm = Hit {
        class: Some(Class::CommInbox),
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
fn uncapped_body_is_keyed_on_the_budget_and_keeps_newlines() {
    // The `body` companion of `match_excerpt`: present ONLY at the uncapped budget, and
    // then the text VERBATIM - the excerpt's whitespace collapse is not applied.
    let text = "first line\n\nthird line\twith a tab";
    assert_eq!(uncapped_body(text, usize::MAX).as_deref(), Some(text));
    assert_eq!(
        uncapped_body(text, usize::MAX)
            .expect("body")
            .matches('\n')
            .count(),
        2,
        "both newlines survive - a body is not one line"
    );
    // Any real budget yields nothing, including a budget wide enough for this text: the
    // pairing is with the MODE (`--no-truncate` / an address), never with whether this
    // particular record happened to fit.
    assert_eq!(uncapped_body(text, EXCERPT_MAX), None);
    assert_eq!(uncapped_body(text, usize::MAX - 1), None);
    assert_eq!(uncapped_body("", usize::MAX).as_deref(), Some(""));
}

#[test]
fn a_channel_delivery_renders_verbatim_as_a_message() {
    // The record-text view of `agent.communication.channel` is the injected string
    // VERBATIM (header line + body): the header is the receipt a reader audits, and a
    // verbatim byte substring is what keeps the 7d/7f prefilter laws sound.
    let rec: Record = serde_json::from_str(
        r#"{"type":"attachment","uuid":"att1","attachment":{"type":"hook_additional_context","content":["[csift-channel v1 id=abc part=1/1 mode=steer from=aRelay-0123456789abcdef]\nthrottlebeacon the region queue","a second block"]}}"#,
    )
    .unwrap();
    let labels = rec.classify(&crate::model::ClassifyCtx::top_level());
    let (class, text) = record_text_emission(
        &rec,
        None,
        &labels,
        LabelFilter::all(),
        &PlanIndex::default(),
        &ClassifyCtx::top_level(),
    )
    .expect("a channel delivery emits");
    assert_eq!(class, Class::CommChannel, "the message view is the richest");
    assert_eq!(
        text,
        "[csift-channel v1 id=abc part=1/1 mode=steer from=aRelay-0123456789abcdef]\nthrottlebeacon the region queue"
    );
    // Under a hook-only selector the SAME record renders the harness view instead: the
    // joined content, exactly as every other hook context renders.
    let include = vec!["harness.hook.context".to_string()];
    let hook_only = LabelFilter::new(&include, &[]);
    let (class, text) = record_text_emission(
        &rec,
        None,
        &labels,
        hook_only,
        &PlanIndex::default(),
        &ClassifyCtx::top_level(),
    )
    .expect("hook view");
    assert_eq!(class, Class::HookContext);
    assert!(
        text.ends_with("\na second block"),
        "the hook view is the joined content: {text:?}"
    );
    // A delivery renders as a message: the agent glyph, the comm direction, and no
    // sibling cap (harness leaves are capped at 2 per turn; a message never is).
    assert_eq!(role_glyph(Some(Class::CommChannel)), '▸');
    assert_eq!(sibling_cap(Class::CommChannel), None);
    let hit = Hit {
        class: Some(Class::CommChannel),
        labels: vec!["agent.communication.channel", "harness.hook.context"],
        excerpt: String::new(),
        body: None,
        timestamp_utc: None,
        tool_name: None,
        model: None,
        attachment_type: Some("hook_additional_context".into()),
        version: None,
        is_error: None,
        denial_kind: None,
        direction: Some(("aRelay-0123456789abcdef".into(), "self".into())),
        tool_use_id: None,
        pair: None,
        line: 2,
        uuid: None,
        raw: None,
        image_ids: Vec::new(),
        from_sidecar: false,
        queue_operation: None,
        queue_reason: None,
        task_ids: Vec::new(),
        orphan_kind: None,
        delivery: None,
        resume_paired: None,
        scheduled_at: None,
        survival: crate::model::Survival::Live,
        rewound_branch: false,
        replay_copy_of: None,
        compaction: None,
        truncated: false,
        for_root: crate::model::RootLink::default(),
        markers: Vec::new(),
        facts: Vec::new(),
    };
    assert_eq!(
        render_label(&hit),
        "agent.communication.channel  aRelay-0123456789abcdef ⇨ self"
    );
}

#[test]
fn sibling_cap_policy_is_fixed_and_message_classes_uncapped() {
    // Message classes always render (None = uncapped); chattier machinery is capped.
    assert_eq!(sibling_cap(Class::UserMessage), None);
    assert_eq!(sibling_cap(Class::AgentMessagePlain), None);
    assert_eq!(sibling_cap(Class::CommInbox), None);
    assert_eq!(sibling_cap(Class::AgentThinkingSummary), Some(2));
    assert_eq!(sibling_cap(Class::AgentThinkingNarration), Some(1));
    assert_eq!(sibling_cap(Class::UserUnsent), None);
    assert_eq!(sibling_cap(Class::AgentToolUse), Some(3));
    assert_eq!(sibling_cap(Class::AgentToolResult), Some(3));
    assert_eq!(sibling_cap(Class::CommandStdout), Some(2));
}

/// A compaction summary written by a `/rewind` summarize names its direction in the
/// label zone (display-only, like `[narration summary]`); an ordinary compaction summary
/// carries no direction, so it renders the bare leaf path.
#[test]
fn a_summarize_summary_names_its_direction_in_the_label_zone() {
    let mk = |compaction: Option<Box<CompactionHit>>| Hit {
        class: Some(Class::CompactionSummary),
        labels: vec!["harness.compaction.summary"],
        excerpt: String::new(),
        body: None,
        timestamp_utc: None,
        tool_name: None,
        model: None,
        attachment_type: None,
        version: None,
        is_error: None,
        denial_kind: None,
        direction: None,
        tool_use_id: None,
        pair: None,
        line: 9,
        uuid: None,
        raw: None,
        image_ids: Vec::new(),
        from_sidecar: false,
        queue_operation: None,
        queue_reason: None,
        task_ids: Vec::new(),
        orphan_kind: None,
        delivery: None,
        resume_paired: None,
        scheduled_at: None,
        survival: crate::model::Survival::Live,
        rewound_branch: false,
        replay_copy_of: None,
        compaction,
        truncated: false,
        for_root: crate::model::RootLink::default(),
        markers: Vec::new(),
        facts: Vec::new(),
    };
    let tagged = mk(Some(Box::new(CompactionHit {
        metadata: None,
        mode: Some(crate::model::SummarizeMode::UpToHere),
        direction: Some("up_to".into()),
    })));
    assert_eq!(
        render_label(&tagged),
        "harness.compaction.summary [summarize up_to]"
    );
    // An ordinary compaction: a mode, but no direction to name.
    let plain = mk(Some(Box::new(CompactionHit {
        metadata: None,
        mode: Some(crate::model::SummarizeMode::Compact),
        direction: None,
    })));
    assert_eq!(render_label(&plain), "harness.compaction.summary");
    assert_eq!(render_label(&mk(None)), "harness.compaction.summary");
    // An unmodeled direction still renders its own verbatim word rather than nothing.
    let odd = mk(Some(Box::new(CompactionHit {
        metadata: None,
        mode: None,
        direction: Some("sideways".into()),
    })));
    assert_eq!(
        render_label(&odd),
        "harness.compaction.summary [summarize sideways]"
    );
}

/// PERF: the boundary -> mode pairing is built only when the active selection can
/// surface a compaction leaf, so a `-t user`/`-t agent.*` scan never walks the records for a
/// feature it cannot show. The gate is a named predicate precisely so this is pinned.
#[test]
fn the_compaction_pairing_is_skipped_when_no_compaction_leaf_is_selectable() {
    use crate::cli::LabelFilter;
    let none: Vec<String> = Vec::new();
    // No `-t` at all: every leaf is selectable, so the pairing is built.
    assert!(wants_compaction_pairing(&LabelFilter::all()));
    assert!(wants_compaction_pairing(&LabelFilter::new(&none, &none)));
    for reaching in [
        "harness",
        "harness.compaction",
        "harness.compaction.boundary",
        "harness.compaction.summary",
    ] {
        let inc = vec![reaching.to_string()];
        assert!(
            wants_compaction_pairing(&LabelFilter::new(&inc, &none)),
            "{reaching} reaches a compaction leaf"
        );
    }
    for missing in [
        "user",
        "user.unsent",
        "agent",
        "agent.tool",
        "harness.bookkeeping",
    ] {
        let inc = vec![missing.to_string()];
        assert!(
            !wants_compaction_pairing(&LabelFilter::new(&inc, &none)),
            "{missing} reaches neither compaction leaf, so the index must be skipped"
        );
    }
    // `-T` takes the leaves back out: an excluded pair is also a skipped index.
    let inc = vec!["harness".to_string()];
    let exc = vec!["harness.compaction".to_string()];
    assert!(!wants_compaction_pairing(&LabelFilter::new(&inc, &exc)));
}

/// The two EXTENSION POINTS a family adds to, pinned at their contract rather than
/// through a surface: a `Marker` renders `[head]` or `[head detail]`, and a `HitFact`
/// projects itself into the label zone and into JSON. A family that adds a variant gets
/// both surfaces for free only while these two projections stay the one place it is
/// decided.
#[test]
fn a_marker_and_a_fact_each_project_themselves() {
    assert_eq!(Marker::plain("not delivered").render(), "[not delivered]");
    let detailed = Marker {
        head: "scheduled fire",
        detail: Some("2026-09-15T10:00:00Z".to_string()),
    };
    assert_eq!(detailed.render(), "[scheduled fire 2026-09-15T10:00:00Z]");

    let fact = HitFact::Unmodeled("system/progress".to_string());
    // The shape IS the label for an unmodeled unit, so it adds no second label-zone tag.
    assert!(fact.label_zone().is_none());
    assert_eq!(
        fact.json(),
        vec![("unmodeled", serde_json::json!("system/progress"))]
    );
}

/// The chain footer's gate is an OR over four independent facts, and each one is reason
/// enough on its own: read as an AND, a transcript would disclose nothing unless all four
/// were true at once. Pinned on the predicate itself rather than through a rendered footer,
/// because the footer's other lines would keep printing and hide the loss.
#[test]
fn the_chain_disclosure_gate_answers_to_each_of_its_four_facts_alone() {
    use crate::model::{CrossingBudget, CrossingReport};

    let base = ChainCounts::default();
    assert!(!base.any(), "an ordinary transcript discloses nothing");

    let mut abandoned = base.clone();
    abandoned.abandoned_records = 1;
    assert!(abandoned.any(), "records off the conversation");

    let mut replays = base.clone();
    replays.replay_copies = 1;
    assert!(replays.any(), "a compaction re-anchor's copies");

    let mut cut = base.clone();
    cut.boundary_cut_line = Some(42);
    assert!(cut.any(), "a compaction cut");

    let mut crossed = base.clone();
    crossed.crossing = CrossingReport {
        budget: CrossingBudget::Limited(1),
        boundaries_crossed: 1,
        ..CrossingReport::default()
    };
    assert!(crossed.any(), "a boundary the walk stepped over");
    assert!(
        !base.crossing.any(),
        "and the default report says nothing, which is what makes the fourth fact a fact"
    );
}
