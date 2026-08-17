use crate::harness::*;

#[test]
fn reserialized_spaced_json_records_are_full_citizens() {
    // R13: a valid-JSON record whose serialization differs from CC's compact wire
    // format by one space (`"role": "user"` — python json.dumps defaults, a jq /
    // editor round-trip) used to vanish one layer BEFORE any malformed counter
    // could see it: no preview, no record count, no search match, skipped_lines 0 —
    // invisible on every surface with zero disclosure. Stage-1 candidate detection
    // is now serialization-tolerant (`parse::line_has_role_marker`), so such
    // records are full citizens everywhere.
    let h = Home::new();
    let enc = "-Users-test-Projects-spaced";
    let sess = "eeeeeeee-aaaa-4bbb-8ccc-00000005aced";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type": "user", "uuid": "u1", "timestamp": "2026-06-07T05:00:00.000Z", "message": {"role": "user", "content": "SPACED_ALPHA question"}}"#,
            "\n",
            r#"{"type": "assistant", "uuid": "a1", "timestamp": "2026-06-07T05:00:01.000Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "SPACED_BETA answer"}]}}"#,
            "\n",
        ),
    );
    let at = format!("@{sess}");
    let s = h.run(&["search", "SPACED_BETA", &at, "--no-subagents", "-c"]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert_eq!(
        s.stdout.trim(),
        "1",
        "a spaced record must match: {}",
        s.stdout
    );
    let st = h.run(&["stats", &at, "--no-subagents", "--format", "json"]);
    assert!(st.success, "stderr: {}", st.stderr);
    let row = &json_rows(&st.stdout, "session")[0];
    assert_eq!(row["user_records"], 1, "{}", st.stdout);
    assert_eq!(row["assistant_records"], 1, "{}", st.stdout);
    let l = h.run(&["list", &at, "--no-subagents", "--format", "json"]);
    assert!(l.success, "stderr: {}", l.stderr);
    let lr = &json_rows(&l.stdout, "session")[0];
    assert!(
        lr["first_user"]["excerpt"]
            .as_str()
            .is_some_and(|e| e.contains("SPACED_ALPHA")),
        "{}",
        l.stdout
    );
    assert!(
        lr["last_agent"]["excerpt"]
            .as_str()
            .is_some_and(|e| e.contains("SPACED_BETA")),
        "{}",
        l.stdout
    );
    assert_eq!(json_summary(&l.stdout)["skipped_lines"], 0, "{}", l.stdout);
}

#[test]
fn clean_run_notes_are_absent_in_terminal_modes() {
    // Mutation pin (duals of the `> 0` note gates): on a clean, uncapped run the -l /
    // --count-by / --raw surfaces must print NO zero-count accounting notes.
    let h = Home::new();
    let _ = header_collision_scenario(&h); // clean, three sessions, no caps in play
    let l = h.run(&["search", "SEEDWORD", "-l"]);
    assert!(l.success, "stderr: {}", l.stderr);
    assert!(
        !l.stderr.contains("note:"),
        "-l prints no notes on a clean uncapped run: {}",
        l.stderr
    );
    let cb = h.run(&["search", "SEEDWORD", "--count-by", "label"]);
    assert!(cb.success, "stderr: {}", cb.stderr);
    assert!(
        !cb.stderr.contains("dropped") && !cb.stderr.contains("malformed"),
        "--count-by prints no drop/malformed notes on a clean run: {}",
        cb.stderr
    );
    let raw = h.run(&["search", "SEEDWORD", "--raw"]);
    assert!(raw.success, "stderr: {}", raw.stderr);
    assert!(
        !raw.stderr.contains("note:"),
        "--raw prints no notes on a clean uncapped run: {}",
        raw.stderr
    );
}

#[test]
fn agent_twelve_hex_fallback_ambiguity_fails_loud() {
    // Mutation pin: the 12+-hex exact-miss prefix FALLBACK has its own ambiguity guard —
    // two agents sharing 12 leading hex chars must produce the AMBIGUOUS error naming
    // both ids, never the generic no-subagent miss (and never a silent pick).
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "55667788-9900-4000-8000-00000000000a";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"lead"}}"#, "\n",
        ),
    );
    let twin_a = "abcd1234abcd1234aaaa";
    let twin_b = "abcd1234abcd1234bbbb";
    for (agent, word) in [(twin_a, "TWINALPHA"), (twin_b, "TWINBETA")] {
        h.write(
            &format!("{enc}/{sess}/subagents/agent-{agent}.jsonl"),
            &format!(
                "{}\n",
                format_args!(
                    r#"{{"type":"user","isSidechain":true,"agentId":"{agent}","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"user","content":"seed {word}"}}}}"#
                ),
            ),
        );
    }
    // 16 shared leading hex chars -> the 16-char token is an exact-miss AND a 2-way prefix.
    let ambi = h.run(&["search", "TWIN", &format!("@{}", &twin_a[..16])]);
    assert!(!ambi.success, "ambiguous fallback must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS")
            && ambi.stderr.contains(twin_a)
            && ambi.stderr.contains(twin_b),
        "the fallback names both candidates: {}",
        ambi.stderr
    );
}

#[test]
fn help_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--help"]);
    assert!(out.success);
    assert!(out.stdout.contains("ripgrep for Claude Code"));
}

#[test]
fn version_exits_zero() {
    let h = Home::new();
    let out = h.run(&["--version"]);
    assert!(out.success);
    assert!(out.stdout.contains("csift"));
}

#[test]
fn no_subcommand_errors() {
    let h = Home::new();
    let out = h.run(&[]);
    assert!(!out.success, "missing subcommand must exit nonzero");
}

/// CRITICAL data-safety: an empty reconstruction (no recoverable history / over-budget) must
/// NOT clobber the `--out` destination and must NOT print a false `(wrote …)` line. Covers
/// recover --patches, recover --at, and turns over-budget.
#[test]
fn empty_out_never_clobbers_or_lies() {
    let h = populated_home();
    let scratch = h.root.join("precious.md");
    let seed = "PRECIOUS USER CONTENT\n";

    // recover --patches on a non-existent file → empty → must leave the file untouched.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/tmp/no_such_file_xyz.md",
        "--patches",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote concatenated patches"),
        "false write line:\n{}",
        out.stdout
    );
    assert!(
        out.stderr.contains("left untouched"),
        "missing untouched note:\n{}",
        out.stderr
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "recover --patches clobbered --out"
    );

    // recover --at on a non-existent file → empty → untouched.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--file",
        "/tmp/no_such_file_xyz.md",
        "--at",
        "1w",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote partial snapshot"),
        "false write line:\n{}",
        out.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "recover --at clobbered --out"
    );

    // turns with an impossibly small budget → nothing rendered → untouched + no false write.
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--budget",
        "5",
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("wrote full reconstruction"),
        "false write line:\n{}",
        out.stdout
    );
    assert_eq!(
        std::fs::read_to_string(&scratch).unwrap(),
        seed,
        "turns clobbered --out"
    );

    // CONTROL: a NON-empty reconstruction DOES write (guard is not over-eager).
    std::fs::write(&scratch, seed).unwrap();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--out",
        scratch.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "real write missing:\n{}",
        out.stdout
    );
    let written = std::fs::read_to_string(&scratch).unwrap();
    assert_ne!(
        written, seed,
        "a non-empty turns reconstruction must overwrite --out"
    );
    assert!(!written.is_empty(), "written artifact is empty");
}

#[test]
fn acceptance_user_role_message_shapes() {
    // §A1 string · §A2 text-block array · §A3 recovered <command-args> prose — all `user.message`.
    let h = acceptance_home();
    for (oracle, token) in [
        ("A1 string", "zzgenuine"),
        ("A2 text-block", "zzblocktext"),
        ("A3 command-args", "zzcmdargs"),
    ] {
        let out = acc(&h, token, "user.message");
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains("user.message"),
            "{oracle} must classify user.message:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_communication_signals_render_direction() {
    // §C2 bare-lead inbox · §C3 idle_notification · §C4 teammate_terminated · §C5 shutdown_approved
    // · §C7 SendMessage shutdown_request — each renders `from ⇨ to` (the owner side is `self`).
    let h = acceptance_home();
    let cases = [
        (
            "C2 inbox",
            "zzbareinbox",
            "agent.communication.inbox",
            "team-lead ⇨ self",
        ),
        (
            "C3 idle",
            "zzidle",
            "agent.communication.signal",
            "SOurDnd ⇨ self",
        ),
        (
            "C4 terminated",
            "zzterminated",
            "agent.communication.signal",
            "system ⇨ self",
        ),
        (
            "C5 approved",
            "zzapproved",
            "agent.communication.signal",
            "B38 ⇨ self",
        ),
        (
            "C7 shutdown_req",
            "zzshutdownreq",
            "agent.communication.signal",
            "self ⇨ GraftBoard",
        ),
    ];
    for (oracle, token, selector, dir) in cases {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
        assert!(
            out.stdout.contains(dir),
            "{oracle} must render direction `{dir}`:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_harness_notification_monitor() {
    // §D4 / §G6 — a Monitor `<task-notification>` pulse (UNATTESTED in the corpus → synthetic) →
    // `harness.notification.monitor`, rendered as the `[monitor <id> <status>] <summary>` label.
    let h = acceptance_home();
    let out = acc(&h, "zzmonitor", "harness.notification.monitor");
    assert!(out.success, "D4: stderr {}", out.stderr);
    assert!(
        out.stdout.contains("harness.notification.monitor"),
        "D4/G6 monitor pulse → harness.notification.monitor:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[monitor mon1"),
        "D4/G6 must render the automation_label, not raw XML:\n{}",
        out.stdout
    );
}

#[test]
fn acceptance_harness_command_and_interrupt() {
    // §D8 <command-name> wrapper · §D9 <local-command-stdout> · §D10/§D11 the two interrupt markers.
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D8 invocation", "zzcmdargs", "harness.command.invocation"),
        ("D9 stdout", "zzstdout", "harness.command.stdout"),
        (
            "D10 interrupt.user",
            "interrupted by user",
            "harness.interrupt.user",
        ),
        (
            "D11 interrupt.tool",
            "interrupted by user for tool",
            "harness.interrupt.tool",
        ),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_harness_schedule_and_meta() {
    // §D12 fired wakeup tick · §D13 continuation · §G2 meta.hook (stop-hook feedback) · §G2 meta.loop
    // (autonomous-loop driver). All ride on isMeta records that classify (not user.message).
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D12 wakeup", "zzwakeup", "harness.schedule.wakeup"),
        (
            "D13 continuation",
            "Continue from where you left off",
            "harness.schedule.continuation",
        ),
        ("G2 meta.hook", "zzhook", "harness.meta.hook"),
        ("G2 meta.loop", "zzloop", "harness.meta.loop"),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}
