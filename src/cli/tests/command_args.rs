//! Per-command flag surfaces: label selectors, span-switch pairs, agents/files/image args.

#[allow(unused_imports)]
use super::*;

#[test]
fn image_out_format_ext_and_media_type_pinned() {
    // Mutation pin: the transcode surface's extension/media-type mapping, per variant.
    use crate::cli::ImageOutFormat as F;
    for (f, ext, mt) in [
        (F::Png, "png", "image/png"),
        (F::Jpeg, "jpg", "image/jpeg"),
        (F::Gif, "gif", "image/gif"),
        (F::Webp, "webp", "image/webp"),
    ] {
        assert_eq!(f.ext(), ext);
        assert_eq!(f.media_type(), mt);
    }
}

/// clap's own internal consistency check - catches duplicate flags, bad
/// `overrides_with` targets, malformed value parsers at build time.
#[test]
fn command_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn whoami_accepts_trap_and_main_self_target_else_none() {
    // `@trap:<marker>` and `@main` land in whoami's optional positional; bare whoami = None.
    let cli =
        parse(&["csift", "whoami", "@trap:CrimsonWillowFen5180"]).expect("whoami @trap parses");
    match cli.command {
        Command::Whoami(a) => {
            assert_eq!(a.self_target.as_deref(), Some("@trap:CrimsonWillowFen5180"));
        }
        _ => panic!("expected whoami"),
    }
    let cli = parse(&["csift", "whoami", "@main"]).expect("whoami @main parses");
    match cli.command {
        Command::Whoami(a) => assert_eq!(a.self_target.as_deref(), Some("@main")),
        _ => panic!("expected whoami"),
    }
    let cli = parse(&["csift", "whoami"]).expect("bare whoami parses");
    match cli.command {
        Command::Whoami(a) => assert!(a.self_target.is_none()),
        _ => panic!("expected whoami"),
    }
}

#[test]
fn parse_search_rejects_old_flat_category() {
    // 0 back-compat (GOLD §6): the old flat `thinking`/`tool`/`tool-response` HARD-error.
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
    // The segment-prefix rule (GOLD §6): a partial trailing segment never leaks.
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
    assert!(!selector_is_valid("thinking")); // old flat value
    assert!(!selector_is_valid("bogus.path"));
    // label_selected: empty ⇒ all; otherwise prefix-gated.
    assert!(label_selected(&[], "harness.interrupt.user"));
    assert!(label_selected(&["agent".to_string()], "agent.tool.use"));
    assert!(!label_selected(
        &["user".to_string()],
        "agent.communication.inbox"
    ));
}

// ── Subagent inclusion flags (default include) ──

#[test]
fn list_includes_subagents_by_default() {
    let cli = parse(&["csift", "list"]).unwrap();
    match cli.command {
        Command::List(a) => assert!(a.want_subagents(), "default must include subagents"),
        _ => panic!("expected list"),
    }
}

#[test]
fn list_no_subagents_excludes() {
    let cli = parse(&["csift", "list", "--no-subagents"]).unwrap();
    match cli.command {
        Command::List(a) => assert!(!a.want_subagents()),
        _ => panic!("expected list"),
    }
}

#[test]
fn search_no_subagents_excludes() {
    let cli = parse(&["csift", "search", "x", "--no-subagents"]).unwrap();
    match cli.command {
        Command::Search(a) => assert!(!a.want_subagents()),
        _ => panic!("expected search"),
    }
}

#[test]
fn span_switch_pair_is_uniform_across_commands() {
    // ONE mental axis, two switches everywhere: `--subagents` / `--no-subagents`.
    // `--include-subagents` is GONE (0 backcompat). Defaults differ per command
    // (verbatim=off, the rest=on); passing the default-matching switch is a no-op.
    for argv in [
        vec!["csift", "list", SESS_UUID, "--include-subagents"],
        vec!["csift", "verbatim", SESS_UUID, "--include-subagents"],
    ] {
        assert!(
            parse(&argv).is_err(),
            "{argv:?}: --include-subagents must be an unknown argument now"
        );
    }
    // verbatim opts in with --subagents.
    let cli = parse(&["csift", "verbatim", SESS_UUID, "--subagents"]).unwrap();
    match cli.command {
        Command::Verbatim(a) => assert!(a.want_subagents(), "verbatim --subagents opts in"),
        _ => panic!("expected verbatim"),
    }
    // The default-on commands accept the explicit no-op --subagents…
    let cli = parse(&["csift", "list", SESS_UUID, "--subagents"]).unwrap();
    match cli.command {
        Command::List(a) => assert!(a.want_subagents()),
        _ => panic!("expected list"),
    }
    // …and the PAIR conflicts (contradictory flags are a parse error, not last-wins).
    assert!(
        parse(&["csift", "list", SESS_UUID, "--subagents", "--no-subagents"]).is_err(),
        "contradictory span switches must clash"
    );
}

#[test]
fn no_subagents_excludes_on_every_default_on_command() {
    // On the default-ON spanning commands the only span flag is `--no-subagents`; it
    // restricts to the top-level session regardless of position.
    let cli = parse(&["csift", "list", SESS_UUID, "--no-subagents"]).unwrap();
    match cli.command {
        Command::List(a) => assert!(!a.want_subagents(), "list: no-subagents excludes"),
        _ => panic!("expected list"),
    }
    let cli = parse(&["csift", "search", "x", SESS_UUID, "--no-subagents"]).unwrap();
    match cli.command {
        Command::Search(a) => assert!(!a.want_subagents(), "search: no-subagents excludes"),
        _ => panic!("expected search"),
    }
    let cli = parse(&["csift", "recover", SESS_UUID, "--no-subagents"]).unwrap();
    match cli.command {
        Command::Recover(a) => assert!(!a.want_subagents(), "recover: no-subagents excludes"),
        _ => panic!("expected recover"),
    }
    let cli = parse(&["csift", "files", SESS_UUID, "--no-subagents"]).unwrap();
    match cli.command {
        Command::Files(a) => assert!(!a.want_subagents(), "files: no-subagents excludes"),
        _ => panic!("expected files"),
    }
    let cli = parse(&["csift", "image", SESS_UUID, "--no-subagents"]).unwrap();
    match cli.command {
        Command::Image(a) => assert!(!a.want_subagents(), "image: no-subagents excludes"),
        _ => panic!("expected image"),
    }
}

#[test]
fn turns_include_then_no_subagents_cancels_the_opt_in() {
    // `verbatim` defaults to top-level-only; `--subagents` opts in. A LATER
    // `--no-subagents` cancels it - the field's `overrides_with` makes the last flag win,
    // and `want_subagents()` ANDs `include && !no_subagents`. (Bare opt-in spans; the
    // trailing `--no-subagents` here suppresses it.)
    let cli = parse(&[
        "csift",
        "verbatim",
        SESS_UUID,
        "--subagents",
        "--no-subagents",
    ])
    .unwrap();
    match cli.command {
        Command::Verbatim(a) => assert!(
            !a.want_subagents(),
            "verbatim: a trailing --no-subagents must cancel --subagents"
        ),
        _ => panic!("expected verbatim"),
    }
    // Default (no span flag) is top-level only.
    let cli = parse(&["csift", "verbatim", SESS_UUID]).unwrap();
    match cli.command {
        Command::Verbatim(a) => assert!(
            !a.want_subagents(),
            "verbatim defaults to top-level only (no opt-in)"
        ),
        _ => panic!("expected verbatim"),
    }
}

// ── agents subcommand parsing ──

#[test]
fn agents_session_target_and_window() {
    // An `@<uuid>` session token is a POSITIONAL: it lands in `paths`.
    let at = format!("@{SESS_UUID}");
    let cli = parse(&[
        "csift",
        "agents",
        at.as_str(),
        "--since",
        "2h",
        "--order-by",
        "completion",
    ])
    .unwrap();
    match cli.command {
        Command::Agents(a) => {
            assert_eq!(a.paths.len(), 1);
            assert_eq!(a.paths[0].to_string_lossy(), at);
            assert_eq!(a.since.as_deref(), Some("2h"));
            assert_eq!(a.order_by, AgentTimeAxis::Completion);
        }
        _ => panic!("expected agents"),
    }
}

#[test]
fn agents_old_by_and_tree_flags_are_gone() {
    // `--by` was renamed to `--order-by`, and `--tree` was removed (tree is always on);
    // both now error as unknown arguments.
    let by = parse(&["csift", "agents", ".", "--by", "trigger"]);
    assert!(by.is_err(), "--by must be an unknown argument now");
    let tree = parse(&["csift", "agents", ".", "--tree"]);
    assert!(tree.is_err(), "--tree must be an unknown argument now");
}

#[test]
fn agents_kind_filter_and_default_axis() {
    let cli = parse(&["csift", "agents", ".", "--shape", "workflow"]).unwrap();
    match cli.command {
        Command::Agents(a) => {
            assert_eq!(a.kinds, vec![AgentKindFilter::Workflow]);
            assert_eq!(
                a.order_by,
                AgentTimeAxis::Trigger,
                "default axis is trigger (the true spawn instant)"
            );
            assert!(a.agent.is_none());
            assert!(!a.with_files);
            assert!(!a.returned_message);
        }
        _ => panic!("expected agents"),
    }
}

// ── files subcommand parsing ──

#[test]
fn files_default_detail_is_summary() {
    let at = format!("@{SESS_UUID}");
    let cli = parse(&["csift", "files", at.as_str()]).unwrap();
    match cli.command {
        Command::Files(a) => {
            assert_eq!(a.detail(), FilesDetail::Summary, "default is summary");
            assert!(a.want_subagents(), "subagents spanned by default");
            assert_eq!(a.regex, None);
            assert_eq!(a.glob, None);
            assert_eq!(a.paths.len(), 1);
            assert_eq!(a.paths[0].to_string_lossy(), at);
        }
        _ => panic!("expected files"),
    }
}

#[test]
fn files_by_file_selects_by_file() {
    let at = format!("@{SESS_UUID}");
    let cli = parse(&["csift", "files", at.as_str(), "--by", "file"]).unwrap();
    match cli.command {
        Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByFile),
        _ => panic!("expected files"),
    }
}

#[test]
fn files_by_dir_and_timeline_select_levels() {
    let by_dir = parse(&["csift", "files", ".", "--by", "dir"]).unwrap();
    match by_dir.command {
        Command::Files(a) => assert_eq!(a.detail(), FilesDetail::ByDir),
        _ => panic!("expected files"),
    }
    let timeline = parse(&["csift", "files", ".", "--by", "timeline", "--since", "2h"]).unwrap();
    match timeline.command {
        Command::Files(a) => {
            assert_eq!(a.detail(), FilesDetail::Timeline);
            assert_eq!(a.since.as_deref(), Some("2h"));
        }
        _ => panic!("expected files"),
    }
}

#[test]
fn files_explicit_summary_value_is_summary() {
    let cli = parse(&["csift", "files", ".", "--by", "summary"]).unwrap();
    match cli.command {
        Command::Files(a) => assert_eq!(a.detail(), FilesDetail::Summary),
        _ => panic!("expected files"),
    }
}

#[test]
fn files_by_rejects_unknown_value() {
    // The clap `ValueEnum` for `--by` rejects a spelling outside the four allowed values
    // (in particular the OLD `--by-dir`-style value name is NOT accepted).
    let err = parse(&["csift", "files", ".", "--by", "by-dir"]);
    assert!(err.is_err(), "an unknown --by value must be a parse error");
    let err2 = parse(&["csift", "files", ".", "--by", "files"]);
    assert!(err2.is_err(), "an unknown --by value must be a parse error");
}

#[test]
fn files_old_detail_flags_are_gone() {
    // The 4 old boolean detail flags were replaced by `--by <value>`; passing one now
    // errors as an unknown argument.
    for flag in ["--summary", "--by-dir", "--by-file", "--timeline"] {
        let err = parse(&["csift", "files", ".", flag]);
        assert!(err.is_err(), "{flag} must be an unknown argument now");
    }
}

#[test]
fn files_no_subagents_excludes() {
    let cli = parse(&["csift", "files", ".", "--no-subagents"]).unwrap();
    match cli.command {
        Command::Files(a) => assert!(!a.want_subagents()),
        _ => panic!("expected files"),
    }
}

#[test]
fn files_subagents_only_flag_is_gone() {
    // `--subagents-only` was removed from `files`; it is now an unknown argument.
    let err = parse(&["csift", "files", ".", "--subagents-only"]);
    assert!(err.is_err(), "--subagents-only must be gone from files");
}

#[test]
fn files_regex_and_glob_parse() {
    let cli = parse(&[
        "csift",
        "files",
        ".",
        "--by",
        "file",
        "--regex",
        r"\.rs$",
        "--glob",
        "**/src/**",
    ])
    .unwrap();
    match cli.command {
        Command::Files(a) => {
            assert_eq!(a.detail(), FilesDetail::ByFile);
            assert_eq!(a.regex.as_deref(), Some(r"\.rs$"));
            assert_eq!(a.glob.as_deref(), Some("**/src/**"));
        }
        _ => panic!("expected files"),
    }
}

#[test]
fn files_encoded_token_then_flag_ordering() {
    // normalize_argv must route a trailing flag ahead of a leading-`-` token.
    let cli = parse(&["csift", "files", "-Users-foo", "--format", "json"]).unwrap();
    match cli.command {
        Command::Files(a) => {
            assert_eq!(a.format, OutputFormat::Json);
            assert_eq!(a.paths.len(), 1);
            assert_eq!(a.paths[0].to_string_lossy(), "-Users-foo");
        }
        _ => panic!("expected files"),
    }
}

#[test]
fn selector_three_forms_and_visibility() {
    // Bare ROLE: LLM-visible leaves only.
    assert!(selector_matches("user", "user.message"));
    assert!(selector_matches("user", "user.answer"));
    assert!(
        !selector_matches("user", "user.unsent"),
        "drafts are not the conversation"
    );
    assert!(selector_matches("harness", "harness.compaction.summary"));
    assert!(
        !selector_matches("harness", "harness.compaction.boundary"),
        "a metrics-only system record"
    );
    // GLOB: everything under the prefix, visibility ignored.
    assert!(selector_matches("user.*", "user.unsent"));
    assert!(selector_matches("user.*", "user.message"));
    assert!(selector_matches("harness.*", "harness.compaction.boundary"));
    assert!(!selector_matches("user.*", "agent.message"));
    // Intermediate prefix / exact leaf: a drill-down keeps its full set.
    assert!(selector_matches(
        "harness.compaction",
        "harness.compaction.boundary"
    ));
    assert!(selector_matches("user.unsent", "user.unsent"));
    assert!(selector_matches("agent.tool", "agent.tool.result"));
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
