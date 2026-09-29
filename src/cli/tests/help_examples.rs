//! The runnable EXAMPLES on the help pages: each one, pasted as written, must be a
//! command the binary accepts and one that reads the rows it names.

/// Every `csift wait` EXAMPLE carries `--timeout`, because the binary requires one: a wait
/// with no bound can never end on a background task designed never to return, so an
/// example without it is a usage error when run as written. Two of the page's examples were.
#[test]
fn every_wait_example_carries_the_required_timeout() {
    use clap::CommandFactory;

    let help = crate::cli::Cli::command()
        .find_subcommand_mut("wait")
        .expect("the wait subcommand")
        .render_long_help()
        .to_string();
    let examples: Vec<&str> = help
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("csift wait "))
        .collect();
    assert!(
        examples.len() >= 3,
        "the page's examples were found: {examples:?}"
    );
    for ex in examples {
        assert!(
            ex.contains("--timeout"),
            "`csift wait --help` example runs as a usage error without `--timeout`: {ex}"
        );
    }
}

/// No `whoami --help` recipe reads `parent_session_id` off a `whoami` identity row. The env
/// form nulls that key - the environment names the top-level session in every lane, so which
/// lane the caller is cannot be read from it - and a recipe splicing it into a target builds
/// `@null`. The key is read off the `agents` node instead, which does know it.
#[test]
fn no_whoami_recipe_reads_the_parent_off_an_identity_row() {
    use clap::CommandFactory;

    let help = crate::cli::Cli::command()
        .find_subcommand_mut("whoami")
        .expect("the whoami subcommand")
        .render_long_help()
        .to_string();
    for line in help.lines() {
        assert!(
            !line.contains(r#"select(.kind=="identity").parent_session_id"#),
            "a recipe reads the key the env form leaves null: {line}"
        );
    }
    assert!(
        help.contains(r#"select(.kind=="agent").parent_session_id"#),
        "the fallback reads the parent off the agents node"
    );
}
