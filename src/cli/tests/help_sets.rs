//! Help pages that enumerate a CLOSED SET name all of it, read off the set's own source.
//!
//! A page listing a set by hand drifts from the set the moment one member is added: the
//! root page named six `status` verdicts after the seventh shipped. Each test here reads
//! the members from where the binary defines them and asks the rendered page for each.

#[allow(unused_imports)]
use super::*;

/// The ROOT page's `status` entry names the verdict set, so it names ALL of it, and so does
/// `status --help`'s own VERDICTS paragraph.
///
/// The root entry listed six and dropped `idle-background-open` while `status --help`
/// carried seven, so a reader of the root page learned a closed set one member short. The
/// set is read off [`Verdict::ALL`], which the enum and its slugs are generated beside, so
/// a verdict added to the enum lands in this check with no edit here.
#[test]
fn the_root_help_names_every_status_verdict() {
    use crate::commands::live::Verdict;
    use clap::CommandFactory;

    let root = crate::cli::Cli::command().render_long_help().to_string();
    let start = root
        .find("  status   ")
        .expect("the root SUBCOMMANDS block has a status entry");
    let end = root[start..]
        .find("  wait     ")
        .expect("the wait entry follows it")
        + start;
    let entry = &root[start..end];
    let status = crate::cli::Cli::command()
        .find_subcommand_mut("status")
        .expect("the status subcommand")
        .render_long_help()
        .to_string();
    assert!(
        Verdict::ALL.len() >= 7,
        "the set is read, not assumed empty"
    );
    for verdict in Verdict::ALL {
        let slug = verdict.slug();
        assert!(
            entry.contains(slug),
            "`csift --help`'s status entry omits the verdict `{slug}`:\n{entry}"
        );
        assert!(
            status.contains(&format!("`{slug}`")),
            "`csift status --help` omits the verdict `{slug}`"
        );
    }
}

/// `recover --help` counts its modes the way the parser does: the default restore plus one
/// per flag of the `mode` group, and it lists every one of those flags under the count.
///
/// The head said FIVE and listed the default plus four flags while the page's own MODE
/// section named five exclusive flags. The group is read off the clap command, so a sixth
/// flag joining it moves this check rather than a number typed into the page.
#[test]
fn the_recover_head_counts_and_lists_its_mode_group() {
    use clap::CommandFactory;

    let mut cli = crate::cli::Cli::command();
    // Building resolves the IMPLICIT group each `group = "mode"` argument names.
    cli.build();
    let recover = cli
        .find_subcommand_mut("recover")
        .expect("the recover subcommand");
    let flags: Vec<String> = {
        let group = recover
            .get_groups()
            .find(|g| g.get_id() == "mode")
            .expect("recover declares its exclusive modes as the `mode` group");
        group
            .get_args()
            .map(|id| {
                let arg = recover
                    .get_arguments()
                    .find(|a| a.get_id() == id)
                    .expect("a group member is an argument");
                format!("--{}", arg.get_long().expect("every mode is a long flag"))
            })
            .collect()
    };
    assert!(flags.contains(&"--list-backups".to_string()), "{flags:?}");
    let words = [
        "ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT",
    ];
    let modes = flags.len() + 1; // the default restore takes no flag
    let help = recover.render_long_help().to_string();
    let head = format!("{} MUTUALLY-EXCLUSIVE MODES", words[modes]);
    let start = help
        .find(&head)
        .unwrap_or_else(|| panic!("`recover --help` must open its mode list with `{head}`"));
    let end = help[start..]
        .find("The TARGET selects")
        .expect("the mode list is followed by the target paragraph")
        + start;
    let list = &help[start..end];
    for flag in &flags {
        assert!(
            list.lines()
                .any(|l| l.trim_start().starts_with(flag.as_str())),
            "`recover --help`'s mode list has no line for `{flag}`:\n{list}"
        );
    }
}
