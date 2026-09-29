//! Single-page statements held to the code that decides them: a help sentence that names a
//! set, a source or a rule, read against that set, source or rule.

#[allow(unused_imports)]
use super::*;

/// One subcommand's long help page, rendered as `csift <name> --help` prints it.
fn page(name: &str) -> String {
    use clap::CommandFactory;
    Cli::command()
        .find_subcommand_mut(name)
        .unwrap_or_else(|| panic!("the {name} subcommand"))
        .render_long_help()
        .to_string()
}

/// The paragraph of `page` that opens at `head`, up to the next blank line.
fn paragraph<'a>(page: &'a str, head: &str) -> &'a str {
    let at = page
        .find(head)
        .unwrap_or_else(|| panic!("the page carries `{head}`"));
    let rest = &page[at..];
    &rest[..rest.find("\n\n").unwrap_or(rest.len())]
}

/// `agents --help` names the automation kinds by the slugs a pulse's label renders, the
/// selector that reaches the pulses, and the file a run row's status is read from.
///
/// The `--shape` note put the pulses under `search -t user` and left `workflow` out of the
/// kind list while saying the two axes share that token; the run row's `status` was said to
/// come from the journal, which carries no run status.
#[test]
fn the_agents_page_names_the_pulse_kinds_and_the_run_status_source() {
    let help = page("agents");
    let note = paragraph(&help, "`--shape` here is the TRANSCRIPT-SHAPE filter");
    // The kind list itself, between the pulse and its label: `workflow` also occurs later
    // in the note as the shared token, so the whole note would pass without it.
    let list = note
        .split_once("pulse carries")
        .and_then(|(_, rest)| rest.split_once("in its rendered label"))
        .map(|(list, _)| list)
        .expect("the note lists the kinds a pulse's label carries");
    let words: Vec<&str> = list
        .split(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .collect();
    for kind in crate::commands::turns::AUTOMATION_KINDS {
        assert!(
            words.contains(&kind.slug()),
            "the note's kind list names `{}`: {list}",
            kind.slug()
        );
    }
    assert!(
        note.contains("harness.notification") && !note.contains("search -t user"),
        "{note}"
    );
    assert!(
        help.contains("run MANIFEST") && !help.contains("workflow journal's own last status"),
        "the run row's status source"
    );
}

/// `plan --help`'s `--reverse` paragraph names both binding laws the lookup resolves
/// through - the `plan_mode` attachment and the first-slug fallback - since a session bound
/// by its slug alone is found as surely as one bound by the attachment.
#[test]
fn the_plan_reverse_help_names_both_binding_laws() {
    let help = page("plan");
    let para = paragraph(&help, "--reverse <PLAN_FILE>");
    for word in [
        "`plan_mode` attachment",
        "`<plans-dir>/<slug>.md`",
        "`slug-only`",
    ] {
        assert!(
            para.contains(word),
            "the --reverse help names {word}: {para}"
        );
    }
}
