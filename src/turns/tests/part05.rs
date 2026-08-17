use super::*;

#[test]
fn agent_msg_is_rich_rejects_a_short_signalless_declaration() {
    let c = rich_cfg();
    // A short, signal-less intent-verb opener is NOT rich.
    assert!(!agent_msg_is_rich("let me read the file", &c));
    assert!(!agent_msg_is_rich("now i will look into this", &c));
    // A plain decimal (no a–f) is NOT a commit hash, and "1" alone has no substance noun.
    assert!(!agent_msg_is_rich("step 1 next", &c));
}

#[test]
fn agent_msg_is_rich_is_codepoint_safe_for_multibyte_with_a_digit() {
    // REGRESSION: a digit adjacent to multi-byte text used to panic — the ±16-byte
    // number-of-substance window sliced mid-codepoint. The window bounds must snap to a
    // char boundary; this must NOT panic, for a 2-digit number AND a single digit.
    let c = rich_cfg();
    // A multi-byte line with a time/number right next to 4-byte chars (no substance noun
    // → not rich, but the point is it must not panic).
    let _ = agent_msg_is_rich("🤖 07:40 watching 🚀, 9 left to go", &c);
    let _ = agent_msg_is_rich("🤖 confirmed 42 times, root cause at src/x.rs:9", &c);
    let _ = agent_msg_is_rich("🤖 step 7 done", &c);
    // A multi-byte phrase with a number that DOES carry a finding lexeme is rich.
    assert!(agent_msg_is_rich("🤖 root cause confirmed at line 42", &c));
    // The droppable predicate is codepoint-safe too (it calls agent_msg_is_rich first).
    let _ = agent_msg_is_droppable("🤖 looking at the 07:40 log", &c);
}

#[test]
fn agent_msg_is_droppable_and_keep_on_doubt() {
    let c = rich_cfg();
    // Droppable: short + intent-verb opener + no signal.
    assert!(agent_msg_is_droppable("let me read the file", &c));
    assert!(agent_msg_is_droppable("now i will open this file", &c));
    // NOT droppable — rich wins even with an intent-verb opener (fusion case).
    assert!(!agent_msg_is_droppable(
        "let me note: root cause confirmed in src/x.rs:42",
        &c
    ));
    // KEEP-ON-DOUBT: a sub-280 signal-less body WITHOUT an intent-verb opener is KEPT
    // (neither rich nor droppable → falls through → kept).
    assert!(!agent_msg_is_rich(
        "the boundary handling here is subtle",
        &c
    ));
    assert!(!agent_msg_is_droppable(
        "the boundary handling here is subtle",
        &c
    ));
    // A signal-less intent-verb opener AT/ABOVE the declaration length is NOT droppable.
    let long_decl = format!("let me {}", "x".repeat(210));
    assert!(!agent_msg_is_droppable(&long_decl, &c));
}

#[test]
fn select_eot_only_keeps_only_the_last_agent_message() {
    // The non-breaking default: a multi-agent turn collapses to its last message, no
    // placeholder, byte-identical selection to the pre-expansion single EOT.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &["let me look", "still working", "the final answer"],
        0,
    );
    let lane = select_agent_messages(&t, &cfg());
    assert_eq!(lane.len(), 1);
    match &lane[0] {
        AgentRender::Kept(a) => assert_eq!(a.unit.text, "the final answer"),
        _ => panic!("expected the last message kept"),
    }
}

/// A deterministic body of exactly `n` ASCII chars whose tail token is unique (`TAGn`), so
/// a test can assert WHICH message survived by substring without coupling to the filler.
fn body_chars(tag: &str, n: usize) -> String {
    let suffix = format!(" {tag}");
    let fill = n.saturating_sub(suffix.chars().count());
    let mut s = "x".repeat(fill);
    s.push_str(&suffix);
    s
}

// ── Default `Longest` mode (the user-specified new default) ──

#[test]
fn longest_default_keeps_the_longest_not_the_last() {
    // THE HEADLINE CASE. A turn = [a long substantive Rich Response, a ~50-char throwaway
    // wrap-up]. The OLD default kept `agents.last()` → the wrap-up, silently dropping the
    // substance. The NEW default keeps the LONGEST → the Rich Response; the short non-rich
    // wrap-up collapses into a placeholder. The Rich Response is the FIRST message here, so
    // this proves the default is "longest", NOT "last" and NOT merely "first".
    let rich_response = body_chars("RICHRESP", 600); // longest, > rich_min_chars
    let wrap_up = "Done — let me know if you'd like anything else."; // ~48 chars, not rich
    assert!(wrap_up.chars().count() < 60);
    let t = mk_turn_agents(0, Some("ask"), &[&rich_response, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "only the longest survives: {lane:?}");
    assert!(
        kept[0].contains("RICHRESP"),
        "the LONGEST (Rich Response) is kept, not the wrap-up: {kept:?}"
    );
    // The wrap-up collapsed into exactly one placeholder.
    let phs = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(phs, 1, "the throwaway wrap-up collapses into a placeholder");
}

#[test]
fn longest_default_longest_is_a_middle_message() {
    // The substantive message is a MIDDLE (the realistic shape: a short opener, the big
    // Rich Response in the middle, a short wrap-up). Default keeps the middle longest;
    // the short non-rich opener + wrap-up both collapse.
    let opener = "Let me look into this."; // short, not substantive, not rich
    let middle = body_chars("MIDRESP", 700); // the longest → kept
    let wrap_up = "All set."; // tiny → collapse
    let t = mk_turn_agents(0, Some("ask"), &[opener, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "only the middle longest survives: {lane:?}");
    assert!(kept[0].contains("MIDRESP"), "the middle Rich Response wins");
}

#[test]
fn longest_default_also_keeps_a_substantive_first() {
    // The FIRST is ALSO kept when substantive (>= rich_min_chars), even though it is not
    // the longest. Here: a long-but-not-longest first (states the plan), the longest in the
    // middle, a short wrap-up. Kept = {first, longest middle}; the wrap-up collapses.
    let first = body_chars("PLANFIRST", 400); // substantive (>= 280) but < the longest
    let middle = body_chars("BIGMID", 900); // the longest
    let wrap_up = "ok done"; // tiny → collapse
    let t = mk_turn_agents(0, Some("ask"), &[&first, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        kept.iter().any(|k| k.contains("PLANFIRST")),
        "a substantive first is ALSO kept: {kept:?}"
    );
    assert!(
        kept.iter().any(|k| k.contains("BIGMID")),
        "the longest is always kept: {kept:?}"
    );
    assert_eq!(kept.len(), 2, "exactly first + longest: {lane:?}");
}

#[test]
fn longest_default_drops_a_non_substantive_first() {
    // A SHORT first (below rich_min_chars, not rich) is NOT kept by position — only the
    // longest survives. Distinguishes `Longest` from `Rich`'s unconditional keep-first.
    let first = "let me start"; // short + not rich → not kept
    let middle = body_chars("ONLYLONG", 600); // the longest
    let wrap_up = "fin"; // tiny
    let t = mk_turn_agents(0, Some("ask"), &[first, &middle, wrap_up], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kept.len(), 1, "non-substantive first is dropped: {lane:?}");
    assert!(kept[0].contains("ONLYLONG"));
}

#[test]
fn longest_default_keeps_a_rich_middle_with_a_major_finding() {
    // A MIDDLE that is RICH by SIGNAL (not length) — a file:line + ratio finding — is kept
    // even though it is not the longest, because major findings can live mid-run. Here the
    // longest is the final answer; the rich middle ALSO survives.
    let opener = "starting now"; // short → collapse
    let finding = "12 passed 3 failed in src/x.rs:9"; // rich by signal, not longest
    let longest = body_chars("FINALANS", 500); // the longest → kept (last)
    let t = mk_turn_agents(0, Some("ask"), &[opener, finding, &longest], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        kept.iter().any(|k| k.contains("src/x.rs:9")),
        "the rich middle finding survives: {kept:?}"
    );
    assert!(
        kept.iter().any(|k| k.contains("FINALANS")),
        "the longest survives: {kept:?}"
    );
}

#[test]
fn longest_default_single_message_turn_keeps_it() {
    // A 1-message turn keeps its sole message regardless of richness (it is the longest).
    let t = mk_turn_agents(0, Some("ask"), &["let me look into this"], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    assert_eq!(lane.len(), 1);
    assert!(matches!(lane[0], AgentRender::Kept(_)));
}

#[test]
fn longest_default_tie_breaks_to_the_last_maximum() {
    // All messages equal length → `max_by_key` returns the LAST maximum, so the default
    // coincides with the old `agents.last()` pick on an all-equal run (documented tie rule).
    // None are rich/substantive, so ONLY the tie-winning last survives.
    let a = "alpha beta gamma"; // 16 chars
    let b = "delta epsilon ze"; // 16 chars
    let c = "eta theta iota k"; // 16 chars
    assert_eq!(a.chars().count(), b.chars().count());
    assert_eq!(b.chars().count(), c.chars().count());
    let t = mk_turn_agents(0, Some("ask"), &[a, b, c], 0);
    let lane = select_agent_messages(&t, &longest_cfg());
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kept,
        vec![c],
        "tie → the LAST maximum (== old agents.last())"
    );
}

#[test]
fn longest_default_collapses_contiguous_runs_into_separate_placeholders() {
    // The placeholder fusing is shared with Rich: two contiguous dropped runs split by a
    // surviving rich middle → TWO placeholders. Longest = a long final answer; a rich
    // middle survives between two declaration runs.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "let me a",                     // drop
            "let me b",                     // drop
            "found 12 cases in src/z.rs:3", // rich middle → kept
            "let me c",                     // drop
            "let me d",                     // drop
            &body_chars("THEANSWER", 400),  // longest (last) → kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &longest_cfg());
    let phs = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(
        phs, 2,
        "two declaration runs split by the rich middle: {lane:?}"
    );
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert!(kept.iter().any(|k| k.contains("src/z.rs:3")));
    assert!(kept.iter().any(|k| k.contains("THEANSWER")));
}

#[test]
fn longest_default_keep_flag_tunes_the_substantive_first_gate() {
    // `--agent-rich-min-chars` (rich_min_chars) is the tuning knob: a first of 300 chars is
    // substantive at the default 280 (kept) but NOT at a raised 500 (dropped). Same turn,
    // two configs → the flag changes the survivor set.
    let first = body_chars("TUNEFIRST", 300);
    let longest = body_chars("TUNELONG", 800);
    let wrap_up = "bye";
    let t = mk_turn_agents(0, Some("ask"), &[&first, &longest, wrap_up], 0);

    let default_kept: Vec<String> = select_agent_messages(&t, &longest_cfg())
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        default_kept.iter().any(|k| k.contains("TUNEFIRST")),
        "300-char first IS substantive at the default 280 gate"
    );

    let raised = RichnessCfg {
        rich_min_chars: 500,
        ..longest_cfg()
    };
    let raised_kept: Vec<String> = select_agent_messages(&t, &raised)
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !raised_kept.iter().any(|k| k.contains("TUNEFIRST")),
        "the same 300-char first is NOT substantive once the gate is raised to 500"
    );
    assert!(
        raised_kept.iter().any(|k| k.contains("TUNELONG")),
        "the longest still survives regardless of the gate"
    );
}

#[test]
fn select_one_message_turn_keeps_it_as_last() {
    // A 1-agent-message turn's sole message is BOTH first and last → always kept, no
    // richness eval, even a declaration-shaped one.
    let t = mk_turn_agents(0, Some("ask"), &["let me look into this"], 0);
    let lane = select_agent_messages(&t, &rich_cfg());
    assert_eq!(lane.len(), 1);
    assert!(matches!(lane[0], AgentRender::Kept(_)));
}

#[test]
fn select_short_run_keeps_all_under_threshold() {
    // A run at or below the run threshold (6) keeps every message verbatim (no filtering).
    let texts: Vec<String> = (0..6).map(|i| format!("let me step {i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let t = mk_turn_agents(0, Some("ask"), &refs, 0);
    let lane = select_agent_messages(&t, &rich_cfg());
    assert_eq!(lane.len(), 6, "6 <= threshold 6 → keep all");
    assert!(lane.iter().all(|r| matches!(r, AgentRender::Kept(_))));
}

#[test]
fn select_rich_first_kept_and_sudden_rich_middle_survives() {
    // A >6 run: a rich first survives, a sudden rich middle survives whole, the pure
    // declarations around it collapse into placeholders split BY the survivor.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "found the root cause already",     // first — rich → kept
            "let me try the next thing",        // middle decl → collapse
            "now i will check another",         // middle decl → collapse
            "12 passed 3 failed in src/x.rs:9", // sudden rich middle → kept
            "let me write it up",               // middle decl → collapse
            "next i continue here",             // middle decl → collapse
            "now let me finalize",              // middle decl → collapse
            "the final committed answer",       // last → always kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &rich_cfg());
    // Kept: first, the sudden-rich middle, the last → 3 kept; the two declaration runs →
    // 2 placeholders (split by the survivor).
    let kept: Vec<&str> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Kept(a) => Some(a.unit.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        kept,
        vec![
            "found the root cause already",
            "12 passed 3 failed in src/x.rs:9",
            "the final committed answer",
        ]
    );
    let placeholders = lane
        .iter()
        .filter(|r| matches!(r, AgentRender::Placeholder(_)))
        .count();
    assert_eq!(
        placeholders, 2,
        "two contiguous declaration runs → two placeholders"
    );
}

#[test]
fn select_all_middles_droppable_makes_one_placeholder() {
    // Every middle is a signal-less short declaration → ONE placeholder spanning them all,
    // between the kept first(or its collapse) and the kept last.
    let t = mk_turn_agents(
        0,
        Some("ask"),
        &[
            "the opening plan is to refactor", // first: not droppable (no intent verb) → kept
            "let me a",
            "let me b",
            "let me c",
            "let me d",
            "let me e",
            "let me f",
            "the final answer here", // last → kept
        ],
        0,
    );
    let lane = select_agent_messages(&t, &rich_cfg());
    let spans: Vec<&PlaceholderSpan> = lane
        .iter()
        .filter_map(|r| match r {
            AgentRender::Placeholder(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        spans.len(),
        1,
        "one contiguous dropped run → one placeholder"
    );
    assert_eq!(spans[0].messages, 6, "the six middle declarations collapse");
}
