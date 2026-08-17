use crate::cli::*;

/// A canonical 8-4-4-4-12 session uuid for the bare-uuid-positional routing tests.
const SESS_UUID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

/// Parse through the SAME normalization pass the real entrypoint uses
/// (`parse_argv` → `normalize_argv` → clap), so the flag-ordering fix is what we
/// actually test, not bare clap.
fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
    let owned: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    Cli::try_parse_from(normalize_argv(owned))
}

mod argv;
mod command_args;
mod docs;
mod targets;
