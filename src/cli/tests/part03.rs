#[allow(unused_imports)]
use super::*;

// ── Drift guards: the help text and the repo docs must never resurrect a removed
//    surface (the v0.2→v0.3 cleanup found `--help` teaching flags that hard-error;
//    these pins make that class of drift a compile-time-adjacent failure). ──

/// Every `--long-flag` token any rendered help page mentions must be a DECLARED long
/// flag of some command (clap introspection — zero-drift), or sit on the tiny
/// allowlist of spellings the help deliberately names as REMOVED.
#[test]
fn help_mentions_only_declared_flags() {
    use clap::CommandFactory;
    // clap built-ins + documented-as-removed spellings the help may NAME.
    let allow: &[&str] = &["help", "version", "full"];
    let mut cmd = Cli::command();
    let mut declared: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut helps: Vec<(String, String)> = Vec::new();
    helps.push(("csift".into(), cmd.render_long_help().to_string()));
    for a in Cli::command().get_arguments() {
        if let Some(longs) = a.get_long_and_visible_aliases() {
            declared.extend(longs.into_iter().map(str::to_string));
        }
    }
    for sub in cmd.get_subcommands_mut() {
        let name = sub.get_name().to_string();
        for a in sub.get_arguments() {
            if let Some(longs) = a.get_long_and_visible_aliases() {
                declared.extend(longs.into_iter().map(str::to_string));
            }
        }
        helps.push((name, sub.render_long_help().to_string()));
    }
    let flag_re = regex::Regex::new(r"--([a-z][a-z0-9-]*)").unwrap();
    for (page, help) in &helps {
        for cap in flag_re.captures_iter(help) {
            let flag = cap[1].to_string();
            assert!(
                declared.contains(&flag) || allow.contains(&flag.as_str()),
                "`csift {page} --help` mentions `--{flag}`, which no command declares — \
                     a resurrected/renamed flag in help text"
            );
        }
    }
}

/// Every `csift <sub> …` EXAMPLE line in any help page must use only flags that
/// SUB actually declares (+ the global --claude-home) — the exact drift that left
/// dead `search --line` examples in the v0.1 help.
#[test]
fn help_examples_use_the_named_subcommands_own_flags() {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let mut flags_by_sub: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut helps: Vec<String> = vec![cmd.render_long_help().to_string()];
    let global: std::collections::BTreeSet<String> = Cli::command()
        .get_arguments()
        .filter_map(|a| a.get_long().map(str::to_string))
        .collect();
    for sub in cmd.get_subcommands_mut() {
        let mut set: std::collections::BTreeSet<String> = sub
            .get_arguments()
            .flat_map(|a| {
                a.get_long_and_visible_aliases()
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        set.extend(global.iter().cloned());
        flags_by_sub.insert(sub.get_name().to_string(), set);
        helps.push(sub.render_long_help().to_string());
    }
    for help in &helps {
        for line in help.lines() {
            // A line may chain several invocations through pipes or nest them in
            // `$( … )` command substitutions — check each segment independently.
            for seg in line.split('|').flat_map(|s| s.split("$(")) {
                let Some(rest) = seg.trim_start().strip_prefix("csift ") else {
                    continue;
                };
                let mut parts = rest.split_whitespace();
                let Some(sub) = parts.next() else { continue };
                let Some(flags) = flags_by_sub.get(sub) else {
                    continue; // `csift --help` etc — not a subcommand example
                };
                for tok in parts {
                    if let Some(f) = tok.strip_prefix("--") {
                        let f: String = f
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                            .collect();
                        if f.is_empty() {
                            continue;
                        }
                        assert!(
                            flags.contains(&f),
                            "help example `csift {sub} …` uses `--{f}`, which `{sub}` \
                                 does not declare: {line}"
                        );
                    }
                }
            }
        }
    }
}

/// SKILL.md's surface stamp must match the crate version — forces the LLM-facing
/// skill to be (at least) OPENED on every release, so the "re-read this file on an
/// unexpected error" recovery path always lands on current truth.
#[test]
fn skill_stamp_matches_crate_version() {
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root");
    let want = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        skill.contains(&want),
        "SKILL.md must carry the surface stamp `{want}` (found none) — update the \
             stamp (and the surface docs) with every version bump"
    );
}

/// The four repo docs must never resurrect a token this cleanup removed. SPEC's
/// `>`-quoted change LEDGER lines are exempt (they record the renames themselves).
#[test]
fn docs_do_not_resurrect_removed_tokens() {
    let deny: &[&str] = &[
        "--include-subagents",
        "agents --kind",
        "--by-file",
        "--line A-B",
        "category=",
    ];
    for doc in ["SKILL.md", "AGENTS.md", "SPEC.md", "README.md"] {
        let path = format!("{}/{doc}", env!("CARGO_MANIFEST_DIR"));
        let body = std::fs::read_to_string(&path).expect("repo doc");
        for (i, line) in body.lines().enumerate() {
            if line.starts_with('>') {
                continue; // SPEC change-ledger lines legitimately name old spellings
            }
            for tok in deny {
                assert!(
                    !line.contains(tok),
                    "{doc}:{} resurrects the removed token `{tok}`: {line}",
                    i + 1
                );
            }
        }
    }
}
