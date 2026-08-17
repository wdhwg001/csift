use super::*;

fn paths(cmd: &str) -> Vec<(String, &'static str)> {
    parse_bash_mutations(cmd)
        .into_iter()
        .map(|m| (m.path, m.verb))
        .collect()
}

/// Convenience: the set of just the PATHS a command yields (verb-agnostic), for
/// idiom tests that only care that the destination surfaced.
fn just_paths(cmd: &str) -> Vec<String> {
    parse_bash_mutations(cmd)
        .into_iter()
        .map(|m| m.path)
        .collect()
}

mod part01;
mod part02;
mod part03;
mod part04;
mod part05;
