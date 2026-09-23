//! The `-t` / `-T` LABEL SELECTOR surface: the three forms, the visibility law and its
//! one exception, the empty visible face, and the retired spellings.
//!
//! Split out of `command_args.rs` when that file reached the 600-line cap. The cut is by
//! SUBJECT rather than by size: these cases exercise `cli::selectors`, while what stays
//! behind exercises one command's flags at a time. No case changed in the move.

#[allow(unused_imports)]
use super::*;

#[test]
fn parse_search_rejects_old_flat_category() {
    // No back-compat (SPEC section 5): the flat `thinking`/`tool`/`tool-response` spellings
    // HARD-error.
    assert!(parse(&["csift", "search", "x", "-t", "thinking"]).is_err());
    assert!(parse(&["csift", "search", "x", "-t", "tool-response"]).is_err());
    // …while a dotted selector + a bare role are accepted.
    assert!(parse(&["csift", "search", "x", "-t", "agent.thinking"]).is_ok());
    assert!(parse(&["csift", "search", "x", "-t", "agent"]).is_ok());
    assert!(parse(&["csift", "search", "x", "-t", "agent.tool"]).is_ok());
    assert!(parse(&["csift", "search", "x", "-t", "harness.notification"]).is_ok());
}

#[test]
fn category_selector_prefix_and_validity() {
    // The segment-prefix rule (SPEC section 5): a partial trailing segment never leaks.
    assert!(selector_is_segment_prefix("agent", "agent.tool.use"));
    assert!(selector_is_segment_prefix(
        "agent.tool",
        "agent.tool.result"
    ));
    assert!(selector_is_segment_prefix(
        "agent.tool.use",
        "agent.tool.use"
    ));
    assert!(!selector_is_segment_prefix("agent.too", "agent.tool.use"));
    assert!(!selector_is_segment_prefix("user", "agent.message"));
    // Validity is derived from Class::ALL: every emitted selector is valid; junk is not.
    // Mutation pin: the loop below is VACUOUS over an empty selector space, so the
    // space's size and membership are asserted first (an empty `label_selectors()`
    // must never pass by silence).
    let sels = label_selectors();
    assert!(sels.len() > 25, "selector space too small: {}", sels.len());
    for want in [
        "user",
        "agent",
        "harness",
        "agent.tool",
        "agent.tool.use",
        "agent.communication",
        "harness.notification.monitor",
        "user.answer",
    ] {
        assert!(sels.contains(&want.to_string()), "missing selector {want}");
    }
    let set: std::collections::BTreeSet<&String> = sels.iter().collect();
    assert_eq!(set.len(), sels.len(), "selectors must be de-duplicated");
    for s in &sels {
        assert!(selector_is_valid(s), "{s} must be valid");
    }
    assert!(selector_is_valid("user"));
    assert!(selector_is_valid("agent.communication.inbox"));
    assert!(!selector_is_valid("thinking")); // a flat spelling, not a selector
    assert!(!selector_is_valid("bogus.path"));
    // label_selected: empty ⇒ all; otherwise prefix-gated.
    assert!(label_selected(&[], "harness.interrupt.user", None));
    assert!(label_selected(
        &["agent".to_string()],
        "agent.tool.use",
        None
    ));
    assert!(!label_selected(
        &["user".to_string()],
        "agent.communication.inbox",
        None
    ));
}

/// `harness.bookkeeping` is the one CATEGORY whose visible face is empty: the bare form
/// is valid, matches nothing by the law, and the caller says so rather than letting a
/// reader take it for a miss.
#[test]
fn a_category_with_no_visible_face_is_named_not_refused() {
    assert_eq!(
        empty_visible_faces(&["harness.bookkeeping".to_string()]),
        vec!["harness.bookkeeping".to_string()]
    );
    // A category with a visible face, a bare role, and an exact leaf all report nothing.
    assert!(empty_visible_faces(&["harness".to_string()]).is_empty());
    assert!(empty_visible_faces(&["harness.turn.duration".to_string()]).is_empty());
    assert!(empty_visible_faces(&["harness.bookkeeping.*".to_string()]).is_empty());
    // And it is NOT a contradiction: only `-T` cancelling `-t` is.
    let include = vec!["harness.bookkeeping".to_string()];
    assert!(!LabelFilter::new(&include, &[]).is_statically_empty());
    let both = vec!["user".to_string()];
    let glob = vec!["user.*".to_string()];
    assert!(LabelFilter::new(&both, &glob).is_statically_empty());
}

/// The visible-face law's ONE stated exception, and the fact it does not touch.
///
/// A `structured_output` payload is in `inventory::NEVER_CONVERTS`, so the record-level
/// delivery override reads NOT delivered - which drops every other leaf it reaches out
/// of a bare face. `agent.message.structured` is exempt: the model WROTE those words,
/// and a bare `agent.message` that returned only the plain half would hide a whole leaf
/// of a two-leaf category behind a glob nobody knows to type.
#[test]
fn a_bare_agent_face_keeps_the_structured_answer_the_model_wrote() {
    // The exception, on both faces that reach the leaf.
    for face in ["agent", "agent.message"] {
        assert!(
            selector_matches(face, "agent.message.structured", Some(false)),
            "-t {face} keeps the structured answer despite its delivery verdict"
        );
        assert!(
            selector_matches(face, "agent.message.plain", None),
            "-t {face} still reaches the plain half"
        );
    }
    // It is an exception, not a hole: every OTHER leaf still narrows on the axis.
    assert!(
        !selector_matches("harness", "harness.context.task", Some(false)),
        "the delivery axis still narrows a bare face everywhere else"
    );
    assert!(
        !selector_matches("harness.context", "harness.context.mode", Some(false)),
        "a bare category narrows the same way a bare role does"
    );
    // It does not leak to the leaf beside it: the exception is keyed on the PATH, so
    // the plain half of the same category narrows on the axis like everything else.
    assert!(
        !selector_matches("agent.message", "agent.message.plain", Some(false)),
        "the exception names one leaf, not the category"
    );
    // The exempt leaf is exempt on the DELIVERY axis only. It is `llm_visible`, which
    // is what carries it through with no record in hand; a leaf that is not would still
    // be dropped by axis one, where this exception does not reach.
    assert!(selector_matches(
        "agent.message",
        "agent.message.structured",
        None
    ));
    assert!(!selector_matches(
        "agent.thinking",
        "agent.thinking.redacted",
        None
    ));
}

#[test]
fn selector_three_forms_and_visibility() {
    // Bare ROLE: the leaves the model received (no record in hand ⇒ the leaf default).
    assert!(selector_matches("user", "user.message", None));
    assert!(selector_matches("user", "user.answer", None));
    assert!(
        !selector_matches("user", "user.unsent", None),
        "drafts are not the conversation"
    );
    assert!(selector_matches(
        "harness",
        "harness.compaction.summary",
        None
    ));
    assert!(
        !selector_matches("harness", "harness.compaction.boundary", None),
        "a metrics-only system record"
    );
    // GLOB: everything under the prefix, visibility ignored.
    assert!(selector_matches("user.*", "user.unsent", None));
    assert!(selector_matches("user.*", "user.message", None));
    assert!(selector_matches(
        "harness.*",
        "harness.compaction.boundary",
        None
    ));
    assert!(!selector_matches("user.*", "agent.message.plain", None));
    // A bare CATEGORY is a bare FACE, exactly like a bare role: it selects the leaves
    // the model receives and no others.
    assert!(
        !selector_matches("harness.compaction", "harness.compaction.boundary", None),
        "a category selects its visible face"
    );
    assert!(selector_matches(
        "harness.compaction",
        "harness.compaction.summary",
        None
    ));
    assert!(
        selector_matches("harness.compaction.*", "harness.compaction.boundary", None),
        "and the glob form reaches the rest"
    );
    // An exact LEAF is the drill-down: any visibility.
    assert!(selector_matches("user.unsent", "user.unsent", None));
    assert!(selector_matches("agent.tool", "agent.tool.result", None));
    // Validity: globs on valid prefixes parse; degenerate forms error.
    assert!(parse_label_selector("user.*").is_ok());
    assert!(parse_label_selector("harness.compaction.*").is_ok());
    assert!(parse_label_selector(".*").is_err());
    assert!(parse_label_selector("user.**").is_err());
    assert!(parse_label_selector("nope.*").is_err());
    // The -T duality: excluding the bare role leaves the invisible leaf alive.
    let include = vec!["user.*".to_string()];
    let exclude = vec!["user".to_string()];
    let f = LabelFilter::new(&include, &exclude);
    assert!(
        f.selected("user.unsent"),
        "-t 'user.*' -T user = the drafts alone"
    );
    assert!(!f.selected("user.message"));
    // Statically-empty detection speaks glob too.
    let include = vec!["user".to_string()];
    let exclude = vec!["user.*".to_string()];
    let f = LabelFilter::new(&include, &exclude);
    assert!(
        f.is_statically_empty(),
        "-t user -T 'user.*' can never match"
    );
}

/// The bare-role form is RECORD-level: a delivery verdict overrides the leaf default in
/// both directions, and only for that form.
#[test]
fn a_bare_role_selector_follows_the_record_delivery_verdict() {
    // Delivered although the leaf is invisible (a hook RUN record whose conversion
    // produced text - the harness wrote a body onto it).
    assert!(
        !selector_matches("harness", "harness.hook.outcome", None),
        "the leaf default alone hides it"
    );
    assert!(selector_matches(
        "harness",
        "harness.hook.outcome",
        Some(true)
    ));
    // Not delivered although the leaf is visible (an api-error placeholder, an
    // isVirtual record).
    assert!(selector_matches("agent", "agent.message.plain", None));
    assert!(!selector_matches(
        "agent",
        "agent.message.plain",
        Some(false)
    ));
    assert!(!selector_matches("user", "user.message", Some(false)));
    // Every OTHER form keeps its full set, whatever the record says.
    for (sel, path) in [
        ("agent.*", "agent.message.plain"),
        ("agent.message.plain", "agent.message.plain"),
        ("harness.*", "harness.hook.outcome"),
        ("harness.hook.*", "harness.hook.outcome"),
        ("harness.hook.outcome", "harness.hook.outcome"),
    ] {
        assert!(
            selector_matches(sel, path, Some(false)) && selector_matches(sel, path, None),
            "{sel} -> {path} ignores delivery"
        );
    }
    // A bare CATEGORY is NOT one of those forms: it consults the verdict like a role.
    assert!(selector_matches(
        "agent.message",
        "agent.message.plain",
        None
    ));
    assert!(!selector_matches(
        "agent.message",
        "agent.message.plain",
        Some(false)
    ));
    // `-T` speaks the same rule (LabelFilter feeds one verdict to both sides).
    let include: Vec<String> = Vec::new();
    let exclude = vec!["harness".to_string()];
    let f = LabelFilter::new(&include, &exclude);
    assert!(
        f.selected("harness.hook.outcome"),
        "-T harness excludes only what the model received"
    );
    assert!(!f.with_delivery(Some(true)).selected("harness.hook.outcome"));
    // The include side, with no -t at all, is unaffected: every label is eligible.
    let f = LabelFilter::new(&include, &[]);
    assert!(f.with_delivery(Some(false)).selected("agent.message"));
}
