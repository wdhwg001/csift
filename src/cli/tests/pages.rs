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

/// `verbatim --help` names the unit Claude Code's hook-context cap counts - UTF-16 code
/// units - apart from the unit `--window` counts, Unicode scalars (claim MISC-012). It said
/// the cap counts scalars, which is exact for BMP text only: an astral character is one
/// scalar and two code units, so a window sized at the cap can cross it.
#[test]
fn the_verbatim_window_help_names_the_cap_unit_apart_from_its_own() {
    let help = page("verbatim");
    let window = paragraph(&help, "--window <N>");
    assert!(
        window.contains("UTF-16 code units") && window.contains("Unicode scalars"),
        "{window}"
    );
    assert!(!window.contains("the unit Claude Code's"), "{window}");
    let slice = paragraph(&help, "--slice <N>");
    assert!(!slice.contains("10,000 CHARACTERS"), "{slice}");
}

/// `recover --help` states the content-anchor rule per side, as `bash_anchors` and its
/// caller decide it: a READ anchor is one simple command, because a compound command's
/// stdout names no one file, while a WRITE anchor is per segment of a compound command and
/// is held to a clean result. The page said every anchor demands a single simple command
/// and that chains stay in the boundary lanes, which the write-then-run shape contradicts.
#[test]
fn the_recover_page_states_the_read_and_the_write_anchor_rules_apart() {
    use crate::bash::mutations::bash_anchors;
    let help = page("recover");
    let flat = help.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        !flat.contains("Every anchor demands a SINGLE simple command"),
        "{flat}"
    );
    assert!(flat.contains("A READ anchor demands a SINGLE simple command"));
    assert!(flat.contains("A WRITE anchor is per SEGMENT"));
    // The code the two sentences describe: a chained heredoc write anchors (and marks the
    // command compound, which is what makes the caller demand the clean result), a piped
    // read does not, and the same read alone does.
    let write = bash_anchors("cat > notes.txt <<'EOF'\nalpha\nEOF\nwc -l notes.txt");
    assert_eq!(write.writes.len(), 1, "{write:?}");
    assert!(write.multi_segment, "{write:?}");
    assert!(bash_anchors("cat notes.txt | head -n 1").read.is_none());
    assert!(bash_anchors("cat notes.txt").read.is_some());
}
