use crate::cli::*;

/// A canonical 8-4-4-4-12 session uuid for the bare-uuid-positional routing tests.
const SESS_UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

/// Parse through the SAME normalization pass the real entrypoint uses
/// (`parse_argv` -> `normalize_argv` -> clap), so the flag-ordering fix is what we
/// actually test, not bare clap.
fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
    let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    Cli::try_parse_from(normalize_argv(owned))
}

/// The csift skill as ONE text: `skills/csift/SKILL.md` followed by every
/// `references/*.md` in name order.
///
/// The skill is a directory, and a fact it states lives in whichever file owns the topic:
/// the body routes, the references hold the tables. A test asking "does the skill name X"
/// reads the whole folder, so a word moved from the body into a reference (or between two
/// references) is still found where a reader following the routes would find it.
fn skill_folder_text() -> String {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/csift");
    let mut text = std::fs::read_to_string(dir.join("SKILL.md")).expect("skills/csift/SKILL.md");
    let mut refs: Vec<std::path::PathBuf> = std::fs::read_dir(dir.join("references"))
        .expect("skills/csift/references")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    refs.sort();
    assert!(
        !refs.is_empty(),
        "skills/csift/references holds no .md - the folder read broke"
    );
    for path in refs {
        text.push('\n');
        text.push_str(&std::fs::read_to_string(&path).expect("a reference file"));
    }
    text
}

mod argv;
mod command_args;
mod docs;
mod help_examples;
mod help_sets;
mod labelzone;
mod leaf_tree;
mod manuals;
mod pages;
mod selectors;
mod targets;
