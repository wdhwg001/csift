//! search exchange headers: stable id-prefix tokens, collisions, parent tokens, the match banner.

use crate::harness::*;

#[test]
fn search_header_tokens_are_stable_across_invocations() {
    // The header token derives from the transcript id (its leading chars), never from
    // enumeration order - two identical invocations emit byte-identical output, so a token
    // pasted from an earlier run still names the same transcript.
    let h = populated_home();
    let a = h.run(&["search", "carry"]);
    let b = h.run(&["search", "carry"]);
    assert!(
        a.success && b.success,
        "stderr: {} / {}",
        a.stderr,
        b.stderr
    );
    assert_eq!(
        a.stdout, b.stdout,
        "byte-identical output across identical invocations"
    );
}

#[test]
fn search_header_token_collision_lengthens_the_group_only() {
    // Two DISTINCT ids sharing their first 8 chars lengthen TOGETHER to their first 12 raw
    // chars (for a uuid that spans the first dash - still a valid `@` target); the
    // non-colliding third id stays at 8. The bare collided 8-prefix never appears as a token.
    let h = Home::new();
    let _ = header_collision_scenario(&h);
    let out = h.run(&["search", "SEEDWORD"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("aaaabbbb-111\u{b7}t"),
        "colliding id 1 lengthens to 12: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("aaaabbbb-222\u{b7}t"),
        "colliding id 2 lengthens to 12: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("ccccdddd\u{b7}t"),
        "non-collider stays at 8: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("aaaabbbb\u{b7}t"),
        "the collided bare 8-prefix must not appear as a token: {}",
        out.stdout
    );
}

#[test]
fn search_lengthened_uuid_token_resolves_and_short_prefix_fails_loud() {
    // A collision-lengthened header token (12 chars, spanning the uuid's first dash) is a
    // valid `@` target; the ambiguous bare 8-prefix fails loud naming the candidates.
    let h = Home::new();
    let (c1, c2, _) = header_collision_scenario(&h);
    let one = h.run(&["search", "SEEDWORD", "@aaaabbbb-111"]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(one.stdout.contains("COLLIDEONE"), "got: {}", one.stdout);
    assert!(
        !one.stdout.contains("COLLIDETWO"),
        "the sibling session must be out of scope: {}",
        one.stdout
    );
    let ambi = h.run(&["search", "SEEDWORD", "@aaaabbbb"]);
    assert!(!ambi.success, "an ambiguous prefix must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS") && ambi.stderr.contains(c1) && ambi.stderr.contains(c2),
        "the error names both candidates: {}",
        ambi.stderr
    );
}

#[test]
fn search_subagent_header_carries_parent_token_on_every_exchange() {
    // EVERY subagent exchange header carries `(parent <first-8-of-owning-uuid>)` - a
    // tail-truncated read must still resolve ownership; top-level headers carry no parent.
    let h = Home::new();
    subagents_only_scenario(&h);
    let out = h.run(&["search", "", at(SESS).as_str()]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("sub111\u{b7}t0 (parent {})", &SESS[..8])),
        "subagent header carries the parent token: {}",
        out.stdout
    );
    for line in out.stdout.lines() {
        if line.starts_with(&format!("{}\u{b7}t", &SESS[..8])) {
            assert!(
                !line.contains("(parent"),
                "a top-level header must not carry a parent: {line}"
            );
        }
    }
}

#[test]
fn search_and_show_resolve_an_agent_id_prefix_token() {
    // An 8-char prefix of a subagent's bare-hex id - the exact header token `search`
    // emits - resolves as an `@` target on every target-taking surface.
    let h = Home::new();
    let (_, _, gamma) = agent_prefix_scenario(&h);
    let out = h.run(&["search", "AGENTGAMMA", &format!("@{}", &gamma[..8])]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("AGENTGAMMA"), "got: {}", out.stdout);
    let s = h.run(&["show", &format!("@{}", &gamma[..8]), "--line", "1"]);
    assert!(s.success, "show by agent prefix: {}", s.stderr);
    assert!(s.stdout.contains("AGENTGAMMA"), "got: {}", s.stdout);
}

#[test]
fn search_agent_twelve_hex_token_falls_back_to_unique_prefix() {
    // Two agents sharing their first 8 hex lengthen their header tokens to 12; a 12-hex
    // token routes as an exact agent id, misses, and falls back to a UNIQUE literal-prefix
    // match. The ambiguous 8-hex form fails loud naming both ids.
    let h = Home::new();
    let (alpha, beta, _) = agent_prefix_scenario(&h);
    let out = h.run(&["search", "seed"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}\u{b7}t", &alpha[..12]))
            && out.stdout.contains(&format!("{}\u{b7}t", &beta[..12])),
        "colliding agent tokens lengthen to 12: {}",
        out.stdout
    );
    let one = h.run(&["search", "AGENT", &format!("@{}", &alpha[..12])]);
    assert!(one.success, "stderr: {}", one.stderr);
    assert!(one.stdout.contains("AGENTALPHA"), "got: {}", one.stdout);
    assert!(
        !one.stdout.contains("AGENTBETA"),
        "the sibling agent must be out of scope: {}",
        one.stdout
    );
    let ambi = h.run(&["search", "AGENT", &format!("@{}", &alpha[..8])]);
    assert!(!ambi.success, "an ambiguous agent prefix must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS")
            && ambi.stderr.contains(alpha)
            && ambi.stderr.contains(beta),
        "the error names both agent ids: {}",
        ambi.stderr
    );
}

#[test]
fn search_match_banner_at_head_mirrors_footer_and_json() {
    // The head banner carries the TRUE totals + direction before the first exchange; the
    // footer repeats the same numbers; the JSON summary's post-cap `matched` +
    // `dropped_by_cap` reconcile to the banner total.
    let h = Home::new();
    let _ = header_collision_scenario(&h);
    let out = h.run(&["search", "SEEDWORD"]);
    assert!(out.success, "stderr: {}", out.stderr);
    // No subagents in scope → the scope banner is suppressed, so the banner is line 1.
    assert_eq!(
        out.stdout.lines().next(),
        Some("matches  3 exchanges · 3 sessions · oldest first"),
        "head banner: {}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("matched 3 exchanges · 3 sessions · label=all"),
        "footer repeats the totals: {}",
        out.stdout
    );
    // Clean-corpus duals for the footer's `> 0` note gates: no malformed note, no
    // sidecar note, no zero-count drop note in the plain text mode.
    assert!(
        !out.stdout.contains("malformed") && !out.stdout.contains("sidecar"),
        "no zero-count footer notes on a clean run: {}",
        out.stdout
    );
    let js = h.run(&["search", "SEEDWORD", "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    assert!(
        !js.stdout.contains("matches  "),
        "no banner in JSON mode: {}",
        js.stdout
    );
    let last: serde_json::Value =
        serde_json::from_str(js.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(last["matched"], serde_json::json!(3));
    assert_eq!(last["sessions"], serde_json::json!(3));

    // Capped: the banner keeps the TRUE total and discloses the window; JSON reconciles.
    let cap = h.run(&["search", "SEEDWORD", "--max-count", "2"]);
    assert!(cap.success, "stderr: {}", cap.stderr);
    assert_eq!(
        cap.stdout.lines().next(),
        Some("matches  3 exchanges · 3 sessions · oldest first · showing earliest 2"),
        "capped head banner: {}",
        cap.stdout
    );
    assert!(
        cap.stdout.contains("matched 3 exchanges · 3 sessions")
            && cap.stdout.contains("1 later dropped by --max-count"),
        "capped footer: {}",
        cap.stdout
    );
    let capjs = h.run(&["search", "SEEDWORD", "--max-count", "2", "--format", "json"]);
    let last: serde_json::Value =
        serde_json::from_str(capjs.stdout.lines().next_back().unwrap()).unwrap();
    assert_eq!(
        last["matched"].as_u64().unwrap() + last["dropped_by_cap"].as_u64().unwrap(),
        3,
        "JSON post-cap matched + dropped reconcile to the banner total"
    );

    // Single-purpose modes carry no banner.
    let c = h.run(&["search", "SEEDWORD", "-c"]);
    assert!(
        !c.stdout.contains("matches  "),
        "no banner under -c: {}",
        c.stdout
    );
    let l = h.run(&["search", "SEEDWORD", "-l"]);
    assert!(
        !l.stdout.contains("matches  "),
        "no banner under -l: {}",
        l.stdout
    );
    let cb = h.run(&["search", "SEEDWORD", "--count-by", "label"]);
    assert!(
        !cb.stdout.contains("matches  "),
        "no banner under --count-by: {}",
        cb.stdout
    );
}
