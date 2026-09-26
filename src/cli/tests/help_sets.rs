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
